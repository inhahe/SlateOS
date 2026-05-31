//! `OurOS` Notes & Wiki
//!
//! A rich notes/wiki application with:
//! - Note types: Plain text, Markdown (heading/bold/italic/list/code), Checklist, Table
//! - Notebook organization with nesting
//! - Tagging system with tag-based filtering
//! - Full-text search across all notes
//! - Wiki-style `[[Note Title]]` linking between notes
//! - Version history with snapshot restore
//! - Predefined templates (Meeting Notes, To-Do List, Journal, Code Snippet, etc.)
//! - Export to plain text, Markdown, or HTML
//! - Favorites and pinning
//! - Sort options: date modified, date created, title, notebook
//! - Multi-panel UI: notebook sidebar, note list, editor/preview
//! - Word count and reading time statistics
//!
//! Uses the guitk library for UI rendering.

// Lint policy is inherited from the workspace (`[lints] workspace = true`):
// `clippy::all` denied, `clippy::pedantic` at warn, with the curated allow
// list documented in the root Cargo.toml (keeps the discipline centralised).
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::similar_names)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]

use guitk::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

use std::collections::HashMap;

// ============================================================================
// Catppuccin Mocha theme constants
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);

// ============================================================================
// Layout constants
// ============================================================================

const SIDEBAR_WIDTH: f32 = 200.0;
const NOTE_LIST_WIDTH: f32 = 260.0;
const TOOLBAR_HEIGHT: f32 = 36.0;
const STATUS_BAR_HEIGHT: f32 = 24.0;
const ITEM_HEIGHT: f32 = 28.0;
const HEADER_HEIGHT: f32 = 32.0;
const TAG_HEIGHT: f32 = 20.0;
const TAG_PADDING: f32 = 8.0;
const EDITOR_PADDING: f32 = 12.0;
const LINE_HEIGHT: f32 = 20.0;
const READING_WPM: f32 = 238.0;
const MAX_VERSIONS: usize = 50;
const CORNER_RADIUS: f32 = 4.0;

// ============================================================================
// Unique ID generation
// ============================================================================

pub type NoteId = u64;
pub type NotebookId = u64;

/// Simple monotonic ID counter.
#[derive(Debug)]
struct IdGen {
    next: u64,
}

impl IdGen {
    const fn new(start: u64) -> Self {
        Self { next: start }
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next;
        self.next = self.next.saturating_add(1);
        id
    }
}

// ============================================================================
// Note types
// ============================================================================

/// The kind of content a note holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoteKind {
    /// Plain text, no formatting.
    PlainText,
    /// Markdown with headings, bold, italic, lists, code blocks.
    Markdown,
    /// A list of checkable items.
    Checklist,
    /// Tabular data with rows and columns.
    Table,
}

impl NoteKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::PlainText => "Plain Text",
            Self::Markdown => "Markdown",
            Self::Checklist => "Checklist",
            Self::Table => "Table",
        }
    }
}

// ============================================================================
// Checklist item
// ============================================================================

/// A single checklist entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChecklistItem {
    pub text: String,
    pub checked: bool,
}

impl ChecklistItem {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            checked: false,
        }
    }

    pub fn checked(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            checked: true,
        }
    }
}

// ============================================================================
// Table data
// ============================================================================

/// Row-major table data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableData {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl TableData {
    pub fn new(headers: Vec<String>) -> Self {
        Self {
            headers,
            rows: Vec::new(),
        }
    }

    pub fn add_row(&mut self, cells: Vec<String>) {
        self.rows.push(cells);
    }

    pub fn column_count(&self) -> usize {
        self.headers.len()
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Get cell value; returns empty string for out-of-bounds.
    pub fn cell(&self, row: usize, col: usize) -> &str {
        self.rows
            .get(row)
            .and_then(|r| r.get(col))
            .map_or("", String::as_str)
    }

    /// Render table as markdown text.
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        // Header row
        out.push('|');
        for h in &self.headers {
            out.push(' ');
            out.push_str(h);
            out.push_str(" |");
        }
        out.push('\n');
        // Separator
        out.push('|');
        for _ in &self.headers {
            out.push_str(" --- |");
        }
        out.push('\n');
        // Data rows
        for row in &self.rows {
            out.push('|');
            for (i, _) in self.headers.iter().enumerate() {
                out.push(' ');
                out.push_str(row.get(i).map_or("", String::as_str));
                out.push_str(" |");
            }
            out.push('\n');
        }
        out
    }

    /// Render table as plain text.
    pub fn to_plain_text(&self) -> String {
        // Compute column widths
        let ncols = self.headers.len();
        let mut widths: Vec<usize> = self.headers.iter().map(String::len).collect();
        for row in &self.rows {
            for (i, cell) in row.iter().enumerate() {
                if i < ncols
                    && let Some(w) = widths.get_mut(i)
                    && cell.len() > *w
                {
                    *w = cell.len();
                }
            }
        }
        let mut out = String::new();
        // Header
        for (i, h) in self.headers.iter().enumerate() {
            let w = widths.get(i).copied().unwrap_or(0);
            out.push_str(&format!("{h:w$}  "));
        }
        out.push('\n');
        // Separator
        for (i, _) in self.headers.iter().enumerate() {
            let w = widths.get(i).copied().unwrap_or(0);
            for _ in 0..w {
                out.push('-');
            }
            out.push_str("  ");
        }
        out.push('\n');
        // Rows
        for row in &self.rows {
            for (i, _) in self.headers.iter().enumerate() {
                let w = widths.get(i).copied().unwrap_or(0);
                let cell = row.get(i).map_or("", String::as_str);
                out.push_str(&format!("{cell:w$}  "));
            }
            out.push('\n');
        }
        out
    }
}

// ============================================================================
// Version history
// ============================================================================

/// A snapshot of a note at a point in time.
#[derive(Clone, Debug)]
pub struct NoteVersion {
    pub timestamp: u64,
    pub content: String,
    pub summary: String,
}

// ============================================================================
// Note
// ============================================================================

/// A single note.
#[derive(Clone, Debug)]
pub struct Note {
    pub id: NoteId,
    pub title: String,
    pub content: String,
    pub kind: NoteKind,
    pub notebook_id: NotebookId,
    pub tags: Vec<String>,
    pub pinned: bool,
    pub favorited: bool,
    pub created_at: u64,
    pub modified_at: u64,
    pub checklist: Vec<ChecklistItem>,
    pub table: Option<TableData>,
    pub versions: Vec<NoteVersion>,
}

impl Note {
    /// Create a new plain-text note in the given notebook.
    pub fn new(id: NoteId, title: &str, notebook_id: NotebookId) -> Self {
        Self {
            id,
            title: title.to_owned(),
            content: String::new(),
            kind: NoteKind::PlainText,
            notebook_id,
            tags: Vec::new(),
            pinned: false,
            favorited: false,
            created_at: 0,
            modified_at: 0,
            checklist: Vec::new(),
            table: None,
            versions: Vec::new(),
        }
    }

    /// Set the note content and record a version snapshot.
    pub fn set_content(&mut self, new_content: &str, timestamp: u64) {
        // Save current state as a version before overwriting.
        if !self.content.is_empty() || !self.versions.is_empty() {
            let snapshot = NoteVersion {
                timestamp: self.modified_at,
                content: self.content.clone(),
                summary: format!("Edited at {}", self.modified_at),
            };
            self.versions.push(snapshot);
            if self.versions.len() > MAX_VERSIONS {
                self.versions.remove(0);
            }
        }
        self.content = new_content.to_owned();
        self.modified_at = timestamp;
    }

    /// Restore the note to a specific version index.
    pub fn restore_version(&mut self, version_idx: usize, timestamp: u64) -> bool {
        if let Some(ver) = self.versions.get(version_idx).cloned() {
            // Save current as a version first.
            let snapshot = NoteVersion {
                timestamp: self.modified_at,
                content: self.content.clone(),
                summary: format!("Before restore at {timestamp}"),
            };
            self.versions.push(snapshot);
            if self.versions.len() > MAX_VERSIONS {
                self.versions.remove(0);
            }
            self.content = ver.content;
            self.modified_at = timestamp;
            true
        } else {
            false
        }
    }

    /// Add a tag if not already present.
    pub fn add_tag(&mut self, tag: &str) {
        let t = tag.to_owned();
        if !self.tags.contains(&t) {
            self.tags.push(t);
        }
    }

    /// Remove a tag.
    pub fn remove_tag(&mut self, tag: &str) -> bool {
        if let Some(pos) = self.tags.iter().position(|t| t == tag) {
            self.tags.remove(pos);
            true
        } else {
            false
        }
    }

    /// Check if this note has a given tag.
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }

    /// Toggle pinned status.
    pub fn toggle_pin(&mut self) {
        self.pinned = !self.pinned;
    }

    /// Toggle favorite status.
    pub fn toggle_favorite(&mut self) {
        self.favorited = !self.favorited;
    }

    /// Word count of the note content.
    pub fn word_count(&self) -> usize {
        match &self.kind {
            NoteKind::Checklist => self
                .checklist
                .iter()
                .map(|item| item.text.split_whitespace().count())
                .sum(),
            NoteKind::Table => self.table.as_ref().map_or(0, |t| {
                let header_words: usize =
                    t.headers.iter().map(|h| h.split_whitespace().count()).sum();
                let cell_words: usize = t
                    .rows
                    .iter()
                    .flat_map(|r| r.iter())
                    .map(|c| c.split_whitespace().count())
                    .sum();
                header_words.saturating_add(cell_words)
            }),
            _ => self.content.split_whitespace().count(),
        }
    }

    /// Character count.
    pub fn char_count(&self) -> usize {
        match &self.kind {
            NoteKind::Checklist => self.checklist.iter().map(|item| item.text.len()).sum(),
            NoteKind::Table => self.table.as_ref().map_or(0, |t| {
                let h: usize = t.headers.iter().map(String::len).sum();
                let c: usize = t.rows.iter().flat_map(|r| r.iter()).map(String::len).sum();
                h.saturating_add(c)
            }),
            _ => self.content.len(),
        }
    }

    /// Estimated reading time in minutes.
    pub fn reading_time_minutes(&self) -> f32 {
        let words = self.word_count() as f32;
        words / READING_WPM
    }

    /// Extract `[[wiki links]]` from content.
    pub fn extract_links(&self) -> Vec<String> {
        extract_wiki_links(&self.content)
    }

    /// Check if the note matches a search query (case-insensitive).
    pub fn matches_search(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let q = query.to_lowercase();
        if self.title.to_lowercase().contains(&q) {
            return true;
        }
        if self.content.to_lowercase().contains(&q) {
            return true;
        }
        for tag in &self.tags {
            if tag.to_lowercase().contains(&q) {
                return true;
            }
        }
        for item in &self.checklist {
            if item.text.to_lowercase().contains(&q) {
                return true;
            }
        }
        false
    }

    /// Add a checklist item.
    pub fn add_checklist_item(&mut self, text: &str) {
        self.checklist.push(ChecklistItem::new(text));
    }

    /// Toggle a checklist item by index.
    pub fn toggle_checklist_item(&mut self, idx: usize) -> bool {
        if let Some(item) = self.checklist.get_mut(idx) {
            item.checked = !item.checked;
            true
        } else {
            false
        }
    }

    /// Remove a checklist item by index.
    pub fn remove_checklist_item(&mut self, idx: usize) -> bool {
        if idx < self.checklist.len() {
            self.checklist.remove(idx);
            true
        } else {
            false
        }
    }
}

// ============================================================================
// Wiki-link extraction
// ============================================================================

/// Extract all `[[target]]` links from text.
pub fn extract_wiki_links(text: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        let after_open = start.saturating_add(2);
        if let Some(end) = rest.get(after_open..).and_then(|s| s.find("]]")) {
            if let Some(link_text) = rest.get(after_open..after_open.saturating_add(end)) {
                let trimmed = link_text.trim();
                if !trimmed.is_empty() {
                    links.push(trimmed.to_owned());
                }
            }
            rest = rest
                .get(after_open.saturating_add(end).saturating_add(2)..)
                .unwrap_or("");
        } else {
            break;
        }
    }
    links
}

// ============================================================================
// Markdown parsing (simplified)
// ============================================================================

/// A parsed Markdown block.
#[derive(Clone, Debug, PartialEq)]
pub enum MdBlock {
    Heading { level: u8, text: String },
    Paragraph { spans: Vec<MdSpan> },
    UnorderedList { items: Vec<String> },
    OrderedList { items: Vec<String> },
    CodeBlock { language: String, code: String },
    BlockQuote { text: String },
    HorizontalRule,
}

/// An inline span within a paragraph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MdSpan {
    Plain(String),
    Bold(String),
    Italic(String),
    BoldItalic(String),
    InlineCode(String),
    WikiLink(String),
}

