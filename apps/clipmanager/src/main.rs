//! Slate OS Clipboard Manager — a full-featured clipboard history and snippet manager.
//!
//! Provides clipboard history tracking (up to 500 entries), search, filtering by
//! content type, tagging, pinning, template management with placeholder substitution,
//! batch operations, statistics, and export/import. Inspired by CopyQ and Ditto.

// The feature surface (entry types, store, render helpers) is defined ahead
// of the main loop wire-up. Until the loop wires these into the clipboard
// daemon's IPC channel, the items read as dead. Tracked in todo.txt as
// "clipmanager: wire feature surface to main loop".
#![allow(dead_code)]

use std::collections::VecDeque;

use guitk::color::Color;
use guitk::layout::FlexDirection;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::{CornerRadii, Edges};
use guitk::text;
use guitk::widget::{Widget, WidgetTree};

// ---------------------------------------------------------------------------
// Catppuccin Mocha palette
// ---------------------------------------------------------------------------
const BASE: Color = Color::from_hex(0x1E1E2E);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const BLUE: Color = Color::from_hex(0x89B4FA);
const RED: Color = Color::from_hex(0xF38BA8);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const TEAL: Color = Color::from_hex(0x94E2D5);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const MANTLE: Color = Color::from_hex(0x181825);

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------
const MAX_ENTRIES: usize = 500;
const PREVIEW_MAX_CHARS: usize = 120;
const PREVIEW_LINES: usize = 2;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// The kind of content stored in a clipboard entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ClipType {
    PlainText,
    RichText,
    Html,
    Image,
    FilePaths,
    Code,
}

impl ClipType {
    /// Short display label for use in badges.
    fn label(self) -> &'static str {
        match self {
            Self::PlainText => "Text",
            Self::RichText => "Rich",
            Self::Html => "HTML",
            Self::Image => "Image",
            Self::FilePaths => "Files",
            Self::Code => "Code",
        }
    }

    /// Badge colour associated with this type.
    fn badge_color(self) -> Color {
        match self {
            Self::PlainText => BLUE,
            Self::RichText => MAUVE,
            Self::Html => PEACH,
            Self::Image => GREEN,
            Self::FilePaths => YELLOW,
            Self::Code => TEAL,
        }
    }

    /// All variants for iteration.
    fn all() -> &'static [ClipType] {
        &[
            Self::PlainText,
            Self::RichText,
            Self::Html,
            Self::Image,
            Self::FilePaths,
            Self::Code,
        ]
    }
}

/// A single clipboard history entry.
#[derive(Clone, Debug)]
struct ClipEntry {
    id: u64,
    content: String,
    clip_type: ClipType,
    /// Seconds since an arbitrary epoch (monotonic).
    timestamp: u64,
    source_app: String,
    pinned: bool,
    tags: Vec<String>,
    size_bytes: u64,
}

impl ClipEntry {
    fn new(
        id: u64,
        content: String,
        clip_type: ClipType,
        timestamp: u64,
        source_app: String,
    ) -> Self {
        let size_bytes = content.len() as u64;
        Self {
            id,
            content,
            clip_type,
            timestamp,
            source_app,
            pinned: false,
            tags: Vec::new(),
            size_bytes,
        }
    }

    /// Return the first `PREVIEW_LINES` lines of content, truncated to
    /// `PREVIEW_MAX_CHARS` total characters.
    ///
    /// **Characters, as the name says.** This used to count `out.len()`, which
    /// is bytes, and then call `String::truncate(PREVIEW_MAX_CHARS)` — which
    /// *panics* rather than rounding when the byte offset lands inside a
    /// character. The clipboard holds whatever the user copied, so any Greek,
    /// Cyrillic, Hebrew, Arabic, CJK or emoji text longer than 120 bytes had
    /// about a two-in-three chance of taking the whole clipboard manager down
    /// while drawing its own list. The only test covered `"a".repeat(200)`,
    /// which is ASCII and so is exactly the case that cannot fail.
    ///
    /// Counting bytes was also wrong when it did not crash: 120 bytes of
    /// Japanese is 40 characters, so a preview that fits one line in English
    /// was cut to a third of it in Japanese. Nothing here wants a byte count —
    /// the number exists to bound how much text is *drawn*.
    ///
    /// Characters rather than caret stops (`TextCursor`), because this cuts
    /// text that is only ever displayed, never edited, and a cut between a
    /// base letter and a combining mark shows as one wrong glyph rather than a
    /// crash or a bad offset. Characters rather than pixels because a preview
    /// is measured before any font is chosen for it.
    fn preview(&self) -> String {
        let mut out = String::new();
        let mut overflowed = false;
        for line in self.content.lines().take(PREVIEW_LINES) {
            if !out.is_empty() {
                if out.chars().count() >= PREVIEW_MAX_CHARS {
                    overflowed = true;
                    break;
                }
                out.push(' ');
            }
            // Take only what is left of the budget from each line, so a
            // multi-megabyte clipboard entry is never copied whole just to
            // throw all but 120 characters of it away.
            let room = PREVIEW_MAX_CHARS.saturating_sub(out.chars().count());
            let mut rest = line.chars();
            out.extend(rest.by_ref().take(room));
            if rest.next().is_some() {
                overflowed = true;
                break;
            }
        }
        if overflowed {
            out.push_str("...");
        }
        out
    }

    /// Human-readable size string.
    fn size_display(&self) -> String {
        format_size(self.size_bytes)
    }

    /// Human-readable timestamp (simple seconds-ago style).
    fn time_display(&self, now: u64) -> String {
        let diff = now.saturating_sub(self.timestamp);
        if diff < 60 {
            return format!("{diff}s ago");
        }
        let mins = diff / 60;
        if mins < 60 {
            return format!("{mins}m ago");
        }
        let hours = mins / 60;
        if hours < 24 {
            return format!("{hours}h ago");
        }
        let days = hours / 24;
        format!("{days}d ago")
    }
}

/// Named template with placeholders (e.g. `{name}`).
#[derive(Clone, Debug)]
struct ClipTemplate {
    name: String,
    body: String,
}

impl ClipTemplate {
    fn new(name: String, body: String) -> Self {
        Self { name, body }
    }

    /// Substitute all `{key}` placeholders with values from `vars`.
    fn render(&self, vars: &[(String, String)]) -> String {
        let mut result = self.body.clone();
        for (key, value) in vars {
            let placeholder = format!("{{{key}}}");
            result = result.replace(&placeholder, value);
        }
        result
    }

