//! Clipboard history viewer widget for the desktop shell.
//!
//! Provides a popup panel (activated via Super+V or system tray) showing
//! recent clipboard entries with preview, search, pinning, and format info.
//! Integrates with the gui/clipboard service.

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Theme
// ============================================================================

const COL_BASE: Color = Color::from_hex(0x1E1E2E);
const COL_MANTLE: Color = Color::from_hex(0x181825);
const COL_SURFACE0: Color = Color::from_hex(0x313244);
const COL_SURFACE1: Color = Color::from_hex(0x45475A);
const COL_SURFACE2: Color = Color::from_hex(0x585B70);
const COL_TEXT: Color = Color::from_hex(0xCDD6F4);
const COL_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const COL_BLUE: Color = Color::from_hex(0x89B4FA);
const COL_GREEN: Color = Color::from_hex(0xA6E3A1);
const COL_YELLOW: Color = Color::from_hex(0xF9E2AF);
const COL_RED: Color = Color::from_hex(0xF38BA8);
const COL_PEACH: Color = Color::from_hex(0xFAB387);
const COL_LAVENDER: Color = Color::from_hex(0xB4BEFE);

// ============================================================================
// Clipboard entry types
// ============================================================================

/// Format of a clipboard entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipFormat {
    /// Plain text.
    PlainText,
    /// Rich text / HTML.
    RichText,
    /// Image data (with dimensions).
    Image,
    /// File path(s).
    FilePaths,
    /// Custom/binary data.
    Custom,
}

impl ClipFormat {
    /// Human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::PlainText => "Text",
            Self::RichText => "Rich Text",
            Self::Image => "Image",
            Self::FilePaths => "Files",
            Self::Custom => "Custom",
        }
    }

    /// Icon character for the format.
    pub fn icon(&self) -> &'static str {
        match self {
            Self::PlainText => "T",
            Self::RichText => "R",
            Self::Image => "I",
            Self::FilePaths => "F",
            Self::Custom => "?",
        }
    }

    /// Badge color.
    pub fn color(&self) -> Color {
        match self {
            Self::PlainText => COL_BLUE,
            Self::RichText => COL_LAVENDER,
            Self::Image => COL_GREEN,
            Self::FilePaths => COL_PEACH,
            Self::Custom => COL_SURFACE2,
        }
    }
}

/// A single clipboard history entry.
#[derive(Debug, Clone)]
pub struct ClipEntry {
    /// Unique ID.
    pub id: u64,
    /// Format of the data.
    pub format: ClipFormat,
    /// Preview text (first ~200 chars for text, dimensions for images).
    pub preview: String,
    /// Size in bytes of the full data.
    pub size_bytes: usize,
    /// Timestamp when copied (seconds since epoch).
    pub timestamp: u64,
    /// Source application name (if known).
    pub source_app: Option<String>,
    /// Whether this entry is pinned (won't be evicted).
    pub pinned: bool,
    /// Whether this entry is marked as sensitive (auto-cleared).
    pub sensitive: bool,
    /// For images: width.
    pub image_width: Option<u32>,
    /// For images: height.
    pub image_height: Option<u32>,
}

impl ClipEntry {
    /// Create a text entry.
    pub fn text(id: u64, content: &str, timestamp: u64) -> Self {
        let preview = if content.len() > 200 {
            let mut s = content[..197].to_string();
            s.push_str("...");
            s
        } else {
            content.to_string()
        };
        Self {
            id,
            format: ClipFormat::PlainText,
            preview,
            size_bytes: content.len(),
            timestamp,
            source_app: None,
            pinned: false,
            sensitive: false,
            image_width: None,
            image_height: None,
        }
    }

    /// Create an image entry.
    pub fn image(id: u64, width: u32, height: u32, size: usize, timestamp: u64) -> Self {
        Self {
            id,
            format: ClipFormat::Image,
            preview: format!("{}x{} image", width, height),
            size_bytes: size,
            timestamp,
            source_app: None,
            pinned: false,
            sensitive: false,
            image_width: Some(width),
            image_height: Some(height),
        }
    }

