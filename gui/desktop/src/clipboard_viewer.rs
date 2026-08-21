//! Clipboard history viewer widget for the desktop shell.
//!
//! Provides a popup panel (activated via Super+V or system tray) showing
//! recent clipboard entries with preview, search, pinning, and format info.
//! Integrates with the gui/clipboard service.

use guitk::color::Color;
use guitk::idseq::IdSeq;
use guitk::listview::ListViewport;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;

/// How many entries the popup shows at its default height.
const DEFAULT_VISIBLE_ENTRIES: usize = 8;

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

/// The ellipsis marking a cut. `guitk::table` keeps its own copy private, so
/// this module needs one.
const ELLIPSIS: &str = "…";

/// How much of a text entry's content the history retains.
///
/// A clipboard entry can be megabytes and the history holds many of them, so an
/// entry keeps a bounded prefix rather than the whole thing. This is a bound on
/// *retention*, not on display: how much fits on screen depends on the panel's
/// width and is decided at draw time.
///
/// It is a **character** count. It used to be a byte slice — `content[..197]`
/// behind an `if content.len() > 200` guard — which aborted the process
/// whenever byte 197 landed inside a character. The guard made that likelier,
/// not less likely: a Japanese clipping reaches 200 bytes at ~67 characters, so
/// the guard fired only for content whose byte 197 was very probably a
/// continuation byte.
const PREVIEW_CHARS: usize = 200;

/// Where an entry row's text starts, measured from the panel's left edge. The
/// format badge occupies everything to the left of it.
const ROW_TEXT_X: f32 = 40.0;

/// Where the pin / sensitive indicators start, measured back from the panel's
/// right edge. Text on a row that has one must stop short of it.
const ROW_INDICATOR_INSET: f32 = 28.0;

/// The row background's inset from the panel edge; text on a row with no
/// indicator still stops here.
const ROW_RIGHT_PAD: f32 = 8.0;