    /// Extract placeholder names from the body.
    fn placeholders(&self) -> Vec<String> {
        let mut out = Vec::new();
        let bytes = self.body.as_bytes();
        let len = bytes.len();
        let mut i = 0usize;
        while i < len {
            if bytes.get(i).copied() == Some(b'{')
                && let Some(end) = self.body[i..].find('}')
            {
                let name = &self.body[i + 1..i + end];
                if !name.is_empty() && !out.contains(&name.to_string()) {
                    out.push(name.to_string());
                }
                i = i + end + 1;
                continue;
            }
            i = i.saturating_add(1);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Export format primitives
// ---------------------------------------------------------------------------

/// The line that begins a record in the text export.
///
/// It is only ever recognised as an entire line, never as a substring. That
/// distinction is the whole bug the length-prefixed body exists to close: a
/// clipboard entry containing this text used to split its own record in half.
const ENTRY_MARKER: &str = "---ENTRY---";

/// Render a short string so it can occupy one header line safely.
///
/// Header values are display strings — an application name, a tag — for which
/// the format offers no way to spell a line break. Per the reject-or-sanitise
/// rule this leaves only sanitising, and control characters become spaces: the
/// value stays legible and, more importantly, stays on its own line, so it
/// cannot invent a sibling field or a record marker below itself.
fn header_value(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// Split off the next line, returning it (without its terminator) and the rest.
///
/// Accepts both `\n` and `\r\n`, and treats an unterminated final line as a
/// line, so a hand-edited or truncated export still parses.
fn take_line(s: &str) -> Option<(&str, &str)> {
    if s.is_empty() {
        return None;
    }
    let (raw, rest) = match s.find('\n') {
        Some(i) => (
            s.get(..i).unwrap_or(""),
            s.get(i.saturating_add(1)..).unwrap_or(""),
        ),
        None => (s, ""),
    };
    Some((raw.strip_suffix('\r').unwrap_or(raw), rest))
}

/// Advance past the next record marker, returning the text after it.
fn next_record(mut s: &str) -> Option<&str> {
    while let Some((line, after)) = take_line(s) {
        s = after;
        if line == ENTRY_MARKER {
            return Some(s);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Clipboard store
// ---------------------------------------------------------------------------

/// Persistent store holding clipboard history with search, filtering, tagging,
/// pinning, deduplication, and statistics.
struct ClipboardStore {
    entries: VecDeque<ClipEntry>,
    next_id: u64,
    total_size: u64,
    templates: Vec<ClipTemplate>,
}

impl ClipboardStore {
    fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            next_id: 1,
            total_size: 0,
            templates: Vec::new(),
        }
    }

    /// Add a new entry, deduplicating by content. Returns the entry id.
    fn add(
        &mut self,
        content: String,
        clip_type: ClipType,
        timestamp: u64,
        source_app: String,
    ) -> u64 {
        // Deduplicate: if identical content exists, move it to front instead.
        if let Some(pos) = self.entries.iter().position(|e| e.content == content)
            && let Some(mut existing) = self.entries.remove(pos)
        {
            existing.timestamp = timestamp;
            existing.source_app = source_app;
            let id = existing.id;
            self.entries.push_front(existing);
            return id;
        }

        // Evict oldest unpinned entries if at capacity.
        while self.entries.len() >= MAX_ENTRIES {
            if !self.evict_oldest_unpinned() {
                break; // all entries are pinned; cannot evict
            }
        }

        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let entry = ClipEntry::new(id, content, clip_type, timestamp, source_app);
        self.total_size = self.total_size.saturating_add(entry.size_bytes);
        self.entries.push_front(entry);
        id
    }

    /// Remove the oldest unpinned entry. Returns `true` if one was removed.
    fn evict_oldest_unpinned(&mut self) -> bool {
        // Search from the back (oldest) for an unpinned entry.
        let mut idx = None;
        for (i, e) in self.entries.iter().enumerate().rev() {
            if !e.pinned {
                idx = Some(i);
                break;
            }
        }
        if let Some(i) = idx
            && let Some(removed) = self.entries.remove(i)
        {
            self.total_size = self.total_size.saturating_sub(removed.size_bytes);
            return true;
        }
        false
    }

    /// Retrieve an entry by id.
    fn get(&self, id: u64) -> Option<&ClipEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Retrieve a mutable entry by id.
    fn get_mut(&mut self, id: u64) -> Option<&mut ClipEntry> {
        self.entries.iter_mut().find(|e| e.id == id)
    }

    /// Delete an entry by id.
    fn delete(&mut self, id: u64) -> bool {
        if let Some(pos) = self.entries.iter().position(|e| e.id == id)
            && let Some(removed) = self.entries.remove(pos)
        {
            self.total_size = self.total_size.saturating_sub(removed.size_bytes);
            return true;
        }
        false
    }

    /// Delete multiple entries by id.
    fn delete_many(&mut self, ids: &[u64]) {
        for &id in ids {
            self.delete(id);
        }
    }

    /// Clear all unpinned entries.
    fn clear_unpinned(&mut self) {
        let before = self.entries.len();
        self.entries.retain(|e| e.pinned);
        let after = self.entries.len();
        if before != after {
            self.recalculate_total_size();
        }
    }

    /// Toggle pin state for an entry.
    fn toggle_pin(&mut self, id: u64) {
        if let Some(entry) = self.get_mut(id) {
            entry.pinned = !entry.pinned;
        }
    }

    /// Add a tag to an entry (no duplicates).
    fn add_tag(&mut self, id: u64, tag: String) {
        if let Some(entry) = self.get_mut(id)
            && !entry.tags.contains(&tag)
        {
            entry.tags.push(tag);
        }
    }

    /// Remove a tag from an entry.
    fn remove_tag(&mut self, id: u64, tag: &str) {
        if let Some(entry) = self.get_mut(id) {
            entry.tags.retain(|t| t != tag);
        }
    }

    /// Case-insensitive substring search across content.
    fn search(&self, query: &str) -> Vec<u64> {
        if query.is_empty() {
            return self.entries.iter().map(|e| e.id).collect();
        }
        let q = query.to_lowercase();
        self.entries
            .iter()
            .filter(|e| e.content.to_lowercase().contains(&q))
            .map(|e| e.id)
            .collect()
    }

    /// Filter by content type.
    fn filter_by_type(&self, clip_type: ClipType) -> Vec<u64> {
        self.entries
            .iter()
            .filter(|e| e.clip_type == clip_type)
            .map(|e| e.id)
            .collect()
    }

    /// Filter by tag.
    fn filter_by_tag(&self, tag: &str) -> Vec<u64> {
        self.entries
            .iter()
            .filter(|e| e.tags.iter().any(|t| t == tag))
            .map(|e| e.id)
            .collect()
    }

    /// Combined search + type filter.
    fn search_filtered(&self, query: &str, type_filter: Option<ClipType>) -> Vec<u64> {
        let q = query.to_lowercase();
        self.entries
            .iter()
            .filter(|e| {
                let type_ok = type_filter.is_none_or(|t| e.clip_type == t);
                let search_ok = q.is_empty() || e.content.to_lowercase().contains(&q);
                type_ok && search_ok
            })
            .map(|e| e.id)
            .collect()
    }

    /// Get all unique tags across all entries.
    fn all_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = Vec::new();
        for entry in &self.entries {
            for tag in &entry.tags {
                if !tags.contains(tag) {
                    tags.push(tag.clone());
                }
            }
        }
        tags.sort();
        tags
    }

    /// Statistics: count of entries per type.
    fn stats_by_type(&self) -> Vec<(ClipType, usize)> {
        ClipType::all()
            .iter()
            .map(|&t| {
                let count = self.entries.iter().filter(|e| e.clip_type == t).count();
                (t, count)
            })
            .collect()
    }

    /// Total number of entries.
    fn total_entries(&self) -> usize {
        self.entries.len()
    }

    /// Number of pinned entries.
    fn pinned_count(&self) -> usize {
        self.entries.iter().filter(|e| e.pinned).count()
    }

    fn recalculate_total_size(&mut self) {
        self.total_size = self.entries.iter().map(|e| e.size_bytes).sum();
    }

    /// Export all entries to a simple text format.
    ///
    /// The format is deliberately escape-free. Every field is one of two
    /// shapes, and neither can be forged by its own value:
    ///
    /// * a **header line** `key:value`, whose value is a short display string
    ///   with control characters replaced by spaces, so it cannot start a new
    ///   line and therefore cannot invent a sibling field or a record;
    /// * the **body**, introduced by `content:<byte length>` and read by
    ///   counting bytes rather than by scanning for a terminator.
    ///
    /// The body is why this is not merely an escaping question. A clipboard
    /// entry is arbitrary copied text — the one field in the whole desktop
    /// that is guaranteed to contain whatever the user last selected in a
    /// browser. The previous format wrote it raw after a bare `content:` line
    /// and recovered it by splitting the file on the substring `---ENTRY---`,
    /// so copying any text that happened to contain that marker split one
    /// entry into two on import, and the second half's lines were then parsed
    /// as *headers* — meaning copied text could name its own `source:` and set
    /// `pinned:`. Escaping the body would work, but it would also make the
    /// export unreadable for the one format whose whole point is that you can
    /// open it and see what you copied. A length prefix keeps it readable and
    /// removes the ambiguity outright: bytes inside the body are never
    /// examined, so no byte sequence in them means anything.
    ///
    /// Tags get a line each rather than a comma-joined list for the same
    /// reason at smaller scale: a comma-separated field cannot represent a tag
    /// containing a comma, and one line per tag is a fix rather than an escape.
    fn export_text(&self) -> String {
        let mut out = String::new();
        // Oldest first, though the store is newest first. The file is a log,
        // and import replays it through `add`, which prepends -- so writing it
        // in store order made importing your own export reverse your history.
        for entry in self.entries.iter().rev() {
            out.push_str(ENTRY_MARKER);
            out.push('\n');
            out.push_str(&format!("id:{}\n", entry.id));
            out.push_str(&format!("type:{}\n", entry.clip_type.label()));
            out.push_str(&format!("timestamp:{}\n", entry.timestamp));
            out.push_str(&format!("source:{}\n", header_value(&entry.source_app)));
            out.push_str(&format!("pinned:{}\n", entry.pinned));
            for tag in &entry.tags {
                out.push_str(&format!("tag:{}\n", header_value(tag)));
            }
            out.push_str(&format!("content:{}\n", entry.content.len()));
            out.push_str(&entry.content);
            out.push('\n');
        }
        out
    }

    /// Import entries from text format. Returns count of imported entries.
    ///
    /// A single left-to-right pass over the document, because the body length
    /// is only known once its header has been read — the previous
    /// `split("---ENTRY---")` could not have consulted it.
    fn import_text(&mut self, data: &str, base_timestamp: u64) -> usize {
        let mut count = 0usize;
        let mut rest = data;

        // Anything before the first record marker is preamble and is skipped.
        while let Some(after_marker) = next_record(rest) {
            let mut cursor = after_marker;
            let mut clip_type = ClipType::PlainText;
            let mut source = String::from("import");
            let mut pinned = false;
            let mut tags: Vec<String> = Vec::new();
            let mut content: Option<String> = None;

            // Headers, up to and including the `content:` line that ends them.
            while let Some((line, after_line)) = take_line(cursor) {
                // A marker ends the record without consuming it, so the outer
                // loop sees it as the start of the next one. Deciding before
                // advancing is what makes that possible.
                if line == ENTRY_MARKER {
                    break;
                }
                cursor = after_line;
                if let Some(val) = line.strip_prefix("content:") {
                    // A declared length that runs past the end of the document
                    // or lands inside a character is a truncated or corrupt
                    // file, not an attack: take the longest valid prefix.
                    let want: usize = val.trim().parse().unwrap_or(0);
                    let mut end = want.min(cursor.len());
                    while end > 0 && !cursor.is_char_boundary(end) {
                        end = end.saturating_sub(1);
                    }
                    let body = cursor.get(..end).unwrap_or("");
                    content = Some(body.to_string());
                    cursor = cursor.get(end..).unwrap_or("");
                    break;
                }
                if let Some(val) = line.strip_prefix("type:") {
                    clip_type = match val.trim() {
                        "Rich" => ClipType::RichText,
                        "HTML" => ClipType::Html,
                        "Image" => ClipType::Image,
                        "Files" => ClipType::FilePaths,
                        "Code" => ClipType::Code,
                        _ => ClipType::PlainText,
                    };
                } else if let Some(val) = line.strip_prefix("source:") {
                    source = val.trim().to_string();
                } else if let Some(val) = line.strip_prefix("pinned:") {
                    pinned = val.trim() == "true";
                } else if let Some(val) = line.strip_prefix("tag:") {
                    tags.push(val.trim().to_string());
                }
                // id: and timestamp: are ignored on import (we assign fresh ones)
            }

            rest = cursor;

            // An entry with no body at all is a malformed record, not an empty
            // clip: skip it. An entry whose body is legitimately empty is
            // representable (`content:0`) and is kept.
            let Some(content) = content else {
                continue;
            };

            let id = self.add(content, clip_type, base_timestamp, source);
            if let Some(entry) = self.get_mut(id) {
                entry.pinned = pinned;
                entry.tags = tags;
            }
            count = count.saturating_add(1);
        }
        count
    }

    // Template management -------------------------------------------------

    fn add_template(&mut self, name: String, body: String) {
        // Replace if same name exists.
        if let Some(existing) = self.templates.iter_mut().find(|t| t.name == name) {
            existing.body = body;
        } else {
            self.templates.push(ClipTemplate::new(name, body));
        }
    }

    fn remove_template(&mut self, name: &str) {
        self.templates.retain(|t| t.name != name);
    }

    fn get_template(&self, name: &str) -> Option<&ClipTemplate> {
        self.templates.iter().find(|t| t.name == name)
    }
}

// ---------------------------------------------------------------------------
// Code snippet detection heuristics
// ---------------------------------------------------------------------------

/// Detect whether a piece of text looks like a code snippet and return a
/// syntax hint string (e.g. "rust", "python", "javascript", "generic").
fn detect_code_language(text: &str) -> Option<&'static str> {
    let trimmed = text.trim();
    if trimmed.contains("fn ") && trimmed.contains("->") {
        return Some("rust");
    }
    if trimmed.contains("def ") && trimmed.contains(':') && !trimmed.contains('{') {
        return Some("python");
    }
    if trimmed.contains("function ") || trimmed.contains("const ") || trimmed.contains("=> {") {
        return Some("javascript");
    }
    if trimmed.contains("#include") {
        return Some("c/c++");
    }
    if trimmed.contains("public class") || trimmed.contains("private void") {
        return Some("java");
    }
    // Generic code detection: multiple lines with indentation and braces/semicolons.
    let line_count = trimmed.lines().count();
    let has_braces = trimmed.contains('{') && trimmed.contains('}');
    let has_semicolons = trimmed.matches(';').count() > 1;
    if line_count > 2 && (has_braces || has_semicolons) {
        return Some("generic");
    }
    None
}

// ---------------------------------------------------------------------------
// Helper: human-readable file size
// ---------------------------------------------------------------------------

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let kb = bytes as f64 / 1024.0;
    if kb < 1024.0 {
        return format!("{kb:.1} KB");
    }
    let mb = kb / 1024.0;
    format!("{mb:.2} MB")
}

// ---------------------------------------------------------------------------
// GUI state
// ---------------------------------------------------------------------------

/// Which tab is currently active.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActiveTab {
    History,
    Templates,
}

/// Application-level GUI state.
struct AppState {
    store: ClipboardStore,
    search_query: String,
    type_filter: Option<ClipType>,
    /// Indices into the filtered result set.
    filtered_ids: Vec<u64>,
    selected_id: Option<u64>,
    selected_indices: Vec<usize>,
    scroll_offset: usize,
    visible_rows: usize,
    active_tab: ActiveTab,
    /// Current simulated time (seconds).
    now: u64,
    /// Tag being added via the tag editor.
    tag_input: String,
    /// Template name input.
    template_name_input: String,
    /// Template body input.
    template_body_input: String,
    /// Template placeholder values (key, value).
    template_vars: Vec<(String, String)>,
    /// Currently selected template index.
    selected_template: Option<usize>,
}

impl AppState {
    fn new() -> Self {
        Self {
            store: ClipboardStore::new(),
            search_query: String::new(),
            type_filter: None,
            filtered_ids: Vec::new(),
            selected_id: None,
            selected_indices: Vec::new(),
            scroll_offset: 0,
            visible_rows: 15,
            active_tab: ActiveTab::History,
            now: 1000,
            tag_input: String::new(),
            template_name_input: String::new(),
            template_body_input: String::new(),
            template_vars: Vec::new(),
            selected_template: None,
        }
    }