    /// Create a file paths entry.
    pub fn files(id: u64, paths: &[&str], timestamp: u64) -> Self {
        let preview = if paths.len() == 1 {
            paths[0].to_string()
        } else {
            format!("{} files", paths.len())
        };
        let total_bytes: usize = paths.iter().map(|p| p.len()).sum();
        Self {
            id,
            format: ClipFormat::FilePaths,
            preview,
            size_bytes: total_bytes,
            timestamp,
            source_app: None,
            pinned: false,
            sensitive: false,
            image_width: None,
            image_height: None,
        }
    }

    /// Format the size for display.
    pub fn size_display(&self) -> String {
        if self.size_bytes < 1024 {
            format!("{} B", self.size_bytes)
        } else if self.size_bytes < 1024 * 1024 {
            format!("{:.1} KB", self.size_bytes as f64 / 1024.0)
        } else {
            format!("{:.1} MB", self.size_bytes as f64 / (1024.0 * 1024.0))
        }
    }

    /// Format the age for display.
    pub fn age_display(&self, now: u64) -> String {
        let elapsed = now.saturating_sub(self.timestamp);
        if elapsed < 60 {
            "just now".to_string()
        } else if elapsed < 3600 {
            format!("{}m ago", elapsed / 60)
        } else if elapsed < 86400 {
            format!("{}h ago", elapsed / 3600)
        } else {
            format!("{}d ago", elapsed / 86400)
        }
    }
}

// ============================================================================
// Clipboard history store
// ============================================================================

/// Clipboard history with capacity limit, search, and pin support.
pub struct ClipboardHistory {
    entries: Vec<ClipEntry>,
    max_entries: usize,
    next_id: u64,
}

impl ClipboardHistory {
    /// Create with default capacity (50).
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            max_entries: 50,
            next_id: 1,
        }
    }

    /// Create with a specific capacity.
    pub fn with_capacity(max: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_entries: max.max(5),
            next_id: 1,
        }
    }

    /// Add a new text entry. Returns the assigned ID.
    pub fn push_text(&mut self, content: &str, timestamp: u64) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let entry = ClipEntry::text(id, content, timestamp);
        self.push_entry(entry);
        id
    }

    /// Add a new image entry.
    pub fn push_image(&mut self, w: u32, h: u32, size: usize, timestamp: u64) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let entry = ClipEntry::image(id, w, h, size, timestamp);
        self.push_entry(entry);
        id
    }

    /// Add a new file paths entry.
    pub fn push_files(&mut self, paths: &[&str], timestamp: u64) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let entry = ClipEntry::files(id, paths, timestamp);
        self.push_entry(entry);
        id
    }

    fn push_entry(&mut self, entry: ClipEntry) {
        self.entries.insert(0, entry); // Most recent first.
        self.evict_if_needed();
    }

    fn evict_if_needed(&mut self) {
        while self.entries.len() > self.max_entries {
            // Find the oldest non-pinned entry to remove.
            if let Some(pos) = self.entries.iter().rposition(|e| !e.pinned) {
                self.entries.remove(pos);
            } else {
                break; // All pinned — can't evict.
            }
        }
    }

    /// Remove an entry by ID.
    pub fn remove(&mut self, id: u64) -> bool {
        if let Some(pos) = self.entries.iter().position(|e| e.id == id) {
            self.entries.remove(pos);
            true
        } else {
            false
        }
    }

    /// Clear all non-pinned entries.
    pub fn clear_unpinned(&mut self) {
        self.entries.retain(|e| e.pinned);
    }

    /// Clear all entries (including pinned).
    pub fn clear_all(&mut self) {
        self.entries.clear();
    }

    /// Toggle pin status for an entry.
    pub fn toggle_pin(&mut self, id: u64) -> bool {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            entry.pinned = !entry.pinned;
            true
        } else {
            false
        }
    }

    /// Get all entries.
    pub fn entries(&self) -> &[ClipEntry] {
        &self.entries
    }

    /// Get entry count.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether history is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get pinned entry count.
    pub fn pinned_count(&self) -> usize {
        self.entries.iter().filter(|e| e.pinned).count()
    }

    /// Search entries by text.
    pub fn search(&self, query: &str) -> Vec<&ClipEntry> {
        if query.is_empty() {
            return self.entries.iter().collect();
        }
        let q = query.to_lowercase();
        self.entries
            .iter()
            .filter(|e| {
                e.preview.to_lowercase().contains(&q)
                    || e.source_app.as_ref().is_some_and(|a| a.to_lowercase().contains(&q))
                    || e.format.label().to_lowercase().contains(&q)
            })
            .collect()
    }

    /// Get entries of a specific format.
    pub fn by_format(&self, format: ClipFormat) -> Vec<&ClipEntry> {
        self.entries.iter().filter(|e| e.format == format).collect()
    }

    /// Get the most recent entry.
    pub fn latest(&self) -> Option<&ClipEntry> {
        self.entries.first()
    }
}

