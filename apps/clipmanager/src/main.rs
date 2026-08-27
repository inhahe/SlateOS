//! Slate OS Clipboard Manager — a full-featured clipboard history and snippet manager.
//!
//! Provides clipboard history tracking (up to 500 entries), search, filtering by
//! content type, tagging, pinning, template management with placeholder substitution,
//! batch operations, statistics, and export/import. Inspired by CopyQ and Ditto.

use std::collections::{HashSet, VecDeque};
use std::process::ExitCode;
use std::time::Duration;

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, MouseButton, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use guitk::wheel;
use oswindow::app::{self, App, Response};

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

    /// Human-readable timestamp.
    ///
    /// The Clipboard Manager and the desktop's clipboard viewer list the same
    /// entries, and used to age them in different words: this one counted
    /// seconds (`"45s ago"`) where the viewer said `"just now"`, and neither
    /// ever stopped counting days, so a pinned snippet from last spring read
    /// `"400d ago"`.
    fn time_display(&self, now: u64) -> String {
        guitk::duration::relative(now.saturating_sub(self.timestamp))
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
    ///
    /// Walks by splitting the remaining text rather than by advancing a byte
    /// index into it: `{` and `}` are one byte each, so every offset `find`
    /// returns is a character boundary, but an index built by adding to one is
    /// only a boundary by argument — and a body is user text that may be any
    /// UTF-8 at all.
    fn placeholders(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut rest = self.body.as_str();
        while let Some(open) = rest.find('{') {
            // `open` is the offset of a one-byte '{', so `open + 1` is a
            // boundary and the slice is always `Some`; the fallback only
            // exists so this cannot be written with an index.
            let after = rest.get(open.saturating_add(1)..).unwrap_or("");
            let Some(close) = after.find('}') else {
                // An unclosed brace ends the scan: every later `{` is inside
                // the text this one opened.
                break;
            };
            let name = after.get(..close).unwrap_or("");
            if !name.is_empty() && !out.iter().any(|existing| existing == name) {
                out.push(name.to_string());
            }
            rest = after.get(close.saturating_add(1)..).unwrap_or("");
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
    ///
    /// Composed from [`Self::search`] and [`Self::filter_by_type`] rather than
    /// spelling both predicates a third time: three copies of "does this entry
    /// match" is three places for them to stop agreeing, and the searching one
    /// has already been case-folded once.
    fn search_filtered(&self, query: &str, type_filter: Option<ClipType>) -> Vec<u64> {
        let of_type: Option<HashSet<u64>> =
            type_filter.map(|t| self.filter_by_type(t).into_iter().collect());
        self.search(query)
            .into_iter()
            .filter(|id| of_type.as_ref().is_none_or(|ids| ids.contains(id)))
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
    guitk::bytes::iec(bytes)
}

// ---------------------------------------------------------------------------
// GUI state
// ---------------------------------------------------------------------------

/// Which tab is currently active.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveTab {
    History,
    Templates,
}

/// What [`AppState::save_template`] did with the new-template form.
#[derive(Clone, Debug, PartialEq, Eq)]
enum TemplateSaved {
    Created(String),
    Replaced(String),
}

/// Application-level GUI state.
struct AppState {
    store: ClipboardStore,
    search_query: String,
    type_filter: Option<ClipType>,
    /// Indices into the filtered result set.
    filtered_ids: Vec<u64>,
    selected_id: Option<u64>,
    /// Entries the user has marked for a batch delete, **by id**: a mark is
    /// meant to survive the deletion of a row above it, and a position is not.
    marked: Vec<u64>,
    /// The tag the list is narrowed to, if any.
    tag_filter: Option<String>,
    scroll_offset: usize,
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
    /// Which text box holds the keyboard, if any.
    focus: Option<Field>,
    /// The line the toolbar writes: what the last button did, or why it
    /// refused. A button that silently does nothing is indistinguishable from
    /// a button that is broken.
    status: String,
    /// The size the compositor last gave us.
    ///
    /// Needed because scrolling has to agree with the renderer about how many
    /// rows are on screen, and that depends on the window height — see
    /// [`rows_that_fit`].
    window_size: (f32, f32),
    /// Milliseconds seen since the clock last advanced a whole second.
    ///
    /// Carried rather than truncated: a 250 ms tick that rounds down to zero
    /// seconds four times a second is a clock that never moves.
    tick_carry_ms: u64,
    /// Fractional wheel notches a trackpad has sent but not yet spent.
    wheel: wheel::Accumulator,
}

impl AppState {
    fn new() -> Self {
        Self {
            store: ClipboardStore::new(),
            search_query: String::new(),
            type_filter: None,
            filtered_ids: Vec::new(),
            selected_id: None,
            marked: Vec::new(),
            tag_filter: None,
            scroll_offset: 0,
            active_tab: ActiveTab::History,
            now: 1000,
            tag_input: String::new(),
            template_name_input: String::new(),
            template_body_input: String::new(),
            template_vars: Vec::new(),
            selected_template: None,
            focus: None,
            status: String::new(),
            window_size: (WINDOW_WIDTH, WINDOW_HEIGHT),
            tick_carry_ms: 0,
            wheel: wheel::Accumulator::default(),
        }
    }

    /// How many history rows the list pane is currently drawing.
    fn visible_rows(&self) -> usize {
        rows_that_fit(self.window_size.1)
    }

    /// Re-run the current search/filter and update `filtered_ids`.
    fn refresh_filter(&mut self) {
        let mut ids = self
            .store
            .search_filtered(&self.search_query, self.type_filter);
        if let Some(tag) = &self.tag_filter {
            let tagged: HashSet<u64> = self.store.filter_by_tag(tag).into_iter().collect();
            ids.retain(|id| tagged.contains(id));
        }
        self.filtered_ids = ids;
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
        let visible = self.visible_rows();
        if new_pos >= self.scroll_offset.saturating_add(visible) {
            self.scroll_offset = new_pos.saturating_sub(visible.saturating_sub(1));
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

    /// Add the tag in `tag_input` to the selected entry and clear the box.
    ///
    /// Returns the tag added, or `None` if there was nothing to add or the
    /// entry already carried it — the caller turns that into a line the user
    /// can read, which is the difference between a button that refused and a
    /// button that is broken.
    fn add_tag_to_selected(&mut self) -> Option<String> {
        let tag = self.tag_input.trim().to_string();
        if tag.is_empty() {
            return None;
        }
        let id = self.selected_id?;
        if self.store.get(id).is_some_and(|e| e.tags.contains(&tag)) {
            return None;
        }
        self.store.add_tag(id, tag.clone());
        self.tag_input.clear();
        Some(tag)
    }

    /// Remove a tag from the selected entry.
    fn remove_tag_from_selected(&mut self, tag: &str) {
        if let Some(id) = self.selected_id {
            self.store.remove_tag(id, tag);
        }
    }

    /// Save the template currently in the input fields.
    ///
    /// Distinguishes creating from replacing, because
    /// [`ClipboardStore::add_template`] overwrites a template of the same name
    /// and a user who has just lost a body they spent five minutes on deserves
    /// to be told which of the two happened.
    fn save_template(&mut self) -> Result<TemplateSaved, &'static str> {
        let name = self.template_name_input.trim().to_string();
        let body = self.template_body_input.trim().to_string();
        if name.is_empty() {
            return Err("a template needs a name");
        }
        if body.is_empty() {
            return Err("a template needs a body");
        }
        let existed = self.store.get_template(&name).is_some();
        self.store.add_template(name.clone(), body);
        self.template_name_input.clear();
        self.template_body_input.clear();
        Ok(if existed {
            TemplateSaved::Replaced(name)
        } else {
            TemplateSaved::Created(name)
        })
    }

    /// Delete the currently selected template, returning its name.
    fn delete_selected_template(&mut self) -> Option<String> {
        let idx = self.selected_template?;
        let name = self.store.templates.get(idx)?.name.clone();
        self.store.remove_template(&name);
        self.selected_template = None;
        self.template_vars.clear();
        Some(name)
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

    /// Render the selected template with the current placeholder values and
    /// copy the result — that is, add it as a new entry at the top.
    ///
    /// Returns the template's name, or `None` if none was selected.
    fn render_template(&mut self) -> Option<String> {
        let idx = self.selected_template?;
        let tmpl = self.store.templates.get(idx)?;
        let (name, text) = (tmpl.name.clone(), tmpl.render(&self.template_vars));
        let ts = self.now;
        let id = self
            .store
            .add(text, ClipType::PlainText, ts, "template".to_string());
        self.selected_id = Some(id);
        self.scroll_offset = 0;
        self.refresh_filter();
        Some(name)
    }

    /// Format statistics as a status string.
    ///
    /// Includes the per-type breakdown, skipping the types with nothing in
    /// them: a row of six zeroes is six words the reader has to discard before
    /// finding the one number that moved.
    fn stats_line(&self) -> String {
        let total = self.store.total_entries();
        let pinned = self.store.pinned_count();
        let size = format_size(self.store.total_size);
        let breakdown: Vec<String> = self
            .store
            .stats_by_type()
            .into_iter()
            .filter(|&(_, count)| count > 0)
            .map(|(clip_type, count)| format!("{} {count}", clip_type.label()))
            .collect();
        let mut line = format!("{total} entries | {pinned} pinned | {size}");
        if !breakdown.is_empty() {
            line.push_str(" | ");
            line.push_str(&breakdown.join(", "));
        }
        if let Some(tag) = &self.tag_filter {
            line.push_str(&format!(" | tag: {tag}"));
        }
        if !self.marked.is_empty() {
            line.push_str(&format!(" | {} marked", self.marked.len()));
        }
        line
    }
}

// ---------------------------------------------------------------------------
// Layout
//
// The bands of the window are constants rather than numbers spelled into
// `build_frame`, because one of them — how many rows fit in the list — is
// needed by the *scroll* arithmetic as well as by the renderer. Two copies of
// that number is how a keyboard selection scrolls to a row that is not where
// the renderer put it.
// ---------------------------------------------------------------------------

const MARGIN: f32 = 12.0;
const TOP_BAR_H: f32 = 36.0;
const TAB_BAR_H: f32 = 32.0;
const TAG_BAR_H: f32 = 26.0;
const STATS_BAR_H: f32 = 24.0;
const TOOLBAR_H: f32 = 36.0;
const ROW_H: f32 = 52.0;
const ROW_GAP: f32 = 2.0;
const TAB_W: f32 = 100.0;
const BUTTON_W: f32 = 80.0;
const BUTTON_GAP: f32 = 8.0;

/// The window size asked for on startup. The compositor may say otherwise, and
/// [`App::render`] believes whatever it is handed.
const WINDOW_WIDTH: f32 = 1040.0;
const WINDOW_HEIGHT: f32 = 680.0;

/// The y coordinate the content area starts at.
fn content_top() -> f32 {
    8.0 + TOP_BAR_H + 4.0 + TAB_BAR_H + 4.0 + TAG_BAR_H + 4.0
}

/// How tall the content area is in a window of this height.
fn content_height(height: f32) -> f32 {
    (height - content_top() - STATS_BAR_H - TOOLBAR_H - 12.0).max(0.0)
}

/// How many history rows fit in the list pane of a window of this height.
///
/// The single source of truth for that number: the renderer draws this many
/// and [`AppState::select_next`] scrolls by it, so a selection moved with the
/// arrow keys lands where a click would have landed.
fn rows_that_fit(height: f32) -> usize {
    let usable = content_height(height) - 8.0;
    if usable < ROW_H {
        return 0;
    }
    // The last row needs no gap after it, so add one back before dividing.
    (((usable + ROW_GAP) / (ROW_H + ROW_GAP)) as usize).max(1)
}

// ---------------------------------------------------------------------------
// Controls
// ---------------------------------------------------------------------------

/// Everything in the window a click can land on.
///
/// Recorded by the renderer as it paints, and read back by
/// [`AppState::hit_test`]; see [`guitk::frame`] for why the two are the same
/// walk rather than two descriptions of the same geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The search box. Clicking it puts the caret in it.
    SearchBox,
    /// The type badge on the search bar, which cycles the filter.
    TypeFilter,
    /// One of the two tabs.
    Tab(ActiveTab),
    /// A chip on the tag strip, which narrows the list to that tag, and the
    /// chip that widens it again.
    ///
    /// Indexed into [`ClipboardStore::all_tags`], which is recomputed
    /// identically by the hit-test — the frame that recorded the chip and the
    /// click that reads it back see the same store.
    TagChip(usize),
    TagAll,
    /// A history row, **addressed by entry id rather than by position**:
    /// deleting or re-copying an entry renumbers every index below it but no
    /// id, so an index recorded here would select a different entry than the
    /// one that was drawn.
    Entry(u64),
    /// A tag chip on the detail panel; clicking it removes that tag.
    DetailTag(usize),
    /// The tag entry box on the detail panel, and the button that commits it.
    TagField,
    AddTag,
    /// A saved template row.
    Template(usize),
    /// The new-template form, and the buttons that act on it.
    TemplateName,
    TemplateBody,
    SaveTemplate,
    DeleteTemplate,
    UseTemplate,
    /// The toolbar along the bottom.
    CopyEntry,
    PinEntry,
    MarkEntry,
    DeleteEntry,
    ClearAll,
    ExportAll,
    ImportSelected,
}

/// A text box that can hold the caret.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Search,
    Tag,
    TemplateName,
    TemplateBody,
}

impl Field {
    /// The box Tab moves the caret to.
    fn next(self) -> Self {
        match self {
            Self::Search => Self::Tag,
            Self::Tag => Self::TemplateName,
            Self::TemplateName => Self::TemplateBody,
            Self::TemplateBody => Self::Search,
        }
    }
}

/// What the window should do after an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    /// Nothing visible changed; do not spend a frame saying so.
    None,
    Redraw,
    Quit,
}

/// A frame of this program's controls.
pub type Frame = guitk::frame::Frame<Target>;

// ---------------------------------------------------------------------------
// GUI rendering
// ---------------------------------------------------------------------------

/// Draw the whole window and record every control as it is painted.
fn build_frame(state: &AppState, width: f32, height: f32) -> Frame {
    let mut frame = Frame::new(width, height);

    frame.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width,
        height,
        color: BASE,
        corner_radii: CornerRadii::ZERO,
    });

    let inner_w = (width - MARGIN * 2.0).max(0.0);

    render_search_bar(&mut frame, state, MARGIN, 8.0, inner_w, TOP_BAR_H);

    let tab_y = 8.0 + TOP_BAR_H + 4.0;
    render_tab_bar(&mut frame, state, MARGIN, tab_y, inner_w, TAB_BAR_H);

    let tag_y = tab_y + TAB_BAR_H + 4.0;
    render_tag_strip(&mut frame, state, MARGIN, tag_y, inner_w, TAG_BAR_H);

    let content_y = content_top();
    let content_h = content_height(height);

    match state.active_tab {
        ActiveTab::History => {
            render_history_panel(
                &mut frame,
                state,
                Rect::new(MARGIN, content_y, inner_w, content_h),
                rows_that_fit(height),
            );
        }
        ActiveTab::Templates => {
            render_templates_panel(&mut frame, state, MARGIN, content_y, inner_w, content_h);
        }
    }

    let toolbar_y = height - STATS_BAR_H - TOOLBAR_H - 4.0;
    render_toolbar(&mut frame, state, MARGIN, toolbar_y, inner_w, TOOLBAR_H);

    let stats_y = height - STATS_BAR_H - 2.0;
    render_stats_bar(&mut frame, state, MARGIN, stats_y, inner_w, STATS_BAR_H);

    frame
}