/// Parse inline markdown formatting into spans.
pub fn parse_inline(text: &str) -> Vec<MdSpan> {
    let mut spans = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        // Check for wiki links [[...]]
        if remaining.starts_with("[[")
            && let Some(end) = remaining.get(2..).and_then(|s| s.find("]]"))
        {
            let link = remaining.get(2..end.saturating_add(2)).unwrap_or("");
            spans.push(MdSpan::WikiLink(link.to_owned()));
            remaining = remaining.get(end.saturating_add(4)..).unwrap_or("");
            continue;
        }

        // Check for inline code `...`
        if remaining.starts_with('`')
            && let Some(end) = remaining.get(1..).and_then(|s| s.find('`'))
        {
            let code = remaining.get(1..end.saturating_add(1)).unwrap_or("");
            spans.push(MdSpan::InlineCode(code.to_owned()));
            remaining = remaining.get(end.saturating_add(2)..).unwrap_or("");
            continue;
        }

        // Check for bold+italic ***...***
        if remaining.starts_with("***")
            && let Some(end) = remaining.get(3..).and_then(|s| s.find("***"))
        {
            let inner = remaining.get(3..end.saturating_add(3)).unwrap_or("");
            spans.push(MdSpan::BoldItalic(inner.to_owned()));
            remaining = remaining.get(end.saturating_add(6)..).unwrap_or("");
            continue;
        }

        // Check for bold **...**
        if remaining.starts_with("**")
            && let Some(end) = remaining.get(2..).and_then(|s| s.find("**"))
        {
            let inner = remaining.get(2..end.saturating_add(2)).unwrap_or("");
            spans.push(MdSpan::Bold(inner.to_owned()));
            remaining = remaining.get(end.saturating_add(4)..).unwrap_or("");
            continue;
        }

        // Check for italic *...*
        if remaining.starts_with('*')
            && !remaining.starts_with("**")
            && let Some(end) = remaining.get(1..).and_then(|s| s.find('*'))
        {
            let inner = remaining.get(1..end.saturating_add(1)).unwrap_or("");
            if !inner.is_empty() {
                spans.push(MdSpan::Italic(inner.to_owned()));
                remaining = remaining.get(end.saturating_add(2)..).unwrap_or("");
                continue;
            }
        }

        // Collect plain text until the next special character.
        let next_special = remaining.find(['*', '`', '[']).unwrap_or(remaining.len());
        let chunk_end = if next_special == 0 { 1 } else { next_special };
        let chunk = remaining.get(..chunk_end).unwrap_or("");
        if !chunk.is_empty() {
            // Merge with previous plain span if possible.
            if let Some(MdSpan::Plain(prev)) = spans.last_mut() {
                prev.push_str(chunk);
            } else {
                spans.push(MdSpan::Plain(chunk.to_owned()));
            }
        }
        remaining = remaining.get(chunk_end..).unwrap_or("");
    }

    if spans.is_empty() {
        spans.push(MdSpan::Plain(String::new()));
    }
    spans
}

/// Parse a markdown string into blocks.
pub fn parse_markdown(input: &str) -> Vec<MdBlock> {
    let mut blocks = Vec::new();
    let lines: Vec<&str> = input.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines.get(i).copied().unwrap_or("");

        // Blank line — skip
        if line.trim().is_empty() {
            i = i.saturating_add(1);
            continue;
        }

        // Horizontal rule
        if line.trim() == "---" || line.trim() == "***" || line.trim() == "___" {
            blocks.push(MdBlock::HorizontalRule);
            i = i.saturating_add(1);
            continue;
        }

        // Heading
        if line.starts_with('#') {
            let level = line.chars().take_while(|c| *c == '#').count().min(6) as u8;
            let text = line.get(level as usize..).unwrap_or("").trim().to_owned();
            blocks.push(MdBlock::Heading { level, text });
            i = i.saturating_add(1);
            continue;
        }

        // Fenced code block
        if line.starts_with("```") {
            let language = line.get(3..).unwrap_or("").trim().to_owned();
            let mut code_lines = Vec::new();
            i = i.saturating_add(1);
            while i < lines.len() {
                let cl = lines.get(i).copied().unwrap_or("");
                if cl.starts_with("```") {
                    break;
                }
                code_lines.push(cl);
                i = i.saturating_add(1);
            }
            // Skip closing ```
            if i < lines.len() {
                i = i.saturating_add(1);
            }
            blocks.push(MdBlock::CodeBlock {
                language,
                code: code_lines.join("\n"),
            });
            continue;
        }

        // Block quote
        if line.starts_with('>') {
            let mut quote_lines = Vec::new();
            while i < lines.len() {
                let ql = lines.get(i).copied().unwrap_or("");
                if ql.starts_with('>') {
                    let content = ql.get(1..).unwrap_or("").trim_start();
                    quote_lines.push(content);
                    i = i.saturating_add(1);
                } else {
                    break;
                }
            }
            blocks.push(MdBlock::BlockQuote {
                text: quote_lines.join("\n"),
            });
            continue;
        }

        // Unordered list
        if line.starts_with("- ") || line.starts_with("* ") || line.starts_with("+ ") {
            let mut items = Vec::new();
            while i < lines.len() {
                let ll = lines.get(i).copied().unwrap_or("");
                if ll.starts_with("- ") || ll.starts_with("* ") || ll.starts_with("+ ") {
                    items.push(ll.get(2..).unwrap_or("").to_owned());
                    i = i.saturating_add(1);
                } else {
                    break;
                }
            }
            blocks.push(MdBlock::UnorderedList { items });
            continue;
        }

        // Ordered list
        if is_ordered_list_item(line) {
            let mut items = Vec::new();
            while i < lines.len() {
                let ll = lines.get(i).copied().unwrap_or("");
                if is_ordered_list_item(ll) {
                    if let Some(dot_pos) = ll.find(". ") {
                        items.push(ll.get(dot_pos.saturating_add(2)..).unwrap_or("").to_owned());
                    }
                    i = i.saturating_add(1);
                } else {
                    break;
                }
            }
            blocks.push(MdBlock::OrderedList { items });
            continue;
        }

        // Paragraph: collect consecutive non-empty, non-special lines.
        let mut para_text = String::new();
        while i < lines.len() {
            let pl = lines.get(i).copied().unwrap_or("");
            if pl.trim().is_empty()
                || pl.starts_with('#')
                || pl.starts_with("```")
                || pl.starts_with('>')
                || pl.starts_with("- ")
                || pl.starts_with("* ")
                || pl.starts_with("+ ")
                || is_ordered_list_item(pl)
                || pl.trim() == "---"
                || pl.trim() == "***"
                || pl.trim() == "___"
            {
                break;
            }
            if !para_text.is_empty() {
                para_text.push(' ');
            }
            para_text.push_str(pl);
            i = i.saturating_add(1);
        }
        if !para_text.is_empty() {
            blocks.push(MdBlock::Paragraph {
                spans: parse_inline(&para_text),
            });
        }
    }

    blocks
}

/// Check whether a line looks like `1. `, `2. `, etc.
fn is_ordered_list_item(line: &str) -> bool {
    if let Some(dot_pos) = line.find(". ") {
        line.get(..dot_pos)
            .is_some_and(|prefix| !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_digit()))
    } else {
        false
    }
}

// ============================================================================
// Export formats
// ============================================================================

/// Export format selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    PlainText,
    Markdown,
    Html,
}

impl ExportFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::PlainText => "Plain Text",
            Self::Markdown => "Markdown",
            Self::Html => "HTML",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::PlainText => "txt",
            Self::Markdown => "md",
            Self::Html => "html",
        }
    }
}

/// Export a note to the given format.
pub fn export_note(note: &Note, format: ExportFormat) -> String {
    match format {
        ExportFormat::PlainText => export_plain_text(note),
        ExportFormat::Markdown => export_markdown(note),
        ExportFormat::Html => export_html(note),
    }
}

fn export_plain_text(note: &Note) -> String {
    let mut out = String::new();
    out.push_str(&note.title);
    out.push_str("\n\n");
    match &note.kind {
        NoteKind::Checklist => {
            for item in &note.checklist {
                let marker = if item.checked { "[x]" } else { "[ ]" };
                out.push_str(&format!("{marker} {}\n", item.text));
            }
        }
        NoteKind::Table => {
            if let Some(table) = &note.table {
                out.push_str(&table.to_plain_text());
            }
        }
        _ => {
            out.push_str(&note.content);
        }
    }
    out
}

fn export_markdown(note: &Note) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", note.title));
    if !note.tags.is_empty() {
        out.push_str("**Tags:** ");
        out.push_str(&note.tags.join(", "));
        out.push_str("\n\n");
    }
    match &note.kind {
        NoteKind::Checklist => {
            for item in &note.checklist {
                let marker = if item.checked { "[x]" } else { "[ ]" };
                out.push_str(&format!("- {marker} {}\n", item.text));
            }
        }
        NoteKind::Table => {
            if let Some(table) = &note.table {
                out.push_str(&table.to_markdown());
            }
        }
        _ => {
            out.push_str(&note.content);
        }
    }
    out
}

fn export_html(note: &Note) -> String {
    let mut out = String::new();
    out.push_str("<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\">\n");
    out.push_str(&format!("<title>{}</title>\n", html_escape(&note.title)));
    out.push_str("<style>body{font-family:sans-serif;max-width:800px;margin:0 auto;padding:20px;");
    out.push_str("background:#1e1e2e;color:#cdd6f4}");
    out.push_str("h1{color:#89b4fa}code{background:#313244;padding:2px 6px;border-radius:4px}");
    out.push_str("pre{background:#313244;padding:16px;border-radius:8px;overflow-x:auto}");
    out.push_str("blockquote{border-left:4px solid #89b4fa;padding-left:16px;color:#a6adc8}");
    out.push_str(
        "table{border-collapse:collapse;width:100%}th,td{border:1px solid #45475a;padding:8px}",
    );
    out.push_str("th{background:#313244}");
    out.push_str("</style></head><body>\n");
    out.push_str(&format!("<h1>{}</h1>\n", html_escape(&note.title)));

    if !note.tags.is_empty() {
        out.push_str("<p><strong>Tags:</strong> ");
        out.push_str(&html_escape(&note.tags.join(", ")));
        out.push_str("</p>\n");
    }

    match &note.kind {
        NoteKind::Checklist => {
            out.push_str("<ul style=\"list-style:none\">\n");
            for item in &note.checklist {
                let marker = if item.checked { "&#9745;" } else { "&#9744;" };
                out.push_str(&format!(
                    "<li>{} {}</li>\n",
                    marker,
                    html_escape(&item.text)
                ));
            }
            out.push_str("</ul>\n");
        }
        NoteKind::Table => {
            if let Some(table) = &note.table {
                out.push_str("<table>\n<thead><tr>\n");
                for h in &table.headers {
                    out.push_str(&format!("<th>{}</th>", html_escape(h)));
                }
                out.push_str("\n</tr></thead>\n<tbody>\n");
                for row in &table.rows {
                    out.push_str("<tr>");
                    for (i, _) in table.headers.iter().enumerate() {
                        let cell = row.get(i).map_or("", String::as_str);
                        out.push_str(&format!("<td>{}</td>", html_escape(cell)));
                    }
                    out.push_str("</tr>\n");
                }
                out.push_str("</tbody></table>\n");
            }
        }
        NoteKind::Markdown => {
            let blocks = parse_markdown(&note.content);
            out.push_str(&render_blocks_to_html(&blocks));
        }
        NoteKind::PlainText => {
            // Wrap paragraphs in <p> tags, preserving blank lines.
            for para in note.content.split("\n\n") {
                let trimmed = para.trim();
                if !trimmed.is_empty() {
                    out.push_str(&format!(
                        "<p>{}</p>\n",
                        html_escape(trimmed).replace('\n', "<br>")
                    ));
                }
            }
        }
    }

    out.push_str("</body></html>\n");
    out
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn render_blocks_to_html(blocks: &[MdBlock]) -> String {
    let mut out = String::new();
    for block in blocks {
        match block {
            MdBlock::Heading { level, text } => {
                out.push_str(&format!("<h{l}>{}</h{l}>\n", html_escape(text), l = level));
            }
            MdBlock::Paragraph { spans } => {
                out.push_str("<p>");
                out.push_str(&render_spans_to_html(spans));
                out.push_str("</p>\n");
            }
            MdBlock::UnorderedList { items } => {
                out.push_str("<ul>\n");
                for item in items {
                    out.push_str(&format!("<li>{}</li>\n", html_escape(item)));
                }
                out.push_str("</ul>\n");
            }
            MdBlock::OrderedList { items } => {
                out.push_str("<ol>\n");
                for item in items {
                    out.push_str(&format!("<li>{}</li>\n", html_escape(item)));
                }
                out.push_str("</ol>\n");
            }
            MdBlock::CodeBlock { language, code } => {
                if language.is_empty() {
                    out.push_str("<pre><code>");
                } else {
                    out.push_str(&format!(
                        "<pre><code class=\"language-{}\">",
                        html_escape(language)
                    ));
                }
                out.push_str(&html_escape(code));
                out.push_str("</code></pre>\n");
            }
            MdBlock::BlockQuote { text } => {
                out.push_str(&format!("<blockquote>{}</blockquote>\n", html_escape(text)));
            }
            MdBlock::HorizontalRule => {
                out.push_str("<hr>\n");
            }
        }
    }
    out
}

fn render_spans_to_html(spans: &[MdSpan]) -> String {
    let mut out = String::new();
    for span in spans {
        match span {
            MdSpan::Plain(t) => out.push_str(&html_escape(t)),
            MdSpan::Bold(t) => {
                out.push_str(&format!("<strong>{}</strong>", html_escape(t)));
            }
            MdSpan::Italic(t) => {
                out.push_str(&format!("<em>{}</em>", html_escape(t)));
            }
            MdSpan::BoldItalic(t) => {
                out.push_str(&format!("<strong><em>{}</em></strong>", html_escape(t)));
            }
            MdSpan::InlineCode(t) => {
                out.push_str(&format!("<code>{}</code>", html_escape(t)));
            }
            MdSpan::WikiLink(target) => {
                out.push_str(&format!(
                    "<a href=\"#{}\">{}</a>",
                    html_escape(target),
                    html_escape(target)
                ));
            }
        }
    }
    out
}

// ============================================================================
// Notebook
// ============================================================================

/// A notebook that contains notes and can be nested.
#[derive(Clone, Debug)]
pub struct Notebook {
    pub id: NotebookId,
    pub name: String,
    pub parent_id: Option<NotebookId>,
    pub expanded: bool,
}

impl Notebook {
    pub fn new(id: NotebookId, name: &str) -> Self {
        Self {
            id,
            name: name.to_owned(),
            parent_id: None,
            expanded: true,
        }
    }

    pub fn with_parent(id: NotebookId, name: &str, parent_id: NotebookId) -> Self {
        Self {
            id,
            name: name.to_owned(),
            parent_id: Some(parent_id),
            expanded: true,
        }
    }
}

// ============================================================================
// Sort options
// ============================================================================

/// How to sort notes in the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortOrder {
    DateModified,
    DateCreated,
    Title,
    Notebook,
}