    /// Re-run the current search/filter and update `filtered_ids`.
    fn refresh_filter(&mut self) {
        self.filtered_ids = self
            .store
            .search_filtered(&self.search_query, self.type_filter);
    }

    /// Select an entry by id.
    fn select(&mut self, id: u64) {
        self.selected_id = Some(id);
    }

    /// Move selection up within filtered list.
    fn select_prev(&mut self) {
        if self.filtered_ids.is_empty() {
            return;
        }
        let current_pos = self
            .selected_id
            .and_then(|id| self.filtered_ids.iter().position(|&fid| fid == id));
        let new_pos = match current_pos {
            Some(0) | None => 0,
            Some(p) => p.saturating_sub(1),
        };
        self.selected_id = self.filtered_ids.get(new_pos).copied();
        if new_pos < self.scroll_offset {
            self.scroll_offset = new_pos;
        }
    }

    /// Move selection down within filtered list.
    fn select_next(&mut self) {
        if self.filtered_ids.is_empty() {
            return;
        }
        let current_pos = self
            .selected_id
            .and_then(|id| self.filtered_ids.iter().position(|&fid| fid == id));
        let last = self.filtered_ids.len().saturating_sub(1);
        let new_pos = match current_pos {
            None => 0,
            Some(p) => {
                if p < last {
                    p.saturating_add(1)
                } else {
                    last
                }
            }
        };
        self.selected_id = self.filtered_ids.get(new_pos).copied();
        if new_pos >= self.scroll_offset.saturating_add(self.visible_rows) {
            self.scroll_offset = new_pos.saturating_sub(self.visible_rows.saturating_sub(1));
        }
    }

    /// Delete selected entry.
    fn delete_selected(&mut self) {
        if let Some(id) = self.selected_id {
            self.store.delete(id);
            self.selected_id = None;
            self.refresh_filter();
        }
    }

    /// Toggle pin on selected entry.
    fn toggle_pin_selected(&mut self) {
        if let Some(id) = self.selected_id {
            self.store.toggle_pin(id);
        }
    }

    /// Add tag from `tag_input` to selected entry and clear input.
    fn add_tag_to_selected(&mut self) {
        let tag = self.tag_input.trim().to_string();
        if tag.is_empty() {
            return;
        }
        if let Some(id) = self.selected_id {
            self.store.add_tag(id, tag);
            self.tag_input.clear();
        }
    }

    /// Remove a tag from the selected entry.
    fn remove_tag_from_selected(&mut self, tag: &str) {
        if let Some(id) = self.selected_id {
            self.store.remove_tag(id, tag);
        }
    }

    /// Save the template currently in the input fields.
    fn save_template(&mut self) {
        let name = self.template_name_input.trim().to_string();
        let body = self.template_body_input.trim().to_string();
        if name.is_empty() || body.is_empty() {
            return;
        }
        self.store.add_template(name, body);
        self.template_name_input.clear();
        self.template_body_input.clear();
    }

    /// Delete the currently selected template.
    fn delete_selected_template(&mut self) {
        if let Some(idx) = self.selected_template
            && let Some(tmpl) = self.store.templates.get(idx)
        {
            let name = tmpl.name.clone();
            self.store.remove_template(&name);
            self.selected_template = None;
        }
    }

    /// Select a template by index and populate placeholder vars.
    fn select_template(&mut self, idx: usize) {
        self.selected_template = Some(idx);
        if let Some(tmpl) = self.store.templates.get(idx) {
            let placeholders = tmpl.placeholders();
            self.template_vars = placeholders
                .into_iter()
                .map(|p| (p, String::new()))
                .collect();
        }
    }

    /// Render selected template with current var values and copy to clipboard
    /// (i.e. add as a new entry).
    fn render_template(&mut self) {
        if let Some(idx) = self.selected_template {
            let rendered = if let Some(tmpl) = self.store.templates.get(idx) {
                Some(tmpl.render(&self.template_vars))
            } else {
                None
            };
            if let Some(text) = rendered {
                let ts = self.now;
                self.store
                    .add(text, ClipType::PlainText, ts, "template".to_string());
                self.refresh_filter();
            }
        }
    }

    /// Format statistics as a status string.
    fn stats_line(&self) -> String {
        let total = self.store.total_entries();
        let pinned = self.store.pinned_count();
        let size = format_size(self.store.total_size);
        format!("{total} entries | {pinned} pinned | {size}")
    }
}

// ---------------------------------------------------------------------------
// GUI rendering helpers
// ---------------------------------------------------------------------------

/// Build a full render tree for the current application state.
fn build_render_tree(state: &AppState, width: f32, height: f32) -> RenderTree {
    let mut rt = RenderTree::new();

    // Full background
    rt.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width,
        height,
        color: BASE,
        corner_radii: CornerRadii::ZERO,
    });

    let margin = 12.0_f32;
    let top_bar_h = 36.0_f32;
    let tab_bar_h = 32.0_f32;
    let stats_bar_h = 24.0_f32;
    let toolbar_h = 36.0_f32;

    // ---- Title bar / search area ----
    render_search_bar(&mut rt, state, margin, 8.0, width - margin * 2.0, top_bar_h);

    // ---- Tab bar ----
    let tab_y = 8.0 + top_bar_h + 4.0;
    render_tab_bar(
        &mut rt,
        state,
        margin,
        tab_y,
        width - margin * 2.0,
        tab_bar_h,
    );

    // ---- Content area ----
    let content_y = tab_y + tab_bar_h + 4.0;
    let content_h = height - content_y - stats_bar_h - toolbar_h - 12.0;

    match state.active_tab {
        ActiveTab::History => {
            render_history_panel(
                &mut rt,
                state,
                margin,
                content_y,
                width - margin * 2.0,
                content_h,
            );
        }
        ActiveTab::Templates => {
            render_templates_panel(
                &mut rt,
                state,
                margin,
                content_y,
                width - margin * 2.0,
                content_h,
            );
        }
    }

    // ---- Toolbar ----
    let toolbar_y = height - stats_bar_h - toolbar_h - 4.0;
    render_toolbar(
        &mut rt,
        state,
        margin,
        toolbar_y,
        width - margin * 2.0,
        toolbar_h,
    );

    // ---- Statistics bar ----
    let stats_y = height - stats_bar_h - 2.0;
    render_stats_bar(
        &mut rt,
        state,
        margin,
        stats_y,
        width - margin * 2.0,
        stats_bar_h,
    );

    rt
}