// ============================================================================
// Clipboard viewer widget
// ============================================================================

/// State for the clipboard viewer popup.
pub struct ClipboardViewer {
    /// Whether the viewer popup is open.
    pub is_open: bool,
    /// History store.
    pub history: ClipboardHistory,
    /// Current search query.
    pub search_query: String,
    /// Whether search is focused.
    pub search_focused: bool,
    /// Currently selected entry index.
    pub selected_index: Option<usize>,
    /// Scroll offset (in entries).
    pub scroll_offset: usize,
    /// Width of the popup.
    pub width: f32,
    /// Height of the popup.
    pub height: f32,
    /// Maximum visible entries (depends on height).
    pub max_visible: usize,
    /// Active filter (None = all formats).
    pub format_filter: Option<ClipFormat>,
    /// Current timestamp for age display.
    pub now_timestamp: u64,
}

impl ClipboardViewer {
    /// Create a new clipboard viewer.
    pub fn new() -> Self {
        Self {
            is_open: false,
            history: ClipboardHistory::new(),
            search_query: String::new(),
            search_focused: false,
            selected_index: None,
            scroll_offset: 0,
            width: 360.0,
            height: 500.0,
            max_visible: 8,
            format_filter: None,
            now_timestamp: 0,
        }
    }

    /// Toggle the popup open/closed.
    pub fn toggle(&mut self) {
        self.is_open = !self.is_open;
        if self.is_open {
            self.search_query.clear();
            self.search_focused = false;
            self.selected_index = None;
            self.scroll_offset = 0;
        }
    }

    /// Get the visible entries (filtered and scrolled).
    pub fn visible_entries(&self) -> Vec<&ClipEntry> {
        let filtered: Vec<&ClipEntry> = if !self.search_query.is_empty() {
            self.history.search(&self.search_query)
        } else if let Some(fmt) = self.format_filter {
            self.history.by_format(fmt)
        } else {
            self.history.entries().iter().collect()
        };

        filtered
            .into_iter()
            .skip(self.scroll_offset)
            .take(self.max_visible)
            .collect()
    }

    /// Total filtered entry count (for scrolling).
    pub fn filtered_count(&self) -> usize {
        if !self.search_query.is_empty() {
            self.history.search(&self.search_query).len()
        } else if let Some(fmt) = self.format_filter {
            self.history.by_format(fmt).len()
        } else {
            self.history.len()
        }
    }

    /// Type a character into the search field.
    pub fn type_search_char(&mut self, ch: char) {
        if self.search_focused {
            self.search_query.push(ch);
            self.scroll_offset = 0;
            self.selected_index = None;
        }
    }

    /// Backspace in search field.
    pub fn search_backspace(&mut self) {
        if self.search_focused {
            self.search_query.pop();
            self.scroll_offset = 0;
        }
    }

    /// Move selection up.
    pub fn select_prev(&mut self) {
        let count = self.filtered_count();
        if count == 0 {
            return;
        }
        match self.selected_index {
            Some(0) => {} // Already at top.
            Some(i) => {
                self.selected_index = Some(i - 1);
                if i - 1 < self.scroll_offset {
                    self.scroll_offset = i - 1;
                }
            }
            None => self.selected_index = Some(0),
        }
    }

    /// Move selection down.
    pub fn select_next(&mut self) {
        let count = self.filtered_count();
        if count == 0 {
            return;
        }
        match self.selected_index {
            Some(i) if i + 1 < count => {
                self.selected_index = Some(i + 1);
                if i + 1 >= self.scroll_offset + self.max_visible {
                    self.scroll_offset = (i + 1).saturating_sub(self.max_visible - 1);
                }
            }
            None => self.selected_index = Some(0),
            _ => {}
        }
    }