impl SortOrder {
    pub fn label(self) -> &'static str {
        match self {
            Self::DateModified => "Modified",
            Self::DateCreated => "Created",
            Self::Title => "Title",
            Self::Notebook => "Notebook",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::DateModified => Self::DateCreated,
            Self::DateCreated => Self::Title,
            Self::Title => Self::Notebook,
            Self::Notebook => Self::DateModified,
        }
    }
}

// ============================================================================
// Templates
// ============================================================================

/// Available note templates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteTemplate {
    Blank,
    MeetingNotes,
    TodoList,
    Journal,
    CodeSnippet,
    ProjectPlan,
    BugReport,
    WeeklyReview,
}

impl NoteTemplate {
    pub fn label(self) -> &'static str {
        match self {
            Self::Blank => "Blank",
            Self::MeetingNotes => "Meeting Notes",
            Self::TodoList => "To-Do List",
            Self::Journal => "Journal Entry",
            Self::CodeSnippet => "Code Snippet",
            Self::ProjectPlan => "Project Plan",
            Self::BugReport => "Bug Report",
            Self::WeeklyReview => "Weekly Review",
        }
    }

    pub fn kind(self) -> NoteKind {
        match self {
            Self::TodoList => NoteKind::Checklist,
            Self::MeetingNotes
            | Self::Journal
            | Self::CodeSnippet
            | Self::ProjectPlan
            | Self::BugReport
            | Self::WeeklyReview => NoteKind::Markdown,
            Self::Blank => NoteKind::PlainText,
        }
    }

    pub fn content(self) -> &'static str {
        match self {
            Self::Blank => "",
            Self::MeetingNotes => concat!(
                "# Meeting Notes\n\n",
                "**Date:** YYYY-MM-DD\n",
                "**Time:** HH:MM\n",
                "**Location:** \n\n",
                "## Attendees\n\n",
                "- Person 1\n",
                "- Person 2\n\n",
                "## Agenda\n\n",
                "1. Topic 1\n",
                "2. Topic 2\n\n",
                "## Notes\n\n",
                "\n\n",
                "## Action Items\n\n",
                "- [ ] Action 1\n",
                "- [ ] Action 2\n",
            ),
            Self::TodoList => "",
            Self::Journal => concat!(
                "# Journal Entry\n\n",
                "**Date:** YYYY-MM-DD\n\n",
                "## Today's Highlights\n\n",
                "\n\n",
                "## Thoughts & Reflections\n\n",
                "\n\n",
                "## Gratitude\n\n",
                "- \n\n",
                "## Tomorrow's Goals\n\n",
                "- \n",
            ),
            Self::CodeSnippet => concat!(
                "# Code Snippet\n\n",
                "**Language:** \n",
                "**Description:** \n\n",
                "```\n",
                "// Your code here\n",
                "```\n\n",
                "## Notes\n\n",
                "- \n",
            ),
            Self::ProjectPlan => concat!(
                "# Project Plan\n\n",
                "**Project:** \n",
                "**Start Date:** YYYY-MM-DD\n",
                "**Target Date:** YYYY-MM-DD\n\n",
                "## Overview\n\n",
                "\n\n",
                "## Goals\n\n",
                "1. Goal 1\n",
                "2. Goal 2\n\n",
                "## Milestones\n\n",
                "- [ ] Milestone 1\n",
                "- [ ] Milestone 2\n\n",
                "## Resources\n\n",
                "- \n\n",
                "## Risks\n\n",
                "- \n",
            ),
            Self::BugReport => concat!(
                "# Bug Report\n\n",
                "**Severity:** \n",
                "**Component:** \n",
                "**Version:** \n\n",
                "## Description\n\n",
                "\n\n",
                "## Steps to Reproduce\n\n",
                "1. Step 1\n",
                "2. Step 2\n\n",
                "## Expected Behavior\n\n",
                "\n\n",
                "## Actual Behavior\n\n",
                "\n\n",
                "## Screenshots\n\n",
                "\n",
            ),
            Self::WeeklyReview => concat!(
                "# Weekly Review\n\n",
                "**Week of:** YYYY-MM-DD\n\n",
                "## Accomplishments\n\n",
                "- \n\n",
                "## Challenges\n\n",
                "- \n\n",
                "## Lessons Learned\n\n",
                "- \n\n",
                "## Next Week's Priorities\n\n",
                "1. Priority 1\n",
                "2. Priority 2\n\n",
                "## Metrics\n\n",
                "| Metric | Target | Actual |\n",
                "| --- | --- | --- |\n",
                "| | | |\n",
            ),
        }
    }

    /// All available templates.
    pub fn all() -> &'static [NoteTemplate] {
        &[
            NoteTemplate::Blank,
            NoteTemplate::MeetingNotes,
            NoteTemplate::TodoList,
            NoteTemplate::Journal,
            NoteTemplate::CodeSnippet,
            NoteTemplate::ProjectPlan,
            NoteTemplate::BugReport,
            NoteTemplate::WeeklyReview,
        ]
    }
}

// ============================================================================
// UI view state
// ============================================================================

/// Which panel is currently focused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivePanel {
    NotebookSidebar,
    NoteList,
    Editor,
}

// ============================================================================
// Main application state
// ============================================================================

/// The main notes/wiki application.
pub struct NotesApp {
    pub notebooks: Vec<Notebook>,
    pub notes: Vec<Note>,
    pub selected_notebook: Option<NotebookId>,
    pub selected_note: Option<NoteId>,
    pub search_query: String,
    pub active_tag_filter: Option<String>,
    pub sort_order: SortOrder,
    pub active_panel: ActivePanel,
    pub show_favorites_only: bool,
    pub window_width: f32,
    pub window_height: f32,
    note_id_gen: IdGen,
    notebook_id_gen: IdGen,
    timestamp_counter: u64,
}

impl Default for NotesApp {
    fn default() -> Self {
        Self::new()
    }
}

impl NotesApp {
    /// Create a new empty application.
    pub fn new() -> Self {
        Self {
            notebooks: Vec::new(),
            notes: Vec::new(),
            selected_notebook: None,
            selected_note: None,
            search_query: String::new(),
            active_tag_filter: None,
            sort_order: SortOrder::DateModified,
            active_panel: ActivePanel::NoteList,
            show_favorites_only: false,
            window_width: 1280.0,
            window_height: 800.0,
            note_id_gen: IdGen::new(1),
            notebook_id_gen: IdGen::new(1),
            timestamp_counter: 1000,
        }
    }

    /// Advance the internal timestamp counter and return the new value.
    fn tick(&mut self) -> u64 {
        self.timestamp_counter = self.timestamp_counter.saturating_add(1);
        self.timestamp_counter
    }

    // -----------------------------------------------------------------------
    // Notebook management
    // -----------------------------------------------------------------------

    /// Create a new top-level notebook.
    pub fn create_notebook(&mut self, name: &str) -> NotebookId {
        let id = self.notebook_id_gen.next_id();
        self.notebooks.push(Notebook::new(id, name));
        id
    }

    /// Create a nested notebook under a parent.
    pub fn create_child_notebook(&mut self, name: &str, parent_id: NotebookId) -> NotebookId {
        let id = self.notebook_id_gen.next_id();
        self.notebooks
            .push(Notebook::with_parent(id, name, parent_id));
        id
    }

    /// Find a notebook by ID.
    pub fn find_notebook(&self, id: NotebookId) -> Option<&Notebook> {
        self.notebooks.iter().find(|nb| nb.id == id)
    }

    /// Find a notebook by ID (mutable).
    pub fn find_notebook_mut(&mut self, id: NotebookId) -> Option<&mut Notebook> {
        self.notebooks.iter_mut().find(|nb| nb.id == id)
    }

    /// Get direct children notebooks of a parent.
    pub fn child_notebooks(&self, parent_id: NotebookId) -> Vec<&Notebook> {
        self.notebooks
            .iter()
            .filter(|nb| nb.parent_id == Some(parent_id))
            .collect()
    }

    /// Get top-level notebooks (no parent).
    pub fn root_notebooks(&self) -> Vec<&Notebook> {
        self.notebooks
            .iter()
            .filter(|nb| nb.parent_id.is_none())
            .collect()
    }

    /// Rename a notebook.
    pub fn rename_notebook(&mut self, id: NotebookId, new_name: &str) -> bool {
        if let Some(nb) = self.find_notebook_mut(id) {
            nb.name = new_name.to_owned();
            true
        } else {
            false
        }
    }

    /// Delete a notebook and all its notes (and child notebooks recursively).
    pub fn delete_notebook(&mut self, id: NotebookId) -> bool {
        if !self.notebooks.iter().any(|nb| nb.id == id) {
            return false;
        }
        // Collect all descendant notebook IDs.
        let mut to_delete = vec![id];
        let mut frontier = vec![id];
        while let Some(current) = frontier.pop() {
            for nb in &self.notebooks {
                if nb.parent_id == Some(current) && !to_delete.contains(&nb.id) {
                    to_delete.push(nb.id);
                    frontier.push(nb.id);
                }
            }
        }
        // Remove notes in those notebooks.
        self.notes.retain(|n| !to_delete.contains(&n.notebook_id));
        // Remove the notebooks themselves.
        self.notebooks.retain(|nb| !to_delete.contains(&nb.id));
        // Clear selection if it was in a deleted notebook.
        if let Some(sel) = self.selected_notebook
            && to_delete.contains(&sel)
        {
            self.selected_notebook = None;
        }
        true
    }

    // -----------------------------------------------------------------------
    // Note management
    // -----------------------------------------------------------------------

    /// Create a new note in the given notebook.
    pub fn create_note(&mut self, title: &str, notebook_id: NotebookId) -> NoteId {
        let id = self.note_id_gen.next_id();
        let ts = self.tick();
        let mut note = Note::new(id, title, notebook_id);
        note.created_at = ts;
        note.modified_at = ts;
        self.notes.push(note);
        id
    }

    /// Create a note from a template.
    pub fn create_note_from_template(
        &mut self,
        template: NoteTemplate,
        notebook_id: NotebookId,
    ) -> NoteId {
        let id = self.note_id_gen.next_id();
        let ts = self.tick();
        let mut note = Note::new(id, template.label(), notebook_id);
        note.kind = template.kind();
        note.content = template.content().to_owned();
        note.created_at = ts;
        note.modified_at = ts;
        // For to-do template, pre-populate some items.
        if template == NoteTemplate::TodoList {
            note.add_checklist_item("Task 1");
            note.add_checklist_item("Task 2");
            note.add_checklist_item("Task 3");
        }
        self.notes.push(note);
        id
    }

    /// Find a note by ID.
    pub fn find_note(&self, id: NoteId) -> Option<&Note> {
        self.notes.iter().find(|n| n.id == id)
    }

    /// Find a note by ID (mutable).
    pub fn find_note_mut(&mut self, id: NoteId) -> Option<&mut Note> {
        self.notes.iter_mut().find(|n| n.id == id)
    }