/// Gap kept between a row's text and whatever is drawn to its right.
const ROW_GUTTER: f32 = 6.0;

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
        // `nth` walks at most `PREVIEW_CHARS` characters, so this stays cheap
        // even when `content` is a megabyte of pasted text.
        let preview = if content.chars().nth(PREVIEW_CHARS).is_some() {
            let kept: String = content.chars().take(PREVIEW_CHARS - 1).collect();
            format!("{kept}{ELLIPSIS}")
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
        let preview = match paths {
            [only] => (*only).to_string(),
            _ => format!("{} files", paths.len()),
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
        guitk::bytes::iec(u64::try_from(self.size_bytes).unwrap_or(u64::MAX))
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
    ids: IdSeq,
}

impl ClipboardHistory {
    /// Create with default capacity (50).
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            max_entries: 50,
            ids: IdSeq::new(),
        }
    }

    /// Create with a specific capacity.
    pub fn with_capacity(max: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_entries: max.max(5),
            ids: IdSeq::new(),
        }
    }

    /// Add a new text entry. Returns the assigned ID.
    pub fn push_text(&mut self, content: &str, timestamp: u64) -> u64 {
        let id = self.ids.issue_infallible();
        let entry = ClipEntry::text(id, content, timestamp);
        self.push_entry(entry);
        id
    }

    /// Add a new image entry.
    pub fn push_image(&mut self, w: u32, h: u32, size: usize, timestamp: u64) -> u64 {
        let id = self.ids.issue_infallible();
        let entry = ClipEntry::image(id, w, h, size, timestamp);
        self.push_entry(entry);
        id
    }

    /// Add a new file paths entry.
    pub fn push_files(&mut self, paths: &[&str], timestamp: u64) -> u64 {
        let id = self.ids.issue_infallible();
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
                    || e.source_app
                        .as_ref()
                        .is_some_and(|a| a.to_lowercase().contains(&q))
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
    /// Which entry is picked and which slice of the list is on screen.
    ///
    /// Private, unlike the three public fields it replaced: the selection and
    /// the scroll position are only meaningful together, and every rule tying
    /// them lives in [`ListViewport`].
    list: ListViewport,
    /// Width of the popup.
    pub width: f32,
    /// Height of the popup.
    pub height: f32,
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
            list: ListViewport::new(DEFAULT_VISIBLE_ENTRIES),
            width: 360.0,
            height: 500.0,
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
            self.list.reset();
        }
    }

    /// Which entry is picked, as an index into the filtered list.
    pub const fn selected_index(&self) -> Option<usize> {
        self.list.selected()
    }

    /// The filtered-list index of the first entry drawn.
    pub const fn scroll_offset(&self) -> usize {
        self.list.first_visible()
    }

    /// How many entries fit in the popup at once.
    pub const fn max_visible(&self) -> usize {
        self.list.height()
    }

    /// Change how many entries fit, scrolling if that would hide the
    /// selection.
    pub fn set_max_visible(&mut self, max_visible: usize) {
        let count = self.filtered_count();
        self.list.set_height(max_visible, count);
    }

    /// Pick an entry by index, or clear the selection with `None`.
    pub fn select(&mut self, index: Option<usize>) {
        let count = self.filtered_count();
        self.list.select(index, count);
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

        let window = self.list.visible_range(filtered.len());
        filtered
            .into_iter()
            .skip(window.start)
            .take(window.len())
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
            // The list underneath has been replaced, not edited, so a position
            // into the old one means nothing.
            self.list.reset();
        }
    }

    /// Backspace in search field.
    pub fn search_backspace(&mut self) {
        if self.search_focused {
            self.search_query.pop();
            self.list.reset();
        }
    }

    /// Move selection up.
    pub fn select_prev(&mut self) {
        let count = self.filtered_count();
        self.list.select_prev(count);
    }

    /// Move selection down.
    pub fn select_next(&mut self) {
        let count = self.filtered_count();
        self.list.select_next(count);
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
            overflow: TextOverflow::Ellipsis,
        });

        // Entry count badge.
        let count_text = format!("{}", self.history.len());
        cmds.push(RenderCommand::FillRect {
            x: x + w - 50.0,
            y: y + 8.0,
            width: 30.0,
            height: 20.0,
            color: COL_SURFACE1,
            corner_radii: CornerRadii::all(10.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + w - 44.0,
            y: y + 11.0,
            text: count_text,
            color: COL_SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(24.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Search field.
        let search_y = y + 36.0;
        let search_bg = if self.search_focused {
            COL_SURFACE1
        } else {
            COL_SURFACE0
        };
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
            overflow: TextOverflow::Ellipsis,
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
            let tab_w = text::padded_width(label, 8.0, 11.0, FontWeightHint::Regular);
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
                overflow: TextOverflow::Ellipsis,
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
                overflow: TextOverflow::Ellipsis,
            });
        } else {
            let window = self.list.visible_range(self.filtered_count());
            for ((i, entry), abs_idx) in visible.iter().enumerate().zip(window) {
                let ey = list_y + (i as f32 * entry_h);
                let is_selected = self.list.selected() == Some(abs_idx);

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
                    overflow: TextOverflow::Ellipsis,
                });

                // Preview text.
                //
                // This used to be cut at 40 *bytes* — which aborted on any
                // non-Latin clipping, since the `len() > 40` guard fires only
                // for strings long enough in bytes and so selected for the ones
                // whose byte 40 is a continuation byte. A byte count was not a
                // bound on the row either: the row's width depends on the
                // panel's, and the pin indicator sits at a fixed inset from its
                // right edge. Eliding to the room that is actually there makes
                // the cut and the space agree at every width.
                let text_x = x + ROW_TEXT_X;
                let preview_stop = if entry.pinned {
                    x + w - ROW_INDICATOR_INSET
                } else {
                    x + w - ROW_RIGHT_PAD
                };
                let preview_room = (preview_stop - ROW_GUTTER - text_x).max(0.0);
                let preview_text = entry.preview.lines().next().unwrap_or("");
                cmds.push(RenderCommand::Text {
                    x: text_x,
                    y: ey + 6.0,
                    text: text::elide(
                        preview_text,
                        preview_room,
                        ELLIPSIS,
                        12.0,
                        FontWeightHint::Regular,
                    ),
                    color: COL_TEXT,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(preview_room),
                    overflow: TextOverflow::Ellipsis,
                });

                // Meta line (age, size, source).
                let mut meta_parts = Vec::new();
                meta_parts.push(entry.age_display(self.now_timestamp));
                meta_parts.push(entry.size_display());
                if let Some(ref app) = entry.source_app {
                    meta_parts.push(app.clone());
                }
                let meta = meta_parts.join(" · ");
                // `source_app` is an application-supplied name, so the meta
                // line is variable-length too, and its own neighbour is the
                // sensitive indicator rather than the pin.
                let meta_stop = if entry.sensitive {
                    x + w - ROW_INDICATOR_INSET
                } else {
                    x + w - ROW_RIGHT_PAD
                };
                let meta_room = (meta_stop - ROW_GUTTER - text_x).max(0.0);
                cmds.push(RenderCommand::Text {
                    x: text_x,
                    y: ey + 22.0,
                    text: text::elide(&meta, meta_room, ELLIPSIS, 10.0, FontWeightHint::Light),
                    color: COL_SUBTEXT0,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Light,
                    max_width: Some(meta_room),
                    overflow: TextOverflow::Ellipsis,
                });

                // Pin indicator.
                if entry.pinned {
                    cmds.push(RenderCommand::Text {
                        x: x + w - ROW_INDICATOR_INSET,
                        y: ey + 6.0,
                        text: "P".to_string(),
                        color: COL_YELLOW,
                        font_size: 11.0,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(16.0),
                        overflow: TextOverflow::Ellipsis,
                    });
                }

                // Sensitive indicator.
                if entry.sensitive {
                    cmds.push(RenderCommand::Text {
                        x: x + w - ROW_INDICATOR_INSET,
                        y: ey + 22.0,
                        text: "S".to_string(),
                        color: COL_RED,
                        font_size: 10.0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(16.0),
                        overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
        });

        // Pinned count.
        let pinned = self.history.pinned_count();
        if pinned > 0 {
            cmds.push(RenderCommand::Text {
                x: x + w - 100.0,
                y: bottom_y + 8.0,
                text: format!("{} pinned", pinned),
                color: COL_YELLOW,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(80.0),
                overflow: TextOverflow::Ellipsis,
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
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

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
        // A *character* count: the old assertion was on `.len()`, a byte count,
        // which is exactly the confusion that made this abort on non-ASCII.
        assert_eq!(e.preview.chars().count(), PREVIEW_CHARS);
        assert!(e.preview.ends_with(ELLIPSIS));
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
        assert_eq!(e.size_display(), "2.0 KiB");
    }

    #[test]
    fn test_size_display_mb() {
        let mut e = ClipEntry::text(1, "hi", 100);
        e.size_bytes = 1024 * 1024 * 5;
        assert_eq!(e.size_display(), "5.0 MiB");
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
        assert_eq!(v.selected_index(), Some(0));
        v.select_next();
        assert_eq!(v.selected_index(), Some(1));
        v.select_prev();
        assert_eq!(v.selected_index(), Some(0));
    }

    #[test]
    fn test_viewer_select_prev_at_top() {
        let mut v = ClipboardViewer::new();
        v.toggle();
        v.history.push_text("a", 100);
        v.select(Some(0));
        v.select_prev();
        assert_eq!(v.selected_index(), Some(0)); // Stays at 0.
    }

    #[test]
    fn test_viewer_visible_entries() {
        let mut v = ClipboardViewer::new();
        for i in 0..20 {
            v.history.push_text(&format!("entry {}", i), i as u64 * 100);
        }
        v.is_open = true;
        let visible = v.visible_entries();
        assert_eq!(visible.len(), v.max_visible());
    }

    #[test]
    fn the_selected_entry_is_always_one_of_the_drawn_ones() {
        // The old navigation scrolled up to reach a selection above the window
        // but never down to reach one below it, and computed the downward
        // scroll from `max_visible - 1`, which panicked at a height of zero.
        let mut v = ClipboardViewer::new();
        for i in 0..30u64 {
            v.history.push_text(&format!("entry {i}"), i * 100);
        }
        v.is_open = true;

        for height in [0usize, 1, 3, 8, 40] {
            v.set_max_visible(height);
            v.select(None);
            for _ in 0..40 {
                v.select_next();
                assert_selection_visible(&v);
            }
            for _ in 0..40 {
                v.select_prev();
                assert_selection_visible(&v);
            }
            // Jumping straight to an out-of-range row clamps rather than
            // leaving the list pointing past its own end.
            v.select(Some(1000));
            assert_eq!(v.selected_index(), Some(29));
            assert_selection_visible(&v);
        }
    }

    /// The invariant the viewer exists to keep: what is picked is on screen,
    /// and the window never runs off the end of the list.
    fn assert_selection_visible(v: &ClipboardViewer) {
        let count = v.filtered_count();
        let start = v.scroll_offset();
        let shown = v.visible_entries().len();
        assert!(
            start + shown <= count,
            "window {start}..{} runs past the {count} entries",
            start + shown
        );
        assert_eq!(
            shown,
            v.max_visible().min(count.saturating_sub(start)),
            "the window left blank rows it could have filled"
        );
        if let Some(selected) = v.selected_index() {
            assert!(selected < count, "selection {selected} is past the end");
            if v.max_visible() > 0 {
                assert!(
                    selected >= start && selected < start + shown,
                    "selection {selected} is outside the drawn {start}..{}",
                    start + shown
                );
            }
        }
    }

    #[test]
    fn narrowing_the_search_does_not_leave_a_stale_selection() {
        let mut v = ClipboardViewer::new();
        for i in 0..20u64 {
            v.history.push_text(&format!("entry {i}"), i * 100);
        }
        v.is_open = true;
        for _ in 0..15 {
            v.select_next();
        }
        assert_eq!(v.selected_index(), Some(14));

        // Typing narrows the list to one match; the old code left the
        // selection and the scroll offset pointing into the wider list.
        v.search_focused = true;
        for ch in "entry 7".chars() {
            v.type_search_char(ch);
        }
        assert_eq!(v.filtered_count(), 1);
        assert_eq!(v.selected_index(), None);
        assert_eq!(v.scroll_offset(), 0);
        v.select_next();
        assert_eq!(v.selected_index(), Some(0));
        assert_selection_visible(&v);
    }

    #[test]
    fn a_single_file_entry_previews_its_path() {
        let one = ClipEntry::files(1, &["/home/a/notes.txt"], 100);
        assert_eq!(one.preview, "/home/a/notes.txt");
        let many = ClipEntry::files(2, &["/a", "/b"], 100);
        assert_eq!(many.preview, "2 files");
        // An empty list is not a path, and must not be read as one.
        let none = ClipEntry::files(3, &[], 100);
        assert_eq!(none.preview, "0 files");
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

    // -- Previews are bounded in characters and elided to the row -------------

    /// Content whose byte length picks a cut its character length cannot take.
    ///
    /// Both byte budgets this replaced were anti-protective: `content[..197]`
    /// behind `len() > 200`, and `&preview_text[..40]` behind `len() > 40`.
    /// Each fired *only* for strings long enough in bytes, and Japanese reaches
    /// 200 bytes at ~67 characters and 40 bytes at ~13 — so each guard selected
    /// for exactly the content whose cut index is a continuation byte.
    fn adversarial_clips() -> Vec<String> {
        vec![
            "\u{3053}\u{308c}\u{306f}\u{30af}\u{30ea}\u{30c3}\u{30d7}\u{30dc}\u{30fc}\u{30c9}\u{306e}\u{5185}\u{5bb9}\u{3067}\u{3059}".repeat(20),
            "\u{42d}\u{442}\u{43e} \u{441}\u{43e}\u{434}\u{435}\u{440}\u{436}\u{438}\u{43c}\u{43e}\u{435} \u{431}\u{443}\u{444}\u{435}\u{440}\u{430} \u{43e}\u{431}\u{43c}\u{435}\u{43d}\u{430}".repeat(12),
            "\u{1f4cc}\u{1f4dd}\u{1f5d2}\u{fe0f}\u{1f4a1}\u{1f9e0}\u{1f4da}".repeat(30),
            // Byte 197 and byte 40 both land inside one of these U+00E9s.
            format!("{}\u{e9}{}", "x".repeat(39), "y".repeat(400)),
            format!("{}\u{e9}{}", "x".repeat(196), "y".repeat(400)),
            "brief".to_string(),
        ]
    }

    fn viewer_with_adversarial_clips(width: f32) -> ClipboardViewer {
        let mut v = ClipboardViewer::new();
        v.width = width;
        v.is_open = true;
        for (i, clip) in adversarial_clips().iter().enumerate() {
            let id = v.history.push_text(clip, 1000 + i as u64);
            // Exercise both indicator layouts: a pinned row loses width on its
            // preview line, a sensitive one on its meta line.
            if let Some(e) = v.history.entries.iter_mut().find(|e| e.id == id) {
                e.pinned = i % 2 == 0;
                e.sensitive = i % 3 == 0;
                e.source_app = Some(
                    "\u{3068}\u{3066}\u{3082}\u{9577}\u{3044}\u{30a2}\u{30d7}\u{30ea}\u{540d}"
                        .repeat(3),
                );
            }
        }
        v
    }

    #[test]
    fn a_non_ascii_clip_does_not_abort_the_viewer() {
        for width in [240.0_f32, 360.0, 520.0] {
            let v = viewer_with_adversarial_clips(width);
            assert!(!v.render().is_empty(), "no commands at width {width}");
        }
    }

    #[test]
    fn a_non_ascii_clip_is_retained_by_characters_not_bytes() {
        for clip in adversarial_clips() {
            let e = ClipEntry::text(1, &clip, 1000);
            assert!(
                e.preview.chars().count() <= PREVIEW_CHARS,
                "retained {} chars of {:?}",
                e.preview.chars().count(),
                clip
            );
            // A prefix of the original, so search over the preview still means
            // something.
            let body = e.preview.strip_suffix(ELLIPSIS).unwrap_or(&e.preview);
            assert!(
                clip.starts_with(body),
                "preview is not a prefix of the clip"
            );
        }
    }

    /// No row text may run into whatever is drawn beside it, at any width the
    /// popup can be given.
    ///
    /// The bound is read off the *drawn* commands rather than recomputed from
    /// the constants: each line is paired with the indicator sharing its `y`,
    /// if there is one, and otherwise stops at the row background's edge. A
    /// line on an unpinned row genuinely has more room than one on a pinned
    /// row, and a test that applied the tighter bound to both would be
    /// asserting a layout the code is right not to use.
    #[test]
    fn no_entry_row_text_reaches_its_indicator() {
        let mut checked = 0usize;
        for width in [240.0_f32, 360.0, 520.0] {
            let v = viewer_with_adversarial_clips(width);
            let cmds = v.render();

            // Indicators are the only text drawn at the indicator inset.
            let indicator_x = width - ROW_INDICATOR_INSET;
            let indicators: Vec<f32> = cmds
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Text { x, y, .. }
                        if (*x - indicator_x).abs() <= f32::EPSILON =>
                    {
                        Some(*y)
                    }
                    _ => None,
                })
                .collect();

            for cmd in &cmds {
                let RenderCommand::Text {
                    x,
                    y,
                    text,
                    font_size,
                    font_weight,
                    ..
                } = cmd
                else {
                    continue;
                };
                // Only the entry rows' own text starts at ROW_TEXT_X.
                if (*x - ROW_TEXT_X).abs() > f32::EPSILON {
                    continue;
                }
                let has_indicator = indicators.iter().any(|iy| (iy - y).abs() <= f32::EPSILON);
                let stop = if has_indicator {
                    indicator_x
                } else {
                    width - ROW_RIGHT_PAD
                };
                let right = x + text::measure(text, *font_size, *font_weight);
                assert!(
                    right <= stop - ROW_GUTTER + 0.5,
                    "at width {width} the row text {text:?} runs to {right}, past \
                     the {} at {stop}",
                    if has_indicator {
                        "indicator"
                    } else {
                        "row edge"
                    }
                );
                checked += 1;
            }
        }
        assert!(
            checked >= adversarial_clips().len() * 2,
            "expected a preview and a meta line per row, checked {checked}"
        );
    }

    /// The pairing the test above relies on has to actually happen: if no row
    /// ever drew an indicator, that test would be checking only the loose
    /// bound and would pass with the indicator-aware branch deleted.
    #[test]
    fn the_adversarial_rows_cover_both_indicator_layouts() {
        let v = viewer_with_adversarial_clips(360.0);
        let pinned = v.history.entries.iter().filter(|e| e.pinned).count();
        let sensitive = v.history.entries.iter().filter(|e| e.sensitive).count();
        assert!(
            pinned > 0 && pinned < v.history.entries.len(),
            "pinned {pinned}"
        );
        assert!(
            sensitive > 0 && sensitive < v.history.entries.len(),
            "sensitive {sensitive}"
        );
    }

    #[test]
    fn a_short_clip_is_drawn_verbatim() {
        let v = viewer_with_adversarial_clips(520.0);
        let cmds = v.render();
        let drawn: Vec<&str> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            drawn.contains(&"brief"),
            "a short preview was altered: {drawn:?}"
        );
    }
}