fn render_search_bar(rt: &mut RenderTree, state: &AppState, x: f32, y: f32, w: f32, h: f32) {
    // Background
    rt.push(RenderCommand::FillRect {
        x,
        y,
        width: w,
        height: h,
        color: SURFACE0,
        corner_radii: CornerRadii::all(6.0),
    });

    // Search icon placeholder
    rt.push(RenderCommand::Text {
        x: x + 10.0,
        y: y + 10.0,
        text: "Search:".to_string(),
        color: SUBTEXT0,
        font_size: 13.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // Search query text
    let query_display = if state.search_query.is_empty() {
        "type to filter..."
    } else {
        &state.search_query
    };
    let query_color = if state.search_query.is_empty() {
        OVERLAY0
    } else {
        TEXT
    };
    rt.push(RenderCommand::Text {
        x: x + 72.0,
        y: y + 10.0,
        text: query_display.to_string(),
        color: query_color,
        font_size: 13.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(w - 200.0),
        overflow: TextOverflow::Ellipsis,
    });

    // Type filter badge
    let filter_label = state.type_filter.map_or("All Types", |t| t.label());
    let filter_color = state.type_filter.map_or(OVERLAY0, |t| t.badge_color());
    let badge_x = x + w - 100.0;
    rt.push(RenderCommand::FillRect {
        x: badge_x,
        y: y + 7.0,
        width: 80.0,
        height: 22.0,
        color: SURFACE1,
        corner_radii: CornerRadii::all(4.0),
    });
    rt.push(RenderCommand::Text {
        x: badge_x + 8.0,
        y: y + 11.0,
        text: filter_label.to_string(),
        color: filter_color,
        font_size: 11.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
}

fn render_tab_bar(rt: &mut RenderTree, state: &AppState, x: f32, y: f32, w: f32, h: f32) {
    rt.push(RenderCommand::FillRect {
        x,
        y,
        width: w,
        height: h,
        color: MANTLE,
        corner_radii: CornerRadii::all(4.0),
    });

    let tabs = [
        ("History", ActiveTab::History),
        ("Templates", ActiveTab::Templates),
    ];
    let mut tx = x + 4.0;
    for (label, tab) in &tabs {
        let is_active = state.active_tab == *tab;
        let tab_w = 100.0_f32;
        if is_active {
            rt.push(RenderCommand::FillRect {
                x: tx,
                y: y + 2.0,
                width: tab_w,
                height: h - 4.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });
        }
        let text_color = if is_active { BLUE } else { SUBTEXT0 };
        rt.push(RenderCommand::Text {
            x: tx + 16.0,
            y: y + 8.0,
            text: label.to_string(),
            color: text_color,
            font_size: 13.0,
            font_weight: if is_active {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            },
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        tx += tab_w + 4.0;
    }
}

fn render_history_panel(rt: &mut RenderTree, state: &AppState, x: f32, y: f32, w: f32, h: f32) {
    // Split: left = entry list, right = preview/detail.
    let list_w = w * 0.55;
    let detail_w = w - list_w - 8.0;

    // ---- Entry list ----
    rt.push(RenderCommand::FillRect {
        x,
        y,
        width: list_w,
        height: h,
        color: SURFACE0,
        corner_radii: CornerRadii::all(6.0),
    });

    rt.push(RenderCommand::PushClip {
        x,
        y,
        width: list_w,
        height: h,
    });

    let row_h = 52.0_f32;
    let end = state
        .filtered_ids
        .len()
        .min(state.scroll_offset.saturating_add(state.visible_rows));
    let mut ry = y + 4.0;
    for i in state.scroll_offset..end {
        if let Some(&id) = state.filtered_ids.get(i)
            && let Some(entry) = state.store.get(id)
        {
            let is_selected = state.selected_id == Some(id);
            render_entry_row(
                rt,
                entry,
                x + 4.0,
                ry,
                list_w - 8.0,
                row_h,
                is_selected,
                state.now,
            );
        }
        ry += row_h + 2.0;
    }

    if state.filtered_ids.is_empty() {
        rt.push(RenderCommand::Text {
            x: x + 16.0,
            y: y + 24.0,
            text: "No entries".to_string(),
            color: OVERLAY0,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    rt.push(RenderCommand::PopClip);

    // ---- Detail/preview panel ----
    let detail_x = x + list_w + 8.0;
    rt.push(RenderCommand::FillRect {
        x: detail_x,
        y,
        width: detail_w,
        height: h,
        color: SURFACE0,
        corner_radii: CornerRadii::all(6.0),
    });

    if let Some(id) = state.selected_id {
        if let Some(entry) = state.store.get(id) {
            render_detail_panel(rt, entry, detail_x, y, detail_w, h, state.now);
        }
    } else {
        rt.push(RenderCommand::Text {
            x: detail_x + 16.0,
            y: y + 24.0,
            text: "Select an entry to preview".to_string(),
            color: OVERLAY0,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }
}

// 8 args mirror the (rt, entry, x, y, w, h, selected, now) row-render
// signature shared with the other entry painters; bundling into a struct
// would only shift verbosity to the call site.
#[allow(clippy::too_many_arguments)]
fn render_entry_row(
    rt: &mut RenderTree,
    entry: &ClipEntry,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    selected: bool,
    now: u64,
) {
    // Row background
    let bg = if selected { SURFACE1 } else { SURFACE0 };
    rt.push(RenderCommand::FillRect {
        x,
        y,
        width: w,
        height: h,
        color: bg,
        corner_radii: CornerRadii::all(4.0),
    });

    if selected {
        // Selection indicator
        rt.push(RenderCommand::FillRect {
            x,
            y,
            width: 3.0,
            height: h,
            color: BLUE,
            corner_radii: CornerRadii::ZERO,
        });
    }

    // Type badge
    let badge_color = entry.clip_type.badge_color();
    rt.push(RenderCommand::FillRect {
        x: x + 8.0,
        y: y + 6.0,
        width: 40.0,
        height: 16.0,
        color: badge_color,
        corner_radii: CornerRadii::all(3.0),
    });
    rt.push(RenderCommand::Text {
        x: x + 12.0,
        y: y + 7.0,
        text: entry.clip_type.label().to_string(),
        color: MANTLE,
        font_size: 10.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // Pin indicator
    if entry.pinned {
        rt.push(RenderCommand::Text {
            x: x + 52.0,
            y: y + 7.0,
            text: "PIN".to_string(),
            color: YELLOW,
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    // Preview text
    let preview = entry.preview();
    rt.push(RenderCommand::Text {
        x: x + 8.0,
        y: y + 26.0,
        text: preview,
        color: TEXT,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(w - 80.0),
        overflow: TextOverflow::Ellipsis,
    });

    // Timestamp + source on right
    let time_str = entry.time_display(now);
    rt.push(RenderCommand::Text {
        x: x + w - 70.0,
        y: y + 6.0,
        text: time_str,
        color: SUBTEXT0,
        font_size: 10.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    rt.push(RenderCommand::Text {
        x: x + w - 70.0,
        y: y + 18.0,
        text: entry.source_app.clone(),
        color: OVERLAY0,
        font_size: 10.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // Tag count
    if !entry.tags.is_empty() {
        let tag_str = format!("{} tags", entry.tags.len());
        rt.push(RenderCommand::Text {
            x: x + w - 70.0,
            y: y + 34.0,
            text: tag_str,
            color: TEAL,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }
}

fn render_detail_panel(
    rt: &mut RenderTree,
    entry: &ClipEntry,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    now: u64,
) {
    rt.push(RenderCommand::PushClip {
        x,
        y,
        width: w,
        height: h,
    });

    let pad = 12.0_f32;
    let mut cy = y + pad;

    // Title: type + id
    rt.push(RenderCommand::Text {
        x: x + pad,
        y: cy,
        text: format!("{} #{}", entry.clip_type.label(), entry.id),
        color: BLUE,
        font_size: 15.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    cy += 22.0;

    // Metadata line
    let meta = format!(
        "{} | {} | {}",
        entry.time_display(now),
        entry.source_app,
        entry.size_display()
    );
    rt.push(RenderCommand::Text {
        x: x + pad,
        y: cy,
        text: meta,
        color: SUBTEXT0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(w - pad * 2.0),
        overflow: TextOverflow::Ellipsis,
    });
    cy += 18.0;

    // Pinned status
    if entry.pinned {
        rt.push(RenderCommand::Text {
            x: x + pad,
            y: cy,
            text: "Pinned".to_string(),
            color: YELLOW,
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += 16.0;
    }

    // Tags
    if !entry.tags.is_empty() {
        let mut tx = x + pad;
        for tag in &entry.tags {
            let tag_w = text::padded_width(tag, 8.0, 10.0, FontWeightHint::Regular);
            rt.push(RenderCommand::FillRect {
                x: tx,
                y: cy,
                width: tag_w,
                height: 18.0,
                color: SURFACE1,
                corner_radii: CornerRadii::all(3.0),
            });
            rt.push(RenderCommand::Text {
                x: tx + 6.0,
                y: cy + 3.0,
                text: tag.clone(),
                color: TEAL,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            tx += tag_w + 4.0;
        }
        cy += 24.0;
    }

    // Code detection hint
    if (entry.clip_type == ClipType::Code || entry.clip_type == ClipType::PlainText)
        && let Some(lang) = detect_code_language(&entry.content)
    {
        rt.push(RenderCommand::FillRect {
            x: x + pad,
            y: cy,
            width: 100.0,
            height: 18.0,
            color: SURFACE1,
            corner_radii: CornerRadii::all(3.0),
        });
        rt.push(RenderCommand::Text {
            x: x + pad + 6.0,
            y: cy + 3.0,
            text: format!("lang: {lang}"),
            color: MAUVE,
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += 24.0;
    }

    // Separator
    cy += 4.0;
    rt.push(RenderCommand::Line {
        x1: x + pad,
        y1: cy,
        x2: x + w - pad,
        y2: cy,
        color: SURFACE1,
        width: 1.0,
    });
    cy += 8.0;

    // Full content preview
    let available_h = (y + h) - cy - pad;
    rt.push(RenderCommand::PushClip {
        x: x + pad,
        y: cy,
        width: w - pad * 2.0,
        height: available_h,
    });

    let line_h = 16.0_f32;
    let max_lines = (available_h / line_h) as usize;
    for (i, line) in entry.content.lines().enumerate() {
        if i >= max_lines {
            break;
        }
        rt.push(RenderCommand::Text {
            x: x + pad,
            y: cy + (i as f32) * line_h,
            text: line.to_string(),
            color: TEXT,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(w - pad * 2.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    rt.push(RenderCommand::PopClip);
    rt.push(RenderCommand::PopClip);
}

fn render_templates_panel(rt: &mut RenderTree, state: &AppState, x: f32, y: f32, w: f32, h: f32) {
    rt.push(RenderCommand::FillRect {
        x,
        y,
        width: w,
        height: h,
        color: SURFACE0,
        corner_radii: CornerRadii::all(6.0),
    });

    rt.push(RenderCommand::PushClip {
        x,
        y,
        width: w,
        height: h,
    });

    let pad = 12.0_f32;
    let mut cy = y + pad;

    // Section: existing templates
    rt.push(RenderCommand::Text {
        x: x + pad,
        y: cy,
        text: "Templates".to_string(),
        color: BLUE,
        font_size: 14.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    cy += 22.0;

    if state.store.templates.is_empty() {
        rt.push(RenderCommand::Text {
            x: x + pad,
            y: cy,
            text: "No templates defined. Create one below.".to_string(),
            color: OVERLAY0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += 20.0;
    } else {
        for (idx, tmpl) in state.store.templates.iter().enumerate() {
            let is_sel = state.selected_template == Some(idx);
            let row_bg = if is_sel { SURFACE1 } else { SURFACE0 };
            rt.push(RenderCommand::FillRect {
                x: x + pad,
                y: cy,
                width: w - pad * 2.0,
                height: 32.0,
                color: row_bg,
                corner_radii: CornerRadii::all(4.0),
            });
            rt.push(RenderCommand::Text {
                x: x + pad + 8.0,
                y: cy + 8.0,
                text: tmpl.name.clone(),
                color: if is_sel { BLUE } else { TEXT },
                font_size: 13.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Show placeholder count
            let ph_count = tmpl.placeholders().len();
            if ph_count > 0 {
                rt.push(RenderCommand::Text {
                    x: x + pad + 200.0,
                    y: cy + 10.0,
                    text: format!("{ph_count} placeholders"),
                    color: SUBTEXT0,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }
            cy += 36.0;
        }
    }

    // Separator
    cy += 8.0;
    rt.push(RenderCommand::Line {
        x1: x + pad,
        y1: cy,
        x2: x + w - pad,
        y2: cy,
        color: SURFACE1,
        width: 1.0,
    });
    cy += 12.0;

    // New template form
    rt.push(RenderCommand::Text {
        x: x + pad,
        y: cy,
        text: "New Template".to_string(),
        color: PEACH,
        font_size: 13.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    cy += 20.0;

    // Name field
    rt.push(RenderCommand::Text {
        x: x + pad,
        y: cy,
        text: "Name:".to_string(),
        color: SUBTEXT0,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    let name_display = if state.template_name_input.is_empty() {
        "e.g. Email Reply"
    } else {
        &state.template_name_input
    };
    rt.push(RenderCommand::FillRect {
        x: x + pad + 60.0,
        y: cy - 2.0,
        width: w - pad * 2.0 - 60.0,
        height: 20.0,
        color: MANTLE,
        corner_radii: CornerRadii::all(3.0),
    });
    rt.push(RenderCommand::Text {
        x: x + pad + 66.0,
        y: cy,
        text: name_display.to_string(),
        color: if state.template_name_input.is_empty() {
            OVERLAY0
        } else {
            TEXT
        },
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(w - pad * 2.0 - 80.0),
        overflow: TextOverflow::Ellipsis,
    });
    cy += 26.0;

    // Body field
    rt.push(RenderCommand::Text {
        x: x + pad,
        y: cy,
        text: "Body:".to_string(),
        color: SUBTEXT0,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    let body_display = if state.template_body_input.is_empty() {
        "Dear {name}, ..."
    } else {
        &state.template_body_input
    };
    rt.push(RenderCommand::FillRect {
        x: x + pad + 60.0,
        y: cy - 2.0,
        width: w - pad * 2.0 - 60.0,
        height: 40.0,
        color: MANTLE,
        corner_radii: CornerRadii::all(3.0),
    });
    rt.push(RenderCommand::Text {
        x: x + pad + 66.0,
        y: cy,
        text: body_display.to_string(),
        color: if state.template_body_input.is_empty() {
            OVERLAY0
        } else {
            TEXT
        },
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(w - pad * 2.0 - 80.0),
        overflow: TextOverflow::Ellipsis,
    });

    rt.push(RenderCommand::PopClip);
}

fn render_toolbar(rt: &mut RenderTree, _state: &AppState, x: f32, y: f32, w: f32, h: f32) {
    rt.push(RenderCommand::FillRect {
        x,
        y,
        width: w,
        height: h,
        color: MANTLE,
        corner_radii: CornerRadii::all(4.0),
    });

    let buttons = [
        ("Copy", BLUE),
        ("Pin", YELLOW),
        ("Delete", RED),
        ("Clear All", PEACH),
    ];
    let btn_w = 80.0_f32;
    let gap = 8.0_f32;
    let mut bx = x + 8.0;
    for (label, color) in &buttons {
        rt.push(RenderCommand::FillRect {
            x: bx,
            y: y + 5.0,
            width: btn_w,
            height: h - 10.0,
            color: SURFACE1,
            corner_radii: CornerRadii::all(4.0),
        });
        rt.push(RenderCommand::Text {
            x: bx + 12.0,
            y: y + 11.0,
            text: label.to_string(),
            color: *color,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        bx += btn_w + gap;
    }
}

fn render_stats_bar(rt: &mut RenderTree, state: &AppState, x: f32, y: f32, w: f32, h: f32) {
    rt.push(RenderCommand::FillRect {
        x,
        y,
        width: w,
        height: h,
        color: MANTLE,
        corner_radii: CornerRadii::all(3.0),
    });

    let stats = state.stats_line();
    rt.push(RenderCommand::Text {
        x: x + 10.0,
        y: y + 5.0,
        text: stats,
        color: SUBTEXT0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(w - 20.0),
        overflow: TextOverflow::Ellipsis,
    });

    // Show filtered count on right
    let filtered_info = format!("{} shown", state.filtered_ids.len());
    rt.push(RenderCommand::Text {
        x: x + w - 80.0,
        y: y + 5.0,
        text: filtered_info,
        color: OVERLAY0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
}

// ---------------------------------------------------------------------------
// Widget-tree builder (for toolkit integration)
// ---------------------------------------------------------------------------

/// Build a widget tree representing the clipboard manager UI.
fn build_widget_tree(state: &AppState) -> WidgetTree {
    let root = Widget::container()
        .with_background(BASE)
        .with_flex_direction(FlexDirection::Column)
        .with_padding(Edges::all(8.0))
        .with_child(
            Widget::label(&format!("Clipboard Manager - {}", state.stats_line()))
                .with_background(SURFACE0)
                .with_padding(Edges::symmetric(6.0, 12.0)),
        );
    WidgetTree::new(root, 800.0, 600.0)
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // == Export format ======================================================

    /// Contents a user could plausibly copy that the old format could not
    /// survive. Every one of these is ordinary text somewhere -- a diff, a
    /// config file, a paste of this very export -- not a crafted payload.
    const HOSTILE_CONTENT: &[&str] = &[
        "---ENTRY---",
        "before\n---ENTRY---\nsource:forged\npinned:true\ncontent:3\nbad\n",
        "content:0",
        "id:99\ntype:Code\n",
        "\n\n   \n",
        "trailing newline\n",
        "  leading and trailing spaces  ",
        "",
        "tag:injected",
    ];

    /// Round-trip a store through the text format, returning the reloaded one.
    fn reload(store: &ClipboardStore) -> ClipboardStore {
        let text = store.export_text();
        let mut out = ClipboardStore::new();
        out.import_text(&text, 500);
        out
    }

    #[test]
    fn copied_text_cannot_forge_a_second_entry() {
        for content in HOSTILE_CONTENT {
            let mut store = ClipboardStore::new();
            store.add(
                (*content).to_string(),
                ClipType::PlainText,
                100,
                "real".to_string(),
            );
            let back = reload(&store);
            // One entry in, one entry out -- regardless of what it contained.
            // Counting is the assertion because correct output legitimately
            // *contains* the marker; only the record count can tell whether it
            // was interpreted as one.
            assert_eq!(
                back.entries.len(),
                1,
                "content forged a record boundary: {content:?}"
            );
            let entry = back.entries.front().expect("one entry");
            assert_eq!(entry.content, *content, "content changed: {content:?}");
            assert_eq!(entry.source_app, "real", "content forged a header field");
            assert!(!entry.pinned, "content forged a header field");
            assert!(entry.tags.is_empty(), "content forged a header field");
        }
    }

    #[test]
    fn several_hostile_entries_survive_together() {
        // Alone, a bad record could be masked by the parser recovering at the
        // end of input; in a run, a miscount shifts every entry after it.
        let mut store = ClipboardStore::new();
        for (i, content) in HOSTILE_CONTENT.iter().enumerate() {
            store.add(
                format!("{i}:{content}"),
                ClipType::PlainText,
                100,
                "real".to_string(),
            );
        }
        let expected = store.entries.len();
        let back = reload(&store);
        assert_eq!(back.entries.len(), expected);
        for (got, want) in back.entries.iter().zip(store.entries.iter()) {
            assert_eq!(got.content, want.content, "history order changed");
        }
    }

    #[test]
    fn a_tag_containing_a_comma_stays_one_tag() {
        let mut store = ClipboardStore::new();
        let id = store.add("x".to_string(), ClipType::PlainText, 100, "app".to_string());
        if let Some(e) = store.get_mut(id) {
            e.tags = vec!["a,b".to_string(), "plain".to_string()];
        }
        let back = reload(&store);
        let entry = back.entries.front().expect("one entry");
        assert_eq!(entry.tags, vec!["a,b".to_string(), "plain".to_string()]);
    }

    #[test]
    fn a_newline_in_a_source_name_cannot_add_a_field() {
        let mut store = ClipboardStore::new();
        store.add(
            "x".to_string(),
            ClipType::PlainText,
            100,
            "app\npinned:true\ntag:forged".to_string(),
        );
        let back = reload(&store);
        assert_eq!(back.entries.len(), 1);
        let entry = back.entries.front().expect("one entry");
        assert!(!entry.pinned, "source name forged a field");
        assert!(entry.tags.is_empty(), "source name forged a field");
    }

    #[test]
    fn a_truncated_export_yields_the_entries_it_has() {
        let mut store = ClipboardStore::new();
        store.add(
            "first".to_string(),
            ClipType::PlainText,
            100,
            "a".to_string(),
        );
        store.add(
            "second entry".to_string(),
            ClipType::PlainText,
            100,
            "a".to_string(),
        );
        let text = store.export_text();
        let cut = text.len().saturating_sub(6);
        let mut back = ClipboardStore::new();
        // Must not panic, and must not invent an entry.
        let n = back.import_text(text.get(..cut).unwrap_or(""), 500);
        assert!(n <= 2);
        assert_eq!(back.entries.len(), n);
    }

    #[test]
    fn a_marker_mentioned_inside_a_line_does_not_start_a_record() {
        // Reachable because the scan for the first record runs over whatever
        // preamble the file happens to have -- a covering note, an email
        // header, a paste into a bug report. Inside a record this case cannot
        // arise, since the body is skipped by length and header values are
        // sanitised; the equality test is what keeps the preamble honest.
        let mut back = ClipboardStore::new();
        let n = back.import_text(
            "note: the ---ENTRY--- lines below are records\ncontent:5\nfake\n",
            0,
        );
        assert_eq!(n, 0, "a mention of the marker started a record");
        assert!(back.entries.is_empty());
    }

    #[test]
    fn a_declared_length_past_the_end_does_not_panic() {
        let mut back = ClipboardStore::new();
        back.import_text("---ENTRY---\ncontent:9999\nshort", 0);
        assert_eq!(back.entries.len(), 1);
        assert_eq!(
            back.entries.front().map(|e| e.content.as_str()),
            Some("short")
        );
    }

    #[test]
    fn a_declared_length_inside_a_character_does_not_panic() {
        // 3 bytes into a 3-byte character's 4-byte neighbour: the length lands
        // mid-scalar, which `str` slicing would reject.
        let mut back = ClipboardStore::new();
        back.import_text("---ENTRY---\ncontent:2\n\u{1F600}", 0);
        assert_eq!(back.entries.len(), 1);
    }

    // == ClipEntry tests ====================================================

    #[test]
    fn test_clip_entry_creation() {
        let e = ClipEntry::new(
            1,
            "hello".to_string(),
            ClipType::PlainText,
            100,
            "app".to_string(),
        );
        assert_eq!(e.id, 1);
        assert_eq!(e.content, "hello");
        assert_eq!(e.clip_type, ClipType::PlainText);
        assert!(!e.pinned);
        assert!(e.tags.is_empty());
        assert_eq!(e.size_bytes, 5);
    }

    #[test]
    fn test_clip_entry_preview_short() {
        let e = ClipEntry::new(
            1,
            "short text".to_string(),
            ClipType::PlainText,
            0,
            String::new(),
        );
        assert_eq!(e.preview(), "short text");
    }

    #[test]
    fn test_clip_entry_preview_multiline() {
        let text = "line one\nline two\nline three\nline four";
        let e = ClipEntry::new(1, text.to_string(), ClipType::PlainText, 0, String::new());
        let p = e.preview();
        assert!(p.contains("line one"));
        assert!(p.contains("line two"));
        assert!(!p.contains("line three"));
    }

    #[test]
    fn test_clip_entry_preview_truncation() {
        let long = "a".repeat(200);
        let e = ClipEntry::new(1, long, ClipType::PlainText, 0, String::new());
        let p = e.preview();
        // Characters, not bytes — see `preview`. On this all-ASCII input the
        // two agree, which is why this test alone could not see the bug.
        assert_eq!(p.chars().count(), PREVIEW_MAX_CHARS + 3); // +3 for "..."
    }

    /// The clipboard holds whatever the user copied. `preview` used to measure
    /// it in bytes and then cut it at byte 120 with `String::truncate`, which
    /// panics rather than rounding when that offset is inside a character — so
    /// copying a couple of sentences of any non-Latin script crashed the
    /// clipboard manager as it drew its own list.
    #[test]
    fn a_long_multibyte_preview_is_cut_by_characters_and_does_not_panic() {
        // A uniform-width script does *not* demonstrate this on its own:
        // `PREVIEW_MAX_CHARS` is 120, which is divisible by 2, 3 and 4, so byte
        // 120 of solid Cyrillic, CJK or emoji lands on a boundary by accident
        // and the old code survived. Every case below is asserted to actually
        // split a character, so this test cannot quietly stop testing the bug
        // if the constant changes.
        // Note these are a *single* ASCII prefix followed by a run, not a
        // repeated pattern: repeating "xЯ" gives a 3-byte period, and 120 is
        // divisible by that too. Shifting the whole run by one or two bytes is
        // what actually moves byte 120 off the grid.
        for text in [
            format!("x{}", "Я".repeat(200)),  // 2-byte run, shifted by 1
            format!("x{}", "日".repeat(200)), // 3-byte run, shifted by 1
            format!("x{}", "🎉".repeat(200)), // 4-byte run, shifted by 1
            format!("xx{}", "日".repeat(200)), // 3-byte run, shifted by 2
        ] {
            let head: String = text.chars().take(4).collect();
            assert!(
                !text.is_char_boundary(PREVIEW_MAX_CHARS),
                "this case no longer splits a character: {head:?}"
            );
            let e = ClipEntry::new(1, text.clone(), ClipType::PlainText, 0, String::new());
            let p = e.preview();
            assert_eq!(p.chars().count(), PREVIEW_MAX_CHARS + 3, "on {head:?}");
            assert!(p.ends_with("..."));
            // Every character kept is a whole one from the original.
            assert!(text.starts_with(p.trim_end_matches('.')));
        }
    }

    /// 120 characters of Japanese is 360 bytes. Counting bytes cut that to 40
    /// characters — a third of the preview an English entry got — on the
    /// entries that did not crash outright.
    #[test]
    fn a_preview_holds_the_same_number_of_characters_in_every_script() {
        let ascii = ClipEntry::new(1, "a".repeat(300), ClipType::PlainText, 0, String::new());
        let cjk = ClipEntry::new(2, "日".repeat(300), ClipType::PlainText, 0, String::new());
        assert_eq!(
            ascii.preview().chars().count(),
            cjk.preview().chars().count()
        );
    }

    /// Short content is returned whole, with no ellipsis and no padding.
    #[test]
    fn a_short_preview_is_verbatim() {
        for text in ["", "hi", "héllo wörld", "日本語"] {
            let e = ClipEntry::new(1, text.to_string(), ClipType::PlainText, 0, String::new());
            assert_eq!(e.preview(), text);
        }
    }

    #[test]
    fn test_clip_entry_size_display() {
        let e = ClipEntry::new(1, "x".repeat(500), ClipType::PlainText, 0, String::new());
        assert_eq!(e.size_display(), "500 B");
    }

    #[test]
    fn test_clip_entry_time_display_seconds() {
        let e = ClipEntry::new(1, String::new(), ClipType::PlainText, 990, String::new());
        assert_eq!(e.time_display(1000), "10s ago");
    }

    #[test]
    fn test_clip_entry_time_display_minutes() {
        let e = ClipEntry::new(1, String::new(), ClipType::PlainText, 700, String::new());
        assert_eq!(e.time_display(1000), "5m ago");
    }

    #[test]
    fn test_clip_entry_time_display_hours() {
        let e = ClipEntry::new(1, String::new(), ClipType::PlainText, 0, String::new());
        assert_eq!(e.time_display(7200), "2h ago");
    }

    #[test]
    fn test_clip_entry_time_display_days() {
        let e = ClipEntry::new(1, String::new(), ClipType::PlainText, 0, String::new());
        assert_eq!(e.time_display(172800), "2d ago");
    }

    // == ClipType tests =====================================================

    #[test]
    fn test_clip_type_label() {
        assert_eq!(ClipType::PlainText.label(), "Text");
        assert_eq!(ClipType::RichText.label(), "Rich");
        assert_eq!(ClipType::Html.label(), "HTML");
        assert_eq!(ClipType::Image.label(), "Image");
        assert_eq!(ClipType::FilePaths.label(), "Files");
        assert_eq!(ClipType::Code.label(), "Code");
    }

    #[test]
    fn test_clip_type_all_variants() {
        assert_eq!(ClipType::all().len(), 6);
    }

    #[test]
    fn test_clip_type_badge_colors_unique() {
        let colors: Vec<Color> = ClipType::all().iter().map(|t| t.badge_color()).collect();
        for (i, c) in colors.iter().enumerate() {
            for (j, d) in colors.iter().enumerate() {
                if i != j {
                    assert_ne!(c, d, "Badge colors must be unique");
                }
            }
        }
    }

    // == ClipboardStore tests ===============================================

    #[test]
    fn test_store_add_and_get() {
        let mut store = ClipboardStore::new();
        let id = store.add(
            "hello".to_string(),
            ClipType::PlainText,
            100,
            "vim".to_string(),
        );
        let entry = store.get(id);
        assert!(entry.is_some());
        assert_eq!(entry.map(|e| e.content.as_str()), Some("hello"));
    }

    #[test]
    fn test_store_deduplication() {
        let mut store = ClipboardStore::new();
        let id1 = store.add("dup".to_string(), ClipType::PlainText, 100, "a".to_string());
        let id2 = store.add("dup".to_string(), ClipType::PlainText, 200, "b".to_string());
        assert_eq!(id1, id2);
        assert_eq!(store.total_entries(), 1);
    }

    #[test]
    fn test_store_ordering_most_recent_first() {
        let mut store = ClipboardStore::new();
        store.add("first".to_string(), ClipType::PlainText, 1, String::new());
        store.add("second".to_string(), ClipType::PlainText, 2, String::new());
        let front = store.entries.front().map(|e| e.content.as_str());
        assert_eq!(front, Some("second"));
    }

    #[test]
    fn test_store_capacity_eviction() {
        let mut store = ClipboardStore::new();
        for i in 0..MAX_ENTRIES + 10 {
            store.add(
                format!("entry-{i}"),
                ClipType::PlainText,
                i as u64,
                String::new(),
            );
        }
        assert!(store.total_entries() <= MAX_ENTRIES);
    }

    #[test]
    fn test_store_pinned_not_evicted() {
        let mut store = ClipboardStore::new();
        let pin_id = store.add("pinned".to_string(), ClipType::PlainText, 0, String::new());
        store.toggle_pin(pin_id);
        for i in 1..=MAX_ENTRIES {
            store.add(
                format!("entry-{i}"),
                ClipType::PlainText,
                i as u64,
                String::new(),
            );
        }
        assert!(
            store.get(pin_id).is_some(),
            "Pinned entry must survive eviction"
        );
    }

    #[test]
    fn test_store_delete() {
        let mut store = ClipboardStore::new();
        let id = store.add("del".to_string(), ClipType::PlainText, 0, String::new());
        assert!(store.delete(id));
        assert!(store.get(id).is_none());
    }

    #[test]
    fn test_store_delete_nonexistent() {
        let mut store = ClipboardStore::new();
        assert!(!store.delete(999));
    }

    #[test]
    fn test_store_delete_many() {
        let mut store = ClipboardStore::new();
        let a = store.add("a".to_string(), ClipType::PlainText, 0, String::new());
        let b = store.add("b".to_string(), ClipType::PlainText, 0, String::new());
        let c = store.add("c".to_string(), ClipType::PlainText, 0, String::new());
        store.delete_many(&[a, c]);
        assert!(store.get(a).is_none());
        assert!(store.get(b).is_some());
        assert!(store.get(c).is_none());
    }

    #[test]
    fn test_store_clear_unpinned() {
        let mut store = ClipboardStore::new();
        let a = store.add("a".to_string(), ClipType::PlainText, 0, String::new());
        let b = store.add("b".to_string(), ClipType::PlainText, 0, String::new());
        store.toggle_pin(a);
        store.clear_unpinned();
        assert!(store.get(a).is_some());
        assert!(store.get(b).is_none());
        assert_eq!(store.total_entries(), 1);
    }

    #[test]
    fn test_store_search_case_insensitive() {
        let mut store = ClipboardStore::new();
        store.add(
            "Hello World".to_string(),
            ClipType::PlainText,
            0,
            String::new(),
        );
        store.add(
            "goodbye world".to_string(),
            ClipType::PlainText,
            0,
            String::new(),
        );
        let results = store.search("HELLO");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_store_search_empty_returns_all() {
        let mut store = ClipboardStore::new();
        store.add("a".to_string(), ClipType::PlainText, 0, String::new());
        store.add("b".to_string(), ClipType::PlainText, 0, String::new());
        assert_eq!(store.search("").len(), 2);
    }

    #[test]
    fn test_store_filter_by_type() {
        let mut store = ClipboardStore::new();
        store.add("text".to_string(), ClipType::PlainText, 0, String::new());
        store.add("<b>bold</b>".to_string(), ClipType::Html, 0, String::new());
        store.add("fn main()".to_string(), ClipType::Code, 0, String::new());
        assert_eq!(store.filter_by_type(ClipType::Html).len(), 1);
        assert_eq!(store.filter_by_type(ClipType::Code).len(), 1);
        assert_eq!(store.filter_by_type(ClipType::Image).len(), 0);
    }

    #[test]
    fn test_store_filter_by_tag() {
        let mut store = ClipboardStore::new();
        let id = store.add("tagged".to_string(), ClipType::PlainText, 0, String::new());
        store.add_tag(id, "important".to_string());
        store.add(
            "untagged".to_string(),
            ClipType::PlainText,
            0,
            String::new(),
        );
        assert_eq!(store.filter_by_tag("important").len(), 1);
        assert_eq!(store.filter_by_tag("nope").len(), 0);
    }

    #[test]
    fn test_store_search_filtered() {
        let mut store = ClipboardStore::new();
        store.add(
            "hello text".to_string(),
            ClipType::PlainText,
            0,
            String::new(),
        );
        store.add(
            "<p>hello html</p>".to_string(),
            ClipType::Html,
            0,
            String::new(),
        );
        let results = store.search_filtered("hello", Some(ClipType::Html));
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_store_tag_operations() {
        let mut store = ClipboardStore::new();
        let id = store.add("x".to_string(), ClipType::PlainText, 0, String::new());
        store.add_tag(id, "work".to_string());
        store.add_tag(id, "work".to_string()); // duplicate ignored
        assert_eq!(store.get(id).map(|e| e.tags.len()), Some(1));
        store.remove_tag(id, "work");
        assert_eq!(store.get(id).map(|e| e.tags.len()), Some(0));
    }

    #[test]
    fn test_store_all_tags() {
        let mut store = ClipboardStore::new();
        let a = store.add("a".to_string(), ClipType::PlainText, 0, String::new());
        let b = store.add("b".to_string(), ClipType::PlainText, 0, String::new());
        store.add_tag(a, "alpha".to_string());
        store.add_tag(b, "beta".to_string());
        store.add_tag(b, "alpha".to_string());
        let tags = store.all_tags();
        assert_eq!(tags.len(), 2);
        assert!(tags.contains(&"alpha".to_string()));
        assert!(tags.contains(&"beta".to_string()));
    }

    #[test]
    fn test_store_toggle_pin() {
        let mut store = ClipboardStore::new();
        let id = store.add("x".to_string(), ClipType::PlainText, 0, String::new());
        assert_eq!(store.get(id).map(|e| e.pinned), Some(false));
        store.toggle_pin(id);
        assert_eq!(store.get(id).map(|e| e.pinned), Some(true));
        store.toggle_pin(id);
        assert_eq!(store.get(id).map(|e| e.pinned), Some(false));
    }

    #[test]
    fn test_store_stats_by_type() {
        let mut store = ClipboardStore::new();
        store.add("a".to_string(), ClipType::PlainText, 0, String::new());
        store.add("b".to_string(), ClipType::PlainText, 0, String::new());
        store.add("c".to_string(), ClipType::Code, 0, String::new());
        let stats = store.stats_by_type();
        let text_count = stats
            .iter()
            .find(|(t, _)| *t == ClipType::PlainText)
            .map(|(_, c)| *c);
        let code_count = stats
            .iter()
            .find(|(t, _)| *t == ClipType::Code)
            .map(|(_, c)| *c);
        assert_eq!(text_count, Some(2));
        assert_eq!(code_count, Some(1));
    }

    #[test]
    fn test_store_total_size_tracking() {
        let mut store = ClipboardStore::new();
        store.add("12345".to_string(), ClipType::PlainText, 0, String::new());
        assert_eq!(store.total_size, 5);
        store.add("abc".to_string(), ClipType::PlainText, 0, String::new());
        assert_eq!(store.total_size, 8);
    }

    #[test]
    fn test_store_total_size_after_delete() {
        let mut store = ClipboardStore::new();
        let id = store.add("12345".to_string(), ClipType::PlainText, 0, String::new());
        store.delete(id);
        assert_eq!(store.total_size, 0);
    }

    #[test]
    fn test_store_pinned_count() {
        let mut store = ClipboardStore::new();
        let a = store.add("a".to_string(), ClipType::PlainText, 0, String::new());
        store.add("b".to_string(), ClipType::PlainText, 0, String::new());
        store.toggle_pin(a);
        assert_eq!(store.pinned_count(), 1);
    }

    // == Template tests =====================================================

    #[test]
    fn test_template_render_no_placeholders() {
        let t = ClipTemplate::new("greeting".to_string(), "Hello!".to_string());
        assert_eq!(t.render(&[]), "Hello!");
    }

    #[test]
    fn test_template_render_with_placeholders() {
        let t = ClipTemplate::new(
            "email".to_string(),
            "Dear {name}, re: {subject}".to_string(),
        );
        let vars = vec![
            ("name".to_string(), "Alice".to_string()),
            ("subject".to_string(), "Meeting".to_string()),
        ];
        assert_eq!(t.render(&vars), "Dear Alice, re: Meeting");
    }

    #[test]
    fn test_template_render_missing_var() {
        let t = ClipTemplate::new("t".to_string(), "Hello {who}!".to_string());
        let result = t.render(&[]);
        assert_eq!(result, "Hello {who}!");
    }

    #[test]
    fn test_template_placeholders_extraction() {
        let t = ClipTemplate::new("t".to_string(), "{a} and {b} and {a}".to_string());
        let ph = t.placeholders();
        assert_eq!(ph.len(), 2);
        assert!(ph.contains(&"a".to_string()));
        assert!(ph.contains(&"b".to_string()));
    }

    #[test]
    fn test_template_placeholders_empty() {
        let t = ClipTemplate::new("t".to_string(), "no placeholders here".to_string());
        assert!(t.placeholders().is_empty());
    }

    #[test]
    fn test_store_add_template() {
        let mut store = ClipboardStore::new();
        store.add_template("greet".to_string(), "Hi {name}".to_string());
        assert_eq!(store.templates.len(), 1);
    }

    #[test]
    fn test_store_add_template_replaces_duplicate_name() {
        let mut store = ClipboardStore::new();
        store.add_template("greet".to_string(), "Hi {name}".to_string());
        store.add_template("greet".to_string(), "Hey {name}!".to_string());
        assert_eq!(store.templates.len(), 1);
        assert_eq!(
            store.get_template("greet").map(|t| t.body.as_str()),
            Some("Hey {name}!")
        );
    }

    #[test]
    fn test_store_remove_template() {
        let mut store = ClipboardStore::new();
        store.add_template("greet".to_string(), "Hi".to_string());
        store.remove_template("greet");
        assert!(store.templates.is_empty());
    }

    // == Export/Import tests ================================================

    #[test]
    fn test_export_import_roundtrip() {
        let mut store = ClipboardStore::new();
        let id = store.add(
            "test content".to_string(),
            ClipType::PlainText,
            100,
            "editor".to_string(),
        );
        store.toggle_pin(id);
        store.add_tag(id, "important".to_string());
        let exported = store.export_text();

        let mut store2 = ClipboardStore::new();
        let count = store2.import_text(&exported, 200);
        assert_eq!(count, 1);
        let entry = store2.entries.front();
        assert!(entry.is_some());
        let entry = entry.map(|e| (e.content.as_str(), e.pinned, e.tags.len()));
        assert_eq!(entry, Some(("test content", true, 1)));
    }

    #[test]
    fn test_import_empty() {
        let mut store = ClipboardStore::new();
        assert_eq!(store.import_text("", 0), 0);
    }

    #[test]
    fn test_export_multiple_entries() {
        let mut store = ClipboardStore::new();
        store.add("aaa".to_string(), ClipType::PlainText, 1, String::new());
        store.add("bbb".to_string(), ClipType::Code, 2, String::new());
        let text = store.export_text();
        assert!(text.contains("aaa"));
        assert!(text.contains("bbb"));
        assert!(text.contains("Code"));
    }

    // == Code detection tests ===============================================

    #[test]
    fn test_detect_rust() {
        assert_eq!(
            detect_code_language("fn main() -> Result<()> { }"),
            Some("rust")
        );
    }

    #[test]
    fn test_detect_python() {
        assert_eq!(
            detect_code_language("def hello():\n    pass"),
            Some("python")
        );
    }

    #[test]
    fn test_detect_javascript() {
        assert_eq!(
            detect_code_language("function foo() {}"),
            Some("javascript")
        );
    }

    #[test]
    fn test_detect_c() {
        assert_eq!(detect_code_language("#include <stdio.h>"), Some("c/c++"));
    }

    #[test]
    fn test_detect_java() {
        assert_eq!(detect_code_language("public class Foo {}"), Some("java"));
    }

    #[test]
    fn test_detect_generic_code() {
        let code = "if (x) {\n  y = 1;\n  z = 2;\n}";
        assert_eq!(detect_code_language(code), Some("generic"));
    }

    #[test]
    fn test_detect_plain_text() {
        assert_eq!(
            detect_code_language("Hello world, how are you today?"),
            None
        );
    }

    // == Format size tests ==================================================

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(42), "42 B");
    }

    #[test]
    fn test_format_size_kilobytes() {
        assert_eq!(format_size(2048), "2.0 KB");
    }

    #[test]
    fn test_format_size_megabytes() {
        assert_eq!(format_size(2 * 1024 * 1024), "2.00 MB");
    }

    // == AppState tests =====================================================

    #[test]
    fn test_app_state_refresh_filter() {
        let mut state = AppState::new();
        state
            .store
            .add("hello".to_string(), ClipType::PlainText, 0, String::new());
        state
            .store
            .add("world".to_string(), ClipType::PlainText, 0, String::new());
        state.refresh_filter();
        assert_eq!(state.filtered_ids.len(), 2);
    }

    #[test]
    fn test_app_state_search_filter() {
        let mut state = AppState::new();
        state
            .store
            .add("alpha".to_string(), ClipType::PlainText, 0, String::new());
        state
            .store
            .add("beta".to_string(), ClipType::PlainText, 0, String::new());
        state.search_query = "alpha".to_string();
        state.refresh_filter();
        assert_eq!(state.filtered_ids.len(), 1);
    }

    #[test]
    fn test_app_state_type_filter() {
        let mut state = AppState::new();
        state
            .store
            .add("txt".to_string(), ClipType::PlainText, 0, String::new());
        state
            .store
            .add("code".to_string(), ClipType::Code, 0, String::new());
        state.type_filter = Some(ClipType::Code);
        state.refresh_filter();
        assert_eq!(state.filtered_ids.len(), 1);
    }

    #[test]
    fn test_app_state_select_next_prev() {
        let mut state = AppState::new();
        state
            .store
            .add("a".to_string(), ClipType::PlainText, 0, String::new());
        state
            .store
            .add("b".to_string(), ClipType::PlainText, 0, String::new());
        state
            .store
            .add("c".to_string(), ClipType::PlainText, 0, String::new());
        state.refresh_filter();

        state.select_next();
        let first = state.selected_id;
        assert!(first.is_some());

        state.select_next();
        let second = state.selected_id;
        assert_ne!(first, second);

        state.select_prev();
        assert_eq!(state.selected_id, first);
    }

    #[test]
    fn test_app_state_select_on_empty() {
        let mut state = AppState::new();
        state.refresh_filter();
        state.select_next(); // should not panic
        state.select_prev(); // should not panic
        assert!(state.selected_id.is_none());
    }

    #[test]
    fn test_app_state_delete_selected() {
        let mut state = AppState::new();
        let id = state
            .store
            .add("del".to_string(), ClipType::PlainText, 0, String::new());
        state.refresh_filter();
        state.selected_id = Some(id);
        state.delete_selected();
        assert!(state.selected_id.is_none());
        assert_eq!(state.store.total_entries(), 0);
    }

    #[test]
    fn test_app_state_toggle_pin_selected() {
        let mut state = AppState::new();
        let id = state
            .store
            .add("pin".to_string(), ClipType::PlainText, 0, String::new());
        state.selected_id = Some(id);
        state.toggle_pin_selected();
        assert_eq!(state.store.get(id).map(|e| e.pinned), Some(true));
    }

    #[test]
    fn test_app_state_add_tag_to_selected() {
        let mut state = AppState::new();
        let id = state
            .store
            .add("t".to_string(), ClipType::PlainText, 0, String::new());
        state.selected_id = Some(id);
        state.tag_input = "work".to_string();
        state.add_tag_to_selected();
        assert!(state.tag_input.is_empty());
        assert_eq!(state.store.get(id).map(|e| e.tags.len()), Some(1));
    }

    #[test]
    fn test_app_state_add_empty_tag_ignored() {
        let mut state = AppState::new();
        let id = state
            .store
            .add("t".to_string(), ClipType::PlainText, 0, String::new());
        state.selected_id = Some(id);
        state.tag_input = "   ".to_string();
        state.add_tag_to_selected();
        assert_eq!(state.store.get(id).map(|e| e.tags.len()), Some(0));
    }

    #[test]
    fn test_app_state_remove_tag_from_selected() {
        let mut state = AppState::new();
        let id = state
            .store
            .add("t".to_string(), ClipType::PlainText, 0, String::new());
        state.store.add_tag(id, "work".to_string());
        state.selected_id = Some(id);
        state.remove_tag_from_selected("work");
        assert_eq!(state.store.get(id).map(|e| e.tags.len()), Some(0));
    }

    #[test]
    fn test_app_state_save_template() {
        let mut state = AppState::new();
        state.template_name_input = "greet".to_string();
        state.template_body_input = "Hi {name}".to_string();
        state.save_template();
        assert_eq!(state.store.templates.len(), 1);
        assert!(state.template_name_input.is_empty());
        assert!(state.template_body_input.is_empty());
    }

    #[test]
    fn test_app_state_save_empty_template_ignored() {
        let mut state = AppState::new();
        state.template_name_input = String::new();
        state.template_body_input = "body".to_string();
        state.save_template();
        assert!(state.store.templates.is_empty());
    }

    #[test]
    fn test_app_state_delete_selected_template() {
        let mut state = AppState::new();
        state
            .store
            .add_template("t".to_string(), "body".to_string());
        state.selected_template = Some(0);
        state.delete_selected_template();
        assert!(state.store.templates.is_empty());
        assert!(state.selected_template.is_none());
    }

    #[test]
    fn test_app_state_select_template_populates_vars() {
        let mut state = AppState::new();
        state.store.add_template(
            "email".to_string(),
            "Dear {name}, re: {subject}".to_string(),
        );
        state.select_template(0);
        assert_eq!(state.template_vars.len(), 2);
    }

    #[test]
    fn test_app_state_render_template() {
        let mut state = AppState::new();
        state
            .store
            .add_template("greet".to_string(), "Hi {name}!".to_string());
        state.select_template(0);
        state.template_vars = vec![("name".to_string(), "Bob".to_string())];
        state.render_template();
        // Should have added a new entry
        assert_eq!(state.store.total_entries(), 1);
        let front = state.store.entries.front().map(|e| e.content.as_str());
        assert_eq!(front, Some("Hi Bob!"));
    }

    #[test]
    fn test_app_state_stats_line() {
        let mut state = AppState::new();
        state
            .store
            .add("data".to_string(), ClipType::PlainText, 0, String::new());
        let line = state.stats_line();
        assert!(line.contains("1 entries"));
        assert!(line.contains("0 pinned"));
    }

    // == Render tests =======================================================

    #[test]
    fn test_build_render_tree_not_empty() {
        let state = AppState::new();
        let rt = build_render_tree(&state, 800.0, 600.0);
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_build_render_tree_with_entries() {
        let mut state = AppState::new();
        state.store.add(
            "hello".to_string(),
            ClipType::PlainText,
            100,
            "app".to_string(),
        );
        state.refresh_filter();
        state.selected_id = state.filtered_ids.first().copied();
        let rt = build_render_tree(&state, 800.0, 600.0);
        assert!(rt.commands.len() > 5);
    }

    #[test]
    fn test_build_render_tree_templates_tab() {
        let mut state = AppState::new();
        state.active_tab = ActiveTab::Templates;
        state
            .store
            .add_template("t".to_string(), "body".to_string());
        let rt = build_render_tree(&state, 800.0, 600.0);
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_build_widget_tree() {
        let state = AppState::new();
        let wt = build_widget_tree(&state);
        assert!(wt.window_width > 0.0);
        assert!(wt.window_height > 0.0);
    }

    // == Edge case / stress tests ==========================================

    #[test]
    fn test_store_many_entries_and_search() {
        let mut store = ClipboardStore::new();
        for i in 0..200 {
            store.add(
                format!("entry number {i}"),
                ClipType::PlainText,
                i,
                String::new(),
            );
        }
        let results = store.search("number 15");
        // Should match "entry number 15", "entry number 150", etc.
        assert!(!results.is_empty());
    }

    #[test]
    fn test_scroll_offset_adjustment() {
        let mut state = AppState::new();
        for i in 0..30 {
            state
                .store
                .add(format!("e{i}"), ClipType::PlainText, i, String::new());
        }
        state.refresh_filter();
        state.visible_rows = 5;
        // Navigate down past visible window
        for _ in 0..10 {
            state.select_next();
        }
        assert!(state.scroll_offset > 0);
    }

    #[test]
    fn test_dedup_updates_timestamp_and_source() {
        let mut store = ClipboardStore::new();
        store.add(
            "same".to_string(),
            ClipType::PlainText,
            10,
            "old".to_string(),
        );
        store.add(
            "same".to_string(),
            ClipType::PlainText,
            20,
            "new".to_string(),
        );
        let front = store.entries.front();
        assert_eq!(front.map(|e| e.timestamp), Some(20));
        assert_eq!(front.map(|e| e.source_app.as_str()), Some("new"));
    }

    #[test]
    fn test_import_preserves_type() {
        let mut store = ClipboardStore::new();
        store.add("<b>x</b>".to_string(), ClipType::Html, 0, String::new());
        let exported = store.export_text();

        let mut store2 = ClipboardStore::new();
        store2.import_text(&exported, 100);
        let entry = store2.entries.front();
        assert_eq!(entry.map(|e| e.clip_type), Some(ClipType::Html));
    }

    #[test]
    fn test_template_multiple_same_placeholder() {
        let t = ClipTemplate::new("t".to_string(), "{x} and {x}".to_string());
        let result = t.render(&[("x".to_string(), "val".to_string())]);
        assert_eq!(result, "val and val");
    }

    #[test]
    fn test_render_tree_history_selected_detail() {
        let mut state = AppState::new();
        let id = state.store.add(
            "fn main() -> Result<()> { Ok(()) }".to_string(),
            ClipType::Code,
            500,
            "vscode".to_string(),
        );
        state.store.add_tag(id, "rust".to_string());
        state.store.toggle_pin(id);
        state.refresh_filter();
        state.selected_id = Some(id);
        let rt = build_render_tree(&state, 1024.0, 768.0);
        // Should produce substantial render commands for the detail panel
        assert!(rt.commands.len() > 20);
    }
}