    /// Find a note by title (for wiki linking).
    pub fn find_note_by_title(&self, title: &str) -> Option<&Note> {
        let lower = title.to_lowercase();
        self.notes.iter().find(|n| n.title.to_lowercase() == lower)
    }

    /// Update a note's content.
    pub fn update_note_content(&mut self, id: NoteId, new_content: &str) -> bool {
        let ts = self.tick();
        if let Some(note) = self.find_note_mut(id) {
            note.set_content(new_content, ts);
            true
        } else {
            false
        }
    }

    /// Update a note's title.
    pub fn update_note_title(&mut self, id: NoteId, new_title: &str) -> bool {
        let ts = self.tick();
        if let Some(note) = self.find_note_mut(id) {
            note.title = new_title.to_owned();
            note.modified_at = ts;
            true
        } else {
            false
        }
    }

    /// Delete a note by ID.
    pub fn delete_note(&mut self, id: NoteId) -> bool {
        let len_before = self.notes.len();
        self.notes.retain(|n| n.id != id);
        let deleted = self.notes.len() < len_before;
        if deleted && self.selected_note == Some(id) {
            self.selected_note = None;
        }
        deleted
    }

    /// Move a note to a different notebook.
    pub fn move_note(&mut self, note_id: NoteId, new_notebook_id: NotebookId) -> bool {
        if !self.notebooks.iter().any(|nb| nb.id == new_notebook_id) {
            return false;
        }
        if let Some(note) = self.find_note_mut(note_id) {
            note.notebook_id = new_notebook_id;
            true
        } else {
            false
        }
    }

    // -----------------------------------------------------------------------
    // Tagging
    // -----------------------------------------------------------------------

    /// Add a tag to a note.
    pub fn add_tag_to_note(&mut self, note_id: NoteId, tag: &str) -> bool {
        if let Some(note) = self.find_note_mut(note_id) {
            note.add_tag(tag);
            true
        } else {
            false
        }
    }

    /// Remove a tag from a note.
    pub fn remove_tag_from_note(&mut self, note_id: NoteId, tag: &str) -> bool {
        if let Some(note) = self.find_note_mut(note_id) {
            note.remove_tag(tag)
        } else {
            false
        }
    }