    /// Render the clipboard viewer popup.
    pub fn render(&self) -> Vec<RenderCommand> {
        if !self.is_open {
            return Vec::new();
        }

        let mut cmds = Vec::with_capacity(100);
        let x = 0.0;
        let y = 0.0;
        let w = self.width;
        let h = self.height;

        // Background.
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: COL_BASE,
            corner_radii: CornerRadii::all(8.0),
        });

        // Border.
        cmds.push(RenderCommand::StrokeRect {
            x,
            y,
            width: w,
            height: h,
            color: COL_SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title bar.
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: y + 10.0,
            text: "Clipboard History".to_string(),
            color: COL_TEXT,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(w - 80.0),
        });

        // Entry count badge.
        let count_text = format!("{}", self.history.len());
        cmds.push(RenderCommand::FillRect {
            x: w - 50.0,
            y: y + 8.0,
            width: 30.0,
            height: 20.0,
            color: COL_SURFACE1,
            corner_radii: CornerRadii::all(10.0),
        });
        cmds.push(RenderCommand::Text {
            x: w - 44.0,
            y: y + 11.0,
            text: count_text,
            color: COL_SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(24.0),
        });

        // Search field.
        let search_y = y + 36.0;
        let search_bg = if self.search_focused { COL_SURFACE1 } else { COL_SURFACE0 };
        cmds.push(RenderCommand::FillRect {
            x: x + 8.0,
            y: search_y,
            width: w - 16.0,
            height: 28.0,
            color: search_bg,
            corner_radii: CornerRadii::all(6.0),
        });
        let search_text = if self.search_query.is_empty() {
            "Search clipboard...".to_string()
        } else {
            self.search_query.clone()
        };
        let search_color = if self.search_query.is_empty() {
            COL_SUBTEXT0
        } else {
            COL_TEXT
        };
        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: search_y + 7.0,
            text: search_text,
            color: search_color,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(w - 40.0),
        });

        // Format filter tabs.
        let filter_y = search_y + 36.0;
        let filters = [
            (None, "All"),
            (Some(ClipFormat::PlainText), "Text"),
            (Some(ClipFormat::Image), "Image"),
            (Some(ClipFormat::FilePaths), "Files"),
        ];
        let mut tab_x = x + 8.0;
        for (fmt, label) in &filters {
            let is_active = self.format_filter == *fmt;
            let tab_w = label.len() as f32 * 7.0 + 16.0;
            let bg = if is_active { COL_BLUE } else { COL_SURFACE0 };
            let fg = if is_active { COL_BASE } else { COL_SUBTEXT0 };
            cmds.push(RenderCommand::FillRect {
                x: tab_x,
                y: filter_y,
                width: tab_w,
                height: 22.0,
                color: bg,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: tab_x + 8.0,
                y: filter_y + 5.0,
                text: label.to_string(),
                color: fg,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(tab_w - 16.0),
            });
            tab_x += tab_w + 4.0;
        }

        // Entry list.
        let list_y = filter_y + 30.0;
        let entry_h = 52.0;
        let visible = self.visible_entries();

        if visible.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + w / 2.0 - 60.0,
                y: list_y + 40.0,
                text: "No clipboard entries".to_string(),
                color: COL_SUBTEXT0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(w - 40.0),
            });
        } else {
            for (i, entry) in visible.iter().enumerate() {
                let ey = list_y + (i as f32 * entry_h);
                let abs_idx = self.scroll_offset + i;
                let is_selected = self.selected_index == Some(abs_idx);

                // Row background.
                if is_selected {
                    cmds.push(RenderCommand::FillRect {
                        x: x + 4.0,
                        y: ey,
                        width: w - 8.0,
                        height: entry_h - 2.0,
                        color: COL_SURFACE1,
                        corner_radii: CornerRadii::all(4.0),
                    });
                }

                // Format badge.
                let badge_color = entry.format.color();
                cmds.push(RenderCommand::FillRect {
                    x: x + 12.0,
                    y: ey + 6.0,
                    width: 20.0,
                    height: 20.0,
                    color: Color::rgba(badge_color.r, badge_color.g, badge_color.b, 60),
                    corner_radii: CornerRadii::all(4.0),
                });
                cmds.push(RenderCommand::Text {
                    x: x + 16.0,
                    y: ey + 9.0,
                    text: entry.format.icon().to_string(),
                    color: badge_color,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(16.0),
                });

                // Preview text.
                let preview_text = entry.preview.lines().next().unwrap_or("").to_string();
                let max_preview_len = 40;
                let display_text = if preview_text.len() > max_preview_len {
                    format!("{}...", &preview_text[..max_preview_len.min(preview_text.len())])
                } else {
                    preview_text
                };
                cmds.push(RenderCommand::Text {
                    x: x + 40.0,
                    y: ey + 6.0,
                    text: display_text,
                    color: COL_TEXT,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(w - 80.0),
                });

                // Meta line (age, size, source).
                let mut meta_parts = Vec::new();
                meta_parts.push(entry.age_display(self.now_timestamp));
                meta_parts.push(entry.size_display());
                if let Some(ref app) = entry.source_app {
                    meta_parts.push(app.clone());
                }
                let meta = meta_parts.join(" · ");
                cmds.push(RenderCommand::Text {
                    x: x + 40.0,
                    y: ey + 22.0,
                    text: meta,
                    color: COL_SUBTEXT0,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Light,
                    max_width: Some(w - 80.0),
                });

                // Pin indicator.
                if entry.pinned {
                    cmds.push(RenderCommand::Text {
                        x: w - 28.0,
                        y: ey + 6.0,
                        text: "P".to_string(),
                        color: COL_YELLOW,
                        font_size: 11.0,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(16.0),
                    });
                }

                // Sensitive indicator.
                if entry.sensitive {
                    cmds.push(RenderCommand::Text {
                        x: w - 28.0,
                        y: ey + 22.0,
                        text: "S".to_string(),
                        color: COL_RED,
                        font_size: 10.0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(16.0),
                    });
                }
            }
        }

        // Bottom bar.
        let bottom_y = h - 30.0;
        cmds.push(RenderCommand::FillRect {
            x,
            y: bottom_y,
            width: w,
            height: 30.0,
            color: COL_MANTLE,
            corner_radii: CornerRadii {
                top_left: 0.0,
                top_right: 0.0,
                bottom_left: 8.0,
                bottom_right: 8.0,
            },
        });

        // "Clear all" text.
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: bottom_y + 8.0,
            text: "Clear All".to_string(),
            color: COL_RED,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(80.0),
        });

        // Pinned count.
        let pinned = self.history.pinned_count();
        if pinned > 0 {
            cmds.push(RenderCommand::Text {
                x: w - 100.0,
                y: bottom_y + 8.0,
                text: format!("{} pinned", pinned),
                color: COL_YELLOW,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(80.0),
            });
        }

        cmds
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- ClipEntry --

    #[test]
    fn test_text_entry() {
        let e = ClipEntry::text(1, "hello world", 1000);
        assert_eq!(e.id, 1);
        assert_eq!(e.format, ClipFormat::PlainText);
        assert_eq!(e.preview, "hello world");
        assert_eq!(e.size_bytes, 11);
    }

    #[test]
    fn test_text_entry_long_preview_truncated() {
        let long = "a".repeat(300);
        let e = ClipEntry::text(1, &long, 1000);
        assert_eq!(e.preview.len(), 200); // 197 + "..."
        assert!(e.preview.ends_with("..."));
    }

    #[test]
    fn test_image_entry() {
        let e = ClipEntry::image(2, 1920, 1080, 8294400, 2000);
        assert_eq!(e.format, ClipFormat::Image);
        assert_eq!(e.preview, "1920x1080 image");
        assert_eq!(e.image_width, Some(1920));
    }

    #[test]
    fn test_files_entry_single() {
        let e = ClipEntry::files(3, &["/home/user/doc.txt"], 3000);
        assert_eq!(e.format, ClipFormat::FilePaths);
        assert_eq!(e.preview, "/home/user/doc.txt");
    }

    #[test]
    fn test_files_entry_multiple() {
        let e = ClipEntry::files(4, &["/a.txt", "/b.txt", "/c.txt"], 4000);
        assert_eq!(e.preview, "3 files");
    }

    #[test]
    fn test_size_display_bytes() {
        let e = ClipEntry::text(1, "hi", 100);
        assert_eq!(e.size_display(), "2 B");
    }

    #[test]
    fn test_size_display_kb() {
        let mut e = ClipEntry::text(1, "hi", 100);
        e.size_bytes = 2048;
        assert_eq!(e.size_display(), "2.0 KB");
    }

    #[test]
    fn test_size_display_mb() {
        let mut e = ClipEntry::text(1, "hi", 100);
        e.size_bytes = 1024 * 1024 * 5;
        assert_eq!(e.size_display(), "5.0 MB");
    }

    #[test]
    fn test_age_display_just_now() {
        let e = ClipEntry::text(1, "hi", 1000);
        assert_eq!(e.age_display(1030), "just now");
    }

    #[test]
    fn test_age_display_minutes() {
        let e = ClipEntry::text(1, "hi", 1000);
        assert_eq!(e.age_display(1180), "3m ago");
    }

    #[test]
    fn test_age_display_hours() {
        let e = ClipEntry::text(1, "hi", 1000);
        assert_eq!(e.age_display(8600), "2h ago");
    }

    #[test]
    fn test_age_display_days() {
        let e = ClipEntry::text(1, "hi", 1000);
        // 260000 - 1000 = 259000s = 2.998 days → "2d ago" (truncated, the
        // conventional way to express elapsed time, e.g. "2 days ago").
        assert_eq!(e.age_display(260000), "2d ago");
        // Bump past the 3-day threshold to verify the "3d ago" path too.
        assert_eq!(e.age_display(1000 + 3 * 86400), "3d ago");
    }

    // -- ClipFormat --

    #[test]
    fn test_format_labels() {
        assert_eq!(ClipFormat::PlainText.label(), "Text");
        assert_eq!(ClipFormat::Image.label(), "Image");
        assert_eq!(ClipFormat::FilePaths.label(), "Files");
    }

    #[test]
    fn test_format_icons() {
        assert_eq!(ClipFormat::PlainText.icon(), "T");
        assert_eq!(ClipFormat::RichText.icon(), "R");
    }

    #[test]
    fn test_format_colors_distinct() {
        let colors = [
            ClipFormat::PlainText.color(),
            ClipFormat::RichText.color(),
            ClipFormat::Image.color(),
            ClipFormat::FilePaths.color(),
        ];
        // All should be different.
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(colors[i], colors[j]);
            }
        }
    }

    // -- ClipboardHistory --

    #[test]
    fn test_history_push_and_len() {
        let mut h = ClipboardHistory::new();
        h.push_text("hello", 100);
        h.push_text("world", 200);
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn test_history_most_recent_first() {
        let mut h = ClipboardHistory::new();
        h.push_text("first", 100);
        h.push_text("second", 200);
        assert_eq!(h.latest().map(|e| e.preview.as_str()), Some("second"));
    }

    #[test]
    fn test_history_capacity_eviction() {
        let mut h = ClipboardHistory::with_capacity(5);
        for i in 0..10 {
            h.push_text(&format!("entry {}", i), i as u64 * 100);
        }
        assert_eq!(h.len(), 5);
    }

    #[test]
    fn test_history_pinned_not_evicted() {
        let mut h = ClipboardHistory::with_capacity(5);
        let id = h.push_text("important", 100);
        h.toggle_pin(id);

        for i in 0..10 {
            h.push_text(&format!("entry {}", i), (i + 2) as u64 * 100);
        }
        // The pinned entry should still be there.
        assert!(h.entries().iter().any(|e| e.id == id && e.pinned));
    }

    #[test]
    fn test_history_remove() {
        let mut h = ClipboardHistory::new();
        let id = h.push_text("to remove", 100);
        assert!(h.remove(id));
        assert!(h.is_empty());
    }

    #[test]
    fn test_history_remove_nonexistent() {
        let mut h = ClipboardHistory::new();
        assert!(!h.remove(999));
    }

    #[test]
    fn test_history_clear_unpinned() {
        let mut h = ClipboardHistory::new();
        let id = h.push_text("pinned", 100);
        h.toggle_pin(id);
        h.push_text("unpinned", 200);
        h.clear_unpinned();
        assert_eq!(h.len(), 1);
        assert!(h.entries()[0].pinned);
    }

    #[test]
    fn test_history_clear_all() {
        let mut h = ClipboardHistory::new();
        let id = h.push_text("pinned", 100);
        h.toggle_pin(id);
        h.push_text("unpinned", 200);
        h.clear_all();
        assert!(h.is_empty());
    }

    #[test]
    fn test_history_search() {
        let mut h = ClipboardHistory::new();
        h.push_text("hello world", 100);
        h.push_text("goodbye world", 200);
        h.push_text("hello there", 300);

        let results = h.search("hello");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_history_search_case_insensitive() {
        let mut h = ClipboardHistory::new();
        h.push_text("Hello World", 100);
        let results = h.search("hello");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_history_search_empty_returns_all() {
        let mut h = ClipboardHistory::new();
        h.push_text("a", 100);
        h.push_text("b", 200);
        assert_eq!(h.search("").len(), 2);
    }

    #[test]
    fn test_history_by_format() {
        let mut h = ClipboardHistory::new();
        h.push_text("text", 100);
        h.push_image(800, 600, 1000, 200);
        h.push_text("more text", 300);

        let texts = h.by_format(ClipFormat::PlainText);
        assert_eq!(texts.len(), 2);
        let images = h.by_format(ClipFormat::Image);
        assert_eq!(images.len(), 1);
    }

    #[test]
    fn test_history_toggle_pin() {
        let mut h = ClipboardHistory::new();
        let id = h.push_text("entry", 100);
        assert!(!h.entries()[0].pinned);
        h.toggle_pin(id);
        assert!(h.entries()[0].pinned);
        h.toggle_pin(id);
        assert!(!h.entries()[0].pinned);
    }

    #[test]
    fn test_history_pinned_count() {
        let mut h = ClipboardHistory::new();
        let id1 = h.push_text("a", 100);
        let id2 = h.push_text("b", 200);
        h.toggle_pin(id1);
        h.toggle_pin(id2);
        assert_eq!(h.pinned_count(), 2);
    }

    // -- ClipboardViewer --

    #[test]
    fn test_viewer_default_closed() {
        let v = ClipboardViewer::new();
        assert!(!v.is_open);
    }

    #[test]
    fn test_viewer_toggle() {
        let mut v = ClipboardViewer::new();
        v.toggle();
        assert!(v.is_open);
        v.toggle();
        assert!(!v.is_open);
    }

    #[test]
    fn test_viewer_render_closed_empty() {
        let v = ClipboardViewer::new();
        assert!(v.render().is_empty());
    }

    #[test]
    fn test_viewer_render_open_not_empty() {
        let mut v = ClipboardViewer::new();
        v.toggle();
        let cmds = v.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_viewer_render_with_entries() {
        let mut v = ClipboardViewer::new();
        v.toggle();
        v.now_timestamp = 5000;
        v.history.push_text("hello", 4000);
        v.history.push_text("world", 4500);
        let cmds = v.render();
        assert!(cmds.len() > 10); // Should have many render commands.
    }

    #[test]
    fn test_viewer_search_input() {
        let mut v = ClipboardViewer::new();
        v.toggle();
        v.search_focused = true;
        v.type_search_char('h');
        v.type_search_char('e');
        v.type_search_char('l');
        assert_eq!(v.search_query, "hel");
    }

    #[test]
    fn test_viewer_search_backspace() {
        let mut v = ClipboardViewer::new();
        v.toggle();
        v.search_focused = true;
        v.type_search_char('a');
        v.type_search_char('b');
        v.search_backspace();
        assert_eq!(v.search_query, "a");
    }

    #[test]
    fn test_viewer_select_navigation() {
        let mut v = ClipboardViewer::new();
        v.toggle();
        v.history.push_text("a", 100);
        v.history.push_text("b", 200);
        v.history.push_text("c", 300);

        v.select_next();
        assert_eq!(v.selected_index, Some(0));
        v.select_next();
        assert_eq!(v.selected_index, Some(1));
        v.select_prev();
        assert_eq!(v.selected_index, Some(0));
    }

    #[test]
    fn test_viewer_select_prev_at_top() {
        let mut v = ClipboardViewer::new();
        v.toggle();
        v.history.push_text("a", 100);
        v.selected_index = Some(0);
        v.select_prev();
        assert_eq!(v.selected_index, Some(0)); // Stays at 0.
    }

    #[test]
    fn test_viewer_visible_entries() {
        let mut v = ClipboardViewer::new();
        for i in 0..20 {
            v.history.push_text(&format!("entry {}", i), i as u64 * 100);
        }
        v.is_open = true;
        let visible = v.visible_entries();
        assert_eq!(visible.len(), v.max_visible);
    }

    #[test]
    fn test_viewer_format_filter() {
        let mut v = ClipboardViewer::new();
        v.history.push_text("text", 100);
        v.history.push_image(800, 600, 1000, 200);
        v.format_filter = Some(ClipFormat::Image);
        v.is_open = true;
        assert_eq!(v.filtered_count(), 1);
    }
}