/// The colour a text box's border takes when it holds the caret.
fn field_border(focused: bool) -> Color {
    if focused { BLUE } else { SURFACE1 }
}

fn render_search_bar(frame: &mut Frame, state: &AppState, x: f32, y: f32, w: f32, h: f32) {
    let focused = state.focus == Some(Field::Search);
    let box_rect = Rect::new(x, y, w, h);
    frame.push(RenderCommand::FillRect {
        x,
        y,
        width: w,
        height: h,
        color: SURFACE0,
        corner_radii: CornerRadii::all(6.0),
    });
    if focused {
        frame.push(RenderCommand::StrokeRect {
            x,
            y,
            width: w,
            height: h,
            color: field_border(true),
            line_width: 1.0,
            corner_radii: CornerRadii::all(6.0),
        });
    }

    frame.push(RenderCommand::Text {
        x: x + 10.0,
        y: y + 10.0,
        text: "Search:".to_string(),
        color: SUBTEXT0,
        font_size: 13.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // A caret while the box holds the keyboard, so an empty focused box does
    // not look identical to an empty unfocused one.
    let query_display = if state.search_query.is_empty() {
        if focused {
            "_".to_string()
        } else {
            "type to filter...".to_string()
        }
    } else if focused {
        format!("{}_", state.search_query)
    } else {
        state.search_query.clone()
    };
    let query_color = if state.search_query.is_empty() && !focused {
        OVERLAY0
    } else {
        TEXT
    };
    frame.push(RenderCommand::Text {
        x: x + 72.0,
        y: y + 10.0,
        text: query_display,
        color: query_color,
        font_size: 13.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some((w - 200.0).max(0.0)),
        overflow: TextOverflow::Ellipsis,
    });

    // The badge sits on top of the box, so it is recorded after it and wins
    // the clicks that land on both.
    let filter_label = state.type_filter.map_or("All Types", ClipType::label);
    let filter_color = state.type_filter.map_or(OVERLAY0, ClipType::badge_color);
    let badge = Rect::new(x + w - 100.0, y + 7.0, 80.0, 22.0);
    frame.push(RenderCommand::FillRect {
        x: badge.x,
        y: badge.y,
        width: badge.w,
        height: badge.h,
        color: SURFACE1,
        corner_radii: CornerRadii::all(4.0),
    });
    frame.push(RenderCommand::Text {
        x: badge.x + 8.0,
        y: badge.y + 4.0,
        text: filter_label.to_string(),
        color: filter_color,
        font_size: 11.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    frame.hit(Target::SearchBox, box_rect);
    frame.hit(Target::TypeFilter, badge);
}

fn render_tab_bar(frame: &mut Frame, state: &AppState, x: f32, y: f32, w: f32, h: f32) {
    frame.push(RenderCommand::FillRect {
        x,
        y,
        width: w,
        height: h,
        color: MANTLE,
        corner_radii: CornerRadii::all(4.0),
    });

    let mut tx = x + 4.0;
    for (label, tab) in [
        ("History", ActiveTab::History),
        ("Templates", ActiveTab::Templates),
    ] {
        let is_active = state.active_tab == tab;
        let rect = Rect::new(tx, y + 2.0, TAB_W, h - 4.0);
        if is_active {
            frame.push(RenderCommand::FillRect {
                x: rect.x,
                y: rect.y,
                width: rect.w,
                height: rect.h,
                color: SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });
        }
        frame.push(RenderCommand::Text {
            x: rect.x + 16.0,
            y: y + 8.0,
            text: label.to_string(),
            color: if is_active { BLUE } else { SUBTEXT0 },
            font_size: 13.0,
            font_weight: if is_active {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            },
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        frame.hit(Target::Tab(tab), rect);
        tx += TAB_W + 4.0;
    }
}

/// The strip of every tag in use, which narrows the history to one of them.
///
/// This is the only way a tag is worth adding: a label nothing can be filtered
/// by is a label nobody types twice.
fn render_tag_strip(frame: &mut Frame, state: &AppState, x: f32, y: f32, w: f32, h: f32) {
    frame.push(RenderCommand::PushClip {
        x,
        y,
        width: w,
        height: h,
    });

    frame.push(RenderCommand::Text {
        x,
        y: y + 6.0,
        text: "Tags:".to_string(),
        color: SUBTEXT0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    let mut tx = x + 40.0;
    let all = Rect::new(tx, y + 3.0, 36.0, 19.0);
    frame.push(RenderCommand::FillRect {
        x: all.x,
        y: all.y,
        width: all.w,
        height: all.h,
        color: if state.tag_filter.is_none() {
            SURFACE1
        } else {
            MANTLE
        },
        corner_radii: CornerRadii::all(3.0),
    });
    frame.push(RenderCommand::Text {
        x: all.x + 7.0,
        y: all.y + 4.0,
        text: "All".to_string(),
        color: if state.tag_filter.is_none() {
            BLUE
        } else {
            OVERLAY0
        },
        font_size: 10.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    frame.hit(Target::TagAll, all);
    tx = all.right() + 6.0;

    for (index, tag) in state.store.all_tags().iter().enumerate() {
        let chip_w = text::padded_width(tag, 8.0, 10.0, FontWeightHint::Regular);
        let chip = Rect::new(tx, y + 3.0, chip_w, 19.0);
        let active = state.tag_filter.as_deref() == Some(tag.as_str());
        frame.push(RenderCommand::FillRect {
            x: chip.x,
            y: chip.y,
            width: chip.w,
            height: chip.h,
            color: if active { SURFACE1 } else { MANTLE },
            corner_radii: CornerRadii::all(3.0),
        });
        frame.push(RenderCommand::Text {
            x: chip.x + 6.0,
            y: chip.y + 4.0,
            text: tag.clone(),
            color: if active { TEAL } else { OVERLAY0 },
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        // Recorded inside the clip, so a chip that has scrolled off the end of
        // the strip is not clickable where it is not drawn.
        frame.hit(Target::TagChip(index), chip);
        tx += chip_w + 4.0;
    }

    frame.push(RenderCommand::PopClip);
}

fn render_history_panel(frame: &mut Frame, state: &AppState, rect: Rect, visible: usize) {
    let Rect { x, y, w, h } = rect;
    let list_w = w * 0.55;
    let detail_w = (w - list_w - 8.0).max(0.0);

    frame.push(RenderCommand::FillRect {
        x,
        y,
        width: list_w,
        height: h,
        color: SURFACE0,
        corner_radii: CornerRadii::all(6.0),
    });

    // The clip is what keeps a half-scrolled row from taking clicks where it
    // is not drawn: `Frame::hit` intersects with it.
    frame.push(RenderCommand::PushClip {
        x,
        y,
        width: list_w,
        height: h,
    });

    let end = state
        .filtered_ids
        .len()
        .min(state.scroll_offset.saturating_add(visible));
    let mut ry = y + 4.0;
    for i in state.scroll_offset..end {
        if let Some(&id) = state.filtered_ids.get(i)
            && let Some(entry) = state.store.get(id)
        {
            render_entry_row(
                frame,
                entry,
                Rect::new(x + 4.0, ry, (list_w - 8.0).max(0.0), ROW_H),
                RowFlags {
                    selected: state.selected_id == Some(id),
                    marked: state.marked.contains(&id),
                },
                state.now,
            );
        }
        ry += ROW_H + ROW_GAP;
    }

    if state.filtered_ids.is_empty() {
        frame.push(RenderCommand::Text {
            x: x + 16.0,
            y: y + 24.0,
            text: if state.store.total_entries() == 0 {
                "No entries".to_string()
            } else {
                "Nothing matches the current filter".to_string()
            },
            color: OVERLAY0,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some((list_w - 32.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }

    frame.push(RenderCommand::PopClip);

    let detail_x = x + list_w + 8.0;
    frame.push(RenderCommand::FillRect {
        x: detail_x,
        y,
        width: detail_w,
        height: h,
        color: SURFACE0,
        corner_radii: CornerRadii::all(6.0),
    });

    match state.selected_id.and_then(|id| state.store.get(id)) {
        Some(entry) => render_detail_panel(
            frame,
            entry,
            Rect::new(detail_x, y, detail_w, h),
            state.now,
            state.focus == Some(Field::Tag),
            &state.tag_input,
        ),
        None => frame.push(RenderCommand::Text {
            x: detail_x + 16.0,
            y: y + 24.0,
            text: "Select an entry to preview".to_string(),
            color: OVERLAY0,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        }),
    }
}

/// How a history row is drawn differently from the plain case.
///
/// Two bools in a row is one transposition away from a selected row that
/// claims to be marked; a named struct makes the call sites say which is which.
#[derive(Clone, Copy)]
struct RowFlags {
    selected: bool,
    marked: bool,
}

fn render_entry_row(frame: &mut Frame, entry: &ClipEntry, rect: Rect, flags: RowFlags, now: u64) {
    let Rect { x, y, w, h } = rect;

    frame.push(RenderCommand::FillRect {
        x,
        y,
        width: w,
        height: h,
        color: if flags.selected { SURFACE1 } else { SURFACE0 },
        corner_radii: CornerRadii::all(4.0),
    });

    if flags.selected {
        frame.push(RenderCommand::FillRect {
            x,
            y,
            width: 3.0,
            height: h,
            color: BLUE,
            corner_radii: CornerRadii::ZERO,
        });
    }

    frame.push(RenderCommand::FillRect {
        x: x + 8.0,
        y: y + 6.0,
        width: 40.0,
        height: 16.0,
        color: entry.clip_type.badge_color(),
        corner_radii: CornerRadii::all(3.0),
    });
    frame.push(RenderCommand::Text {
        x: x + 12.0,
        y: y + 7.0,
        text: entry.clip_type.label().to_string(),
        color: MANTLE,
        font_size: 10.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    let mut bx = x + 52.0;
    if entry.pinned {
        frame.push(RenderCommand::Text {
            x: bx,
            y: y + 7.0,
            text: "PIN".to_string(),
            color: YELLOW,
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        bx += 26.0;
    }
    if flags.marked {
        frame.push(RenderCommand::Text {
            x: bx,
            y: y + 7.0,
            text: "MARK".to_string(),
            color: PEACH,
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    frame.push(RenderCommand::Text {
        x: x + 8.0,
        y: y + 26.0,
        text: entry.preview(),
        color: TEXT,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some((w - 80.0).max(0.0)),
        overflow: TextOverflow::Ellipsis,
    });

    frame.push(RenderCommand::Text {
        x: x + w - 70.0,
        y: y + 6.0,
        text: entry.time_display(now),
        color: SUBTEXT0,
        font_size: 10.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    frame.push(RenderCommand::Text {
        x: x + w - 70.0,
        y: y + 18.0,
        text: entry.source_app.clone(),
        color: OVERLAY0,
        font_size: 10.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    if !entry.tags.is_empty() {
        frame.push(RenderCommand::Text {
            x: x + w - 70.0,
            y: y + 34.0,
            text: format!("{} tags", entry.tags.len()),
            color: TEAL,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    // Recorded last so the whole painted band takes the click, badge included:
    // a user aiming at a row does not aim around its decorations.
    frame.hit(Target::Entry(entry.id), rect);
}

fn render_detail_panel(
    frame: &mut Frame,
    entry: &ClipEntry,
    rect: Rect,
    now: u64,
    tag_focused: bool,
    tag_input: &str,
) {
    let Rect { x, y, w, h } = rect;
    frame.push(RenderCommand::PushClip {
        x,
        y,
        width: w,
        height: h,
    });

    let pad = 12.0_f32;
    let mut cy = y + pad;

    frame.push(RenderCommand::Text {
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

    frame.push(RenderCommand::Text {
        x: x + pad,
        y: cy,
        text: format!(
            "{} | {} | {}",
            entry.time_display(now),
            entry.source_app,
            entry.size_display()
        ),
        color: SUBTEXT0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some((w - pad * 2.0).max(0.0)),
        overflow: TextOverflow::Ellipsis,
    });
    cy += 18.0;

    if entry.pinned {
        frame.push(RenderCommand::Text {
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

    // Tag chips. Each is clickable and removes itself, which is the only way
    // `ClipboardStore::remove_tag` is reachable from the window.
    if !entry.tags.is_empty() {
        let mut tx = x + pad;
        for (index, tag) in entry.tags.iter().enumerate() {
            let tag_w = text::padded_width(tag, 8.0, 10.0, FontWeightHint::Regular);
            let chip = Rect::new(tx, cy, tag_w, 18.0);
            frame.push(RenderCommand::FillRect {
                x: chip.x,
                y: chip.y,
                width: chip.w,
                height: chip.h,
                color: SURFACE1,
                corner_radii: CornerRadii::all(3.0),
            });
            frame.push(RenderCommand::Text {
                x: chip.x + 6.0,
                y: chip.y + 3.0,
                text: tag.clone(),
                color: TEAL,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            frame.hit(Target::DetailTag(index), chip);
            tx += tag_w + 4.0;
        }
        cy += 24.0;
    }

    // Tag entry: a box that takes the keyboard and a button that commits it.
    let add_w = 44.0_f32;
    let field = Rect::new(x + pad, cy, (w - pad * 2.0 - add_w - 6.0).max(0.0), 20.0);
    frame.push(RenderCommand::FillRect {
        x: field.x,
        y: field.y,
        width: field.w,
        height: field.h,
        color: MANTLE,
        corner_radii: CornerRadii::all(3.0),
    });
    frame.push(RenderCommand::StrokeRect {
        x: field.x,
        y: field.y,
        width: field.w,
        height: field.h,
        color: field_border(tag_focused),
        line_width: 1.0,
        corner_radii: CornerRadii::all(3.0),
    });
    let tag_display = if tag_input.is_empty() && !tag_focused {
        "add a tag...".to_string()
    } else if tag_focused {
        format!("{tag_input}_")
    } else {
        tag_input.to_string()
    };
    frame.push(RenderCommand::Text {
        x: field.x + 6.0,
        y: field.y + 4.0,
        text: tag_display,
        color: if tag_input.is_empty() && !tag_focused {
            OVERLAY0
        } else {
            TEXT
        },
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some((field.w - 12.0).max(0.0)),
        overflow: TextOverflow::Ellipsis,
    });
    frame.hit(Target::TagField, field);

    let add = Rect::new(field.right() + 6.0, cy, add_w, 20.0);
    frame.push(RenderCommand::FillRect {
        x: add.x,
        y: add.y,
        width: add.w,
        height: add.h,
        color: SURFACE1,
        corner_radii: CornerRadii::all(3.0),
    });
    frame.push(RenderCommand::Text {
        x: add.x + 8.0,
        y: add.y + 4.0,
        text: "Tag".to_string(),
        color: TEAL,
        font_size: 11.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    frame.hit(Target::AddTag, add);
    cy += 26.0;

    if (entry.clip_type == ClipType::Code || entry.clip_type == ClipType::PlainText)
        && let Some(lang) = detect_code_language(&entry.content)
    {
        frame.push(RenderCommand::FillRect {
            x: x + pad,
            y: cy,
            width: 100.0,
            height: 18.0,
            color: SURFACE1,
            corner_radii: CornerRadii::all(3.0),
        });
        frame.push(RenderCommand::Text {
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

    cy += 4.0;
    frame.push(RenderCommand::Line {
        x1: x + pad,
        y1: cy,
        x2: x + w - pad,
        y2: cy,
        color: SURFACE1,
        width: 1.0,
    });
    cy += 8.0;

    let available_h = ((y + h) - cy - pad).max(0.0);
    frame.push(RenderCommand::PushClip {
        x: x + pad,
        y: cy,
        width: (w - pad * 2.0).max(0.0),
        height: available_h,
    });

    let line_h = 16.0_f32;
    let max_lines = (available_h / line_h) as usize;
    for (i, line) in entry.content.lines().enumerate() {
        if i >= max_lines {
            break;
        }
        frame.push(RenderCommand::Text {
            x: x + pad,
            y: cy + (i as f32) * line_h,
            text: line.to_string(),
            color: TEXT,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some((w - pad * 2.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }

    frame.push(RenderCommand::PopClip);
    frame.push(RenderCommand::PopClip);
}

fn render_templates_panel(frame: &mut Frame, state: &AppState, x: f32, y: f32, w: f32, h: f32) {
    frame.push(RenderCommand::FillRect {
        x,
        y,
        width: w,
        height: h,
        color: SURFACE0,
        corner_radii: CornerRadii::all(6.0),
    });

    frame.push(RenderCommand::PushClip {
        x,
        y,
        width: w,
        height: h,
    });

    let pad = 12.0_f32;
    let mut cy = y + pad;

    frame.push(RenderCommand::Text {
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
        frame.push(RenderCommand::Text {
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
            let row = Rect::new(x + pad, cy, (w - pad * 2.0).max(0.0), 32.0);
            frame.push(RenderCommand::FillRect {
                x: row.x,
                y: row.y,
                width: row.w,
                height: row.h,
                color: if is_sel { SURFACE1 } else { SURFACE0 },
                corner_radii: CornerRadii::all(4.0),
            });
            frame.push(RenderCommand::Text {
                x: row.x + 8.0,
                y: row.y + 8.0,
                text: tmpl.name.clone(),
                color: if is_sel { BLUE } else { TEXT },
                font_size: 13.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(180.0),
                overflow: TextOverflow::Ellipsis,
            });

            let ph_count = tmpl.placeholders().len();
            if ph_count > 0 {
                frame.push(RenderCommand::Text {
                    x: row.x + 200.0,
                    y: row.y + 10.0,
                    text: format!("{ph_count} placeholders"),
                    color: SUBTEXT0,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }
            frame.hit(Target::Template(idx), row);
            cy += 36.0;
        }
    }

    // Buttons that act on the selected template. Drawn whether or not one is
    // selected — a button that vanishes when it would refuse leaves the user
    // guessing which of the two states they are in — and each says why it
    // refused when there is nothing to act on.
    let use_rect = Rect::new(x + pad, cy, BUTTON_W, 24.0);
    let del_rect = Rect::new(use_rect.right() + BUTTON_GAP, cy, BUTTON_W, 24.0);
    for (rect, label, color, target) in [
        (use_rect, "Use", GREEN, Target::UseTemplate),
        (del_rect, "Delete", RED, Target::DeleteTemplate),
    ] {
        frame.push(RenderCommand::FillRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: SURFACE1,
            corner_radii: CornerRadii::all(4.0),
        });
        frame.push(RenderCommand::Text {
            x: rect.x + 12.0,
            y: rect.y + 6.0,
            text: label.to_string(),
            color,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        frame.hit(target, rect);
    }
    cy += 32.0;

    // Placeholder values for the selected template, so `ClipTemplate::render`
    // has something to substitute.
    for (key, value) in &state.template_vars {
        frame.push(RenderCommand::Text {
            x: x + pad,
            y: cy,
            text: format!("{key}: {}", if value.is_empty() { "-" } else { value }),
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some((w - pad * 2.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 16.0;
    }

    cy += 8.0;
    frame.push(RenderCommand::Line {
        x1: x + pad,
        y1: cy,
        x2: x + w - pad,
        y2: cy,
        color: SURFACE1,
        width: 1.0,
    });
    cy += 12.0;

    frame.push(RenderCommand::Text {
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

    cy = render_template_field(
        frame,
        TemplateField {
            label: "Name:",
            placeholder: "e.g. Email Reply",
            value: &state.template_name_input,
            focused: state.focus == Some(Field::TemplateName),
            target: Target::TemplateName,
            height: 20.0,
        },
        x + pad,
        cy,
        (w - pad * 2.0).max(0.0),
    );

    cy = render_template_field(
        frame,
        TemplateField {
            label: "Body:",
            placeholder: "Dear {name}, ...",
            value: &state.template_body_input,
            focused: state.focus == Some(Field::TemplateBody),
            target: Target::TemplateBody,
            height: 40.0,
        },
        x + pad,
        cy,
        (w - pad * 2.0).max(0.0),
    );

    let save = Rect::new(x + pad, cy, BUTTON_W, 24.0);
    frame.push(RenderCommand::FillRect {
        x: save.x,
        y: save.y,
        width: save.w,
        height: save.h,
        color: SURFACE1,
        corner_radii: CornerRadii::all(4.0),
    });
    frame.push(RenderCommand::Text {
        x: save.x + 12.0,
        y: save.y + 6.0,
        text: "Save".to_string(),
        color: BLUE,
        font_size: 12.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    frame.hit(Target::SaveTemplate, save);

    frame.push(RenderCommand::PopClip);
}

/// One labelled text box in the new-template form.
///
/// A struct rather than six positional arguments, because `&str, &str, &str,
/// bool` in a row is four chances to swap two of them and no chance for the
/// compiler to notice.
struct TemplateField<'a> {
    label: &'a str,
    placeholder: &'a str,
    value: &'a str,
    focused: bool,
    target: Target,
    height: f32,
}

/// Draw one form field and return the y the next one starts at.
fn render_template_field(
    frame: &mut Frame,
    field: TemplateField<'_>,
    x: f32,
    y: f32,
    w: f32,
) -> f32 {
    frame.push(RenderCommand::Text {
        x,
        y,
        text: field.label.to_string(),
        color: SUBTEXT0,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    let rect = Rect::new(x + 60.0, y - 2.0, (w - 60.0).max(0.0), field.height);
    frame.push(RenderCommand::FillRect {
        x: rect.x,
        y: rect.y,
        width: rect.w,
        height: rect.h,
        color: MANTLE,
        corner_radii: CornerRadii::all(3.0),
    });
    frame.push(RenderCommand::StrokeRect {
        x: rect.x,
        y: rect.y,
        width: rect.w,
        height: rect.h,
        color: field_border(field.focused),
        line_width: 1.0,
        corner_radii: CornerRadii::all(3.0),
    });

    let display = if field.value.is_empty() && !field.focused {
        field.placeholder.to_string()
    } else if field.focused {
        format!("{}_", field.value)
    } else {
        field.value.to_string()
    };
    frame.push(RenderCommand::Text {
        x: rect.x + 6.0,
        y,
        text: display,
        color: if field.value.is_empty() && !field.focused {
            OVERLAY0
        } else {
            TEXT
        },
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some((rect.w - 12.0).max(0.0)),
        overflow: TextOverflow::Ellipsis,
    });
    frame.hit(field.target, rect);

    y + field.height + 12.0
}

fn render_toolbar(frame: &mut Frame, state: &AppState, x: f32, y: f32, w: f32, h: f32) {
    frame.push(RenderCommand::FillRect {
        x,
        y,
        width: w,
        height: h,
        color: MANTLE,
        corner_radii: CornerRadii::all(4.0),
    });

    let mut bx = x + 8.0;
    for (label, color, target) in [
        ("Copy", BLUE, Target::CopyEntry),
        ("Pin", YELLOW, Target::PinEntry),
        ("Mark", PEACH, Target::MarkEntry),
        ("Delete", RED, Target::DeleteEntry),
        ("Clear All", PEACH, Target::ClearAll),
        ("Export", TEAL, Target::ExportAll),
        ("Import", MAUVE, Target::ImportSelected),
    ] {
        let rect = Rect::new(bx, y + 5.0, BUTTON_W, (h - 10.0).max(0.0));
        frame.push(RenderCommand::FillRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: SURFACE1,
            corner_radii: CornerRadii::all(4.0),
        });
        frame.push(RenderCommand::Text {
            x: rect.x + 10.0,
            y: y + 11.0,
            text: label.to_string(),
            color,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(BUTTON_W - 12.0),
            overflow: TextOverflow::Ellipsis,
        });
        frame.hit(target, rect);
        bx += BUTTON_W + BUTTON_GAP;
    }

    // The status line the buttons write, on the right of the same bar so a
    // refusal appears next to the button that refused.
    if !state.status.is_empty() {
        frame.push(RenderCommand::Text {
            x: bx + 8.0,
            y: y + 11.0,
            text: state.status.clone(),
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some((x + w - bx - 16.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

fn render_stats_bar(frame: &mut Frame, state: &AppState, x: f32, y: f32, w: f32, h: f32) {
    frame.push(RenderCommand::FillRect {
        x,
        y,
        width: w,
        height: h,
        color: MANTLE,
        corner_radii: CornerRadii::all(3.0),
    });

    frame.push(RenderCommand::Text {
        x: x + 10.0,
        y: y + 5.0,
        text: state.stats_line(),
        color: SUBTEXT0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some((w - 100.0).max(0.0)),
        overflow: TextOverflow::Ellipsis,
    });

    frame.push(RenderCommand::Text {
        x: x + w - 80.0,
        y: y + 5.0,
        text: format!("{} shown", state.filtered_ids.len()),
        color: OVERLAY0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
}

// ---------------------------------------------------------------------------
// Input routing
// ---------------------------------------------------------------------------

impl AppState {
    /// Which control is under a point, by re-drawing the window and reading
    /// back what it recorded.
    fn hit_test(&self, x: f32, y: f32, size: (f32, f32)) -> Option<Target> {
        build_frame(self, size.0, size.1).hit_test(x, y)
    }

    /// Put a line in the toolbar explaining why a button did nothing.
    fn refuse(&mut self, why: &str) -> Action {
        self.status = why.to_string();
        Action::Redraw
    }

    /// Route a click at window coordinates.
    fn handle_click(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> Action {
        if button != MouseButton::Left {
            return Action::None;
        }
        let Some(target) = self.hit_test(x, y, size) else {
            // Clicking bare background puts the caret away, so a stray
            // keystroke afterwards does not land in a box the user has stopped
            // looking at.
            if self.focus.is_some() {
                self.focus = None;
                return Action::Redraw;
            }
            return Action::None;
        };
        self.activate(target)
    }

    /// Do whatever the named control does.
    #[allow(
        clippy::too_many_lines,
        reason = "one arm per control; splitting it would only move the arms \
                  somewhere the reader has to go looking for them"
    )]
    fn activate(&mut self, target: Target) -> Action {
        match target {
            Target::SearchBox => {
                self.focus = Some(Field::Search);
                Action::Redraw
            }
            Target::TypeFilter => {
                self.type_filter = next_type_filter(self.type_filter);
                self.scroll_offset = 0;
                self.refresh_filter();
                self.status = match self.type_filter {
                    Some(t) => format!("showing {} entries only", t.label()),
                    None => "showing every type".to_string(),
                };
                Action::Redraw
            }
            Target::Tab(tab) => {
                if self.active_tab == tab {
                    return Action::None;
                }
                self.active_tab = tab;
                self.focus = None;
                Action::Redraw
            }
            Target::TagAll => {
                if self.tag_filter.is_none() {
                    return Action::None;
                }
                self.tag_filter = None;
                self.scroll_offset = 0;
                self.refresh_filter();
                self.status = "showing every tag".to_string();
                Action::Redraw
            }
            Target::TagChip(index) => {
                let Some(tag) = self.store.all_tags().get(index).cloned() else {
                    return Action::None;
                };
                self.tag_filter = Some(tag.clone());
                self.active_tab = ActiveTab::History;
                self.scroll_offset = 0;
                self.refresh_filter();
                self.status = format!("showing entries tagged {tag}");
                Action::Redraw
            }
            Target::Entry(id) => {
                if self.selected_id == Some(id) {
                    return Action::None;
                }
                self.select(id);
                Action::Redraw
            }
            Target::DetailTag(index) => {
                let tag = self
                    .selected_id
                    .and_then(|id| self.store.get(id))
                    .and_then(|entry| entry.tags.get(index).cloned());
                let Some(tag) = tag else {
                    return Action::None;
                };
                self.remove_tag_from_selected(&tag);
                // The removed tag may have been the one being filtered by, in
                // which case the entry has just left the list it is selected in.
                self.refresh_filter();
                self.clamp_scroll();
                self.status = format!("removed the tag {tag}");
                Action::Redraw
            }
            Target::TagField => {
                self.focus = Some(Field::Tag);
                Action::Redraw
            }
            Target::AddTag => {
                if self.selected_id.is_none() {
                    return self.refuse("select an entry to tag first");
                }
                if self.tag_input.trim().is_empty() {
                    return self.refuse("type a tag before adding it");
                }
                match self.add_tag_to_selected() {
                    Some(tag) => {
                        self.refresh_filter();
                        self.status = format!("tagged {tag}");
                        Action::Redraw
                    }
                    None => self.refuse("that entry already has that tag"),
                }
            }
            Target::Template(index) => {
                self.select_template(index);
                self.active_tab = ActiveTab::Templates;
                Action::Redraw
            }
            Target::TemplateName => {
                self.focus = Some(Field::TemplateName);
                Action::Redraw
            }
            Target::TemplateBody => {
                self.focus = Some(Field::TemplateBody);
                Action::Redraw
            }
            Target::SaveTemplate => match self.save_template() {
                Ok(TemplateSaved::Created(name)) => {
                    self.status = format!("saved the template {name}");
                    Action::Redraw
                }
                Ok(TemplateSaved::Replaced(name)) => {
                    self.status = format!("replaced the template {name}");
                    Action::Redraw
                }
                Err(why) => self.refuse(why),
            },
            Target::DeleteTemplate => match self.delete_selected_template() {
                Some(name) => {
                    self.status = format!("deleted the template {name}");
                    Action::Redraw
                }
                None => self.refuse("select a template to delete first"),
            },
            Target::UseTemplate => match self.render_template() {
                Some(name) => {
                    self.status = format!("copied {name} to the top of the history");
                    Action::Redraw
                }
                None => self.refuse("select a template to use first"),
            },
            Target::CopyEntry => {
                let Some(entry) = self.selected_id.and_then(|id| self.store.get(id)) else {
                    return self.refuse("select an entry to copy first");
                };
                let (content, clip_type) = (entry.content.clone(), entry.clip_type);
                let now = self.now;
                // Deduplicating, so this moves the existing entry back to the
                // front rather than growing a second copy of it — which is what
                // copying something you already copied does everywhere else.
                let id = self
                    .store
                    .add(content, clip_type, now, "clipmanager".to_string());
                self.selected_id = Some(id);
                self.scroll_offset = 0;
                self.refresh_filter();
                self.status = "copied to the top of the history".to_string();
                Action::Redraw
            }
            Target::PinEntry => {
                if self.selected_id.is_none() {
                    return self.refuse("select an entry to pin first");
                }
                self.toggle_pin_selected();
                let pinned = self
                    .selected_id
                    .and_then(|id| self.store.get(id))
                    .is_some_and(|entry| entry.pinned);
                self.status = if pinned { "pinned" } else { "unpinned" }.to_string();
                Action::Redraw
            }
            Target::MarkEntry => {
                let Some(id) = self.selected_id else {
                    return self.refuse("select an entry to mark first");
                };
                if let Some(pos) = self.marked.iter().position(|&m| m == id) {
                    self.marked.remove(pos);
                    self.status = format!("unmarked; {} still marked", self.marked.len());
                } else {
                    self.marked.push(id);
                    self.status = format!("{} marked", self.marked.len());
                }
                Action::Redraw
            }
            Target::DeleteEntry => self.delete_marked_or_selected(),
            Target::ClearAll => {
                let before = self.store.total_entries();
                self.store.clear_unpinned();
                let removed = before.saturating_sub(self.store.total_entries());
                if removed == 0 {
                    return self.refuse("there is nothing unpinned to clear");
                }
                self.forget_deleted();
                self.status = format!("cleared {removed} unpinned entries");
                Action::Redraw
            }
            Target::ExportAll => {
                if self.store.total_entries() == 0 {
                    return self.refuse("there is nothing to export");
                }
                let text = self.store.export_text();
                let now = self.now;
                let id = self
                    .store
                    .add(text, ClipType::PlainText, now, "export".to_string());
                self.selected_id = Some(id);
                self.scroll_offset = 0;
                self.refresh_filter();
                self.status = "the history is now the entry at the top".to_string();
                Action::Redraw
            }
            Target::ImportSelected => {
                let Some(entry) = self.selected_id.and_then(|id| self.store.get(id)) else {
                    return self.refuse("select the entry holding an export first");
                };
                let data = entry.content.clone();
                let now = self.now;
                let count = self.store.import_text(&data, now);
                if count == 0 {
                    return self.refuse("the selected entry is not an export");
                }
                self.refresh_filter();
                self.status = format!("imported {count} entries");
                Action::Redraw
            }
        }
    }

    /// Delete every marked entry, or the selected one if nothing is marked.
    fn delete_marked_or_selected(&mut self) -> Action {
        if self.marked.is_empty() {
            if self.selected_id.is_none() {
                return self.refuse("select an entry to delete first");
            }
            self.delete_selected();
            self.forget_deleted();
            self.status = "deleted".to_string();
            return Action::Redraw;
        }
        let ids = std::mem::take(&mut self.marked);
        let count = ids.len();
        self.store.delete_many(&ids);
        self.forget_deleted();
        self.status = format!("deleted {count} marked entries");
        Action::Redraw
    }

    /// Drop selections and marks pointing at entries that no longer exist, and
    /// bring the view back over the rows that do.
    ///
    /// Every deletion path funnels through here, because a selection left
    /// pointing at a deleted id is a Copy button that reports "select an entry
    /// first" while a row still looks selected.
    fn forget_deleted(&mut self) {
        self.marked.retain(|&id| self.store.get(id).is_some());
        if self
            .selected_id
            .is_some_and(|id| self.store.get(id).is_none())
        {
            self.selected_id = None;
        }
        self.refresh_filter();
        self.clamp_scroll();
    }

    /// Pull the view back if it is showing past the end of the list.
    fn clamp_scroll(&mut self) {
        let max = self.filtered_ids.len().saturating_sub(self.visible_rows());
        self.scroll_offset = self.scroll_offset.min(max);
    }

    /// Scroll the list by whole rows, clamped at both ends.
    ///
    /// Clamped at the bottom as well as the top: a list scrolled past its own
    /// end shows an empty pane with no way back but scrolling up through the
    /// blank space it just created.
    fn scroll_rows(&mut self, rows: usize, towards_end: bool) {
        let max = self.filtered_ids.len().saturating_sub(self.visible_rows());
        let offset = if towards_end {
            self.scroll_offset.saturating_add(rows)
        } else {
            self.scroll_offset.saturating_sub(rows)
        };
        self.scroll_offset = offset.min(max);
    }

    /// [`Self::scroll_rows`] taking the signed row count a wheel produces.
    fn scroll_by(&mut self, rows: isize) {
        self.scroll_rows(rows.unsigned_abs(), rows > 0);
    }

    /// Route a keystroke.
    fn handle_key(&mut self, key: &KeyEvent, size: (f32, f32)) -> Action {
        if !key.pressed {
            return Action::None;
        }
        if let Some(field) = self.focus {
            return self.handle_key_in_field(key, field);
        }
        let page = rows_that_fit(size.1).max(1);
        match key.key {
            Key::Escape => Action::Quit,
            Key::Down => {
                self.select_next();
                Action::Redraw
            }
            Key::Up => {
                self.select_prev();
                Action::Redraw
            }
            Key::Left | Key::Right => self.activate(Target::Tab(match self.active_tab {
                ActiveTab::History => ActiveTab::Templates,
                ActiveTab::Templates => ActiveTab::History,
            })),
            Key::PageDown => {
                self.scroll_rows(page, true);
                Action::Redraw
            }
            Key::PageUp => {
                self.scroll_rows(page, false);
                Action::Redraw
            }
            Key::Home => {
                self.scroll_offset = 0;
                self.selected_id = self.filtered_ids.first().copied();
                Action::Redraw
            }
            Key::Enter => self.activate(Target::CopyEntry),
            Key::Delete => self.activate(Target::DeleteEntry),
            _ => Action::None,
        }
    }

    /// A keystroke while a text box holds the keyboard.
    fn handle_key_in_field(&mut self, key: &KeyEvent, field: Field) -> Action {
        match key.key {
            Key::Escape => {
                self.focus = None;
                Action::Redraw
            }
            Key::Tab => {
                self.focus = Some(field.next());
                Action::Redraw
            }
            Key::Enter => match field {
                Field::Search => {
                    self.focus = None;
                    Action::Redraw
                }
                Field::Tag => self.activate(Target::AddTag),
                Field::TemplateName | Field::TemplateBody => self.activate(Target::SaveTemplate),
            },
            Key::Backspace => {
                if self.field_mut(field).pop().is_none() {
                    return Action::None;
                }
                if field == Field::Search {
                    self.scroll_offset = 0;
                    self.refresh_filter();
                }
                Action::Redraw
            }
            _ => {
                // `typed()` already drops the control characters Enter, Tab,
                // Escape and Backspace produce on most layouts, so an unmatched
                // key cannot smuggle a `\r` into a tag.
                let typed: String = key.typed().collect();
                if typed.is_empty() {
                    return Action::None;
                }
                self.field_mut(field).push_str(&typed);
                if field == Field::Search {
                    self.scroll_offset = 0;
                    self.refresh_filter();
                }
                Action::Redraw
            }
        }
    }

    /// The string behind a text box.
    fn field_mut(&mut self, field: Field) -> &mut String {
        match field {
            Field::Search => &mut self.search_query,
            Field::Tag => &mut self.tag_input,
            Field::TemplateName => &mut self.template_name_input,
            Field::TemplateBody => &mut self.template_body_input,
        }
    }

    /// Advance the clock by `elapsed_ms`, carrying the remainder.
    ///
    /// Returns whether the displayed second changed. Truncating instead of
    /// carrying would stop the clock entirely under a sub-second tick.
    fn advance(&mut self, elapsed_ms: u64) -> bool {
        self.tick_carry_ms = self.tick_carry_ms.saturating_add(elapsed_ms);
        let seconds = self.tick_carry_ms / 1000;
        if seconds == 0 {
            return false;
        }
        self.tick_carry_ms = self
            .tick_carry_ms
            .saturating_sub(seconds.saturating_mul(1000));
        self.now = self.now.saturating_add(seconds);
        true
    }

    /// Route a whole event.
    fn handle_event(&mut self, event: &Event, size: (f32, f32)) -> Action {
        match event {
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Press(button) => self.handle_click(mouse.x, mouse.y, button, size),
                MouseEventKind::Scroll { dy, .. } => {
                    // The accumulator keeps the fractions a trackpad sends, so a
                    // slow drag moves instead of rounding to zero every frame.
                    let rows = self.wheel.rows(dy);
                    if rows == 0 {
                        return Action::None;
                    }
                    let before = self.scroll_offset;
                    // Positive means towards the end of the list, which is the
                    // direction `scroll_offset` counts in.
                    self.scroll_by(rows);
                    if self.scroll_offset == before {
                        return Action::None;
                    }
                    Action::Redraw
                }
                _ => Action::None,
            },
            Event::Key(key) => self.handle_key(key, size),
            Event::Tick { elapsed_ms } => {
                if self.advance(*elapsed_ms) {
                    Action::Redraw
                } else {
                    Action::None
                }
            }
            Event::CloseRequested => Action::Quit,
            _ => Action::None,
        }
    }
}

/// The next type filter in the cycle: all types, then each type in turn.
fn next_type_filter(current: Option<ClipType>) -> Option<ClipType> {
    let all = ClipType::all();
    match current {
        None => all.first().copied(),
        Some(t) => {
            let pos = all.iter().position(|&c| c == t);
            match pos {
                Some(p) => all.get(p.saturating_add(1)).copied(),
                None => None,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

impl App for AppState {
    fn title(&self) -> String {
        String::from("Clipboard Manager")
    }

    fn app_id(&self) -> String {
        String::from("slateos.clipmanager")
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// A second, and only while there is something whose age is on screen.
    ///
    /// Every row shows how long ago it was copied, to the second, so a faster
    /// clock repaints for a figure that did not change and a slower one visibly
    /// skips. With an empty history there is nothing ageing, and an app that
    /// keeps ticking with nothing to advance holds the whole desktop awake.
    fn tick_interval(&self) -> Option<Duration> {
        if self.store.total_entries() == 0 {
            None
        } else {
            Some(Duration::from_secs(1))
        }
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if let Event::Resize { width, height } = *event {
            self.window_size = (width as f32, height as f32);
            // A shorter window shows fewer rows, which can leave the view
            // scrolled past the end of the list.
            self.clamp_scroll();
            return Response::Redraw;
        }
        match self.handle_event(event, self.window_size) {
            Action::None => Response::Idle,
            Action::Redraw => Response::Redraw,
            Action::Quit => Response::Exit,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // Believe the size we are handed: the first frame goes out before any
        // `Event::Resize`, so the stored size is only a starting guess.
        self.window_size = (width, height);
        build_frame(self, width, height).into_tree()
    }
}

/// Lets the tests drive this window by naming its controls rather than
/// measuring them. Three lines of forwarding; the helpers are in
/// [`guitk::probe`].
impl Probe for AppState {
    type Target = Target;
    type Outcome = Action;
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame {
        build_frame(self, size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> Action {
        self.handle_click(x, y, button, size)
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> Action {
        self.handle_key(key, size)
    }
}

fn main() -> ExitCode {
    let mut state = AppState::new();
    state.refresh_filter();
    app::launch("clipmanager", &mut state)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    // Panicking on bad data is the point of a test: an `expect` that fires is
    // a failure report, and an index that is out of range is the assertion.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use guitk::event::{MouseEvent, MouseEventKind};
    use guitk::probe::{
        click, click_background, control_names, is_visible, key, press, rect_of, target_matching,
        type_str,
    };

    /// The size the interaction tests probe at.
    const SIZE: (f32, f32) = <AppState as Probe>::SIZE;

    /// A window with `count` entries in it, the first one selected.
    ///
    /// The contents are distinguishable on sight so a failure message says
    /// which row was reached, and the timestamps ascend so "most recent first"
    /// is the reverse of the loop that built them.
    fn with_entries(count: u64) -> AppState {
        let mut state = AppState::new();
        for i in 0..count {
            state.store.add(
                format!("clip number {i}"),
                ClipType::PlainText,
                i,
                format!("app{i}"),
            );
        }
        state.refresh_filter();
        state.selected_id = state.filtered_ids.first().copied();
        state
    }

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
            format!("x{}", "Я".repeat(200)),   // 2-byte run, shifted by 1
            format!("x{}", "日".repeat(200)),  // 3-byte run, shifted by 1
            format!("x{}", "🎉".repeat(200)),  // 4-byte run, shifted by 1
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
        // Was "10s ago". The desktop's clipboard viewer lists the same
        // entries and said "just now" for the same age; a seconds countdown
        // on a clipboard row is a number nobody reads and that changes on
        // every repaint.
        assert_eq!(e.time_display(1000), "just now");
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
        assert_eq!(format_size(2048), "2.0 KiB");
    }

    #[test]
    fn test_format_size_megabytes() {
        assert_eq!(format_size(2 * 1024 * 1024), "2.0 MiB");
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
        assert_eq!(state.add_tag_to_selected().as_deref(), Some("work"));
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
        assert_eq!(state.add_tag_to_selected(), None);
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
        assert_eq!(
            state.save_template(),
            Ok(TemplateSaved::Created("greet".to_string()))
        );
        assert_eq!(state.store.templates.len(), 1);
        assert!(state.template_name_input.is_empty());
        assert!(state.template_body_input.is_empty());
    }

    #[test]
    fn test_app_state_save_empty_template_ignored() {
        let mut state = AppState::new();
        state.template_name_input = String::new();
        state.template_body_input = "body".to_string();
        assert_eq!(state.save_template(), Err("a template needs a name"));
        assert!(state.store.templates.is_empty());
    }

    #[test]
    fn test_app_state_delete_selected_template() {
        let mut state = AppState::new();
        state
            .store
            .add_template("t".to_string(), "body".to_string());
        state.selected_template = Some(0);
        assert_eq!(state.delete_selected_template().as_deref(), Some("t"));
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
        assert_eq!(state.render_template().as_deref(), Some("greet"));
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
    fn test_build_frame_not_empty() {
        let state = AppState::new();
        let frame = build_frame(&state, 800.0, 600.0);
        assert!(!frame.into_tree().is_empty());
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
        let rt = build_frame(&state, 800.0, 600.0).into_tree();
        assert!(rt.commands.len() > 5);
    }

    #[test]
    fn test_build_render_tree_templates_tab() {
        let mut state = AppState::new();
        state.active_tab = ActiveTab::Templates;
        state
            .store
            .add_template("t".to_string(), "body".to_string());
        let rt = build_frame(&state, 800.0, 600.0).into_tree();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_frame_records_the_size_it_was_given() {
        let state = AppState::new();
        let frame = build_frame(&state, 800.0, 600.0);
        assert_eq!((frame.width, frame.height), (800.0, 600.0));
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
        // A window short enough to show only a handful of rows, so arrowing
        // down ten times has to move the view.
        state.window_size = (WINDOW_WIDTH, 400.0);
        assert!(state.visible_rows() < 10);
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
        let rt = build_frame(&state, 1024.0, 768.0).into_tree();
        // Should produce substantial render commands for the detail panel
        assert!(rt.commands.len() > 20);
    }

    // == The window, driven the way a user drives it ========================
    //
    // Everything below names a control and lets the renderer say where it is,
    // so a test keeps testing the same button after the layout moves it and
    // starts failing the moment the button stops being painted at all. See
    // `guitk::probe`.

    /// A wheel event of `dy` notches over the list pane.
    fn wheel_notch(dy: f32) -> Event {
        Event::Mouse(MouseEvent {
            x: 200.0,
            y: 300.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy },
        })
    }

    #[test]
    fn a_click_on_a_row_selects_that_entry() {
        let mut state = with_entries(5);
        let id = state.filtered_ids[2];
        assert_ne!(
            state.selected_id,
            Some(id),
            "a different row starts selected"
        );
        assert_eq!(click(&mut state, Target::Entry(id)), Action::Redraw);
        assert_eq!(state.selected_id, Some(id));
        assert_eq!(
            click(&mut state, Target::Entry(id)),
            Action::None,
            "clicking the row that is already selected changes nothing visible"
        );
    }

    #[test]
    fn the_detail_panel_sits_beside_the_list_rather_than_over_it() {
        let state = with_entries(3);
        let row = rect_of(&state, Target::Entry(state.filtered_ids[0])).expect("a row is drawn");
        let field = rect_of(&state, Target::TagField).expect("the detail panel is drawn");
        assert!(
            field.x >= row.right(),
            "the detail panel begins at {} but the list runs to {}",
            field.x,
            row.right()
        );
    }

    #[test]
    fn a_row_past_the_bottom_of_the_pane_is_not_clickable() {
        let state = with_entries(40);
        let visible = state.visible_rows();
        assert!(visible < 40, "the fixture must overflow the pane");
        assert!(is_visible(&state, Target::Entry(state.filtered_ids[0])));
        assert!(
            !is_visible(&state, Target::Entry(state.filtered_ids[visible])),
            "the first row past the bottom must not take clicks where it is not drawn"
        );
    }

    #[test]
    fn the_renderer_and_the_scroll_agree_on_how_many_rows_fit() {
        // Two copies of this number is how a keyboard selection scrolls to a
        // row the renderer put somewhere else, so the test is that there is
        // only one.
        let state = with_entries(60);
        for height in [400.0, 680.0, 900.0, 1400.0] {
            let drawn = state
                .draw((WINDOW_WIDTH, height))
                .hits()
                .iter()
                .filter(|(target, _)| matches!(target, Target::Entry(_)))
                .count();
            assert_eq!(
                drawn,
                rows_that_fit(height),
                "at {height}px the list painted {drawn} rows"
            );
        }
    }

    #[test]
    fn the_type_badge_cycles_every_type_and_comes_back_to_all() {
        let mut state = with_entries(3);
        assert_eq!(state.type_filter, None, "unfiltered to begin with");
        let mut seen = Vec::new();
        for _ in ClipType::all() {
            click(&mut state, Target::TypeFilter);
            seen.push(state.type_filter.expect("each click picks a type"));
        }
        assert_eq!(seen, ClipType::all().to_vec());
        click(&mut state, Target::TypeFilter);
        assert_eq!(state.type_filter, None, "and then back to every type");
        assert!(state.status.contains("every type"), "{}", state.status);
    }

    #[test]
    fn typing_in_the_search_box_narrows_the_list_and_backspacing_widens_it() {
        let mut state = with_entries(12);
        assert_eq!(state.filtered_ids.len(), 12);
        click(&mut state, Target::SearchBox);
        type_str(&mut state, "number 7");
        assert_eq!(
            state.filtered_ids.len(),
            1,
            "only `clip number 7` holds that text"
        );
        for _ in 0.."number 7".len() {
            key(&mut state, &press(Key::Backspace));
        }
        assert_eq!(state.filtered_ids.len(), 12);
        assert_eq!(
            key(&mut state, &press(Key::Backspace)),
            Action::None,
            "backspace on an empty box must not cost a frame"
        );
    }

    #[test]
    fn a_tag_added_from_the_detail_panel_reaches_the_strip_and_filters_by_itself() {
        let mut state = with_entries(4);
        assert!(!is_visible(&state, Target::TagChip(0)), "no tags yet");

        click(&mut state, Target::TagField);
        assert_eq!(state.focus, Some(Field::Tag));
        type_str(&mut state, "work");
        click(&mut state, Target::AddTag);
        assert!(state.status.contains("work"), "{}", state.status);

        assert!(
            is_visible(&state, Target::TagChip(0)),
            "the tag must reach the strip that filters by it"
        );
        click(&mut state, Target::TagChip(0));
        assert_eq!(state.filtered_ids.len(), 1, "narrowed to the tagged entry");
        assert!(
            state.stats_line().contains("tag: work"),
            "{}",
            state.stats_line()
        );

        click(&mut state, Target::TagAll);
        assert_eq!(state.filtered_ids.len(), 4);
        assert_eq!(
            click(&mut state, Target::TagAll),
            Action::None,
            "clearing a filter that is already clear changes nothing"
        );
    }

    #[test]
    fn adding_a_tag_an_entry_already_has_says_so_rather_than_doing_nothing() {
        let mut state = with_entries(2);
        click(&mut state, Target::TagField);
        type_str(&mut state, "work");
        click(&mut state, Target::AddTag);
        click(&mut state, Target::TagField);
        type_str(&mut state, "work");
        click(&mut state, Target::AddTag);
        assert!(state.status.contains("already has"), "{}", state.status);
    }

    #[test]
    fn tagging_after_the_selected_entry_was_deleted_says_so() {
        // The tag box lives in the detail panel, so it is only ever drawn with
        // an entry selected -- but the caret stays in it when the Delete button
        // takes that entry away, and Enter then commits into nothing.
        let mut state = with_entries(2);
        click(&mut state, Target::TagField);
        type_str(&mut state, "work");
        click(&mut state, Target::DeleteEntry);
        assert_eq!(state.selected_id, None);
        assert_eq!(state.focus, Some(Field::Tag), "the caret is where it was");

        key(&mut state, &press(Key::Enter));
        assert!(
            state.status.contains("select an entry to tag"),
            "{}",
            state.status
        );
    }

    #[test]
    fn a_tag_chip_in_the_detail_panel_removes_the_tag_it_names() {
        let mut state = with_entries(2);
        let id = state.selected_id.expect("the fixture selects a row");
        state.store.add_tag(id, "keep".to_string());
        state.store.add_tag(id, "drop".to_string());

        click(&mut state, Target::DetailTag(1));
        assert_eq!(
            state.store.get(id).map(|entry| entry.tags.clone()),
            Some(vec!["keep".to_string()])
        );
        assert!(state.status.contains("drop"), "{}", state.status);
    }

    #[test]
    fn copying_an_entry_moves_it_to_the_top_rather_than_growing_a_second_copy() {
        let mut state = with_entries(5);
        let id = state.filtered_ids[3];
        click(&mut state, Target::Entry(id));
        let before = state.store.total_entries();

        click(&mut state, Target::CopyEntry);
        assert_eq!(
            state.store.total_entries(),
            before,
            "copying what you already copied deduplicates everywhere else too"
        );
        assert_eq!(state.filtered_ids.first().copied(), Some(id));
        assert_eq!(state.selected_id, Some(id));
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn clear_all_keeps_the_pinned_entries_and_then_says_there_is_nothing_left() {
        let mut state = with_entries(4);
        let keep = state.filtered_ids[1];
        click(&mut state, Target::Entry(keep));
        click(&mut state, Target::PinEntry);
        assert!(state.status.contains("pinned"), "{}", state.status);

        click(&mut state, Target::ClearAll);
        assert_eq!(state.store.total_entries(), 1);
        assert!(state.store.get(keep).is_some(), "pinned entries survive");
        assert!(state.status.contains("cleared 3"), "{}", state.status);

        click(&mut state, Target::ClearAll);
        assert!(
            state.status.contains("nothing unpinned"),
            "{}",
            state.status
        );
    }

    #[test]
    fn marking_rows_and_deleting_removes_every_one_of_them() {
        let mut state = with_entries(6);
        let doomed = [
            state.filtered_ids[0],
            state.filtered_ids[2],
            state.filtered_ids[4],
        ];
        for id in doomed {
            click(&mut state, Target::Entry(id));
            click(&mut state, Target::MarkEntry);
        }
        assert_eq!(state.marked.len(), 3);
        assert!(
            state.stats_line().contains("3 marked"),
            "{}",
            state.stats_line()
        );

        click(&mut state, Target::DeleteEntry);
        for id in doomed {
            assert!(
                state.store.get(id).is_none(),
                "{id} was marked for deletion"
            );
        }
        assert_eq!(state.store.total_entries(), 3);
        assert!(state.marked.is_empty(), "the marks went with the entries");
    }

    #[test]
    fn a_mark_survives_the_deletion_of_a_row_above_it() {
        // A mark held as a position would slide onto its neighbour the moment
        // anything above it went, and delete the wrong entry.
        let mut state = with_entries(6);
        let mark = state.filtered_ids[4];
        click(&mut state, Target::Entry(mark));
        click(&mut state, Target::MarkEntry);

        let above = state.filtered_ids[1];
        state.store.delete(above);
        state.forget_deleted();
        assert_eq!(state.marked, vec![mark], "a mark is an id, not a position");

        click(&mut state, Target::DeleteEntry);
        assert!(
            state.store.get(mark).is_none(),
            "the marked entry, not its neighbour"
        );
        assert_eq!(state.store.total_entries(), 4);
    }

    #[test]
    fn deleting_the_selected_row_leaves_nothing_selected() {
        let mut state = with_entries(3);
        let id = state.selected_id.expect("the fixture selects a row");
        click(&mut state, Target::DeleteEntry);
        assert!(state.store.get(id).is_none());
        assert_eq!(
            state.selected_id, None,
            "a selection pointing at a deleted id is a Copy button that refuses \
             while a row still looks selected"
        );
        click(&mut state, Target::CopyEntry);
        assert!(state.status.contains("select an entry"), "{}", state.status);
    }

    #[test]
    fn a_button_with_nothing_to_act_on_says_why_instead_of_doing_nothing() {
        let mut state = AppState::new();
        state.refresh_filter();
        for target in [
            Target::CopyEntry,
            Target::PinEntry,
            Target::MarkEntry,
            Target::DeleteEntry,
            Target::ClearAll,
            Target::ExportAll,
            Target::ImportSelected,
            Target::UseTemplate,
            Target::DeleteTemplate,
            Target::SaveTemplate,
        ] {
            state.status.clear();
            state.active_tab = match target {
                Target::UseTemplate | Target::DeleteTemplate | Target::SaveTemplate => {
                    ActiveTab::Templates
                }
                _ => ActiveTab::History,
            };
            assert_eq!(click(&mut state, target), Action::Redraw);
            assert!(
                !state.status.is_empty(),
                "{target:?} did nothing and said nothing, which reads as broken"
            );
        }
    }

    #[test]
    fn an_exported_history_imports_back_out_of_the_entry_it_was_put_in() {
        // The clipboard is the transport: Export puts the whole history in an
        // entry, and Import reads it back out of whichever entry holds one.
        let mut state = with_entries(3);
        click(&mut state, Target::ExportAll);
        let holder = state.selected_id.expect("the export becomes the top entry");

        for id in state.filtered_ids.clone() {
            if id != holder {
                state.store.delete(id);
            }
        }
        state.forget_deleted();
        state.selected_id = Some(holder);
        assert_eq!(state.store.total_entries(), 1, "only the export is left");

        click(&mut state, Target::ImportSelected);
        assert!(state.status.contains("imported 3"), "{}", state.status);
        assert_eq!(
            state.store.total_entries(),
            4,
            "the three entries and the export"
        );
    }

    #[test]
    fn importing_from_an_entry_that_is_not_an_export_says_so() {
        let mut state = with_entries(2);
        click(&mut state, Target::ImportSelected);
        assert!(state.status.contains("not an export"), "{}", state.status);
    }

    #[test]
    fn saving_a_template_over_one_of_the_same_name_says_it_replaced_it() {
        // `add_template` overwrites silently, and a user who has just lost a
        // body they spent five minutes on deserves to be told which happened.
        let mut state = AppState::new();
        state.refresh_filter();
        click(&mut state, Target::Tab(ActiveTab::Templates));

        click(&mut state, Target::TemplateName);
        type_str(&mut state, "greet");
        click(&mut state, Target::TemplateBody);
        type_str(&mut state, "Hello {name}");
        click(&mut state, Target::SaveTemplate);
        assert!(
            state.status.contains("saved the template greet"),
            "{}",
            state.status
        );

        click(&mut state, Target::TemplateName);
        type_str(&mut state, "greet");
        click(&mut state, Target::TemplateBody);
        type_str(&mut state, "Hi {name}");
        click(&mut state, Target::SaveTemplate);
        assert!(
            state.status.contains("replaced the template greet"),
            "{}",
            state.status
        );
        assert_eq!(state.store.templates.len(), 1);
    }

    #[test]
    fn using_a_template_copies_the_substituted_text_to_the_top_of_the_history() {
        let mut state = AppState::new();
        state
            .store
            .add_template("greet".to_string(), "Hello {name}".to_string());
        state.refresh_filter();
        click(&mut state, Target::Tab(ActiveTab::Templates));

        click(&mut state, Target::Template(0));
        assert_eq!(
            state.template_vars,
            vec![("name".to_string(), String::new())],
            "selecting a template offers a box for each placeholder"
        );
        state.template_vars = vec![("name".to_string(), "Ada".to_string())];

        click(&mut state, Target::UseTemplate);
        let top = state
            .filtered_ids
            .first()
            .copied()
            .expect("the rendered text is an entry");
        assert_eq!(
            state.store.get(top).map(|entry| entry.content.as_str()),
            Some("Hello Ada")
        );
        assert_eq!(state.selected_id, Some(top));

        click(&mut state, Target::DeleteTemplate);
        assert!(state.store.templates.is_empty());
        assert!(state.template_vars.is_empty(), "the boxes went with it");
    }

    #[test]
    fn the_tabs_swap_which_panel_is_drawn_and_the_toolbar_stays() {
        let mut state = with_entries(2);
        let row = Target::Entry(state.filtered_ids[0]);
        assert!(is_visible(&state, row));
        assert!(!is_visible(&state, Target::SaveTemplate));

        click(&mut state, Target::Tab(ActiveTab::Templates));
        assert!(is_visible(&state, Target::SaveTemplate));
        assert!(!is_visible(&state, row), "the history pane is gone");
        assert!(
            is_visible(&state, Target::CopyEntry),
            "the toolbar is on both tabs"
        );
        assert_eq!(
            click(&mut state, Target::Tab(ActiveTab::Templates)),
            Action::None,
            "clicking the tab already showing changes nothing"
        );

        // Left and Right do the same thing from the keyboard.
        key(&mut state, &press(Key::Left));
        assert_eq!(state.active_tab, ActiveTab::History);
        assert!(is_visible(&state, row));
    }

    #[test]
    fn the_clock_carries_part_seconds_instead_of_truncating_them() {
        let mut state = with_entries(1);
        state.now = 0;
        // Four quarter-second ticks are one second. Truncating each would stop
        // the clock for good under any sub-second tick.
        for _ in 0..6 {
            state.handle_event(&Event::Tick { elapsed_ms: 250 }, SIZE);
        }
        assert_eq!(state.now, 1, "1500ms is one whole second and a carry");
        for _ in 0..2 {
            state.handle_event(&Event::Tick { elapsed_ms: 250 }, SIZE);
        }
        assert_eq!(state.now, 2, "the carried 500ms paid for the second one");
    }

    #[test]
    fn an_empty_history_does_not_hold_the_desktop_awake() {
        let empty = AppState::new();
        assert_eq!(
            empty.tick_interval(),
            None,
            "nothing on screen is ageing, so nothing needs a clock"
        );
        assert_eq!(
            with_entries(1).tick_interval(),
            Some(Duration::from_secs(1))
        );
    }

    #[test]
    fn the_wheel_scrolls_the_list_and_stops_at_both_ends() {
        let mut state = with_entries(40);
        let max = state.filtered_ids.len() - state.visible_rows();
        assert!(max > 0, "the fixture must overflow the pane");

        // `dy` is positive away from the user, which scrolls towards row 0, so
        // the negative notch is the one that walks down the list.
        for _ in 0..40 {
            state.handle_event(&wheel_notch(-1.0), SIZE);
        }
        assert_eq!(state.scroll_offset, max, "clamped at the end, not past it");
        assert_eq!(
            state.handle_event(&wheel_notch(-1.0), SIZE),
            Action::None,
            "a wheel that moves nothing must not cost a frame"
        );

        for _ in 0..40 {
            state.handle_event(&wheel_notch(1.0), SIZE);
        }
        assert_eq!(state.scroll_offset, 0);
        assert_eq!(state.handle_event(&wheel_notch(1.0), SIZE), Action::None);
    }

    #[test]
    fn a_trackpad_that_sends_fractions_of_a_notch_still_moves_the_list() {
        let mut state = with_entries(40);
        // A tenth of a notch is under a third of a row: rounding each event to
        // zero would leave the list motionless however long the drag went on.
        for _ in 0..9 {
            state.handle_event(&wheel_notch(-0.1), SIZE);
        }
        assert_eq!(
            state.scroll_offset, 2,
            "nine tenths of a notch is 2.7 rows, and the 0.7 is kept for later"
        );
    }

    #[test]
    fn a_taller_window_pulls_the_view_back_over_the_rows_it_can_show() {
        let mut state = with_entries(30);
        state.scroll_offset = 30 - state.visible_rows();
        let deep = state.scroll_offset;
        assert!(deep > 0);

        state.on_event(&Event::Resize {
            width: WINDOW_WIDTH as u32,
            height: 1400,
        });
        assert!(
            state.scroll_offset < deep,
            "a taller window shows more rows, so the old offset now runs past the end"
        );
        assert_eq!(state.scroll_offset, 30 - state.visible_rows());
    }

    #[test]
    fn the_first_frame_is_drawn_at_the_size_it_is_handed() {
        // It goes out before any `Event::Resize`, so a renderer that trusted
        // the size it remembered would draw that frame at the wrong one.
        let mut state = with_entries(4);
        let tree = state.render(1600.0, 900.0);
        assert_eq!(state.window_size, (1600.0, 900.0));
        assert!(!tree.commands.is_empty());
        assert_eq!(state.visible_rows(), rows_that_fit(900.0));
    }

    #[test]
    fn clicking_the_background_puts_the_caret_away() {
        let mut state = with_entries(3);
        click(&mut state, Target::SearchBox);
        assert_eq!(state.focus, Some(Field::Search));

        assert_eq!(click_background(&mut state), Action::Redraw);
        assert_eq!(
            state.focus, None,
            "a stray keystroke must not land in a box the user has stopped looking at"
        );
        assert_eq!(click_background(&mut state), Action::None);
    }

    #[test]
    fn escape_closes_the_window_but_not_from_inside_a_text_box() {
        let mut state = with_entries(2);
        click(&mut state, Target::SearchBox);
        assert_eq!(key(&mut state, &press(Key::Escape)), Action::Redraw);
        assert_eq!(state.focus, None, "the first Escape leaves the box");
        assert_eq!(key(&mut state, &press(Key::Escape)), Action::Quit);
    }

    #[test]
    fn tab_walks_the_caret_through_every_text_box_and_back() {
        let mut state = with_entries(1);
        click(&mut state, Target::SearchBox);
        let mut seen = vec![state.focus];
        for _ in 0..4 {
            key(&mut state, &press(Key::Tab));
            seen.push(state.focus);
        }
        assert_eq!(
            seen,
            vec![
                Some(Field::Search),
                Some(Field::Tag),
                Some(Field::TemplateName),
                Some(Field::TemplateBody),
                Some(Field::Search),
            ]
        );
    }

    #[test]
    fn the_arrow_keys_scroll_the_selection_into_view() {
        let mut state = with_entries(40);
        let visible = state.visible_rows();
        for _ in 0..visible {
            key(&mut state, &press(Key::Down));
        }
        assert_eq!(state.scroll_offset, 1, "one step past the last visible row");

        let selected = state.selected_id.expect("something is selected");
        assert!(
            is_visible(&state, Target::Entry(selected)),
            "the row the keyboard moved to must be a row the renderer painted"
        );
        assert_eq!(
            target_matching(&state, |target| matches!(target, Target::Entry(_))),
            Some(Target::Entry(
                state.filtered_ids[state.scroll_offset + visible - 1]
            )),
            "the last row painted is the bottom of the pane"
        );

        key(&mut state, &press(Key::Home));
        assert_eq!(state.scroll_offset, 0);
        assert_eq!(state.selected_id, state.filtered_ids.first().copied());
    }

    #[test]
    fn page_down_and_page_up_move_by_a_paneful_and_stop_at_the_ends() {
        let mut state = with_entries(40);
        let visible = state.visible_rows();
        key(&mut state, &press(Key::PageDown));
        assert_eq!(state.scroll_offset, visible);
        key(&mut state, &press(Key::PageUp));
        assert_eq!(state.scroll_offset, 0);
        key(&mut state, &press(Key::PageUp));
        assert_eq!(state.scroll_offset, 0, "clamped at the top");
    }

    #[test]
    fn enter_copies_and_delete_deletes_without_touching_the_mouse() {
        let mut state = with_entries(4);
        key(&mut state, &press(Key::Enter));
        assert!(state.status.contains("copied"), "{}", state.status);
        assert_eq!(
            state.store.total_entries(),
            4,
            "deduplicated, not duplicated"
        );

        let id = state.selected_id.expect("the copy is selected");
        key(&mut state, &press(Key::Delete));
        assert!(state.store.get(id).is_none());
        assert_eq!(state.store.total_entries(), 3);
    }

    #[test]
    fn a_keystroke_meant_for_a_text_box_does_not_reach_the_shortcuts() {
        // `d` in the tag box is a letter, not the Delete key -- but a handler
        // that checked the shortcuts first would have eaten it.
        let mut state = with_entries(3);
        click(&mut state, Target::TagField);
        type_str(&mut state, "d");
        assert_eq!(state.tag_input, "d");
        assert_eq!(state.store.total_entries(), 3, "nothing was deleted");
    }

    #[test]
    fn every_control_the_window_can_draw_is_recorded_by_the_renderer() {
        // A control the renderer never records is a control no test can reach
        // and no click can land on, so the count is worth asserting outright.
        let mut state = with_entries(3);
        let id = state.selected_id.expect("the fixture selects a row");
        state.store.add_tag(id, "work".to_string());
        state
            .store
            .add_template("greet".to_string(), "Hello {name}".to_string());
        state.selected_template = Some(0);
        state.refresh_filter();

        let mut names = control_names(&state);
        state.active_tab = ActiveTab::Templates;
        names.extend(control_names(&state));
        names.sort_unstable();
        names.dedup();

        assert_eq!(
            names,
            vec![
                "AddTag",
                "ClearAll",
                "CopyEntry",
                "DeleteEntry",
                "DeleteTemplate",
                "DetailTag",
                "Entry",
                "ExportAll",
                "ImportSelected",
                "MarkEntry",
                "PinEntry",
                "SaveTemplate",
                "SearchBox",
                "Tab",
                "TagAll",
                "TagChip",
                "TagField",
                "Template",
                "TemplateBody",
                "TemplateName",
                "TypeFilter",
                "UseTemplate",
            ]
        );
    }
}