    /// Get all unique tags across all notes.
    pub fn all_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = self
            .notes
            .iter()
            .flat_map(|n| n.tags.iter().cloned())
            .collect();
        tags.sort();
        tags.dedup();
        tags
    }

    /// Set the active tag filter.
    pub fn set_tag_filter(&mut self, tag: Option<&str>) {
        self.active_tag_filter = tag.map(str::to_owned);
    }

    // -----------------------------------------------------------------------
    // Search
    // -----------------------------------------------------------------------

    /// Full-text search across all notes. Returns matching note IDs.
    pub fn search(&self, query: &str) -> Vec<NoteId> {
        if query.is_empty() {
            return self.notes.iter().map(|n| n.id).collect();
        }
        self.notes
            .iter()
            .filter(|n| n.matches_search(query))
            .map(|n| n.id)
            .collect()
    }

    /// Set the search query.
    pub fn set_search_query(&mut self, query: &str) {
        self.search_query = query.to_owned();
    }

    // -----------------------------------------------------------------------
    // Sorting and filtering
    // -----------------------------------------------------------------------

    /// Get a filtered and sorted list of note IDs for the current view.
    pub fn visible_notes(&self) -> Vec<NoteId> {
        let mut notes: Vec<&Note> = self.notes.iter().collect();

        // Filter by selected notebook.
        if let Some(nb_id) = self.selected_notebook {
            // Include notes from the selected notebook and its children.
            let descendant_ids = self.notebook_descendant_ids(nb_id);
            notes.retain(|n| descendant_ids.contains(&n.notebook_id));
        }

        // Filter by tag.
        if let Some(ref tag) = self.active_tag_filter {
            notes.retain(|n| n.has_tag(tag));
        }

        // Filter by search query.
        if !self.search_query.is_empty() {
            notes.retain(|n| n.matches_search(&self.search_query));
        }

        // Filter favorites only.
        if self.show_favorites_only {
            notes.retain(|n| n.favorited);
        }

        // Sort
        match self.sort_order {
            SortOrder::DateModified => notes.sort_by_key(|n| core::cmp::Reverse(n.modified_at)),
            SortOrder::DateCreated => notes.sort_by_key(|n| core::cmp::Reverse(n.created_at)),
            SortOrder::Title => notes.sort_by_key(|a| a.title.to_lowercase()),
            SortOrder::Notebook => notes.sort_by_key(|a| a.notebook_id),
        }

        // Pinned notes always first.
        notes.sort_by_key(|n| core::cmp::Reverse(n.pinned));

        notes.iter().map(|n| n.id).collect()
    }

    /// Get all notebook IDs that are descendants of (or equal to) the given ID.
    fn notebook_descendant_ids(&self, root_id: NotebookId) -> Vec<NotebookId> {
        let mut result = vec![root_id];
        let mut frontier = vec![root_id];
        while let Some(current) = frontier.pop() {
            for nb in &self.notebooks {
                if nb.parent_id == Some(current) && !result.contains(&nb.id) {
                    result.push(nb.id);
                    frontier.push(nb.id);
                }
            }
        }
        result
    }

    /// Cycle to the next sort order.
    pub fn cycle_sort(&mut self) {
        self.sort_order = self.sort_order.next();
    }

    /// Toggle favorites-only filter.
    pub fn toggle_favorites_filter(&mut self) {
        self.show_favorites_only = !self.show_favorites_only;
    }

    // -----------------------------------------------------------------------
    // Wiki linking
    // -----------------------------------------------------------------------

    /// Resolve wiki links in a note, returning (`link_text`, `target_note_id`) pairs.
    pub fn resolve_links(&self, note_id: NoteId) -> Vec<(String, Option<NoteId>)> {
        let links = if let Some(note) = self.find_note(note_id) {
            note.extract_links()
        } else {
            return Vec::new();
        };
        links
            .into_iter()
            .map(|link_text| {
                let target = self.find_note_by_title(&link_text).map(|n| n.id);
                (link_text, target)
            })
            .collect()
    }

    /// Build a backlinks map: for each note, which notes link to it.
    pub fn build_backlinks(&self) -> HashMap<NoteId, Vec<NoteId>> {
        let mut map: HashMap<NoteId, Vec<NoteId>> = HashMap::new();
        for note in &self.notes {
            let links = note.extract_links();
            for link_text in &links {
                if let Some(target) = self.find_note_by_title(link_text) {
                    map.entry(target.id).or_default().push(note.id);
                }
            }
        }
        map
    }

    // -----------------------------------------------------------------------
    // Statistics
    // -----------------------------------------------------------------------

    /// Get total note count.
    pub fn total_notes(&self) -> usize {
        self.notes.len()
    }

    /// Get total notebook count.
    pub fn total_notebooks(&self) -> usize {
        self.notebooks.len()
    }

    /// Get statistics for the selected note.
    pub fn selected_note_stats(&self) -> Option<NoteStats> {
        let note = self.find_note(self.selected_note?)?;
        Some(NoteStats {
            word_count: note.word_count(),
            char_count: note.char_count(),
            reading_time_min: note.reading_time_minutes(),
            version_count: note.versions.len(),
            tag_count: note.tags.len(),
            link_count: note.extract_links().len(),
        })
    }

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

    /// Render the full application frame, returning drawing commands.
    pub fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Full window background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Toolbar
        self.render_toolbar(&mut cmds, width);

        // Status bar
        self.render_status_bar(&mut cmds, width, height);

        // Content area (below toolbar, above status bar)
        let content_y = TOOLBAR_HEIGHT;
        let content_h = height - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT;

        // Notebook sidebar
        self.render_notebook_sidebar(&mut cmds, content_y, content_h);

        // Note list panel
        let list_x = SIDEBAR_WIDTH;
        self.render_note_list(&mut cmds, list_x, content_y, content_h);

        // Editor / preview area
        let editor_x = SIDEBAR_WIDTH + NOTE_LIST_WIDTH;
        let editor_w = width - editor_x;
        self.render_editor_area(&mut cmds, editor_x, content_y, editor_w, content_h);

        cmds
    }

    fn render_toolbar(&self, cmds: &mut Vec<RenderCommand>, width: f32) {
        // Toolbar background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height: TOOLBAR_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // App title
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: 10.0,
            text: "Notes & Wiki".to_owned(),
            color: BLUE,
            font_size: 15.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(140.0),
        });

        // Sort button
        let sort_label = format!("Sort: {}", self.sort_order.label());
        cmds.push(RenderCommand::FillRect {
            x: 160.0,
            y: 6.0,
            width: 100.0,
            height: 24.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: 168.0,
            y: 12.0,
            text: sort_label,
            color: TEXT,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(90.0),
        });

        // Favorites toggle
        let fav_color = if self.show_favorites_only {
            YELLOW
        } else {
            OVERLAY0
        };
        cmds.push(RenderCommand::FillRect {
            x: 270.0,
            y: 6.0,
            width: 24.0,
            height: 24.0,
            color: if self.show_favorites_only {
                SURFACE1
            } else {
                SURFACE0
            },
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: 276.0,
            y: 12.0,
            text: "*".to_owned(),
            color: fav_color,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Search box
        let search_x = 310.0;
        let search_w = 200.0;
        cmds.push(RenderCommand::FillRect {
            x: search_x,
            y: 6.0,
            width: search_w,
            height: 24.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        let search_text = if self.search_query.is_empty() {
            "Search notes...".to_owned()
        } else {
            self.search_query.clone()
        };
        let search_color = if self.search_query.is_empty() {
            OVERLAY0
        } else {
            TEXT
        };
        cmds.push(RenderCommand::Text {
            x: search_x + 8.0,
            y: 12.0,
            text: search_text,
            color: search_color,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(search_w - 16.0),
        });

        // Template buttons area
        let tmpl_x = 530.0;
        cmds.push(RenderCommand::Text {
            x: tmpl_x,
            y: 12.0,
            text: "New:".to_owned(),
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Show a few template buttons
        let template_labels = ["Blank", "Meeting", "Todo", "Journal"];
        let template_colors = [OVERLAY0, TEAL, GREEN, MAUVE];
        let mut tx = tmpl_x + 35.0;
        for (i, label) in template_labels.iter().enumerate() {
            let btn_w = label.len() as f32 * 7.0 + 16.0;
            let color = template_colors.get(i).copied().unwrap_or(OVERLAY0);
            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: 6.0,
                width: btn_w,
                height: 24.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });
            cmds.push(RenderCommand::Text {
                x: tx + 8.0,
                y: 12.0,
                text: (*label).to_owned(),
                color,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(btn_w - 12.0),
            });
            tx += btn_w + 4.0;
        }

        // Separator line below toolbar
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: TOOLBAR_HEIGHT,
            x2: width,
            y2: TOOLBAR_HEIGHT,
            color: SURFACE0,
            width: 1.0,
        });
    }

    fn render_status_bar(&self, cmds: &mut Vec<RenderCommand>, width: f32, height: f32) {
        let bar_y = height - STATUS_BAR_HEIGHT;

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: bar_y,
            width,
            height: STATUS_BAR_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // Separator
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: bar_y,
            x2: width,
            y2: bar_y,
            color: SURFACE0,
            width: 1.0,
        });

        // Note count
        let count_text = format!(
            "{} notebooks, {} notes",
            self.total_notebooks(),
            self.total_notes()
        );
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: bar_y + 6.0,
            text: count_text,
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
        });

        // Selected note stats
        if let Some(stats) = self.selected_note_stats() {
            let stats_text = format!(
                "{} words | {} chars | {:.1} min read | {} versions | {} tags | {} links",
                stats.word_count,
                stats.char_count,
                stats.reading_time_min,
                stats.version_count,
                stats.tag_count,
                stats.link_count,
            );
            cmds.push(RenderCommand::Text {
                x: 250.0,
                y: bar_y + 6.0,
                text: stats_text,
                color: SUBTEXT0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 270.0),
            });
        }

        // Sort order indicator
        cmds.push(RenderCommand::Text {
            x: width - 120.0,
            y: bar_y + 6.0,
            text: format!("Sort: {}", self.sort_order.label()),
            color: OVERLAY0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(110.0),
        });
    }

    fn render_notebook_sidebar(
        &self,
        cmds: &mut Vec<RenderCommand>,
        content_y: f32,
        content_h: f32,
    ) {
        // Sidebar background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: content_y,
            width: SIDEBAR_WIDTH,
            height: content_h,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Header
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: content_y,
            width: SIDEBAR_WIDTH,
            height: HEADER_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: content_y + 9.0,
            text: "Notebooks".to_owned(),
            color: LAVENDER,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(SIDEBAR_WIDTH - 20.0),
        });

        // Separator
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: content_y + HEADER_HEIGHT,
            x2: SIDEBAR_WIDTH,
            y2: content_y + HEADER_HEIGHT,
            color: SURFACE0,
            width: 1.0,
        });

        // Render notebook tree
        let mut y = content_y + HEADER_HEIGHT + 4.0;
        let root_nbs = self.root_notebooks();
        for nb in &root_nbs {
            self.render_notebook_item(cmds, nb, 0, &mut y);
        }

        // Tags section
        let tags = self.all_tags();
        if !tags.is_empty() {
            y += 8.0;
            cmds.push(RenderCommand::Line {
                x1: 8.0,
                y1: y,
                x2: SIDEBAR_WIDTH - 8.0,
                y2: y,
                color: SURFACE0,
                width: 1.0,
            });
            y += 8.0;
            cmds.push(RenderCommand::Text {
                x: 12.0,
                y,
                text: "Tags".to_owned(),
                color: LAVENDER,
                font_size: 12.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(SIDEBAR_WIDTH - 20.0),
            });
            y += 20.0;

            let mut tag_x: f32 = 8.0;
            for tag in &tags {
                let tag_w = tag.len() as f32 * 6.5 + TAG_PADDING * 2.0;
                if tag_x + tag_w > SIDEBAR_WIDTH - 4.0 {
                    tag_x = 8.0;
                    y += TAG_HEIGHT + 4.0;
                }
                let is_active = self.active_tag_filter.as_deref() == Some(tag.as_str());
                let bg = if is_active { BLUE } else { SURFACE0 };
                let fg = if is_active { CRUST } else { SUBTEXT1 };
                cmds.push(RenderCommand::FillRect {
                    x: tag_x,
                    y,
                    width: tag_w,
                    height: TAG_HEIGHT,
                    color: bg,
                    corner_radii: CornerRadii::all(10.0),
                });
                cmds.push(RenderCommand::Text {
                    x: tag_x + TAG_PADDING,
                    y: y + 4.0,
                    text: tag.clone(),
                    color: fg,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(tag_w - TAG_PADDING),
                });
                tag_x += tag_w + 4.0;
            }
        }

        // Right border
        cmds.push(RenderCommand::Line {
            x1: SIDEBAR_WIDTH,
            y1: content_y,
            x2: SIDEBAR_WIDTH,
            y2: content_y + content_h,
            color: SURFACE0,
            width: 1.0,
        });
    }

    fn render_notebook_item(
        &self,
        cmds: &mut Vec<RenderCommand>,
        nb: &Notebook,
        depth: u32,
        y: &mut f32,
    ) {
        let indent = depth.saturating_mul(16) as f32;
        let is_selected = self.selected_notebook == Some(nb.id);

        // Highlight selected
        if is_selected {
            cmds.push(RenderCommand::FillRect {
                x: 0.0,
                y: *y,
                width: SIDEBAR_WIDTH,
                height: ITEM_HEIGHT,
                color: SURFACE0,
                corner_radii: CornerRadii::ZERO,
            });
        }

        // Expand/collapse indicator for notebooks with children
        let has_children = self.notebooks.iter().any(|c| c.parent_id == Some(nb.id));
        if has_children {
            let arrow = if nb.expanded { "v" } else { ">" };
            cmds.push(RenderCommand::Text {
                x: 4.0 + indent,
                y: *y + 7.0,
                text: arrow.to_owned(),
                color: OVERLAY0,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Notebook name
        let name_color = if is_selected { TEXT } else { SUBTEXT1 };
        let note_count = self.notes.iter().filter(|n| n.notebook_id == nb.id).count();
        cmds.push(RenderCommand::Text {
            x: 18.0 + indent,
            y: *y + 7.0,
            text: format!("{} ({})", nb.name, note_count),
            color: name_color,
            font_size: 12.0,
            font_weight: if is_selected {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            },
            max_width: Some(SIDEBAR_WIDTH - 24.0 - indent),
        });

        *y += ITEM_HEIGHT;

        // Render children if expanded
        if nb.expanded && has_children {
            let children = self.child_notebooks(nb.id);
            for child in children {
                self.render_notebook_item(cmds, child, depth.saturating_add(1), y);
            }
        }
    }

    fn render_note_list(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        content_y: f32,
        content_h: f32,
    ) {
        // List panel background
        cmds.push(RenderCommand::FillRect {
            x,
            y: content_y,
            width: NOTE_LIST_WIDTH,
            height: content_h,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Header
        cmds.push(RenderCommand::FillRect {
            x,
            y: content_y,
            width: NOTE_LIST_WIDTH,
            height: HEADER_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: content_y + 9.0,
            text: "Notes".to_owned(),
            color: LAVENDER,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(NOTE_LIST_WIDTH - 20.0),
        });

        cmds.push(RenderCommand::Line {
            x1: x,
            y1: content_y + HEADER_HEIGHT,
            x2: x + NOTE_LIST_WIDTH,
            y2: content_y + HEADER_HEIGHT,
            color: SURFACE0,
            width: 1.0,
        });

        // Note items
        let visible = self.visible_notes();
        let mut iy = content_y + HEADER_HEIGHT + 2.0;
        let item_h = 52.0; // taller items to show subtitle

        for nid in &visible {
            if iy + item_h > content_y + content_h {
                break;
            }
            if let Some(note) = self.find_note(*nid) {
                let is_selected = self.selected_note == Some(*nid);

                // Background for selected
                if is_selected {
                    cmds.push(RenderCommand::FillRect {
                        x,
                        y: iy,
                        width: NOTE_LIST_WIDTH,
                        height: item_h,
                        color: SURFACE0,
                        corner_radii: CornerRadii::ZERO,
                    });
                    // Blue accent bar
                    cmds.push(RenderCommand::FillRect {
                        x,
                        y: iy,
                        width: 3.0,
                        height: item_h,
                        color: BLUE,
                        corner_radii: CornerRadii::ZERO,
                    });
                }

                // Pin indicator
                if note.pinned {
                    cmds.push(RenderCommand::Text {
                        x: x + NOTE_LIST_WIDTH - 20.0,
                        y: iy + 6.0,
                        text: "P".to_owned(),
                        color: PEACH,
                        font_size: 10.0,
                        font_weight: FontWeightHint::Bold,
                        max_width: None,
                    });
                }

                // Favorite indicator
                if note.favorited {
                    cmds.push(RenderCommand::Text {
                        x: x + NOTE_LIST_WIDTH - 34.0,
                        y: iy + 6.0,
                        text: "*".to_owned(),
                        color: YELLOW,
                        font_size: 12.0,
                        font_weight: FontWeightHint::Bold,
                        max_width: None,
                    });
                }

                // Title
                let title_color = if is_selected { TEXT } else { SUBTEXT1 };
                cmds.push(RenderCommand::Text {
                    x: x + 12.0,
                    y: iy + 6.0,
                    text: note.title.clone(),
                    color: title_color,
                    font_size: 13.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(NOTE_LIST_WIDTH - 50.0),
                });

                // Kind badge
                let kind_color = match &note.kind {
                    NoteKind::PlainText => OVERLAY0,
                    NoteKind::Markdown => BLUE,
                    NoteKind::Checklist => GREEN,
                    NoteKind::Table => TEAL,
                };
                cmds.push(RenderCommand::Text {
                    x: x + 12.0,
                    y: iy + 22.0,
                    text: note.kind.label().to_owned(),
                    color: kind_color,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(80.0),
                });

                // Preview snippet
                let snippet = if note.kind == NoteKind::Checklist {
                    let total = note.checklist.len();
                    let done = note.checklist.iter().filter(|i| i.checked).count();
                    format!("{done}/{total} completed")
                } else {
                    let preview = note.content.chars().take(40).collect::<String>();
                    preview.replace('\n', " ")
                };
                cmds.push(RenderCommand::Text {
                    x: x + 80.0,
                    y: iy + 22.0,
                    text: snippet,
                    color: OVERLAY0,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(NOTE_LIST_WIDTH - 92.0),
                });

                // Tags row
                if !note.tags.is_empty() {
                    let tags_str = note.tags.join(", ");
                    cmds.push(RenderCommand::Text {
                        x: x + 12.0,
                        y: iy + 36.0,
                        text: tags_str,
                        color: MAUVE,
                        font_size: 9.0,
                        font_weight: FontWeightHint::Light,
                        max_width: Some(NOTE_LIST_WIDTH - 24.0),
                    });
                }

                // Separator
                cmds.push(RenderCommand::Line {
                    x1: x + 8.0,
                    y1: iy + item_h,
                    x2: x + NOTE_LIST_WIDTH - 8.0,
                    y2: iy + item_h,
                    color: SURFACE0,
                    width: 1.0,
                });

                iy += item_h;
            }
        }

        // Right border
        cmds.push(RenderCommand::Line {
            x1: x + NOTE_LIST_WIDTH,
            y1: content_y,
            x2: x + NOTE_LIST_WIDTH,
            y2: content_y + content_h,
            color: SURFACE0,
            width: 1.0,
        });
    }

    fn render_editor_area(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        // Editor background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        let note = if let Some(n) = self.selected_note.and_then(|id| self.find_note(id)) {
            n
        } else {
            // Empty state message
            cmds.push(RenderCommand::Text {
                x: x + width / 2.0 - 80.0,
                y: y + height / 2.0 - 10.0,
                text: "Select a note to edit".to_owned(),
                color: OVERLAY0,
                font_size: 16.0,
                font_weight: FontWeightHint::Light,
                max_width: Some(200.0),
            });
            return;
        };

        // Note title header
        let title_y = y;
        cmds.push(RenderCommand::FillRect {
            x,
            y: title_y,
            width,
            height: HEADER_HEIGHT + 8.0,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });
        cmds.push(RenderCommand::Text {
            x: x + EDITOR_PADDING,
            y: title_y + 10.0,
            text: note.title.clone(),
            color: TEXT,
            font_size: 18.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - EDITOR_PADDING * 2.0),
        });

        // Note metadata line
        let meta_y = title_y + HEADER_HEIGHT + 8.0;
        cmds.push(RenderCommand::FillRect {
            x,
            y: meta_y,
            width,
            height: 20.0,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        let nb_name = self
            .find_notebook(note.notebook_id)
            .map_or("Unknown", |nb| nb.name.as_str());
        let meta_text = format!(
            "{} | {} | Modified: {}",
            note.kind.label(),
            nb_name,
            note.modified_at
        );
        cmds.push(RenderCommand::Text {
            x: x + EDITOR_PADDING,
            y: meta_y + 4.0,
            text: meta_text,
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - EDITOR_PADDING * 2.0),
        });

        // Tags bar
        let tags_y = meta_y + 20.0;
        if !note.tags.is_empty() {
            let mut tx = x + EDITOR_PADDING;
            for tag in &note.tags {
                let tw = tag.len() as f32 * 6.5 + 16.0;
                cmds.push(RenderCommand::FillRect {
                    x: tx,
                    y: tags_y + 2.0,
                    width: tw,
                    height: 18.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(9.0),
                });
                cmds.push(RenderCommand::Text {
                    x: tx + 8.0,
                    y: tags_y + 5.0,
                    text: tag.clone(),
                    color: MAUVE,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(tw - 10.0),
                });
                tx += tw + 4.0;
            }
        }

        // Separator
        let sep_y = tags_y + 24.0;
        cmds.push(RenderCommand::Line {
            x1: x,
            y1: sep_y,
            x2: x + width,
            y2: sep_y,
            color: SURFACE0,
            width: 1.0,
        });

        // Content area
        let editor_y = sep_y + 2.0;
        let editor_h = y + height - editor_y;

        match &note.kind {
            NoteKind::Checklist => {
                self.render_checklist(cmds, note, x, editor_y, width, editor_h);
            }
            NoteKind::Table => {
                self.render_table_view(cmds, note, x, editor_y, width, editor_h);
            }
            NoteKind::Markdown => {
                self.render_markdown_preview(cmds, note, x, editor_y, width, editor_h);
            }
            NoteKind::PlainText => {
                self.render_plain_text(cmds, note, x, editor_y, width, editor_h);
            }
        }

        // Version history panel on the right edge
        if !note.versions.is_empty() {
            self.render_version_sidebar(cmds, note, x + width - 160.0, editor_y, 160.0, editor_h);
        }
    }

    fn render_plain_text(
        &self,
        cmds: &mut Vec<RenderCommand>,
        note: &Note,
        x: f32,
        y: f32,
        width: f32,
        _height: f32,
    ) {
        let mut ly = y + EDITOR_PADDING;
        for line in note.content.lines() {
            cmds.push(RenderCommand::Text {
                x: x + EDITOR_PADDING,
                y: ly,
                text: line.to_owned(),
                color: TEXT,
                font_size: 14.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - EDITOR_PADDING * 2.0),
            });
            ly += LINE_HEIGHT;
        }
    }

    fn render_checklist(
        &self,
        cmds: &mut Vec<RenderCommand>,
        note: &Note,
        x: f32,
        y: f32,
        width: f32,
        _height: f32,
    ) {
        let mut iy = y + EDITOR_PADDING;
        for item in &note.checklist {
            // Checkbox box
            let box_color = if item.checked { GREEN } else { SURFACE1 };
            cmds.push(RenderCommand::StrokeRect {
                x: x + EDITOR_PADDING,
                y: iy,
                width: 16.0,
                height: 16.0,
                color: box_color,
                line_width: 2.0,
                corner_radii: CornerRadii::all(3.0),
            });
            // Check mark
            if item.checked {
                cmds.push(RenderCommand::FillRect {
                    x: x + EDITOR_PADDING + 3.0,
                    y: iy + 3.0,
                    width: 10.0,
                    height: 10.0,
                    color: GREEN,
                    corner_radii: CornerRadii::all(2.0),
                });
            }
            // Text
            let text_color = if item.checked { OVERLAY0 } else { TEXT };
            cmds.push(RenderCommand::Text {
                x: x + EDITOR_PADDING + 24.0,
                y: iy + 1.0,
                text: item.text.clone(),
                color: text_color,
                font_size: 14.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - EDITOR_PADDING * 2.0 - 30.0),
            });
            iy += LINE_HEIGHT + 4.0;
        }
    }

    fn render_table_view(
        &self,
        cmds: &mut Vec<RenderCommand>,
        note: &Note,
        x: f32,
        y: f32,
        width: f32,
        _height: f32,
    ) {
        let table = match &note.table {
            Some(t) => t,
            None => return,
        };

        let col_count = table.column_count();
        if col_count == 0 {
            return;
        }
        let usable_w = width - EDITOR_PADDING * 2.0;
        let col_w = usable_w / col_count as f32;

        // Header row
        let header_y = y + EDITOR_PADDING;
        cmds.push(RenderCommand::FillRect {
            x: x + EDITOR_PADDING,
            y: header_y,
            width: usable_w,
            height: LINE_HEIGHT + 4.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        for (ci, header) in table.headers.iter().enumerate() {
            cmds.push(RenderCommand::Text {
                x: x + EDITOR_PADDING + ci as f32 * col_w + 8.0,
                y: header_y + 4.0,
                text: header.clone(),
                color: BLUE,
                font_size: 13.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(col_w - 16.0),
            });
        }

        // Data rows
        let mut ry = header_y + LINE_HEIGHT + 8.0;
        for (ri, row) in table.rows.iter().enumerate() {
            let row_bg = if ri % 2 == 0 { BASE } else { MANTLE };
            cmds.push(RenderCommand::FillRect {
                x: x + EDITOR_PADDING,
                y: ry,
                width: usable_w,
                height: LINE_HEIGHT + 4.0,
                color: row_bg,
                corner_radii: CornerRadii::ZERO,
            });
            for (ci, _) in table.headers.iter().enumerate() {
                let cell = row.get(ci).map_or("", String::as_str);
                cmds.push(RenderCommand::Text {
                    x: x + EDITOR_PADDING + ci as f32 * col_w + 8.0,
                    y: ry + 4.0,
                    text: cell.to_owned(),
                    color: TEXT,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(col_w - 16.0),
                });
            }
            ry += LINE_HEIGHT + 4.0;
        }

        // Grid lines
        for ci in 0..=col_count {
            let lx = x + EDITOR_PADDING + ci as f32 * col_w;
            cmds.push(RenderCommand::Line {
                x1: lx,
                y1: header_y,
                x2: lx,
                y2: ry,
                color: SURFACE1,
                width: 1.0,
            });
        }
    }

    fn render_markdown_preview(
        &self,
        cmds: &mut Vec<RenderCommand>,
        note: &Note,
        x: f32,
        y: f32,
        width: f32,
        _height: f32,
    ) {
        let blocks = parse_markdown(&note.content);
        let mut ly = y + EDITOR_PADDING;
        let max_w = width - EDITOR_PADDING * 2.0 - 170.0; // reserve space for version sidebar

        for block in &blocks {
            match block {
                MdBlock::Heading { level, text } => {
                    let (font_size, color) = match level {
                        1 => (22.0, BLUE),
                        2 => (18.0, LAVENDER),
                        3 => (16.0, MAUVE),
                        _ => (14.0, TEAL),
                    };
                    cmds.push(RenderCommand::Text {
                        x: x + EDITOR_PADDING,
                        y: ly,
                        text: text.clone(),
                        color,
                        font_size,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(max_w),
                    });
                    ly += font_size + 8.0;
                }
                MdBlock::Paragraph { spans } => {
                    for span in spans {
                        let (text, color, weight) = match span {
                            MdSpan::Plain(t) => (t.clone(), TEXT, FontWeightHint::Regular),
                            MdSpan::Bold(t) => (t.clone(), TEXT, FontWeightHint::Bold),
                            MdSpan::Italic(t) => (t.clone(), SUBTEXT1, FontWeightHint::Light),
                            MdSpan::BoldItalic(t) => (t.clone(), TEXT, FontWeightHint::Bold),
                            MdSpan::InlineCode(t) => (t.clone(), GREEN, FontWeightHint::Regular),
                            MdSpan::WikiLink(t) => {
                                (format!("[[{t}]]"), BLUE, FontWeightHint::Regular)
                            }
                        };
                        cmds.push(RenderCommand::Text {
                            x: x + EDITOR_PADDING,
                            y: ly,
                            text,
                            color,
                            font_size: 14.0,
                            font_weight: weight,
                            max_width: Some(max_w),
                        });
                        ly += LINE_HEIGHT;
                    }
                    ly += 4.0;
                }
                MdBlock::UnorderedList { items } => {
                    for item in items {
                        cmds.push(RenderCommand::Text {
                            x: x + EDITOR_PADDING + 16.0,
                            y: ly,
                            text: format!("* {item}"),
                            color: TEXT,
                            font_size: 14.0,
                            font_weight: FontWeightHint::Regular,
                            max_width: Some(max_w - 16.0),
                        });
                        ly += LINE_HEIGHT;
                    }
                    ly += 4.0;
                }
                MdBlock::OrderedList { items } => {
                    for (i, item) in items.iter().enumerate() {
                        cmds.push(RenderCommand::Text {
                            x: x + EDITOR_PADDING + 16.0,
                            y: ly,
                            text: format!("{}. {item}", i.saturating_add(1)),
                            color: TEXT,
                            font_size: 14.0,
                            font_weight: FontWeightHint::Regular,
                            max_width: Some(max_w - 16.0),
                        });
                        ly += LINE_HEIGHT;
                    }
                    ly += 4.0;
                }
                MdBlock::CodeBlock { language, code } => {
                    let block_h = code.lines().count() as f32 * LINE_HEIGHT + 16.0;
                    cmds.push(RenderCommand::FillRect {
                        x: x + EDITOR_PADDING,
                        y: ly,
                        width: max_w,
                        height: block_h,
                        color: SURFACE0,
                        corner_radii: CornerRadii::all(6.0),
                    });
                    if !language.is_empty() {
                        cmds.push(RenderCommand::Text {
                            x: x + EDITOR_PADDING + 8.0,
                            y: ly + 4.0,
                            text: language.clone(),
                            color: OVERLAY0,
                            font_size: 10.0,
                            font_weight: FontWeightHint::Light,
                            max_width: Some(max_w - 16.0),
                        });
                    }
                    let code_y_start = if language.is_empty() {
                        ly + 8.0
                    } else {
                        ly + 18.0
                    };
                    for (li, cl) in code.lines().enumerate() {
                        cmds.push(RenderCommand::Text {
                            x: x + EDITOR_PADDING + 12.0,
                            y: code_y_start + li as f32 * LINE_HEIGHT,
                            text: cl.to_owned(),
                            color: GREEN,
                            font_size: 13.0,
                            font_weight: FontWeightHint::Regular,
                            max_width: Some(max_w - 24.0),
                        });
                    }
                    ly += block_h + 8.0;
                }
                MdBlock::BlockQuote { text } => {
                    // Blue left bar
                    cmds.push(RenderCommand::FillRect {
                        x: x + EDITOR_PADDING,
                        y: ly,
                        width: 4.0,
                        height: LINE_HEIGHT,
                        color: BLUE,
                        corner_radii: CornerRadii::all(2.0),
                    });
                    cmds.push(RenderCommand::Text {
                        x: x + EDITOR_PADDING + 12.0,
                        y: ly,
                        text: text.clone(),
                        color: SUBTEXT0,
                        font_size: 14.0,
                        font_weight: FontWeightHint::Light,
                        max_width: Some(max_w - 16.0),
                    });
                    ly += LINE_HEIGHT + 8.0;
                }
                MdBlock::HorizontalRule => {
                    cmds.push(RenderCommand::Line {
                        x1: x + EDITOR_PADDING,
                        y1: ly + 8.0,
                        x2: x + EDITOR_PADDING + max_w,
                        y2: ly + 8.0,
                        color: SURFACE1,
                        width: 1.0,
                    });
                    ly += 20.0;
                }
            }
        }
    }

    fn render_version_sidebar(
        &self,
        cmds: &mut Vec<RenderCommand>,
        note: &Note,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        // Background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Left border
        cmds.push(RenderCommand::Line {
            x1: x,
            y1: y,
            x2: x,
            y2: y + height,
            color: SURFACE0,
            width: 1.0,
        });

        // Header
        cmds.push(RenderCommand::Text {
            x: x + 8.0,
            y: y + 8.0,
            text: "History".to_owned(),
            color: LAVENDER,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 16.0),
        });

        let mut vy = y + 28.0;
        let max_display = ((height - 30.0) / 24.0) as usize;
        let start = note.versions.len().saturating_sub(max_display);
        for (i, ver) in note.versions.iter().enumerate().skip(start) {
            let label = format!("v{} ({})", i.saturating_add(1), ver.timestamp);
            cmds.push(RenderCommand::Text {
                x: x + 8.0,
                y: vy,
                text: label,
                color: SUBTEXT0,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 16.0),
            });
            // Snippet
            let snippet: String = ver.content.chars().take(20).collect();
            cmds.push(RenderCommand::Text {
                x: x + 8.0,
                y: vy + 12.0,
                text: snippet,
                color: OVERLAY0,
                font_size: 9.0,
                font_weight: FontWeightHint::Light,
                max_width: Some(width - 16.0),
            });
            vy += 28.0;
        }
    }
}

// ============================================================================
// Note statistics
// ============================================================================

/// Statistics about a single note.
#[derive(Debug, Clone)]
pub struct NoteStats {
    pub word_count: usize,
    pub char_count: usize,
    pub reading_time_min: f32,
    pub version_count: usize,
    pub tag_count: usize,
    pub link_count: usize,
}

// ============================================================================
// Main entry point
// ============================================================================

fn main() {
    let mut app = NotesApp::new();

    // Create sample notebooks
    let personal = app.create_notebook("Personal");
    let work = app.create_notebook("Work");
    let projects = app.create_child_notebook("Projects", work);
    let _meetings = app.create_child_notebook("Meetings", work);
    let _journal = app.create_child_notebook("Journal", personal);

    // Create sample notes
    let n1 = app.create_note("Welcome to Notes", personal);
    app.update_note_content(
        n1,
        "# Welcome\n\nThis is the **Notes & Wiki** application.\n\n\
         You can create notes in different formats:\n\
         - Plain text\n- Markdown\n- Checklists\n- Tables\n\n\
         Try linking to other notes with [[Project Ideas]].",
    );
    if let Some(note) = app.find_note_mut(n1) {
        note.kind = NoteKind::Markdown;
        note.add_tag("intro");
        note.add_tag("wiki");
        note.favorited = true;
    }

    let n2 = app.create_note("Project Ideas", projects);
    app.update_note_content(
        n2,
        "# Project Ideas\n\n## Web Framework\nBuild a fast async web framework in Rust.\n\n\
         ## Game Engine\nA 2D game engine with ECS architecture.\n\n\
         See also: [[Welcome to Notes]]",
    );
    if let Some(note) = app.find_note_mut(n2) {
        note.kind = NoteKind::Markdown;
        note.add_tag("projects");
        note.add_tag("ideas");
        note.pinned = true;
    }

    // Create a checklist note
    let n3 = app.create_note_from_template(NoteTemplate::TodoList, work);
    if let Some(note) = app.find_note_mut(n3) {
        note.title = "Sprint Tasks".to_owned();
        note.checklist.clear();
        note.add_checklist_item("Review PR #42");
        note.add_checklist_item("Fix build pipeline");
        note.add_checklist_item("Write unit tests");
        note.add_checklist_item("Deploy to staging");
        note.toggle_checklist_item(0);
        note.add_tag("sprint");
        note.add_tag("work");
    }

    // Create a table note
    let n4 = app.create_note("API Endpoints", projects);
    if let Some(note) = app.find_note_mut(n4) {
        note.kind = NoteKind::Table;
        let mut table = TableData::new(vec![
            "Method".to_owned(),
            "Path".to_owned(),
            "Description".to_owned(),
        ]);
        table.add_row(vec![
            "GET".to_owned(),
            "/api/notes".to_owned(),
            "List all notes".to_owned(),
        ]);
        table.add_row(vec![
            "POST".to_owned(),
            "/api/notes".to_owned(),
            "Create a note".to_owned(),
        ]);
        table.add_row(vec![
            "PUT".to_owned(),
            "/api/notes/:id".to_owned(),
            "Update a note".to_owned(),
        ]);
        table.add_row(vec![
            "DELETE".to_owned(),
            "/api/notes/:id".to_owned(),
            "Delete a note".to_owned(),
        ]);
        note.table = Some(table);
        note.add_tag("api");
        note.add_tag("projects");
    }

    // Create from template
    let _n5 = app.create_note_from_template(NoteTemplate::MeetingNotes, _meetings);
    let _n6 = app.create_note_from_template(NoteTemplate::Journal, _journal);
    let _n7 = app.create_note_from_template(NoteTemplate::CodeSnippet, projects);

    // Select a note for display
    app.selected_notebook = Some(work);
    app.selected_note = Some(n2);

    // Render one frame
    let _commands = app.render(1280.0, 800.0);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    // -----------------------------------------------------------------------
    // Helper: create an app with a couple of notebooks and notes
    // -----------------------------------------------------------------------

    fn sample_app() -> NotesApp {
        let mut app = NotesApp::new();
        let nb1 = app.create_notebook("Personal");
        let nb2 = app.create_notebook("Work");
        let _child = app.create_child_notebook("Archive", nb1);

        let n1 = app.create_note("First Note", nb1);
        app.update_note_content(n1, "Hello world");
        app.add_tag_to_note(n1, "greeting");

        let n2 = app.create_note("Second Note", nb2);
        app.update_note_content(n2, "Some **bold** text and *italic* text.");
        if let Some(note) = app.find_note_mut(n2) {
            note.kind = NoteKind::Markdown;
        }
        app.add_tag_to_note(n2, "formatting");

        app
    }

    // -----------------------------------------------------------------------
    // Note CRUD
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_note() {
        let mut app = NotesApp::new();
        let nb = app.create_notebook("Test");
        let nid = app.create_note("My Note", nb);
        let note = app.find_note(nid).unwrap();
        assert_eq!(note.title, "My Note");
        assert_eq!(note.notebook_id, nb);
        assert!(note.content.is_empty());
    }

    #[test]
    fn test_update_note_content() {
        let mut app = NotesApp::new();
        let nb = app.create_notebook("NB");
        let nid = app.create_note("Note", nb);
        assert!(app.update_note_content(nid, "First version"));
        assert!(app.update_note_content(nid, "Second version"));
        let note = app.find_note(nid).unwrap();
        assert_eq!(note.content, "Second version");
        // Should have a version in history
        assert!(!note.versions.is_empty());
    }

    #[test]
    fn test_update_note_title() {
        let mut app = NotesApp::new();
        let nb = app.create_notebook("NB");
        let nid = app.create_note("Old Title", nb);
        assert!(app.update_note_title(nid, "New Title"));
        let note = app.find_note(nid).unwrap();
        assert_eq!(note.title, "New Title");
    }

    #[test]
    fn test_delete_note() {
        let mut app = NotesApp::new();
        let nb = app.create_notebook("NB");
        let nid = app.create_note("Doomed", nb);
        assert_eq!(app.total_notes(), 1);
        assert!(app.delete_note(nid));
        assert_eq!(app.total_notes(), 0);
        assert!(app.find_note(nid).is_none());
    }

    #[test]
    fn test_delete_nonexistent_note() {
        let mut app = NotesApp::new();
        assert!(!app.delete_note(9999));
    }

    #[test]
    fn test_move_note() {
        let mut app = NotesApp::new();
        let nb1 = app.create_notebook("NB1");
        let nb2 = app.create_notebook("NB2");
        let nid = app.create_note("Movable", nb1);
        assert!(app.move_note(nid, nb2));
        assert_eq!(app.find_note(nid).unwrap().notebook_id, nb2);
    }

    #[test]
    fn test_move_note_to_nonexistent_notebook() {
        let mut app = NotesApp::new();
        let nb = app.create_notebook("NB");
        let nid = app.create_note("Note", nb);
        assert!(!app.move_note(nid, 9999));
    }

    // -----------------------------------------------------------------------
    // Notebook management
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_nested_notebooks() {
        let mut app = NotesApp::new();
        let parent = app.create_notebook("Parent");
        let child = app.create_child_notebook("Child", parent);
        let grandchild = app.create_child_notebook("Grandchild", child);

        assert_eq!(app.root_notebooks().len(), 1);
        assert_eq!(app.child_notebooks(parent).len(), 1);
        assert_eq!(app.child_notebooks(child).len(), 1);
        assert!(app.child_notebooks(grandchild).is_empty());
    }

    #[test]
    fn test_delete_notebook_cascades() {
        let mut app = NotesApp::new();
        let parent = app.create_notebook("Parent");
        let child = app.create_child_notebook("Child", parent);
        app.create_note("Note1", parent);
        app.create_note("Note2", child);

        assert_eq!(app.total_notes(), 2);
        assert!(app.delete_notebook(parent));
        assert_eq!(app.total_notes(), 0);
        assert_eq!(app.total_notebooks(), 0);
    }

    #[test]
    fn test_rename_notebook() {
        let mut app = NotesApp::new();
        let nb = app.create_notebook("Old");
        assert!(app.rename_notebook(nb, "New Name"));
        assert_eq!(app.find_notebook(nb).unwrap().name, "New Name");
    }

    // -----------------------------------------------------------------------
    // Tagging
    // -----------------------------------------------------------------------

    #[test]
    fn test_add_and_remove_tags() {
        let mut app = NotesApp::new();
        let nb = app.create_notebook("NB");
        let nid = app.create_note("Tagged", nb);
        app.add_tag_to_note(nid, "alpha");
        app.add_tag_to_note(nid, "beta");
        app.add_tag_to_note(nid, "alpha"); // duplicate, should not add

        let note = app.find_note(nid).unwrap();
        assert_eq!(note.tags.len(), 2);
        assert!(note.has_tag("alpha"));
        assert!(note.has_tag("beta"));

        assert!(app.remove_tag_from_note(nid, "alpha"));
        let note = app.find_note(nid).unwrap();
        assert_eq!(note.tags.len(), 1);
        assert!(!note.has_tag("alpha"));
    }

    #[test]
    fn test_all_tags() {
        let app = sample_app();
        let tags = app.all_tags();
        assert!(tags.contains(&"greeting".to_owned()));
        assert!(tags.contains(&"formatting".to_owned()));
    }

    #[test]
    fn test_tag_filter() {
        let mut app = sample_app();
        app.set_tag_filter(Some("greeting"));
        let visible = app.visible_notes();
        assert_eq!(visible.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Search
    // -----------------------------------------------------------------------

    #[test]
    fn test_search_by_title() {
        let app = sample_app();
        let results = app.search("First");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_by_content() {
        let app = sample_app();
        let results = app.search("bold");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_by_tag() {
        let app = sample_app();
        let results = app.search("greeting");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_case_insensitive() {
        let app = sample_app();
        let results = app.search("HELLO");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_empty_returns_all() {
        let app = sample_app();
        let results = app.search("");
        assert_eq!(results.len(), 2);
    }

    // -----------------------------------------------------------------------
    // Wiki linking
    // -----------------------------------------------------------------------

    #[test]
    fn test_extract_wiki_links() {
        let links = extract_wiki_links("See [[Page One]] and [[Page Two]] for details.");
        assert_eq!(links.len(), 2);
        assert_eq!(links[0], "Page One");
        assert_eq!(links[1], "Page Two");
    }

    #[test]
    fn test_extract_wiki_links_empty_brackets() {
        let links = extract_wiki_links("Empty [[]] should not appear.");
        assert!(links.is_empty());
    }

    #[test]
    fn test_extract_wiki_links_nested() {
        let links = extract_wiki_links("No [[nesting [[here]] allowed]].");
        // Should find "nesting [[here" up to the first ]]
        assert!(!links.is_empty());
    }

    #[test]
    fn test_resolve_links() {
        let mut app = NotesApp::new();
        let nb = app.create_notebook("NB");
        let n1 = app.create_note("Target Note", nb);
        let n2 = app.create_note("Source Note", nb);
        app.update_note_content(n2, "Link to [[Target Note]] here.");

        let resolved = app.resolve_links(n2);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].0, "Target Note");
        assert_eq!(resolved[0].1, Some(n1));
    }

    #[test]
    fn test_resolve_broken_link() {
        let mut app = NotesApp::new();
        let nb = app.create_notebook("NB");
        let nid = app.create_note("Note", nb);
        app.update_note_content(nid, "Link to [[Nonexistent]] page.");

        let resolved = app.resolve_links(nid);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].1, None);
    }

    #[test]
    fn test_backlinks() {
        let mut app = NotesApp::new();
        let nb = app.create_notebook("NB");
        let n1 = app.create_note("Alpha", nb);
        let n2 = app.create_note("Beta", nb);
        app.update_note_content(n2, "Links to [[Alpha]].");
        let n3 = app.create_note("Gamma", nb);
        app.update_note_content(n3, "Also links to [[Alpha]].");

        let backlinks = app.build_backlinks();
        let alpha_backlinks = backlinks.get(&n1).unwrap();
        assert_eq!(alpha_backlinks.len(), 2);
        assert!(alpha_backlinks.contains(&n2));
        assert!(alpha_backlinks.contains(&n3));
    }

    // -----------------------------------------------------------------------
    // Version history
    // -----------------------------------------------------------------------

    #[test]
    fn test_version_created_on_edit() {
        let mut app = NotesApp::new();
        let nb = app.create_notebook("NB");
        let nid = app.create_note("Note", nb);
        app.update_note_content(nid, "Version 1");
        app.update_note_content(nid, "Version 2");
        app.update_note_content(nid, "Version 3");

        let note = app.find_note(nid).unwrap();
        assert_eq!(note.content, "Version 3");
        assert_eq!(note.versions.len(), 2); // v1 and v2 saved
    }

    #[test]
    fn test_restore_version() {
        let mut app = NotesApp::new();
        let nb = app.create_notebook("NB");
        let nid = app.create_note("Note", nb);
        app.update_note_content(nid, "Original content");
        app.update_note_content(nid, "Modified content");

        // Version 0 should be "Original content"
        let note = app.find_note(nid).unwrap();
        assert_eq!(note.versions[0].content, "Original content");

        // Restore it
        assert!(app.find_note_mut(nid).unwrap().restore_version(0, 9999));
        let note = app.find_note(nid).unwrap();
        assert_eq!(note.content, "Original content");
    }

    #[test]
    fn test_restore_invalid_version() {
        let mut app = NotesApp::new();
        let nb = app.create_notebook("NB");
        let nid = app.create_note("Note", nb);
        let note = app.find_note_mut(nid).unwrap();
        assert!(!note.restore_version(99, 1000));
    }

    #[test]
    fn test_version_history_limit() {
        let mut app = NotesApp::new();
        let nb = app.create_notebook("NB");
        let nid = app.create_note("Note", nb);
        app.update_note_content(nid, "seed");
        for i in 0..60 {
            app.update_note_content(nid, &format!("version {i}"));
        }
        let note = app.find_note(nid).unwrap();
        // Should be capped at MAX_VERSIONS
        assert!(note.versions.len() <= MAX_VERSIONS);
    }

    // -----------------------------------------------------------------------
    // Templates
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_from_template_meeting() {
        let mut app = NotesApp::new();
        let nb = app.create_notebook("NB");
        let nid = app.create_note_from_template(NoteTemplate::MeetingNotes, nb);
        let note = app.find_note(nid).unwrap();
        assert_eq!(note.kind, NoteKind::Markdown);
        assert!(note.content.contains("Meeting Notes"));
    }

    #[test]
    fn test_create_from_template_todo() {
        let mut app = NotesApp::new();
        let nb = app.create_notebook("NB");
        let nid = app.create_note_from_template(NoteTemplate::TodoList, nb);
        let note = app.find_note(nid).unwrap();
        assert_eq!(note.kind, NoteKind::Checklist);
        assert_eq!(note.checklist.len(), 3);
    }

    #[test]
    fn test_template_all_list() {
        let all = NoteTemplate::all();
        assert_eq!(all.len(), 8);
    }

    #[test]
    fn test_each_template_has_label() {
        for t in NoteTemplate::all() {
            assert!(!t.label().is_empty());
        }
    }

    // -----------------------------------------------------------------------
    // Export
    // -----------------------------------------------------------------------

    #[test]
    fn test_export_plain_text() {
        let mut app = NotesApp::new();
        let nb = app.create_notebook("NB");
        let nid = app.create_note("Test Export", nb);
        app.update_note_content(nid, "Hello world");
        let note = app.find_note(nid).unwrap();
        let exported = export_note(note, ExportFormat::PlainText);
        assert!(exported.contains("Test Export"));
        assert!(exported.contains("Hello world"));
    }

    #[test]
    fn test_export_markdown() {
        let mut app = NotesApp::new();
        let nb = app.create_notebook("NB");
        let nid = app.create_note("MD Export", nb);
        app.update_note_content(nid, "Some content");
        app.add_tag_to_note(nid, "test");
        let note = app.find_note(nid).unwrap();
        let exported = export_note(note, ExportFormat::Markdown);
        assert!(exported.contains("# MD Export"));
        assert!(exported.contains("**Tags:**"));
        assert!(exported.contains("test"));
    }

    #[test]
    fn test_export_html() {
        let mut app = NotesApp::new();
        let nb = app.create_notebook("NB");
        let nid = app.create_note("HTML Export", nb);
        app.update_note_content(nid, "Hello <world>");
        let note = app.find_note(nid).unwrap();
        let exported = export_note(note, ExportFormat::Html);
        assert!(exported.contains("<!DOCTYPE html>"));
        assert!(exported.contains("HTML Export"));
        assert!(exported.contains("&lt;world&gt;")); // escaped
    }

    #[test]
    fn test_export_checklist_html() {
        let mut note = Note::new(1, "Checklist", 1);
        note.kind = NoteKind::Checklist;
        note.add_checklist_item("Done task");
        note.toggle_checklist_item(0);
        note.add_checklist_item("Pending task");
        let html = export_note(&note, ExportFormat::Html);
        assert!(html.contains("&#9745;")); // checked
        assert!(html.contains("&#9744;")); // unchecked
    }

    #[test]
    fn test_export_table_markdown() {
        let mut note = Note::new(1, "Table", 1);
        note.kind = NoteKind::Table;
        let mut table = TableData::new(vec!["A".to_owned(), "B".to_owned()]);
        table.add_row(vec!["1".to_owned(), "2".to_owned()]);
        note.table = Some(table);
        let md = export_note(&note, ExportFormat::Markdown);
        assert!(md.contains("| A | B |"));
        assert!(md.contains("| 1 | 2 |"));
    }

    // -----------------------------------------------------------------------
    // Markdown parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_heading() {
        let blocks = parse_markdown("# Title");
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            MdBlock::Heading { level, text } => {
                assert_eq!(*level, 1);
                assert_eq!(text, "Title");
            }
            _ => panic!("Expected heading"),
        }
    }

    #[test]
    fn test_parse_multiple_heading_levels() {
        let blocks = parse_markdown("## Subtitle\n### Sub-subtitle");
        assert_eq!(blocks.len(), 2);
        match &blocks[0] {
            MdBlock::Heading { level, .. } => assert_eq!(*level, 2),
            _ => panic!("Expected h2"),
        }
        match &blocks[1] {
            MdBlock::Heading { level, .. } => assert_eq!(*level, 3),
            _ => panic!("Expected h3"),
        }
    }

    #[test]
    fn test_parse_bold() {
        let spans = parse_inline("Hello **world**!");
        assert!(
            spans
                .iter()
                .any(|s| matches!(s, MdSpan::Bold(t) if t == "world"))
        );
    }

    #[test]
    fn test_parse_italic() {
        let spans = parse_inline("Hello *world*!");
        assert!(
            spans
                .iter()
                .any(|s| matches!(s, MdSpan::Italic(t) if t == "world"))
        );
    }

    #[test]
    fn test_parse_bold_italic() {
        let spans = parse_inline("***emphasis***");
        assert!(
            spans
                .iter()
                .any(|s| matches!(s, MdSpan::BoldItalic(t) if t == "emphasis"))
        );
    }

    #[test]
    fn test_parse_inline_code() {
        let spans = parse_inline("Use `println!` for output.");
        assert!(
            spans
                .iter()
                .any(|s| matches!(s, MdSpan::InlineCode(t) if t == "println!"))
        );
    }

    #[test]
    fn test_parse_code_block() {
        let blocks = parse_markdown("```rust\nfn main() {}\n```");
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
    fn test_parse_unordered_list() {
        let blocks = parse_markdown("- Item 1\n- Item 2\n- Item 3");
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            MdBlock::UnorderedList { items } => {
                assert_eq!(items.len(), 3);
                assert_eq!(items[0], "Item 1");
            }
            _ => panic!("Expected unordered list"),
        }
    }

    #[test]
    fn test_parse_ordered_list() {
        let blocks = parse_markdown("1. First\n2. Second");
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            MdBlock::OrderedList { items } => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0], "First");
            }
            _ => panic!("Expected ordered list"),
        }
    }

    #[test]
    fn test_parse_blockquote() {
        let blocks = parse_markdown("> This is a quote");
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            MdBlock::BlockQuote { text } => {
                assert!(text.contains("This is a quote"));
            }
            _ => panic!("Expected blockquote"),
        }
    }

    #[test]
    fn test_parse_horizontal_rule() {
        let blocks = parse_markdown("---");
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], MdBlock::HorizontalRule));
    }

    #[test]
    fn test_parse_wiki_link_inline() {
        let spans = parse_inline("See [[My Page]].");
        assert!(
            spans
                .iter()
                .any(|s| matches!(s, MdSpan::WikiLink(t) if t == "My Page"))
        );
    }

    // -----------------------------------------------------------------------
    // Word count and reading time
    // -----------------------------------------------------------------------

    #[test]
    fn test_word_count_plain() {
        let mut note = Note::new(1, "Test", 1);
        note.content = "Hello world foo bar baz".to_owned();
        assert_eq!(note.word_count(), 5);
    }

    #[test]
    fn test_word_count_checklist() {
        let mut note = Note::new(1, "Checklist", 1);
        note.kind = NoteKind::Checklist;
        note.add_checklist_item("Buy groceries");
        note.add_checklist_item("Walk the dog");
        assert_eq!(note.word_count(), 5);
    }

    #[test]
    fn test_reading_time() {
        let mut note = Note::new(1, "Long", 1);
        // 238 words should be about 1 minute
        let words: Vec<&str> = (0..238).map(|_| "word").collect();
        note.content = words.join(" ");
        let time = note.reading_time_minutes();
        assert!((time - 1.0).abs() < 0.01);
    }

    // -----------------------------------------------------------------------
    // Sorting and filtering
    // -----------------------------------------------------------------------

    #[test]
    fn test_pinned_notes_first() {
        let mut app = NotesApp::new();
        let nb = app.create_notebook("NB");
        let n1 = app.create_note("Unpinned", nb);
        let n2 = app.create_note("Pinned", nb);
        if let Some(note) = app.find_note_mut(n2) {
            note.pinned = true;
        }
        let visible = app.visible_notes();
        assert_eq!(visible[0], n2);
        assert_eq!(visible[1], n1);
    }

    #[test]
    fn test_favorites_filter() {
        let mut app = NotesApp::new();
        let nb = app.create_notebook("NB");
        let n1 = app.create_note("Regular", nb);
        let n2 = app.create_note("Favorite", nb);
        if let Some(note) = app.find_note_mut(n2) {
            note.favorited = true;
        }
        app.show_favorites_only = true;
        let visible = app.visible_notes();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0], n2);
        assert!(!visible.contains(&n1));
    }

    #[test]
    fn test_sort_by_title() {
        let mut app = NotesApp::new();
        let nb = app.create_notebook("NB");
        app.create_note("Banana", nb);
        app.create_note("Apple", nb);
        app.create_note("Cherry", nb);
        app.sort_order = SortOrder::Title;
        let visible = app.visible_notes();
        let titles: Vec<&str> = visible
            .iter()
            .filter_map(|id| app.find_note(*id))
            .map(|n| n.title.as_str())
            .collect();
        assert_eq!(titles, vec!["Apple", "Banana", "Cherry"]);
    }

    // -----------------------------------------------------------------------
    // Checklist operations
    // -----------------------------------------------------------------------

    #[test]
    fn test_toggle_checklist_item() {
        let mut note = Note::new(1, "CL", 1);
        note.kind = NoteKind::Checklist;
        note.add_checklist_item("Task A");
        assert!(!note.checklist[0].checked);
        assert!(note.toggle_checklist_item(0));
        assert!(note.checklist[0].checked);
        assert!(note.toggle_checklist_item(0));
        assert!(!note.checklist[0].checked);
    }

    #[test]
    fn test_toggle_invalid_checklist_index() {
        let mut note = Note::new(1, "CL", 1);
        note.kind = NoteKind::Checklist;
        assert!(!note.toggle_checklist_item(0));
    }

    #[test]
    fn test_remove_checklist_item() {
        let mut note = Note::new(1, "CL", 1);
        note.kind = NoteKind::Checklist;
        note.add_checklist_item("A");
        note.add_checklist_item("B");
        assert!(note.remove_checklist_item(0));
        assert_eq!(note.checklist.len(), 1);
        assert_eq!(note.checklist[0].text, "B");
    }

    // -----------------------------------------------------------------------
    // Table data
    // -----------------------------------------------------------------------

    #[test]
    fn test_table_markdown_export() {
        let mut table = TableData::new(vec!["X".to_owned(), "Y".to_owned()]);
        table.add_row(vec!["1".to_owned(), "2".to_owned()]);
        let md = table.to_markdown();
        assert!(md.contains("| X | Y |"));
        assert!(md.contains("| --- |"));
        assert!(md.contains("| 1 | 2 |"));
    }

    #[test]
    fn test_table_plain_text_export() {
        let mut table = TableData::new(vec!["Name".to_owned(), "Age".to_owned()]);
        table.add_row(vec!["Alice".to_owned(), "30".to_owned()]);
        let txt = table.to_plain_text();
        assert!(txt.contains("Name"));
        assert!(txt.contains("Alice"));
    }

    #[test]
    fn test_table_cell_access() {
        let mut table = TableData::new(vec!["A".to_owned()]);
        table.add_row(vec!["val".to_owned()]);
        assert_eq!(table.cell(0, 0), "val");
        assert_eq!(table.cell(0, 99), ""); // out of bounds
        assert_eq!(table.cell(99, 0), ""); // out of bounds
    }

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

    #[test]
    fn test_render_produces_commands() {
        let app = sample_app();
        let cmds = app.render(1280.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_selected_note() {
        let mut app = sample_app();
        let first_note = app.notes[0].id;
        app.selected_note = Some(first_note);
        let cmds = app.render(1280.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_empty_app() {
        let app = NotesApp::new();
        let cmds = app.render(1280.0, 800.0);
        // Should at least have background and toolbar
        assert!(cmds.len() > 2);
    }

    // -----------------------------------------------------------------------
    // Export format
    // -----------------------------------------------------------------------

    #[test]
    fn test_export_format_labels() {
        assert_eq!(ExportFormat::PlainText.label(), "Plain Text");
        assert_eq!(ExportFormat::Markdown.label(), "Markdown");
        assert_eq!(ExportFormat::Html.label(), "HTML");
    }

    #[test]
    fn test_export_format_extensions() {
        assert_eq!(ExportFormat::PlainText.extension(), "txt");
        assert_eq!(ExportFormat::Markdown.extension(), "md");
        assert_eq!(ExportFormat::Html.extension(), "html");
    }

    // -----------------------------------------------------------------------
    // HTML escaping
    // -----------------------------------------------------------------------

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("a&b"), "a&amp;b");
        assert_eq!(html_escape("\"quoted\""), "&quot;quoted&quot;");
    }

    // -----------------------------------------------------------------------
    // Misc
    // -----------------------------------------------------------------------

    #[test]
    fn test_note_kind_labels() {
        assert_eq!(NoteKind::PlainText.label(), "Plain Text");
        assert_eq!(NoteKind::Markdown.label(), "Markdown");
        assert_eq!(NoteKind::Checklist.label(), "Checklist");
        assert_eq!(NoteKind::Table.label(), "Table");
    }

    #[test]
    fn test_sort_order_cycle() {
        let s = SortOrder::DateModified;
        let s = s.next();
        assert_eq!(s, SortOrder::DateCreated);
        let s = s.next();
        assert_eq!(s, SortOrder::Title);
        let s = s.next();
        assert_eq!(s, SortOrder::Notebook);
        let s = s.next();
        assert_eq!(s, SortOrder::DateModified);
    }

    #[test]
    fn test_note_stats() {
        let mut app = NotesApp::new();
        let nb = app.create_notebook("NB");
        let nid = app.create_note("Stats Note", nb);
        app.update_note_content(nid, "Hello world [[Link Target]]");
        app.add_tag_to_note(nid, "test");
        app.selected_note = Some(nid);

        let stats = app.selected_note_stats().unwrap();
        assert!(stats.word_count > 0);
        assert!(stats.char_count > 0);
        assert_eq!(stats.tag_count, 1);
        assert_eq!(stats.link_count, 1);
    }

    #[test]
    fn test_notebook_descendant_ids() {
        let mut app = NotesApp::new();
        let root = app.create_notebook("Root");
        let child = app.create_child_notebook("Child", root);
        let _grandchild = app.create_child_notebook("Grandchild", child);

        let ids = app.notebook_descendant_ids(root);
        assert_eq!(ids.len(), 3);
    }
}
