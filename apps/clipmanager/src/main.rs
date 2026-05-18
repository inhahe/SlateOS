//! OurOS Clipboard Manager — a full-featured clipboard history and snippet manager.
//!
//! Provides clipboard history tracking (up to 500 entries), search, filtering by
//! content type, tagging, pinning, template management with placeholder substitution,
//! batch operations, statistics, and export/import. Inspired by CopyQ and Ditto.

use std::collections::VecDeque;

use guitk::color::Color;
use guitk::event::{Event, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::layout::{FlexDirection, FlexItem, FlexJustify, LayoutBox, Size, SizeConstraint};
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
use guitk::style::{CornerRadii, Edges};
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
    fn preview(&self) -> String {
        let mut out = String::new();
        let mut lines_taken = 0usize;
        for line in self.content.lines() {
            if lines_taken >= PREVIEW_LINES {
                break;
            }
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(line);
            lines_taken = lines_taken.saturating_add(1);
            if out.len() >= PREVIEW_MAX_CHARS {
                break;
            }
        }
        if out.len() > PREVIEW_MAX_CHARS {
            out.truncate(PREVIEW_MAX_CHARS);
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
            if bytes.get(i).copied() == Some(b'{') {
                if let Some(end) = self.body[i..].find('}') {
                    let name = &self.body[i + 1..i + end];
                    if !name.is_empty() && !out.contains(&name.to_string()) {
                        out.push(name.to_string());
                    }
                    i = i + end + 1;
                    continue;
                }
            }
            i = i.saturating_add(1);
        }
        out
    }
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
        if let Some(pos) = self.entries.iter().position(|e| e.content == content) {
            if let Some(mut existing) = self.entries.remove(pos) {
                existing.timestamp = timestamp;
                existing.source_app = source_app;
                let id = existing.id;
                self.entries.push_front(existing);
                return id;
            }
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
        if let Some(i) = idx {
            if let Some(removed) = self.entries.remove(i) {
                self.total_size = self.total_size.saturating_sub(removed.size_bytes);
                return true;
            }
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
        if let Some(pos) = self.entries.iter().position(|e| e.id == id) {
            if let Some(removed) = self.entries.remove(pos) {
                self.total_size = self.total_size.saturating_sub(removed.size_bytes);
                return true;
            }
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
        if let Some(entry) = self.get_mut(id) {
            if !entry.tags.contains(&tag) {
                entry.tags.push(tag);
            }
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
                let type_ok = type_filter.map_or(true, |t| e.clip_type == t);
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
    fn export_text(&self) -> String {
        let mut out = String::new();
        for entry in &self.entries {
            out.push_str("---ENTRY---\n");
            out.push_str(&format!("id:{}\n", entry.id));
            out.push_str(&format!("type:{}\n", entry.clip_type.label()));
            out.push_str(&format!("timestamp:{}\n", entry.timestamp));
            out.push_str(&format!("source:{}\n", entry.source_app));
            out.push_str(&format!("pinned:{}\n", entry.pinned));
            if !entry.tags.is_empty() {
                out.push_str(&format!("tags:{}\n", entry.tags.join(",")));
            }
            out.push_str("content:\n");
            out.push_str(&entry.content);
            out.push('\n');
        }
        out
    }

    /// Import entries from text format. Returns count of imported entries.
    fn import_text(&mut self, data: &str, base_timestamp: u64) -> usize {
        let mut count = 0usize;
        for block in data.split("---ENTRY---") {
            let block = block.trim();
            if block.is_empty() {
                continue;
            }
            let mut content_lines: Vec<&str> = Vec::new();
            let mut clip_type = ClipType::PlainText;
            let mut source = String::from("import");
            let mut pinned = false;
            let mut tags: Vec<String> = Vec::new();
            let mut in_content = false;

            for line in block.lines() {
                if in_content {
                    content_lines.push(line);
                    continue;
                }
                if line == "content:" {
                    in_content = true;
                    continue;
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
                } else if let Some(val) = line.strip_prefix("tags:") {
                    tags = val.split(',').map(|s| s.trim().to_string()).collect();
                }
                // id: and timestamp: are ignored on import (we assign fresh ones)
            }

            let content = content_lines.join("\n");
            if content.is_empty() {
                continue;
            }

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
        if let Some(idx) = self.selected_template {
            if let Some(tmpl) = self.store.templates.get(idx) {
                let name = tmpl.name.clone();
                self.store.remove_template(&name);
                self.selected_template = None;
            }
        }
    }

    /// Select a template by index and populate placeholder vars.
    fn select_template(&mut self, idx: usize) {
        self.selected_template = Some(idx);
        if let Some(tmpl) = self.store.templates.get(idx) {
            let placeholders = tmpl.placeholders();
            self.template_vars = placeholders.into_iter().map(|p| (p, String::new())).collect();
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
    render_tab_bar(&mut rt, state, margin, tab_y, width - margin * 2.0, tab_bar_h);

    // ---- Content area ----
    let content_y = tab_y + tab_bar_h + 4.0;
    let content_h = height - content_y - stats_bar_h - toolbar_h - 12.0;

    match state.active_tab {
        ActiveTab::History => {
            render_history_panel(&mut rt, state, margin, content_y, width - margin * 2.0, content_h);
        }
        ActiveTab::Templates => {
            render_templates_panel(&mut rt, state, margin, content_y, width - margin * 2.0, content_h);
        }
    }

    // ---- Toolbar ----
    let toolbar_y = height - stats_bar_h - toolbar_h - 4.0;
    render_toolbar(&mut rt, state, margin, toolbar_y, width - margin * 2.0, toolbar_h);

    // ---- Statistics bar ----
    let stats_y = height - stats_bar_h - 2.0;
    render_stats_bar(&mut rt, state, margin, stats_y, width - margin * 2.0, stats_bar_h);

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
    });

    // Type filter badge
    let filter_label = state
        .type_filter
        .map_or("All Types", |t| t.label());
    let filter_color = state
        .type_filter
        .map_or(OVERLAY0, |t| t.badge_color());
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

    let tabs = [("History", ActiveTab::History), ("Templates", ActiveTab::Templates)];
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
        });
        tx += tab_w + 4.0;
    }
}

fn render_history_panel(
    rt: &mut RenderTree,
    state: &AppState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
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
        if let Some(&id) = state.filtered_ids.get(i) {
            if let Some(entry) = state.store.get(id) {
                let is_selected = state.selected_id == Some(id);
                render_entry_row(rt, entry, x + 4.0, ry, list_w - 8.0, row_h, is_selected, state.now);
            }
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
        });
    }
}

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
    });
    rt.push(RenderCommand::Text {
        x: x + w - 70.0,
        y: y + 18.0,
        text: entry.source_app.clone(),
        color: OVERLAY0,
        font_size: 10.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
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
        });
        cy += 16.0;
    }

    // Tags
    if !entry.tags.is_empty() {
        let mut tx = x + pad;
        for tag in &entry.tags {
            let tag_w = tag.len() as f32 * 7.0 + 16.0;
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
            });
            tx += tag_w + 4.0;
        }
        cy += 24.0;
    }

    // Code detection hint
    if entry.clip_type == ClipType::Code || entry.clip_type == ClipType::PlainText {
        if let Some(lang) = detect_code_language(&entry.content) {
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
            });
            cy += 24.0;
        }
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
        });
    }

    rt.push(RenderCommand::PopClip);
    rt.push(RenderCommand::PopClip);
}

fn render_templates_panel(
    rt: &mut RenderTree,
    state: &AppState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
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
    });

    // Show filtered count on right
    let filtered_info = format!(
        "{} shown",
        state.filtered_ids.len()
    );
    rt.push(RenderCommand::Text {
        x: x + w - 80.0,
        y: y + 5.0,
        text: filtered_info,
        color: OVERLAY0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
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

    // == ClipEntry tests ====================================================

    #[test]
    fn test_clip_entry_creation() {
        let e = ClipEntry::new(1, "hello".to_string(), ClipType::PlainText, 100, "app".to_string());
        assert_eq!(e.id, 1);
        assert_eq!(e.content, "hello");
        assert_eq!(e.clip_type, ClipType::PlainText);
        assert!(!e.pinned);
        assert!(e.tags.is_empty());
        assert_eq!(e.size_bytes, 5);
    }

    #[test]
    fn test_clip_entry_preview_short() {
        let e = ClipEntry::new(1, "short text".to_string(), ClipType::PlainText, 0, String::new());
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
        assert!(p.len() <= PREVIEW_MAX_CHARS + 3); // +3 for "..."
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
        let id = store.add("hello".to_string(), ClipType::PlainText, 100, "vim".to_string());
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
            store.add(format!("entry-{i}"), ClipType::PlainText, i as u64, String::new());
        }
        assert!(store.total_entries() <= MAX_ENTRIES);
    }

    #[test]
    fn test_store_pinned_not_evicted() {
        let mut store = ClipboardStore::new();
        let pin_id = store.add("pinned".to_string(), ClipType::PlainText, 0, String::new());
        store.toggle_pin(pin_id);
        for i in 1..=MAX_ENTRIES {
            store.add(format!("entry-{i}"), ClipType::PlainText, i as u64, String::new());
        }
        assert!(store.get(pin_id).is_some(), "Pinned entry must survive eviction");
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
        store.add("Hello World".to_string(), ClipType::PlainText, 0, String::new());
        store.add("goodbye world".to_string(), ClipType::PlainText, 0, String::new());
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
        store.add("untagged".to_string(), ClipType::PlainText, 0, String::new());
        assert_eq!(store.filter_by_tag("important").len(), 1);
        assert_eq!(store.filter_by_tag("nope").len(), 0);
    }

    #[test]
    fn test_store_search_filtered() {
        let mut store = ClipboardStore::new();
        store.add("hello text".to_string(), ClipType::PlainText, 0, String::new());
        store.add("<p>hello html</p>".to_string(), ClipType::Html, 0, String::new());
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
        let text_count = stats.iter().find(|(t, _)| *t == ClipType::PlainText).map(|(_, c)| *c);
        let code_count = stats.iter().find(|(t, _)| *t == ClipType::Code).map(|(_, c)| *c);
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
        let t = ClipTemplate::new("email".to_string(), "Dear {name}, re: {subject}".to_string());
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
        assert_eq!(store.get_template("greet").map(|t| t.body.as_str()), Some("Hey {name}!"));
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
        let id = store.add("test content".to_string(), ClipType::PlainText, 100, "editor".to_string());
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
        assert_eq!(detect_code_language("fn main() -> Result<()> { }"), Some("rust"));
    }

    #[test]
    fn test_detect_python() {
        assert_eq!(detect_code_language("def hello():\n    pass"), Some("python"));
    }

    #[test]
    fn test_detect_javascript() {
        assert_eq!(detect_code_language("function foo() {}"), Some("javascript"));
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
        assert_eq!(detect_code_language("Hello world, how are you today?"), None);
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
        state.store.add("hello".to_string(), ClipType::PlainText, 0, String::new());
        state.store.add("world".to_string(), ClipType::PlainText, 0, String::new());
        state.refresh_filter();
        assert_eq!(state.filtered_ids.len(), 2);
    }

    #[test]
    fn test_app_state_search_filter() {
        let mut state = AppState::new();
        state.store.add("alpha".to_string(), ClipType::PlainText, 0, String::new());
        state.store.add("beta".to_string(), ClipType::PlainText, 0, String::new());
        state.search_query = "alpha".to_string();
        state.refresh_filter();
        assert_eq!(state.filtered_ids.len(), 1);
    }

    #[test]
    fn test_app_state_type_filter() {
        let mut state = AppState::new();
        state.store.add("txt".to_string(), ClipType::PlainText, 0, String::new());
        state.store.add("code".to_string(), ClipType::Code, 0, String::new());
        state.type_filter = Some(ClipType::Code);
        state.refresh_filter();
        assert_eq!(state.filtered_ids.len(), 1);
    }

    #[test]
    fn test_app_state_select_next_prev() {
        let mut state = AppState::new();
        state.store.add("a".to_string(), ClipType::PlainText, 0, String::new());
        state.store.add("b".to_string(), ClipType::PlainText, 0, String::new());
        state.store.add("c".to_string(), ClipType::PlainText, 0, String::new());
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
        let id = state.store.add("del".to_string(), ClipType::PlainText, 0, String::new());
        state.refresh_filter();
        state.selected_id = Some(id);
        state.delete_selected();
        assert!(state.selected_id.is_none());
        assert_eq!(state.store.total_entries(), 0);
    }

    #[test]
    fn test_app_state_toggle_pin_selected() {
        let mut state = AppState::new();
        let id = state.store.add("pin".to_string(), ClipType::PlainText, 0, String::new());
        state.selected_id = Some(id);
        state.toggle_pin_selected();
        assert_eq!(state.store.get(id).map(|e| e.pinned), Some(true));
    }

    #[test]
    fn test_app_state_add_tag_to_selected() {
        let mut state = AppState::new();
        let id = state.store.add("t".to_string(), ClipType::PlainText, 0, String::new());
        state.selected_id = Some(id);
        state.tag_input = "work".to_string();
        state.add_tag_to_selected();
        assert!(state.tag_input.is_empty());
        assert_eq!(state.store.get(id).map(|e| e.tags.len()), Some(1));
    }

    #[test]
    fn test_app_state_add_empty_tag_ignored() {
        let mut state = AppState::new();
        let id = state.store.add("t".to_string(), ClipType::PlainText, 0, String::new());
        state.selected_id = Some(id);
        state.tag_input = "   ".to_string();
        state.add_tag_to_selected();
        assert_eq!(state.store.get(id).map(|e| e.tags.len()), Some(0));
    }

    #[test]
    fn test_app_state_remove_tag_from_selected() {
        let mut state = AppState::new();
        let id = state.store.add("t".to_string(), ClipType::PlainText, 0, String::new());
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
        state.store.add_template("t".to_string(), "body".to_string());
        state.selected_template = Some(0);
        state.delete_selected_template();
        assert!(state.store.templates.is_empty());
        assert!(state.selected_template.is_none());
    }

    #[test]
    fn test_app_state_select_template_populates_vars() {
        let mut state = AppState::new();
        state.store.add_template("email".to_string(), "Dear {name}, re: {subject}".to_string());
        state.select_template(0);
        assert_eq!(state.template_vars.len(), 2);
    }

    #[test]
    fn test_app_state_render_template() {
        let mut state = AppState::new();
        state.store.add_template("greet".to_string(), "Hi {name}!".to_string());
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
        state.store.add("data".to_string(), ClipType::PlainText, 0, String::new());
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
        state.store.add("hello".to_string(), ClipType::PlainText, 100, "app".to_string());
        state.refresh_filter();
        state.selected_id = state.filtered_ids.first().copied();
        let rt = build_render_tree(&state, 800.0, 600.0);
        assert!(rt.commands.len() > 5);
    }

    #[test]
    fn test_build_render_tree_templates_tab() {
        let mut state = AppState::new();
        state.active_tab = ActiveTab::Templates;
        state.store.add_template("t".to_string(), "body".to_string());
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
            store.add(format!("entry number {i}"), ClipType::PlainText, i, String::new());
        }
        let results = store.search("number 15");
        // Should match "entry number 15", "entry number 150", etc.
        assert!(!results.is_empty());
    }

    #[test]
    fn test_scroll_offset_adjustment() {
        let mut state = AppState::new();
        for i in 0..30 {
            state.store.add(format!("e{i}"), ClipType::PlainText, i, String::new());
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
        store.add("same".to_string(), ClipType::PlainText, 10, "old".to_string());
        store.add("same".to_string(), ClipType::PlainText, 20, "new".to_string());
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
