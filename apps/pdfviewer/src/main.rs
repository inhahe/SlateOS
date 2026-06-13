//! Slate OS PDF Viewer
//!
//! Graphical PDF document viewer with:
//! - PDF document model (pages, metadata, bookmarks/outline)
//! - Page rendering (placeholder — renders page boxes with text content)
//! - Zoom controls (fit width, fit page, 25%-400%, zoom in/out)
//! - Page navigation (next/prev, go to page, first/last)
//! - Continuous scroll and single-page view modes
//! - Sidebar with thumbnail strip and bookmarks/outline tree
//! - Text search across pages with highlighting
//! - Page rotation (0/90/180/270)
//! - Toolbar with common actions
//! - Recent files list
//! - Print integration (page range selection)
//! - Annotation support model (highlights, notes, freehand)
//! - Multi-tab document viewing
//! - Dark mode rendering
//!
//! Uses the guitk library for UI rendering.

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

use std::path::{Path, PathBuf};

// ============================================================================
// Catppuccin Mocha theme colors
// ============================================================================

// Full palette is defined for use across the application. Not all colors are
// referenced yet but they are part of the theme and available for future use.
#[allow(dead_code)] const BASE: Color = Color::rgb(30, 30, 46);
#[allow(dead_code)] const MANTLE: Color = Color::rgb(24, 24, 37);
#[allow(dead_code)] const CRUST: Color = Color::rgb(17, 17, 27);
#[allow(dead_code)] const SURFACE0: Color = Color::rgb(49, 50, 68);
#[allow(dead_code)] const SURFACE1: Color = Color::rgb(69, 71, 90);
#[allow(dead_code)] const SURFACE2: Color = Color::rgb(88, 91, 112);
#[allow(dead_code)] const OVERLAY0: Color = Color::rgb(108, 112, 134);
#[allow(dead_code)] const TEXT_COLOR: Color = Color::rgb(205, 214, 244);
#[allow(dead_code)] const SUBTEXT1: Color = Color::rgb(186, 194, 222);
#[allow(dead_code)] const SUBTEXT0: Color = Color::rgb(166, 173, 200);
#[allow(dead_code)] const BLUE: Color = Color::rgb(137, 180, 250);
#[allow(dead_code)] const LAVENDER: Color = Color::rgb(180, 190, 254);
#[allow(dead_code)] const SAPPHIRE: Color = Color::rgb(116, 199, 236);
#[allow(dead_code)] const GREEN: Color = Color::rgb(166, 227, 161);
#[allow(dead_code)] const YELLOW: Color = Color::rgb(249, 226, 175);
#[allow(dead_code)] const PEACH: Color = Color::rgb(250, 179, 135);
#[allow(dead_code)] const RED: Color = Color::rgb(243, 139, 168);
#[allow(dead_code)] const MAUVE: Color = Color::rgb(203, 166, 247);
#[allow(dead_code)] const ROSEWATER: Color = Color::rgb(245, 224, 220);
#[allow(dead_code)] const FLAMINGO: Color = Color::rgb(242, 205, 205);
#[allow(dead_code)] const TEAL: Color = Color::rgb(148, 226, 213);

// ============================================================================
// Layout constants
// ============================================================================

const TOOLBAR_HEIGHT: f32 = 44.0;
const TAB_BAR_HEIGHT: f32 = 36.0;
const STATUS_BAR_HEIGHT: f32 = 28.0;
const SIDEBAR_WIDTH: f32 = 240.0;
const THUMBNAIL_HEIGHT: f32 = 120.0;
const PAGE_GAP: f32 = 12.0;
const PAGE_SHADOW_BLUR: f32 = 8.0;
const PAGE_MARGIN: f32 = 24.0;

const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 4.0;
const ZOOM_STEP: f32 = 0.25;
const DEFAULT_ZOOM: f32 = 1.0;

/// Standard US Letter page dimensions in points (at 72 DPI).
const DEFAULT_PAGE_WIDTH: f32 = 612.0;
const DEFAULT_PAGE_HEIGHT: f32 = 792.0;

// ============================================================================
// Document model
// ============================================================================

/// PDF page rotation in degrees.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[derive(Default)]
pub enum Rotation {
    #[default]
    Deg0,
    Deg90,
    Deg180,
    Deg270,
}

impl Rotation {
    /// Rotate clockwise by 90 degrees.
    pub fn rotate_cw(self) -> Self {
        match self {
            Self::Deg0 => Self::Deg90,
            Self::Deg90 => Self::Deg180,
            Self::Deg180 => Self::Deg270,
            Self::Deg270 => Self::Deg0,
        }
    }

    /// Rotate counter-clockwise by 90 degrees.
    pub fn rotate_ccw(self) -> Self {
        match self {
            Self::Deg0 => Self::Deg270,
            Self::Deg90 => Self::Deg0,
            Self::Deg180 => Self::Deg90,
            Self::Deg270 => Self::Deg180,
        }
    }

    /// Angle in degrees.
    pub fn degrees(self) -> u16 {
        match self {
            Self::Deg0 => 0,
            Self::Deg90 => 90,
            Self::Deg180 => 180,
            Self::Deg270 => 270,
        }
    }

    /// Whether width and height are swapped under this rotation.
    pub fn swaps_dimensions(self) -> bool {
        matches!(self, Self::Deg90 | Self::Deg270)
    }
}


/// A rectangular region on a page (in page-coordinate points).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl PageRect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { x, y, width, height }
    }

    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + self.height
    }
}

/// Text content on a page (for search and rendering placeholder).
#[derive(Clone, Debug)]
pub struct TextSpan {
    pub text: String,
    pub rect: PageRect,
    pub font_size: f32,
}

/// Annotation types.
#[derive(Clone, Debug, PartialEq)]
pub enum AnnotationType {
    /// Text highlight with color.
    Highlight { color: Color },
    /// Sticky note.
    Note { content: String },
    /// Freehand drawing path.
    Freehand { points: Vec<(f32, f32)>, color: Color, width: f32 },
    /// Underline.
    Underline { color: Color },
    /// Strikethrough.
    Strikethrough { color: Color },
}

/// An annotation on a page.
#[derive(Clone, Debug)]
pub struct Annotation {
    pub id: u64,
    pub page_index: usize,
    pub rect: PageRect,
    pub annotation_type: AnnotationType,
    pub author: String,
    pub created_timestamp: u64,
}

/// A single page in a PDF document.
#[derive(Clone, Debug)]
pub struct PdfPage {
    /// Page width in points.
    pub width: f32,
    /// Page height in points.
    pub height: f32,
    /// Text content blocks on this page.
    pub text_spans: Vec<TextSpan>,
    /// Applied rotation for this page.
    pub rotation: Rotation,
    /// Annotations on this page.
    pub annotations: Vec<Annotation>,
    /// Page label (may differ from page number, e.g., roman numerals).
    pub label: Option<String>,
}

impl PdfPage {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            text_spans: Vec::new(),
            rotation: Rotation::Deg0,
            annotations: Vec::new(),
            label: None,
        }
    }

    /// Effective display width after rotation.
    pub fn display_width(&self) -> f32 {
        if self.rotation.swaps_dimensions() {
            self.height
        } else {
            self.width
        }
    }

    /// Effective display height after rotation.
    pub fn display_height(&self) -> f32 {
        if self.rotation.swaps_dimensions() {
            self.width
        } else {
            self.height
        }
    }
}

/// A bookmark (outline entry) in the PDF.
#[derive(Clone, Debug)]
pub struct Bookmark {
    pub title: String,
    pub page_index: usize,
    pub children: Vec<Bookmark>,
    pub expanded: bool,
}

impl Bookmark {
    pub fn new(title: &str, page_index: usize) -> Self {
        Self {
            title: title.to_string(),
            page_index,
            children: Vec::new(),
            expanded: false,
        }
    }

    /// Count total entries including children recursively.
    pub fn total_count(&self) -> usize {
        1 + self.children.iter().map(|c| c.total_count()).sum::<usize>()
    }

    /// Flatten the bookmark tree into a list of (depth, bookmark_ref).
    pub fn flatten(&self, depth: usize) -> Vec<(usize, &Bookmark)> {
        let mut result = vec![(depth, self)];
        if self.expanded {
            for child in &self.children {
                result.extend(child.flatten(depth + 1));
            }
        }
        result
    }
}

/// PDF document metadata.
#[derive(Clone, Debug, Default)]
pub struct PdfMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub creator: Option<String>,
    pub producer: Option<String>,
    pub creation_date: Option<String>,
    pub modification_date: Option<String>,
    pub keywords: Vec<String>,
    pub page_count: usize,
    pub pdf_version: Option<String>,
    pub file_size_bytes: u64,
    pub encrypted: bool,
}

/// Complete PDF document model.
#[derive(Clone, Debug)]
pub struct PdfDocument {
    pub path: PathBuf,
    pub pages: Vec<PdfPage>,
    pub metadata: PdfMetadata,
    pub bookmarks: Vec<Bookmark>,
}

impl PdfDocument {
    /// Create a new empty document.
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            pages: Vec::new(),
            metadata: PdfMetadata::default(),
            bookmarks: Vec::new(),
        }
    }

    /// Total number of pages.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Create a sample document for testing/demonstration.
    pub fn create_sample(path: PathBuf, page_count: usize) -> Self {
        let mut doc = Self::new(path);
        doc.metadata.title = Some("Sample Document".to_string());
        doc.metadata.author = Some("Slate OS PDF Viewer".to_string());
        doc.metadata.pdf_version = Some("1.7".to_string());
        doc.metadata.page_count = page_count;
        doc.metadata.file_size_bytes = (page_count as u64) * 4096;

        for i in 0..page_count {
            let mut page = PdfPage::new(DEFAULT_PAGE_WIDTH, DEFAULT_PAGE_HEIGHT);
            page.text_spans.push(TextSpan {
                text: format!("Page {}", i + 1),
                rect: PageRect::new(72.0, 72.0, 200.0, 24.0),
                font_size: 18.0,
            });
            page.text_spans.push(TextSpan {
                text: "Lorem ipsum dolor sit amet, consectetur adipiscing elit.".to_string(),
                rect: PageRect::new(72.0, 110.0, 468.0, 14.0),
                font_size: 12.0,
            });
            page.text_spans.push(TextSpan {
                text: "Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua."
                    .to_string(),
                rect: PageRect::new(72.0, 130.0, 468.0, 14.0),
                font_size: 12.0,
            });
            doc.pages.push(page);
        }

        // Add sample bookmarks
        let mut ch1 = Bookmark::new("Chapter 1: Introduction", 0);
        ch1.children.push(Bookmark::new("1.1 Background", 0));
        if page_count > 1 {
            ch1.children.push(Bookmark::new("1.2 Scope", 1));
        }
        doc.bookmarks.push(ch1);

        if page_count > 2 {
            let mut ch2 = Bookmark::new("Chapter 2: Methods", 2);
            if page_count > 3 {
                ch2.children.push(Bookmark::new("2.1 Approach", 3));
            }
            doc.bookmarks.push(ch2);
        }

        doc
    }

    /// Get the page label for display (uses custom label or page number).
    pub fn page_label(&self, index: usize) -> String {
        self.pages
            .get(index)
            .and_then(|p| p.label.clone())
            .unwrap_or_else(|| format!("{}", index + 1))
    }

    /// Search for text across all pages. Returns (page_index, rect) for each match.
    pub fn search(&self, query: &str) -> Vec<SearchResult> {
        if query.is_empty() {
            return Vec::new();
        }
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();
        for (page_idx, page) in self.pages.iter().enumerate() {
            for span in &page.text_spans {
                let text_lower = span.text.to_lowercase();
                let mut start = 0;
                while let Some(pos) = text_lower[start..].find(&query_lower) {
                    let abs_pos = start + pos;
                    // Estimate highlight rect based on character position
                    let char_width = if span.rect.width > 0.0 && !span.text.is_empty() {
                        span.rect.width / span.text.len() as f32
                    } else {
                        8.0
                    };
                    let highlight_rect = PageRect::new(
                        span.rect.x + abs_pos as f32 * char_width,
                        span.rect.y,
                        query.len() as f32 * char_width,
                        span.rect.height.max(span.font_size),
                    );
                    results.push(SearchResult {
                        page_index: page_idx,
                        rect: highlight_rect,
                        context: span.text.clone(),
                    });
                    start = abs_pos + query.len();
                }
            }
        }
        results
    }

    /// Flatten all bookmarks into a list of (depth, bookmark_ref).
    pub fn flatten_bookmarks(&self) -> Vec<(usize, &Bookmark)> {
        let mut result = Vec::new();
        for bm in &self.bookmarks {
            result.extend(bm.flatten(0));
        }
        result
    }

    /// Count total bookmark entries (including nested).
    pub fn total_bookmark_count(&self) -> usize {
        self.bookmarks.iter().map(|b| b.total_count()).sum()
    }
}

/// A search hit within the document.
#[derive(Clone, Debug)]
pub struct SearchResult {
    pub page_index: usize,
    pub rect: PageRect,
    pub context: String,
}

// ============================================================================
// View modes
// ============================================================================

/// How pages are displayed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[derive(Default)]
pub enum ViewMode {
    /// Show one page at a time.
    #[default]
    SinglePage,
    /// Continuous vertical scroll through all pages.
    ContinuousScroll,
}


/// Zoom mode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ZoomMode {
    /// Specific zoom percentage.
    Fixed(f32),
    /// Fit page width to viewport.
    FitWidth,
    /// Fit entire page in viewport.
    FitPage,
}

impl ZoomMode {
    /// Compute the effective zoom factor given viewport and page dimensions.
    pub fn effective_zoom(
        &self,
        viewport_width: f32,
        viewport_height: f32,
        page_width: f32,
        page_height: f32,
    ) -> f32 {
        match self {
            Self::Fixed(z) => *z,
            Self::FitWidth => {
                let available = viewport_width - PAGE_MARGIN * 2.0;
                if page_width > 0.0 {
                    (available / page_width).clamp(MIN_ZOOM, MAX_ZOOM)
                } else {
                    DEFAULT_ZOOM
                }
            }
            Self::FitPage => {
                let avail_w = viewport_width - PAGE_MARGIN * 2.0;
                let avail_h = viewport_height - PAGE_MARGIN * 2.0;
                if page_width > 0.0 && page_height > 0.0 {
                    let zoom_w = avail_w / page_width;
                    let zoom_h = avail_h / page_height;
                    zoom_w.min(zoom_h).clamp(MIN_ZOOM, MAX_ZOOM)
                } else {
                    DEFAULT_ZOOM
                }
            }
        }
    }

    /// Zoom level as a percentage string.
    pub fn label(&self, viewport_w: f32, viewport_h: f32, page_w: f32, page_h: f32) -> String {
        match self {
            Self::Fixed(z) => format!("{}%", (*z * 100.0) as u32),
            Self::FitWidth => {
                let z = self.effective_zoom(viewport_w, viewport_h, page_w, page_h);
                format!("Fit Width ({}%)", (z * 100.0) as u32)
            }
            Self::FitPage => {
                let z = self.effective_zoom(viewport_w, viewport_h, page_w, page_h);
                format!("Fit Page ({}%)", (z * 100.0) as u32)
            }
        }
    }
}

impl Default for ZoomMode {
    fn default() -> Self {
        Self::Fixed(DEFAULT_ZOOM)
    }
}

// ============================================================================
// Sidebar mode
// ============================================================================

/// Which sidebar panel is active.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[derive(Default)]
pub enum SidebarPanel {
    #[default]
    Thumbnails,
    Bookmarks,
    Annotations,
}


// ============================================================================
// Print settings
// ============================================================================

/// Page range for printing.
#[derive(Clone, Debug, PartialEq)]
#[derive(Default)]
pub enum PrintPageRange {
    #[default]
    All,
    CurrentPage,
    /// Specific ranges, e.g., [(0,2), (4,4)] for pages 1-3 and 5.
    Custom(Vec<(usize, usize)>),
}


/// Print settings.
#[derive(Clone, Debug)]
pub struct PrintSettings {
    pub page_range: PrintPageRange,
    pub copies: u32,
    pub duplex: bool,
    pub color: bool,
    pub scale_to_fit: bool,
}

impl Default for PrintSettings {
    fn default() -> Self {
        Self {
            page_range: PrintPageRange::All,
            copies: 1,
            duplex: false,
            color: true,
            scale_to_fit: true,
        }
    }
}

impl PrintSettings {
    /// Resolve the page indices to print from the given page count.
    pub fn resolve_pages(&self, page_count: usize, current_page: usize) -> Vec<usize> {
        match &self.page_range {
            PrintPageRange::All => (0..page_count).collect(),
            PrintPageRange::CurrentPage => {
                if current_page < page_count {
                    vec![current_page]
                } else {
                    Vec::new()
                }
            }
            PrintPageRange::Custom(ranges) => {
                let mut pages = Vec::new();
                for &(start, end) in ranges {
                    let clamped_end = end.min(page_count.saturating_sub(1));
                    for p in start..=clamped_end {
                        if !pages.contains(&p) {
                            pages.push(p);
                        }
                    }
                }
                pages.sort();
                pages
            }
        }
    }

    /// Parse a user-entered page range string (e.g. "1-3, 5, 7-9").
    pub fn parse_page_range(input: &str, page_count: usize) -> PrintPageRange {
        if input.trim().is_empty() || input.trim().eq_ignore_ascii_case("all") {
            return PrintPageRange::All;
        }
        let mut ranges = Vec::new();
        for part in input.split(',') {
            let part = part.trim();
            if part.contains('-') {
                let mut parts = part.splitn(2, '-');
                let start = parts
                    .next()
                    .and_then(|s| s.trim().parse::<usize>().ok())
                    .unwrap_or(1);
                let end = parts
                    .next()
                    .and_then(|s| s.trim().parse::<usize>().ok())
                    .unwrap_or(start);
                // Convert from 1-based to 0-based
                let s = start.saturating_sub(1).min(page_count.saturating_sub(1));
                let e = end.saturating_sub(1).min(page_count.saturating_sub(1));
                if s <= e {
                    ranges.push((s, e));
                }
            } else if let Ok(n) = part.parse::<usize>()
                && n >= 1 {
                    let idx = (n - 1).min(page_count.saturating_sub(1));
                    ranges.push((idx, idx));
                }
        }
        if ranges.is_empty() {
            PrintPageRange::All
        } else {
            PrintPageRange::Custom(ranges)
        }
    }
}

// ============================================================================
// Recent files
// ============================================================================

/// A recently opened file entry.
#[derive(Clone, Debug)]
pub struct RecentFile {
    pub path: PathBuf,
    pub title: Option<String>,
    pub last_opened_timestamp: u64,
    pub last_page: usize,
}

/// Recent files list with maximum capacity.
#[derive(Clone, Debug)]
pub struct RecentFilesList {
    pub entries: Vec<RecentFile>,
    pub max_entries: usize,
}

impl RecentFilesList {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_entries,
        }
    }

    /// Add or update a file in the recent list.
    pub fn add(&mut self, path: PathBuf, title: Option<String>, timestamp: u64, page: usize) {
        // Remove existing entry for this path
        self.entries.retain(|e| e.path != path);
        // Insert at front
        self.entries.insert(
            0,
            RecentFile {
                path,
                title,
                last_opened_timestamp: timestamp,
                last_page: page,
            },
        );
        // Trim to capacity
        self.entries.truncate(self.max_entries);
    }

    /// Remove a file from the recent list by path.
    pub fn remove(&mut self, path: &Path) {
        self.entries.retain(|e| e.path != path);
    }

    /// Clear all recent files.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Get the most recent entry for a path.
    pub fn find(&self, path: &Path) -> Option<&RecentFile> {
        self.entries.iter().find(|e| e.path == path)
    }
}

impl Default for RecentFilesList {
    fn default() -> Self {
        Self::new(20)
    }
}

// ============================================================================
// Tab model
// ============================================================================

/// A tab representing an open document.
#[derive(Clone, Debug)]
pub struct DocumentTab {
    pub id: u64,
    pub document: Option<PdfDocument>,
    pub current_page: usize,
    pub zoom: ZoomMode,
    pub view_mode: ViewMode,
    pub scroll_offset_y: f32,
    pub rotation: Rotation,
    pub sidebar_visible: bool,
    pub sidebar_panel: SidebarPanel,
}

impl DocumentTab {
    pub fn new(id: u64) -> Self {
        Self {
            id,
            document: None,
            current_page: 0,
            zoom: ZoomMode::default(),
            view_mode: ViewMode::default(),
            scroll_offset_y: 0.0,
            rotation: Rotation::Deg0,
            sidebar_visible: true,
            sidebar_panel: SidebarPanel::Thumbnails,
        }
    }

    /// Tab title for display.
    pub fn title(&self) -> String {
        self.document
            .as_ref()
            .and_then(|d| d.metadata.title.clone())
            .or_else(|| {
                self.document.as_ref().map(|d| {
                    d.path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "Untitled".to_string())
                })
            })
            .unwrap_or_else(|| "New Tab".to_string())
    }

    /// Page count of the loaded document, or 0.
    pub fn page_count(&self) -> usize {
        self.document.as_ref().map_or(0, |d| d.page_count())
    }

    /// Navigate to a specific page.
    pub fn go_to_page(&mut self, page: usize) {
        let count = self.page_count();
        if count > 0 {
            self.current_page = page.min(count - 1);
        }
    }

    /// Go to the next page.
    pub fn next_page(&mut self) {
        let count = self.page_count();
        if count > 0 && self.current_page + 1 < count {
            self.current_page += 1;
        }
    }

    /// Go to the previous page.
    pub fn prev_page(&mut self) {
        if self.current_page > 0 {
            self.current_page -= 1;
        }
    }

    /// Go to the first page.
    pub fn first_page(&mut self) {
        self.current_page = 0;
    }

    /// Go to the last page.
    pub fn last_page(&mut self) {
        let count = self.page_count();
        if count > 0 {
            self.current_page = count - 1;
        }
    }

    /// Zoom in by one step.
    pub fn zoom_in(&mut self) {
        match self.zoom {
            ZoomMode::Fixed(z) => {
                let new_z = (z + ZOOM_STEP).min(MAX_ZOOM);
                self.zoom = ZoomMode::Fixed(new_z);
            }
            _ => {
                // Switch to fixed zoom at a reasonable level
                self.zoom = ZoomMode::Fixed((DEFAULT_ZOOM + ZOOM_STEP).min(MAX_ZOOM));
            }
        }
    }

    /// Zoom out by one step.
    pub fn zoom_out(&mut self) {
        match self.zoom {
            ZoomMode::Fixed(z) => {
                let new_z = (z - ZOOM_STEP).max(MIN_ZOOM);
                self.zoom = ZoomMode::Fixed(new_z);
            }
            _ => {
                self.zoom = ZoomMode::Fixed((DEFAULT_ZOOM - ZOOM_STEP).max(MIN_ZOOM));
            }
        }
    }

    /// Set zoom to a specific percentage (input is a value like 1.5 for 150%).
    pub fn set_zoom(&mut self, factor: f32) {
        self.zoom = ZoomMode::Fixed(factor.clamp(MIN_ZOOM, MAX_ZOOM));
    }

    /// Rotate all pages clockwise by 90 degrees.
    pub fn rotate_cw(&mut self) {
        self.rotation = self.rotation.rotate_cw();
        if let Some(doc) = &mut self.document {
            for page in &mut doc.pages {
                page.rotation = page.rotation.rotate_cw();
            }
        }
    }

    /// Rotate all pages counter-clockwise by 90 degrees.
    pub fn rotate_ccw(&mut self) {
        self.rotation = self.rotation.rotate_ccw();
        if let Some(doc) = &mut self.document {
            for page in &mut doc.pages {
                page.rotation = page.rotation.rotate_ccw();
            }
        }
    }

    /// Toggle the sidebar visibility.
    pub fn toggle_sidebar(&mut self) {
        self.sidebar_visible = !self.sidebar_visible;
    }

    /// Toggle between single-page and continuous scroll modes.
    pub fn toggle_view_mode(&mut self) {
        self.view_mode = match self.view_mode {
            ViewMode::SinglePage => ViewMode::ContinuousScroll,
            ViewMode::ContinuousScroll => ViewMode::SinglePage,
        };
    }
}

// ============================================================================
// Search state
// ============================================================================

/// State of the text search feature.
#[derive(Clone, Debug)]
pub struct SearchState {
    pub query: String,
    pub results: Vec<SearchResult>,
    pub current_match: Option<usize>,
    pub active: bool,
    pub case_sensitive: bool,
}

impl SearchState {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            results: Vec::new(),
            current_match: None,
            active: false,
            case_sensitive: false,
        }
    }

    /// Perform search on the document.
    pub fn search(&mut self, document: &PdfDocument) {
        self.results = document.search(&self.query);
        self.current_match = if self.results.is_empty() {
            None
        } else {
            Some(0)
        };
    }

    /// Navigate to the next search result.
    pub fn next_match(&mut self) {
        if self.results.is_empty() {
            return;
        }
        self.current_match = Some(match self.current_match {
            Some(i) => (i + 1) % self.results.len(),
            None => 0,
        });
    }

    /// Navigate to the previous search result.
    pub fn prev_match(&mut self) {
        if self.results.is_empty() {
            return;
        }
        self.current_match = Some(match self.current_match {
            Some(0) => self.results.len() - 1,
            Some(i) => i - 1,
            None => 0,
        });
    }

    /// Result count display string.
    pub fn match_count_label(&self) -> String {
        if self.query.is_empty() {
            String::new()
        } else if self.results.is_empty() {
            "No matches".to_string()
        } else {
            match self.current_match {
                Some(i) => format!("{} of {}", i + 1, self.results.len()),
                None => format!("{} matches", self.results.len()),
            }
        }
    }

    /// Clear search state.
    pub fn clear(&mut self) {
        self.query.clear();
        self.results.clear();
        self.current_match = None;
        self.active = false;
    }
}

impl Default for SearchState {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Application state
// ============================================================================

/// Unique identifier generator.
#[derive(Debug)]
pub struct IdGenerator {
    next: u64,
}

impl IdGenerator {
    pub fn new() -> Self {
        Self { next: 1 }
    }

    pub fn next_id(&mut self) -> u64 {
        let id = self.next;
        self.next += 1;
        id
    }
}

impl Default for IdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// The complete PDF viewer application state.
#[derive(Debug)]
pub struct PdfViewerApp {
    pub tabs: Vec<DocumentTab>,
    pub active_tab: usize,
    pub search: SearchState,
    pub recent_files: RecentFilesList,
    pub print_settings: PrintSettings,
    pub dark_mode: bool,
    pub window_width: f32,
    pub window_height: f32,
    pub id_gen: IdGenerator,
    pub next_annotation_id: u64,
}

impl PdfViewerApp {
    pub fn new(width: f32, height: f32) -> Self {
        let mut id_gen = IdGenerator::new();
        let initial_tab = DocumentTab::new(id_gen.next_id());
        Self {
            tabs: vec![initial_tab],
            active_tab: 0,
            search: SearchState::new(),
            recent_files: RecentFilesList::default(),
            print_settings: PrintSettings::default(),
            dark_mode: true,
            window_width: width,
            window_height: height,
            id_gen,
            next_annotation_id: 1,
        }
    }

    /// Get the active tab.
    pub fn active_tab(&self) -> Option<&DocumentTab> {
        self.tabs.get(self.active_tab)
    }

    /// Get the active tab mutably.
    pub fn active_tab_mut(&mut self) -> Option<&mut DocumentTab> {
        self.tabs.get_mut(self.active_tab)
    }

    /// Open a new empty tab and switch to it.
    pub fn new_tab(&mut self) -> u64 {
        let id = self.id_gen.next_id();
        let tab = DocumentTab::new(id);
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        id
    }

    /// Close a tab by index.
    pub fn close_tab(&mut self, index: usize) {
        if self.tabs.len() <= 1 {
            return; // Keep at least one tab
        }
        if index < self.tabs.len() {
            self.tabs.remove(index);
            if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len() - 1;
            } else if self.active_tab > index {
                self.active_tab -= 1;
            }
        }
    }

    /// Switch to a tab by index.
    pub fn switch_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active_tab = index;
        }
    }

    /// Load a document into the active tab.
    pub fn load_document(&mut self, doc: PdfDocument) {
        let path = doc.path.clone();
        let title = doc.metadata.title.clone();
        if let Some(tab) = self.active_tab_mut() {
            tab.document = Some(doc);
            tab.current_page = 0;
            tab.scroll_offset_y = 0.0;
        }
        self.recent_files.add(path, title, 0, 0);
    }

    /// Add a highlight annotation to the current page of the active tab.
    pub fn add_highlight(&mut self, rect: PageRect, color: Color) -> Option<u64> {
        let ann_id = self.next_annotation_id;
        self.next_annotation_id += 1;
        if let Some(tab) = self.active_tab_mut() {
            let page_idx = tab.current_page;
            if let Some(doc) = &mut tab.document
                && let Some(page) = doc.pages.get_mut(page_idx) {
                    page.annotations.push(Annotation {
                        id: ann_id,
                        page_index: page_idx,
                        rect,
                        annotation_type: AnnotationType::Highlight { color },
                        author: String::new(),
                        created_timestamp: 0,
                    });
                    return Some(ann_id);
                }
        }
        None
    }

    /// Add a sticky note annotation.
    pub fn add_note(&mut self, rect: PageRect, content: String) -> Option<u64> {
        let ann_id = self.next_annotation_id;
        self.next_annotation_id += 1;
        if let Some(tab) = self.active_tab_mut() {
            let page_idx = tab.current_page;
            if let Some(doc) = &mut tab.document
                && let Some(page) = doc.pages.get_mut(page_idx) {
                    page.annotations.push(Annotation {
                        id: ann_id,
                        page_index: page_idx,
                        rect,
                        annotation_type: AnnotationType::Note { content },
                        author: String::new(),
                        created_timestamp: 0,
                    });
                    return Some(ann_id);
                }
        }
        None
    }

    /// Add a freehand annotation.
    pub fn add_freehand(
        &mut self,
        rect: PageRect,
        points: Vec<(f32, f32)>,
        color: Color,
        width: f32,
    ) -> Option<u64> {
        let ann_id = self.next_annotation_id;
        self.next_annotation_id += 1;
        if let Some(tab) = self.active_tab_mut() {
            let page_idx = tab.current_page;
            if let Some(doc) = &mut tab.document
                && let Some(page) = doc.pages.get_mut(page_idx) {
                    page.annotations.push(Annotation {
                        id: ann_id,
                        page_index: page_idx,
                        rect,
                        annotation_type: AnnotationType::Freehand { points, color, width },
                        author: String::new(),
                        created_timestamp: 0,
                    });
                    return Some(ann_id);
                }
        }
        None
    }

    /// Remove an annotation by id from the active tab's current page.
    pub fn remove_annotation(&mut self, annotation_id: u64) -> bool {
        if let Some(tab) = self.active_tab_mut() {
            let page_idx = tab.current_page;
            if let Some(doc) = &mut tab.document
                && let Some(page) = doc.pages.get_mut(page_idx) {
                    let before = page.annotations.len();
                    page.annotations.retain(|a| a.id != annotation_id);
                    return page.annotations.len() < before;
                }
        }
        false
    }

    /// Compute the content area dimensions (accounting for toolbar, tabs, status, sidebar).
    pub fn content_area(&self) -> (f32, f32, f32, f32) {
        let sidebar_w = self
            .active_tab()
            .filter(|t| t.sidebar_visible)
            .map_or(0.0, |_| SIDEBAR_WIDTH);
        let x = sidebar_w;
        let y = TOOLBAR_HEIGHT + TAB_BAR_HEIGHT;
        let w = (self.window_width - sidebar_w).max(0.0);
        let h = (self.window_height - y - STATUS_BAR_HEIGHT).max(0.0);
        (x, y, w, h)
    }

    /// Render the entire application to a RenderTree.
    pub fn render(&self) -> RenderTree {
        let mut tree = RenderTree::new();

        // Background
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: self.window_height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_toolbar(&mut tree);
        self.render_tab_bar(&mut tree);

        if let Some(tab) = self.active_tab() {
            if tab.sidebar_visible {
                self.render_sidebar(&mut tree, tab);
            }
            self.render_document_area(&mut tree, tab);
        }

        self.render_status_bar(&mut tree);

        if self.search.active {
            self.render_search_bar(&mut tree);
        }

        tree
    }

    /// Render the toolbar.
    fn render_toolbar(&self, tree: &mut RenderTree) {
        // Toolbar background
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: TOOLBAR_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Bottom border
        tree.push(RenderCommand::Line {
            x1: 0.0,
            y1: TOOLBAR_HEIGHT,
            x2: self.window_width,
            y2: TOOLBAR_HEIGHT,
            color: SURFACE0,
            width: 1.0,
        });

        let mut btn_x: f32 = 8.0;
        let btn_y: f32 = 6.0;
        let btn_h: f32 = 32.0;

        // Navigation buttons
        let nav_buttons = [
            ("<<", "First"),
            ("<", "Prev"),
            (">", "Next"),
            (">>", "Last"),
        ];
        for (label, _tooltip) in &nav_buttons {
            let btn_w: f32 = 36.0;
            self.render_toolbar_button(tree, btn_x, btn_y, btn_w, btn_h, label);
            btn_x += btn_w + 4.0;
        }

        btn_x += 8.0;

        // Page indicator
        if let Some(tab) = self.active_tab() {
            let page_text = format!(
                "Page {} / {}",
                tab.current_page + 1,
                tab.page_count()
            );
            tree.push(RenderCommand::Text {
                x: btn_x,
                y: btn_y + 9.0,
                text: page_text,
                color: TEXT_COLOR,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(120.0),
            });
            btn_x += 128.0;
        }

        // Separator
        tree.push(RenderCommand::Line {
            x1: btn_x,
            y1: btn_y + 2.0,
            x2: btn_x,
            y2: btn_y + btn_h - 2.0,
            color: SURFACE1,
            width: 1.0,
        });
        btn_x += 12.0;

        // Zoom buttons
        let zoom_buttons = [("-", "Zoom Out"), ("+", "Zoom In")];
        for (label, _tooltip) in &zoom_buttons {
            let btn_w: f32 = 32.0;
            self.render_toolbar_button(tree, btn_x, btn_y, btn_w, btn_h, label);
            btn_x += btn_w + 4.0;
        }

        // Zoom indicator
        if let Some(tab) = self.active_tab() {
            let (_, _, vw, vh) = self.content_area();
            let page_w = tab
                .document
                .as_ref()
                .and_then(|d| d.pages.first())
                .map_or(DEFAULT_PAGE_WIDTH, |p| p.display_width());
            let page_h = tab
                .document
                .as_ref()
                .and_then(|d| d.pages.first())
                .map_or(DEFAULT_PAGE_HEIGHT, |p| p.display_height());
            let zoom_label = tab.zoom.label(vw, vh, page_w, page_h);
            tree.push(RenderCommand::Text {
                x: btn_x,
                y: btn_y + 9.0,
                text: zoom_label,
                color: SUBTEXT1,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(140.0),
            });
            btn_x += 148.0;
        }

        // Separator
        tree.push(RenderCommand::Line {
            x1: btn_x,
            y1: btn_y + 2.0,
            x2: btn_x,
            y2: btn_y + btn_h - 2.0,
            color: SURFACE1,
            width: 1.0,
        });
        btn_x += 12.0;

        // Fit buttons
        let fit_buttons = [("FW", "Fit Width"), ("FP", "Fit Page")];
        for (label, _tooltip) in &fit_buttons {
            let btn_w: f32 = 36.0;
            self.render_toolbar_button(tree, btn_x, btn_y, btn_w, btn_h, label);
            btn_x += btn_w + 4.0;
        }

        btn_x += 8.0;

        // Rotation buttons
        let rot_buttons = [("CCW", "Rotate CCW"), ("CW", "Rotate CW")];
        for (label, _tooltip) in &rot_buttons {
            let btn_w: f32 = 40.0;
            self.render_toolbar_button(tree, btn_x, btn_y, btn_w, btn_h, label);
            btn_x += btn_w + 4.0;
        }

        btn_x += 8.0;

        // View mode button
        let vm_label = match self.active_tab().map(|t| t.view_mode) {
            Some(ViewMode::SinglePage) => "1pg",
            Some(ViewMode::ContinuousScroll) => "Scr",
            None => "1pg",
        };
        self.render_toolbar_button(tree, btn_x, btn_y, 36.0, btn_h, vm_label);
        btn_x += 44.0;

        // Sidebar toggle
        self.render_toolbar_button(tree, btn_x, btn_y, 36.0, btn_h, "SB");

        // Right-side buttons (search, print)
        let right_x = self.window_width - 90.0;
        self.render_toolbar_button(tree, right_x, btn_y, 36.0, btn_h, "Srch");
        self.render_toolbar_button(tree, right_x + 44.0, btn_y, 36.0, btn_h, "Prt");
    }

    /// Render a toolbar button.
    fn render_toolbar_button(
        &self,
        tree: &mut RenderTree,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        label: &str,
    ) {
        tree.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        tree.push(RenderCommand::StrokeRect {
            x,
            y,
            width: w,
            height: h,
            color: SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(4.0),
        });
        // Center text in button
        let text_x = x + 4.0;
        let text_y = y + (h - 12.0) / 2.0;
        tree.push(RenderCommand::Text {
            x: text_x,
            y: text_y,
            text: label.to_string(),
            color: TEXT_COLOR,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(w - 8.0),
        });
    }

    /// Render the tab bar.
    fn render_tab_bar(&self, tree: &mut RenderTree) {
        let y = TOOLBAR_HEIGHT;

        // Tab bar background
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.window_width,
            height: TAB_BAR_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        tree.push(RenderCommand::Line {
            x1: 0.0,
            y1: y + TAB_BAR_HEIGHT,
            x2: self.window_width,
            y2: y + TAB_BAR_HEIGHT,
            color: SURFACE0,
            width: 1.0,
        });

        let mut tab_x: f32 = 4.0;
        let tab_w: f32 = 180.0;
        let tab_h: f32 = TAB_BAR_HEIGHT - 4.0;

        for (i, tab) in self.tabs.iter().enumerate() {
            let is_active = i == self.active_tab;
            let bg = if is_active { BASE } else { CRUST };
            let fg = if is_active { TEXT_COLOR } else { SUBTEXT0 };

            // Tab background
            tree.push(RenderCommand::FillRect {
                x: tab_x,
                y: y + 2.0,
                width: tab_w,
                height: tab_h,
                color: bg,
                corner_radii: CornerRadii {
                    top_left: 6.0,
                    top_right: 6.0,
                    bottom_left: 0.0,
                    bottom_right: 0.0,
                },
            });

            if is_active {
                // Active tab indicator line
                tree.push(RenderCommand::Line {
                    x1: tab_x,
                    y1: y + 2.0,
                    x2: tab_x + tab_w,
                    y2: y + 2.0,
                    color: BLUE,
                    width: 2.0,
                });
            }

            // Tab title
            let title = tab.title();
            let display_title = if title.len() > 20 {
                format!("{}...", &title[..17])
            } else {
                title
            };
            tree.push(RenderCommand::Text {
                x: tab_x + 10.0,
                y: y + 10.0,
                text: display_title,
                color: fg,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(tab_w - 30.0),
            });

            // Close button (x) on each tab
            let close_x = tab_x + tab_w - 22.0;
            let close_y = y + 10.0;
            tree.push(RenderCommand::Text {
                x: close_x,
                y: close_y,
                text: "x".to_string(),
                color: OVERLAY0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            tab_x += tab_w + 2.0;
        }

        // New tab button (+)
        tree.push(RenderCommand::FillRect {
            x: tab_x,
            y: y + 6.0,
            width: 28.0,
            height: 24.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        tree.push(RenderCommand::Text {
            x: tab_x + 8.0,
            y: y + 10.0,
            text: "+".to_string(),
            color: SUBTEXT1,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    /// Render the sidebar.
    fn render_sidebar(&self, tree: &mut RenderTree, tab: &DocumentTab) {
        let sidebar_y = TOOLBAR_HEIGHT + TAB_BAR_HEIGHT;
        let sidebar_h = self.window_height - sidebar_y - STATUS_BAR_HEIGHT;

        // Sidebar background
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y: sidebar_y,
            width: SIDEBAR_WIDTH,
            height: sidebar_h.max(0.0),
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Right border
        tree.push(RenderCommand::Line {
            x1: SIDEBAR_WIDTH,
            y1: sidebar_y,
            x2: SIDEBAR_WIDTH,
            y2: sidebar_y + sidebar_h,
            color: SURFACE0,
            width: 1.0,
        });

        // Panel selector tabs
        let panels = [
            (SidebarPanel::Thumbnails, "Thumbs"),
            (SidebarPanel::Bookmarks, "Marks"),
            (SidebarPanel::Annotations, "Notes"),
        ];
        let panel_tab_w = SIDEBAR_WIDTH / panels.len() as f32;
        for (i, (panel, label)) in panels.iter().enumerate() {
            let px = i as f32 * panel_tab_w;
            let is_active = tab.sidebar_panel == *panel;
            let bg = if is_active { BASE } else { MANTLE };
            let fg = if is_active { BLUE } else { SUBTEXT0 };

            tree.push(RenderCommand::FillRect {
                x: px,
                y: sidebar_y,
                width: panel_tab_w,
                height: 28.0,
                color: bg,
                corner_radii: CornerRadii::ZERO,
            });

            tree.push(RenderCommand::Text {
                x: px + 8.0,
                y: sidebar_y + 7.0,
                text: label.to_string(),
                color: fg,
                font_size: 11.0,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(panel_tab_w - 16.0),
            });
        }

        // Panel content
        let content_y = sidebar_y + 32.0;
        let content_h = (sidebar_h - 32.0).max(0.0);

        tree.push(RenderCommand::PushClip {
            x: 0.0,
            y: content_y,
            width: SIDEBAR_WIDTH,
            height: content_h,
        });

        match tab.sidebar_panel {
            SidebarPanel::Thumbnails => {
                self.render_thumbnail_strip(tree, tab, content_y, content_h);
            }
            SidebarPanel::Bookmarks => {
                self.render_bookmarks_panel(tree, tab, content_y, content_h);
            }
            SidebarPanel::Annotations => {
                self.render_annotations_panel(tree, tab, content_y, content_h);
            }
        }

        tree.push(RenderCommand::PopClip);
    }

    /// Render the thumbnail strip in the sidebar.
    fn render_thumbnail_strip(
        &self,
        tree: &mut RenderTree,
        tab: &DocumentTab,
        start_y: f32,
        _height: f32,
    ) {
        let Some(doc) = &tab.document else {
            tree.push(RenderCommand::Text {
                x: 16.0,
                y: start_y + 20.0,
                text: "No document loaded".to_string(),
                color: SUBTEXT0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - 32.0),
            });
            return;
        };

        let thumb_w = SIDEBAR_WIDTH - 32.0;
        let thumb_h = THUMBNAIL_HEIGHT;
        let mut y = start_y + 8.0;

        for (i, _page) in doc.pages.iter().enumerate() {
            let is_current = i == tab.current_page;

            // Thumbnail border highlight
            if is_current {
                tree.push(RenderCommand::StrokeRect {
                    x: 14.0,
                    y: y - 2.0,
                    width: thumb_w + 4.0,
                    height: thumb_h + 4.0 + 18.0,
                    color: BLUE,
                    line_width: 2.0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            // Thumbnail page (white rectangle as placeholder)
            tree.push(RenderCommand::FillRect {
                x: 16.0,
                y,
                width: thumb_w,
                height: thumb_h,
                color: Color::rgb(240, 240, 240),
                corner_radii: CornerRadii::all(2.0),
            });

            // Page number label below thumbnail
            let label = doc.page_label(i);
            tree.push(RenderCommand::Text {
                x: 16.0,
                y: y + thumb_h + 2.0,
                text: label,
                color: if is_current { TEXT_COLOR } else { SUBTEXT0 },
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(thumb_w),
            });

            y += thumb_h + 24.0;
        }
    }

    /// Render the bookmarks/outline panel.
    fn render_bookmarks_panel(
        &self,
        tree: &mut RenderTree,
        tab: &DocumentTab,
        start_y: f32,
        _height: f32,
    ) {
        let Some(doc) = &tab.document else {
            tree.push(RenderCommand::Text {
                x: 16.0,
                y: start_y + 20.0,
                text: "No document loaded".to_string(),
                color: SUBTEXT0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - 32.0),
            });
            return;
        };

        if doc.bookmarks.is_empty() {
            tree.push(RenderCommand::Text {
                x: 16.0,
                y: start_y + 20.0,
                text: "No bookmarks".to_string(),
                color: SUBTEXT0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - 32.0),
            });
            return;
        }

        let entries = doc.flatten_bookmarks();
        let mut y = start_y + 8.0;
        let line_h: f32 = 24.0;

        for (depth, bm) in &entries {
            let indent = 16.0 + (*depth as f32) * 16.0;
            let is_on_current_page = bm.page_index == tab.current_page;

            // Expand/collapse indicator
            if !bm.children.is_empty() {
                let arrow = if bm.expanded { "v" } else { ">" };
                tree.push(RenderCommand::Text {
                    x: indent - 12.0,
                    y: y + 4.0,
                    text: arrow.to_string(),
                    color: OVERLAY0,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }

            // Bookmark title
            tree.push(RenderCommand::Text {
                x: indent,
                y: y + 4.0,
                text: bm.title.clone(),
                color: if is_on_current_page {
                    BLUE
                } else {
                    TEXT_COLOR
                },
                font_size: 12.0,
                font_weight: if is_on_current_page {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(SIDEBAR_WIDTH - indent - 8.0),
            });

            y += line_h;
        }
    }

    /// Render the annotations panel.
    fn render_annotations_panel(
        &self,
        tree: &mut RenderTree,
        tab: &DocumentTab,
        start_y: f32,
        _height: f32,
    ) {
        let Some(doc) = &tab.document else {
            tree.push(RenderCommand::Text {
                x: 16.0,
                y: start_y + 20.0,
                text: "No document loaded".to_string(),
                color: SUBTEXT0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - 32.0),
            });
            return;
        };

        let annotations: Vec<&Annotation> = doc
            .pages
            .iter()
            .flat_map(|p| p.annotations.iter())
            .collect();

        if annotations.is_empty() {
            tree.push(RenderCommand::Text {
                x: 16.0,
                y: start_y + 20.0,
                text: "No annotations".to_string(),
                color: SUBTEXT0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - 32.0),
            });
            return;
        }

        let mut y = start_y + 8.0;
        for ann in &annotations {
            let type_label = match &ann.annotation_type {
                AnnotationType::Highlight { .. } => "Highlight",
                AnnotationType::Note { .. } => "Note",
                AnnotationType::Freehand { .. } => "Drawing",
                AnnotationType::Underline { .. } => "Underline",
                AnnotationType::Strikethrough { .. } => "Strikethrough",
            };
            let type_color = match &ann.annotation_type {
                AnnotationType::Highlight { color } => *color,
                AnnotationType::Note { .. } => YELLOW,
                AnnotationType::Freehand { color, .. } => *color,
                AnnotationType::Underline { color } => *color,
                AnnotationType::Strikethrough { color } => *color,
            };

            // Color dot
            tree.push(RenderCommand::FillRect {
                x: 16.0,
                y: y + 4.0,
                width: 8.0,
                height: 8.0,
                color: type_color,
                corner_radii: CornerRadii::all(4.0),
            });

            // Annotation type and page
            tree.push(RenderCommand::Text {
                x: 30.0,
                y: y + 2.0,
                text: format!("{} - Page {}", type_label, ann.page_index + 1),
                color: TEXT_COLOR,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - 46.0),
            });

            y += 22.0;
        }
    }

    /// Render the main document viewing area.
    fn render_document_area(&self, tree: &mut RenderTree, tab: &DocumentTab) {
        let (area_x, area_y, area_w, area_h) = self.content_area();

        // Clip to content area
        tree.push(RenderCommand::PushClip {
            x: area_x,
            y: area_y,
            width: area_w,
            height: area_h,
        });

        // Dark background
        tree.push(RenderCommand::FillRect {
            x: area_x,
            y: area_y,
            width: area_w,
            height: area_h,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        let Some(doc) = &tab.document else {
            // No document — show welcome message
            self.render_welcome(tree, area_x, area_y, area_w, area_h);
            tree.push(RenderCommand::PopClip);
            return;
        };

        match tab.view_mode {
            ViewMode::SinglePage => {
                self.render_single_page(tree, doc, tab, area_x, area_y, area_w, area_h);
            }
            ViewMode::ContinuousScroll => {
                self.render_continuous_scroll(tree, doc, tab, area_x, area_y, area_w, area_h);
            }
        }

        tree.push(RenderCommand::PopClip);
    }

    /// Render welcome message when no document is loaded.
    fn render_welcome(
        &self,
        tree: &mut RenderTree,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) {
        let cx = x + w / 2.0 - 100.0;
        let cy = y + h / 2.0 - 60.0;

        tree.push(RenderCommand::Text {
            x: cx,
            y: cy,
            text: "PDF Viewer".to_string(),
            color: TEXT_COLOR,
            font_size: 24.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
        });

        tree.push(RenderCommand::Text {
            x: cx,
            y: cy + 36.0,
            text: "Open a PDF to begin".to_string(),
            color: SUBTEXT0,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
        });

        // Recent files
        if !self.recent_files.entries.is_empty() {
            tree.push(RenderCommand::Text {
                x: cx,
                y: cy + 72.0,
                text: "Recent Files:".to_string(),
                color: SUBTEXT1,
                font_size: 13.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(300.0),
            });

            let mut ry = cy + 94.0;
            for (i, entry) in self.recent_files.entries.iter().take(5).enumerate() {
                let name = entry
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "unknown".to_string());
                tree.push(RenderCommand::Text {
                    x: cx + 8.0,
                    y: ry,
                    text: format!("{}. {}", i + 1, name),
                    color: BLUE,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(280.0),
                });
                ry += 20.0;
            }
        }
    }

    /// Render a single page in the document area.
    // self + tree + 2 model refs + 4 area-rect floats; grouping the area into a
    // struct would not improve clarity at the call site.
    #[allow(clippy::too_many_arguments)]
    fn render_single_page(
        &self,
        tree: &mut RenderTree,
        doc: &PdfDocument,
        tab: &DocumentTab,
        area_x: f32,
        area_y: f32,
        area_w: f32,
        area_h: f32,
    ) {
        let Some(page) = doc.pages.get(tab.current_page) else {
            return;
        };

        let page_w = page.display_width();
        let page_h = page.display_height();
        let zoom = tab.zoom.effective_zoom(area_w, area_h, page_w, page_h);
        let rendered_w = page_w * zoom;
        let rendered_h = page_h * zoom;

        // Center the page in the viewport
        let page_x = area_x + (area_w - rendered_w) / 2.0;
        let page_y = area_y + (area_h - rendered_h) / 2.0;

        self.render_page_box(tree, page, tab.current_page, page_x, page_y, rendered_w, rendered_h, zoom);

        // Render search highlights on this page
        self.render_search_highlights(tree, tab.current_page, page_x, page_y, zoom);
    }

    /// Render continuous scroll mode.
    // Same shape as render_single_page; both are render driver entry points.
    #[allow(clippy::too_many_arguments)]
    fn render_continuous_scroll(
        &self,
        tree: &mut RenderTree,
        doc: &PdfDocument,
        tab: &DocumentTab,
        area_x: f32,
        area_y: f32,
        area_w: f32,
        area_h: f32,
    ) {
        // Use the first page for zoom reference
        let ref_page_w = doc
            .pages
            .first()
            .map_or(DEFAULT_PAGE_WIDTH, |p| p.display_width());
        let ref_page_h = doc
            .pages
            .first()
            .map_or(DEFAULT_PAGE_HEIGHT, |p| p.display_height());
        let zoom = tab.zoom.effective_zoom(area_w, area_h, ref_page_w, ref_page_h);

        let mut y_offset = area_y + PAGE_MARGIN - tab.scroll_offset_y;

        for (i, page) in doc.pages.iter().enumerate() {
            let pw = page.display_width() * zoom;
            let ph = page.display_height() * zoom;

            // Only render pages that are visible
            if y_offset + ph >= area_y && y_offset <= area_y + area_h {
                let page_x = area_x + (area_w - pw) / 2.0;
                self.render_page_box(tree, page, i, page_x, y_offset, pw, ph, zoom);
                self.render_search_highlights(tree, i, page_x, y_offset, zoom);
            }

            y_offset += ph + PAGE_GAP;
        }
    }

    /// Render a page box with shadow, background, and content.
    // self + tree + page model + page index + rect (x,y,w,h) + zoom; all needed.
    #[allow(clippy::too_many_arguments)]
    fn render_page_box(
        &self,
        tree: &mut RenderTree,
        page: &PdfPage,
        _page_index: usize,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        zoom: f32,
    ) {
        // Page shadow
        tree.push(RenderCommand::BoxShadow {
            x,
            y,
            width: w,
            height: h,
            offset_x: 2.0,
            offset_y: 2.0,
            blur: PAGE_SHADOW_BLUR,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 80),
            corner_radii: CornerRadii::all(2.0),
        });

        // Page background (white for the document page)
        let page_bg = if self.dark_mode {
            Color::rgb(40, 42, 54)
        } else {
            Color::rgb(255, 255, 255)
        };
        tree.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: page_bg,
            corner_radii: CornerRadii::all(2.0),
        });

        // Page border
        tree.push(RenderCommand::StrokeRect {
            x,
            y,
            width: w,
            height: h,
            color: SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(2.0),
        });

        // Render text spans (placeholder content)
        let text_color = if self.dark_mode {
            TEXT_COLOR
        } else {
            Color::rgb(30, 30, 30)
        };

        for span in &page.text_spans {
            let sx = x + span.rect.x * zoom;
            let sy = y + span.rect.y * zoom;
            let font_sz = span.font_size * zoom;
            let max_w = span.rect.width * zoom;

            tree.push(RenderCommand::Text {
                x: sx,
                y: sy,
                text: span.text.clone(),
                color: text_color,
                font_size: font_sz,
                font_weight: FontWeightHint::Regular,
                max_width: Some(max_w),
            });
        }

        // Render annotations
        for ann in &page.annotations {
            self.render_annotation(tree, ann, x, y, zoom);
        }
    }

    /// Render an annotation overlay on a page.
    fn render_annotation(
        &self,
        tree: &mut RenderTree,
        ann: &Annotation,
        page_x: f32,
        page_y: f32,
        zoom: f32,
    ) {
        let ax = page_x + ann.rect.x * zoom;
        let ay = page_y + ann.rect.y * zoom;
        let aw = ann.rect.width * zoom;
        let ah = ann.rect.height * zoom;

        match &ann.annotation_type {
            AnnotationType::Highlight { color } => {
                tree.push(RenderCommand::FillRect {
                    x: ax,
                    y: ay,
                    width: aw,
                    height: ah,
                    color: Color::rgba(color.r, color.g, color.b, 80),
                    corner_radii: CornerRadii::all(2.0),
                });
            }
            AnnotationType::Note { .. } => {
                // Sticky note icon
                tree.push(RenderCommand::FillRect {
                    x: ax,
                    y: ay,
                    width: 20.0 * zoom,
                    height: 20.0 * zoom,
                    color: YELLOW,
                    corner_radii: CornerRadii::all(3.0),
                });
                tree.push(RenderCommand::Text {
                    x: ax + 3.0 * zoom,
                    y: ay + 3.0 * zoom,
                    text: "N".to_string(),
                    color: CRUST,
                    font_size: 12.0 * zoom,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }
            AnnotationType::Freehand { points, color, width } => {
                // Draw line segments between points
                for pair in points.windows(2) {
                    if let [p1, p2] = pair {
                        tree.push(RenderCommand::Line {
                            x1: page_x + p1.0 * zoom,
                            y1: page_y + p1.1 * zoom,
                            x2: page_x + p2.0 * zoom,
                            y2: page_y + p2.1 * zoom,
                            color: *color,
                            width: width * zoom,
                        });
                    }
                }
            }
            AnnotationType::Underline { color } => {
                tree.push(RenderCommand::Line {
                    x1: ax,
                    y1: ay + ah,
                    x2: ax + aw,
                    y2: ay + ah,
                    color: *color,
                    width: 1.5 * zoom,
                });
            }
            AnnotationType::Strikethrough { color } => {
                tree.push(RenderCommand::Line {
                    x1: ax,
                    y1: ay + ah / 2.0,
                    x2: ax + aw,
                    y2: ay + ah / 2.0,
                    color: *color,
                    width: 1.5 * zoom,
                });
            }
        }
    }

    /// Render search result highlights on a page.
    fn render_search_highlights(
        &self,
        tree: &mut RenderTree,
        page_index: usize,
        page_x: f32,
        page_y: f32,
        zoom: f32,
    ) {
        if !self.search.active {
            return;
        }

        for (i, result) in self.search.results.iter().enumerate() {
            if result.page_index != page_index {
                continue;
            }
            let is_current = self.search.current_match == Some(i);
            let color = if is_current {
                Color::rgba(PEACH.r, PEACH.g, PEACH.b, 120)
            } else {
                Color::rgba(YELLOW.r, YELLOW.g, YELLOW.b, 80)
            };

            let hx = page_x + result.rect.x * zoom;
            let hy = page_y + result.rect.y * zoom;
            let hw = result.rect.width * zoom;
            let hh = result.rect.height * zoom;

            tree.push(RenderCommand::FillRect {
                x: hx,
                y: hy,
                width: hw,
                height: hh,
                color,
                corner_radii: CornerRadii::all(2.0),
            });

            if is_current {
                tree.push(RenderCommand::StrokeRect {
                    x: hx,
                    y: hy,
                    width: hw,
                    height: hh,
                    color: PEACH,
                    line_width: 2.0,
                    corner_radii: CornerRadii::all(2.0),
                });
            }
        }
    }

    /// Render the search bar overlay.
    fn render_search_bar(&self, tree: &mut RenderTree) {
        let bar_w: f32 = 360.0;
        let bar_h: f32 = 44.0;
        let bar_x = self.window_width - bar_w - 16.0;
        let bar_y = TOOLBAR_HEIGHT + TAB_BAR_HEIGHT + 8.0;

        // Shadow
        tree.push(RenderCommand::BoxShadow {
            x: bar_x,
            y: bar_y,
            width: bar_w,
            height: bar_h,
            offset_x: 0.0,
            offset_y: 2.0,
            blur: 8.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 100),
            corner_radii: CornerRadii::all(8.0),
        });

        // Background
        tree.push(RenderCommand::FillRect {
            x: bar_x,
            y: bar_y,
            width: bar_w,
            height: bar_h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });

        tree.push(RenderCommand::StrokeRect {
            x: bar_x,
            y: bar_y,
            width: bar_w,
            height: bar_h,
            color: SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(8.0),
        });

        // Search icon placeholder
        tree.push(RenderCommand::Text {
            x: bar_x + 12.0,
            y: bar_y + 13.0,
            text: "S".to_string(),
            color: OVERLAY0,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Search query text
        let query_display = if self.search.query.is_empty() {
            "Search...".to_string()
        } else {
            self.search.query.clone()
        };
        let query_color = if self.search.query.is_empty() {
            OVERLAY0
        } else {
            TEXT_COLOR
        };
        tree.push(RenderCommand::Text {
            x: bar_x + 32.0,
            y: bar_y + 14.0,
            text: query_display,
            color: query_color,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(180.0),
        });

        // Match count
        let count_label = self.search.match_count_label();
        if !count_label.is_empty() {
            tree.push(RenderCommand::Text {
                x: bar_x + 220.0,
                y: bar_y + 14.0,
                text: count_label,
                color: SUBTEXT0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(80.0),
            });
        }

        // Nav buttons (prev/next match)
        let btn_y = bar_y + 8.0;
        self.render_toolbar_button(tree, bar_x + bar_w - 64.0, btn_y, 26.0, 28.0, "<");
        self.render_toolbar_button(tree, bar_x + bar_w - 34.0, btn_y, 26.0, 28.0, ">");
    }

    /// Render the status bar.
    fn render_status_bar(&self, tree: &mut RenderTree) {
        let y = self.window_height - STATUS_BAR_HEIGHT;

        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.window_width,
            height: STATUS_BAR_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Top border
        tree.push(RenderCommand::Line {
            x1: 0.0,
            y1: y,
            x2: self.window_width,
            y2: y,
            color: SURFACE0,
            width: 1.0,
        });

        if let Some(tab) = self.active_tab() {
            let mut sx: f32 = 12.0;

            // File name
            if let Some(doc) = &tab.document {
                let name = doc
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Untitled".to_string());
                tree.push(RenderCommand::Text {
                    x: sx,
                    y: y + 7.0,
                    text: name,
                    color: SUBTEXT1,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(200.0),
                });
                sx += 210.0;
            }

            // Page info
            tree.push(RenderCommand::Text {
                x: sx,
                y: y + 7.0,
                text: format!("Page {} / {}", tab.current_page + 1, tab.page_count()),
                color: SUBTEXT0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
            });
            sx += 110.0;

            // View mode
            let mode_str = match tab.view_mode {
                ViewMode::SinglePage => "Single Page",
                ViewMode::ContinuousScroll => "Continuous",
            };
            tree.push(RenderCommand::Text {
                x: sx,
                y: y + 7.0,
                text: mode_str.to_string(),
                color: SUBTEXT0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
            });
            sx += 110.0;

            // Rotation
            if tab.rotation != Rotation::Deg0 {
                tree.push(RenderCommand::Text {
                    x: sx,
                    y: y + 7.0,
                    text: format!("{}deg", tab.rotation.degrees()),
                    color: SUBTEXT0,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(60.0),
                });
            }

            // Right side: zoom percentage
            let (_, _, vw, vh) = self.content_area();
            let pw = tab
                .document
                .as_ref()
                .and_then(|d| d.pages.first())
                .map_or(DEFAULT_PAGE_WIDTH, |p| p.display_width());
            let ph = tab
                .document
                .as_ref()
                .and_then(|d| d.pages.first())
                .map_or(DEFAULT_PAGE_HEIGHT, |p| p.display_height());
            let zoom_pct = tab.zoom.effective_zoom(vw, vh, pw, ph) * 100.0;
            tree.push(RenderCommand::Text {
                x: self.window_width - 80.0,
                y: y + 7.0,
                text: format!("{}%", zoom_pct as u32),
                color: SUBTEXT0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(60.0),
            });
        }
    }
}

// ============================================================================
// Utility: Preset zoom levels
// ============================================================================

/// Standard zoom levels for the zoom dropdown.
pub const ZOOM_PRESETS: &[f32] = &[0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0];

/// Find the next higher zoom preset from the given value.
pub fn next_zoom_preset(current: f32) -> f32 {
    for &z in ZOOM_PRESETS {
        if z > current + 0.01 {
            return z;
        }
    }
    MAX_ZOOM
}

/// Find the next lower zoom preset from the given value.
pub fn prev_zoom_preset(current: f32) -> f32 {
    let mut prev = MIN_ZOOM;
    for &z in ZOOM_PRESETS {
        if z >= current - 0.01 {
            return prev;
        }
        prev = z;
    }
    prev
}

/// Calculate total document height for continuous scroll mode.
pub fn total_document_height(doc: &PdfDocument, zoom: f32) -> f32 {
    let mut total = PAGE_MARGIN;
    for page in &doc.pages {
        total += page.display_height() * zoom + PAGE_GAP;
    }
    // Replace last PAGE_GAP with PAGE_MARGIN for bottom padding
    if !doc.pages.is_empty() {
        total = total - PAGE_GAP + PAGE_MARGIN;
    }
    total
}

/// Find which page is at a given scroll offset in continuous mode.
pub fn page_at_offset(doc: &PdfDocument, offset: f32, zoom: f32) -> usize {
    let mut y = PAGE_MARGIN;
    for (i, page) in doc.pages.iter().enumerate() {
        let h = page.display_height() * zoom;
        if offset < y + h {
            return i;
        }
        y += h + PAGE_GAP;
    }
    doc.page_count().saturating_sub(1)
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    // Placeholder: the real entry point would initialize the windowing system,
    // create a PdfViewerApp, and enter the event loop.
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Rotation tests -------------------------------------------------------

    #[test]
    fn test_rotation_cw_cycle() {
        let mut r = Rotation::Deg0;
        r = r.rotate_cw();
        assert_eq!(r, Rotation::Deg90);
        r = r.rotate_cw();
        assert_eq!(r, Rotation::Deg180);
        r = r.rotate_cw();
        assert_eq!(r, Rotation::Deg270);
        r = r.rotate_cw();
        assert_eq!(r, Rotation::Deg0);
    }

    #[test]
    fn test_rotation_ccw_cycle() {
        let mut r = Rotation::Deg0;
        r = r.rotate_ccw();
        assert_eq!(r, Rotation::Deg270);
        r = r.rotate_ccw();
        assert_eq!(r, Rotation::Deg180);
        r = r.rotate_ccw();
        assert_eq!(r, Rotation::Deg90);
        r = r.rotate_ccw();
        assert_eq!(r, Rotation::Deg0);
    }

    #[test]
    fn test_rotation_degrees() {
        assert_eq!(Rotation::Deg0.degrees(), 0);
        assert_eq!(Rotation::Deg90.degrees(), 90);
        assert_eq!(Rotation::Deg180.degrees(), 180);
        assert_eq!(Rotation::Deg270.degrees(), 270);
    }

    #[test]
    fn test_rotation_swaps_dimensions() {
        assert!(!Rotation::Deg0.swaps_dimensions());
        assert!(Rotation::Deg90.swaps_dimensions());
        assert!(!Rotation::Deg180.swaps_dimensions());
        assert!(Rotation::Deg270.swaps_dimensions());
    }

    #[test]
    fn test_rotation_default() {
        assert_eq!(Rotation::default(), Rotation::Deg0);
    }

    // -- PageRect tests -------------------------------------------------------

    #[test]
    fn test_page_rect_contains() {
        let r = PageRect::new(10.0, 20.0, 100.0, 50.0);
        assert!(r.contains(50.0, 40.0));
        assert!(r.contains(10.0, 20.0)); // top-left corner
        assert!(r.contains(110.0, 70.0)); // bottom-right corner
        assert!(!r.contains(9.0, 40.0)); // left of rect
        assert!(!r.contains(111.0, 40.0)); // right of rect
        assert!(!r.contains(50.0, 19.0)); // above rect
        assert!(!r.contains(50.0, 71.0)); // below rect
    }

    #[test]
    fn test_page_rect_new() {
        let r = PageRect::new(5.0, 10.0, 200.0, 300.0);
        assert_eq!(r.x, 5.0);
        assert_eq!(r.y, 10.0);
        assert_eq!(r.width, 200.0);
        assert_eq!(r.height, 300.0);
    }

    // -- PdfPage tests --------------------------------------------------------

    #[test]
    fn test_pdf_page_new() {
        let page = PdfPage::new(612.0, 792.0);
        assert_eq!(page.width, 612.0);
        assert_eq!(page.height, 792.0);
        assert!(page.text_spans.is_empty());
        assert!(page.annotations.is_empty());
        assert_eq!(page.rotation, Rotation::Deg0);
        assert!(page.label.is_none());
    }

    #[test]
    fn test_pdf_page_display_dimensions_no_rotation() {
        let page = PdfPage::new(612.0, 792.0);
        assert_eq!(page.display_width(), 612.0);
        assert_eq!(page.display_height(), 792.0);
    }

    #[test]
    fn test_pdf_page_display_dimensions_rotated_90() {
        let mut page = PdfPage::new(612.0, 792.0);
        page.rotation = Rotation::Deg90;
        assert_eq!(page.display_width(), 792.0);
        assert_eq!(page.display_height(), 612.0);
    }

    #[test]
    fn test_pdf_page_display_dimensions_rotated_180() {
        let mut page = PdfPage::new(612.0, 792.0);
        page.rotation = Rotation::Deg180;
        assert_eq!(page.display_width(), 612.0);
        assert_eq!(page.display_height(), 792.0);
    }

    #[test]
    fn test_pdf_page_display_dimensions_rotated_270() {
        let mut page = PdfPage::new(612.0, 792.0);
        page.rotation = Rotation::Deg270;
        assert_eq!(page.display_width(), 792.0);
        assert_eq!(page.display_height(), 612.0);
    }

    // -- Bookmark tests -------------------------------------------------------

    #[test]
    fn test_bookmark_new() {
        let bm = Bookmark::new("Chapter 1", 0);
        assert_eq!(bm.title, "Chapter 1");
        assert_eq!(bm.page_index, 0);
        assert!(bm.children.is_empty());
        assert!(!bm.expanded);
    }

    #[test]
    fn test_bookmark_total_count_leaf() {
        let bm = Bookmark::new("Leaf", 0);
        assert_eq!(bm.total_count(), 1);
    }

    #[test]
    fn test_bookmark_total_count_nested() {
        let mut parent = Bookmark::new("Parent", 0);
        parent.children.push(Bookmark::new("Child 1", 1));
        parent.children.push(Bookmark::new("Child 2", 2));
        assert_eq!(parent.total_count(), 3);
    }

    #[test]
    fn test_bookmark_total_count_deep() {
        let mut root = Bookmark::new("Root", 0);
        let mut child = Bookmark::new("Child", 1);
        child.children.push(Bookmark::new("Grandchild", 2));
        root.children.push(child);
        assert_eq!(root.total_count(), 3);
    }

    #[test]
    fn test_bookmark_flatten_collapsed() {
        let mut parent = Bookmark::new("Parent", 0);
        parent.children.push(Bookmark::new("Child", 1));
        // Not expanded, so children should not appear
        let flat = parent.flatten(0);
        assert_eq!(flat.len(), 1);
        assert_eq!(flat[0].0, 0);
        assert_eq!(flat[0].1.title, "Parent");
    }

    #[test]
    fn test_bookmark_flatten_expanded() {
        let mut parent = Bookmark::new("Parent", 0);
        parent.expanded = true;
        parent.children.push(Bookmark::new("Child 1", 1));
        parent.children.push(Bookmark::new("Child 2", 2));
        let flat = parent.flatten(0);
        assert_eq!(flat.len(), 3);
        assert_eq!(flat[0].0, 0); // Parent at depth 0
        assert_eq!(flat[1].0, 1); // Child 1 at depth 1
        assert_eq!(flat[2].0, 1); // Child 2 at depth 1
    }

    // -- PdfDocument tests ----------------------------------------------------

    #[test]
    fn test_document_new() {
        let doc = PdfDocument::new(PathBuf::from("/test.pdf"));
        assert_eq!(doc.page_count(), 0);
        assert!(doc.bookmarks.is_empty());
        assert!(doc.metadata.title.is_none());
    }

    #[test]
    fn test_document_create_sample() {
        let doc = PdfDocument::create_sample(PathBuf::from("/sample.pdf"), 5);
        assert_eq!(doc.page_count(), 5);
        assert!(doc.metadata.title.is_some());
        assert!(!doc.bookmarks.is_empty());
        // Each page should have text spans
        for page in &doc.pages {
            assert!(!page.text_spans.is_empty());
        }
    }

    #[test]
    fn test_document_page_label_default() {
        let doc = PdfDocument::create_sample(PathBuf::from("/test.pdf"), 3);
        assert_eq!(doc.page_label(0), "1");
        assert_eq!(doc.page_label(1), "2");
        assert_eq!(doc.page_label(2), "3");
    }

    #[test]
    fn test_document_page_label_custom() {
        let mut doc = PdfDocument::create_sample(PathBuf::from("/test.pdf"), 2);
        doc.pages[0].label = Some("i".to_string());
        doc.pages[1].label = Some("ii".to_string());
        assert_eq!(doc.page_label(0), "i");
        assert_eq!(doc.page_label(1), "ii");
    }

    #[test]
    fn test_document_search_empty_query() {
        let doc = PdfDocument::create_sample(PathBuf::from("/test.pdf"), 3);
        let results = doc.search("");
        assert!(results.is_empty());
    }

    #[test]
    fn test_document_search_no_match() {
        let doc = PdfDocument::create_sample(PathBuf::from("/test.pdf"), 3);
        let results = doc.search("xyzzynotfound");
        assert!(results.is_empty());
    }

    #[test]
    fn test_document_search_finds_match() {
        let doc = PdfDocument::create_sample(PathBuf::from("/test.pdf"), 3);
        let results = doc.search("Lorem");
        // "Lorem" should appear on each page in the sample
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_document_search_case_insensitive() {
        let doc = PdfDocument::create_sample(PathBuf::from("/test.pdf"), 2);
        let results = doc.search("lorem");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_document_search_page_title() {
        let doc = PdfDocument::create_sample(PathBuf::from("/test.pdf"), 3);
        let results = doc.search("Page 2");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].page_index, 1);
    }

    #[test]
    fn test_document_flatten_bookmarks() {
        let doc = PdfDocument::create_sample(PathBuf::from("/test.pdf"), 5);
        let flat = doc.flatten_bookmarks();
        assert!(!flat.is_empty());
    }

    #[test]
    fn test_document_total_bookmark_count() {
        let doc = PdfDocument::create_sample(PathBuf::from("/test.pdf"), 5);
        let count = doc.total_bookmark_count();
        assert!(count > 0);
    }

    // -- ZoomMode tests -------------------------------------------------------

    #[test]
    fn test_zoom_fixed() {
        let z = ZoomMode::Fixed(1.5);
        assert_eq!(z.effective_zoom(1000.0, 800.0, 612.0, 792.0), 1.5);
    }

    #[test]
    fn test_zoom_fit_width() {
        let z = ZoomMode::FitWidth;
        let eff = z.effective_zoom(660.0, 800.0, 612.0, 792.0);
        // Available = 660 - 2*24 = 612, so zoom should be ~1.0
        assert!((eff - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_zoom_fit_page() {
        let z = ZoomMode::FitPage;
        let eff = z.effective_zoom(1000.0, 840.0, 612.0, 792.0);
        // Fit should use the height constraint: (840 - 48) / 792 = 1.0
        assert!(eff > 0.0);
        assert!(eff <= MAX_ZOOM);
    }

    #[test]
    fn test_zoom_fit_width_clamps() {
        // Very narrow viewport
        let z = ZoomMode::FitWidth;
        let eff = z.effective_zoom(50.0, 50.0, 612.0, 792.0);
        assert!(eff >= MIN_ZOOM);
    }

    #[test]
    fn test_zoom_fit_page_zero_page_dims() {
        let z = ZoomMode::FitPage;
        let eff = z.effective_zoom(1000.0, 800.0, 0.0, 0.0);
        assert_eq!(eff, DEFAULT_ZOOM);
    }

    #[test]
    fn test_zoom_label_fixed() {
        let z = ZoomMode::Fixed(1.5);
        let label = z.label(1000.0, 800.0, 612.0, 792.0);
        assert_eq!(label, "150%");
    }

    #[test]
    fn test_zoom_default() {
        let z = ZoomMode::default();
        match z {
            ZoomMode::Fixed(v) => assert_eq!(v, DEFAULT_ZOOM),
            _ => panic!("default should be Fixed"),
        }
    }

    // -- ViewMode tests -------------------------------------------------------

    #[test]
    fn test_view_mode_default() {
        assert_eq!(ViewMode::default(), ViewMode::SinglePage);
    }

    // -- PrintSettings tests --------------------------------------------------

    #[test]
    fn test_print_resolve_all() {
        let ps = PrintSettings::default();
        let pages = ps.resolve_pages(5, 2);
        assert_eq!(pages, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn test_print_resolve_current() {
        let ps = PrintSettings {
            page_range: PrintPageRange::CurrentPage,
            ..Default::default()
        };
        let pages = ps.resolve_pages(5, 2);
        assert_eq!(pages, vec![2]);
    }

    #[test]
    fn test_print_resolve_custom_range() {
        let ps = PrintSettings {
            page_range: PrintPageRange::Custom(vec![(0, 2), (4, 4)]),
            ..Default::default()
        };
        let pages = ps.resolve_pages(10, 0);
        assert_eq!(pages, vec![0, 1, 2, 4]);
    }

    #[test]
    fn test_print_resolve_custom_clamps() {
        let ps = PrintSettings {
            page_range: PrintPageRange::Custom(vec![(0, 100)]),
            ..Default::default()
        };
        let pages = ps.resolve_pages(3, 0);
        assert_eq!(pages, vec![0, 1, 2]);
    }

    #[test]
    fn test_print_resolve_current_out_of_range() {
        let ps = PrintSettings {
            page_range: PrintPageRange::CurrentPage,
            ..Default::default()
        };
        let pages = ps.resolve_pages(5, 10);
        assert!(pages.is_empty());
    }

    #[test]
    fn test_parse_page_range_all() {
        let result = PrintSettings::parse_page_range("all", 10);
        assert_eq!(result, PrintPageRange::All);
    }

    #[test]
    fn test_parse_page_range_empty() {
        let result = PrintSettings::parse_page_range("", 10);
        assert_eq!(result, PrintPageRange::All);
    }

    #[test]
    fn test_parse_page_range_single() {
        let result = PrintSettings::parse_page_range("3", 10);
        match result {
            PrintPageRange::Custom(ranges) => {
                assert_eq!(ranges, vec![(2, 2)]);
            }
            _ => panic!("expected Custom"),
        }
    }

    #[test]
    fn test_parse_page_range_range() {
        let result = PrintSettings::parse_page_range("2-5", 10);
        match result {
            PrintPageRange::Custom(ranges) => {
                assert_eq!(ranges, vec![(1, 4)]);
            }
            _ => panic!("expected Custom"),
        }
    }

    #[test]
    fn test_parse_page_range_mixed() {
        let result = PrintSettings::parse_page_range("1-3, 5, 7-9", 10);
        match result {
            PrintPageRange::Custom(ranges) => {
                assert_eq!(ranges, vec![(0, 2), (4, 4), (6, 8)]);
            }
            _ => panic!("expected Custom"),
        }
    }

    // -- RecentFilesList tests ------------------------------------------------

    #[test]
    fn test_recent_files_add() {
        let mut rf = RecentFilesList::new(5);
        rf.add(PathBuf::from("/a.pdf"), None, 1, 0);
        rf.add(PathBuf::from("/b.pdf"), None, 2, 0);
        assert_eq!(rf.entries.len(), 2);
        // Most recent first
        assert_eq!(rf.entries[0].path, PathBuf::from("/b.pdf"));
    }

    #[test]
    fn test_recent_files_dedup() {
        let mut rf = RecentFilesList::new(5);
        rf.add(PathBuf::from("/a.pdf"), None, 1, 0);
        rf.add(PathBuf::from("/a.pdf"), None, 2, 5);
        assert_eq!(rf.entries.len(), 1);
        assert_eq!(rf.entries[0].last_opened_timestamp, 2);
        assert_eq!(rf.entries[0].last_page, 5);
    }

    #[test]
    fn test_recent_files_capacity() {
        let mut rf = RecentFilesList::new(3);
        rf.add(PathBuf::from("/a.pdf"), None, 1, 0);
        rf.add(PathBuf::from("/b.pdf"), None, 2, 0);
        rf.add(PathBuf::from("/c.pdf"), None, 3, 0);
        rf.add(PathBuf::from("/d.pdf"), None, 4, 0);
        assert_eq!(rf.entries.len(), 3);
        // Oldest (a.pdf) should be removed
        assert!(rf.find(Path::new("/a.pdf")).is_none());
    }

    #[test]
    fn test_recent_files_remove() {
        let mut rf = RecentFilesList::new(5);
        rf.add(PathBuf::from("/a.pdf"), None, 1, 0);
        rf.add(PathBuf::from("/b.pdf"), None, 2, 0);
        rf.remove(Path::new("/a.pdf"));
        assert_eq!(rf.entries.len(), 1);
    }

    #[test]
    fn test_recent_files_clear() {
        let mut rf = RecentFilesList::new(5);
        rf.add(PathBuf::from("/a.pdf"), None, 1, 0);
        rf.clear();
        assert!(rf.entries.is_empty());
    }

    #[test]
    fn test_recent_files_find() {
        let mut rf = RecentFilesList::new(5);
        rf.add(PathBuf::from("/a.pdf"), Some("Title A".to_string()), 1, 3);
        let found = rf.find(Path::new("/a.pdf"));
        assert!(found.is_some());
        assert_eq!(found.unwrap().last_page, 3);
        assert!(rf.find(Path::new("/z.pdf")).is_none());
    }

    // -- DocumentTab tests ----------------------------------------------------

    #[test]
    fn test_tab_new() {
        let tab = DocumentTab::new(1);
        assert_eq!(tab.id, 1);
        assert!(tab.document.is_none());
        assert_eq!(tab.current_page, 0);
        assert!(tab.sidebar_visible);
    }

    #[test]
    fn test_tab_title_no_doc() {
        let tab = DocumentTab::new(1);
        assert_eq!(tab.title(), "New Tab");
    }

    #[test]
    fn test_tab_title_with_doc() {
        let mut tab = DocumentTab::new(1);
        tab.document = Some(PdfDocument::create_sample(PathBuf::from("/test.pdf"), 1));
        assert_eq!(tab.title(), "Sample Document");
    }

    #[test]
    fn test_tab_navigation_next_prev() {
        let mut tab = DocumentTab::new(1);
        tab.document = Some(PdfDocument::create_sample(PathBuf::from("/t.pdf"), 5));
        assert_eq!(tab.current_page, 0);
        tab.next_page();
        assert_eq!(tab.current_page, 1);
        tab.next_page();
        assert_eq!(tab.current_page, 2);
        tab.prev_page();
        assert_eq!(tab.current_page, 1);
    }

    #[test]
    fn test_tab_navigation_first_last() {
        let mut tab = DocumentTab::new(1);
        tab.document = Some(PdfDocument::create_sample(PathBuf::from("/t.pdf"), 5));
        tab.last_page();
        assert_eq!(tab.current_page, 4);
        tab.first_page();
        assert_eq!(tab.current_page, 0);
    }

    #[test]
    fn test_tab_navigation_bounds() {
        let mut tab = DocumentTab::new(1);
        tab.document = Some(PdfDocument::create_sample(PathBuf::from("/t.pdf"), 3));
        tab.prev_page(); // Already at 0
        assert_eq!(tab.current_page, 0);
        tab.last_page();
        tab.next_page(); // Already at last
        assert_eq!(tab.current_page, 2);
    }

    #[test]
    fn test_tab_go_to_page() {
        let mut tab = DocumentTab::new(1);
        tab.document = Some(PdfDocument::create_sample(PathBuf::from("/t.pdf"), 5));
        tab.go_to_page(3);
        assert_eq!(tab.current_page, 3);
        tab.go_to_page(100); // Clamp to last
        assert_eq!(tab.current_page, 4);
    }

    #[test]
    fn test_tab_zoom_in_out() {
        let mut tab = DocumentTab::new(1);
        tab.zoom = ZoomMode::Fixed(1.0);
        tab.zoom_in();
        match tab.zoom {
            ZoomMode::Fixed(z) => assert!((z - 1.25).abs() < 0.01),
            _ => panic!("expected Fixed"),
        }
        tab.zoom_out();
        match tab.zoom {
            ZoomMode::Fixed(z) => assert!((z - 1.0).abs() < 0.01),
            _ => panic!("expected Fixed"),
        }
    }

    #[test]
    fn test_tab_zoom_clamps() {
        let mut tab = DocumentTab::new(1);
        tab.zoom = ZoomMode::Fixed(MAX_ZOOM);
        tab.zoom_in();
        match tab.zoom {
            ZoomMode::Fixed(z) => assert!((z - MAX_ZOOM).abs() < 0.01),
            _ => panic!("expected Fixed"),
        }
        tab.zoom = ZoomMode::Fixed(MIN_ZOOM);
        tab.zoom_out();
        match tab.zoom {
            ZoomMode::Fixed(z) => assert!((z - MIN_ZOOM).abs() < 0.01),
            _ => panic!("expected Fixed"),
        }
    }

    #[test]
    fn test_tab_set_zoom() {
        let mut tab = DocumentTab::new(1);
        tab.set_zoom(2.0);
        match tab.zoom {
            ZoomMode::Fixed(z) => assert_eq!(z, 2.0),
            _ => panic!("expected Fixed"),
        }
        tab.set_zoom(10.0); // Should clamp
        match tab.zoom {
            ZoomMode::Fixed(z) => assert_eq!(z, MAX_ZOOM),
            _ => panic!("expected Fixed"),
        }
    }

    #[test]
    fn test_tab_rotate_cw() {
        let mut tab = DocumentTab::new(1);
        tab.document = Some(PdfDocument::create_sample(PathBuf::from("/t.pdf"), 1));
        assert_eq!(tab.rotation, Rotation::Deg0);
        tab.rotate_cw();
        assert_eq!(tab.rotation, Rotation::Deg90);
    }

    #[test]
    fn test_tab_rotate_ccw() {
        let mut tab = DocumentTab::new(1);
        tab.document = Some(PdfDocument::create_sample(PathBuf::from("/t.pdf"), 1));
        tab.rotate_ccw();
        assert_eq!(tab.rotation, Rotation::Deg270);
    }

    #[test]
    fn test_tab_toggle_sidebar() {
        let mut tab = DocumentTab::new(1);
        assert!(tab.sidebar_visible);
        tab.toggle_sidebar();
        assert!(!tab.sidebar_visible);
        tab.toggle_sidebar();
        assert!(tab.sidebar_visible);
    }

    #[test]
    fn test_tab_toggle_view_mode() {
        let mut tab = DocumentTab::new(1);
        assert_eq!(tab.view_mode, ViewMode::SinglePage);
        tab.toggle_view_mode();
        assert_eq!(tab.view_mode, ViewMode::ContinuousScroll);
        tab.toggle_view_mode();
        assert_eq!(tab.view_mode, ViewMode::SinglePage);
    }

    // -- SearchState tests ----------------------------------------------------

    #[test]
    fn test_search_new() {
        let s = SearchState::new();
        assert!(s.query.is_empty());
        assert!(s.results.is_empty());
        assert!(s.current_match.is_none());
        assert!(!s.active);
    }

    #[test]
    fn test_search_performs_search() {
        let doc = PdfDocument::create_sample(PathBuf::from("/t.pdf"), 3);
        let mut s = SearchState::new();
        s.query = "Lorem".to_string();
        s.search(&doc);
        assert_eq!(s.results.len(), 3);
        assert_eq!(s.current_match, Some(0));
    }

    #[test]
    fn test_search_next_match() {
        let doc = PdfDocument::create_sample(PathBuf::from("/t.pdf"), 3);
        let mut s = SearchState::new();
        s.query = "Lorem".to_string();
        s.search(&doc);
        s.next_match();
        assert_eq!(s.current_match, Some(1));
        s.next_match();
        assert_eq!(s.current_match, Some(2));
        s.next_match(); // Wrap around
        assert_eq!(s.current_match, Some(0));
    }

    #[test]
    fn test_search_prev_match() {
        let doc = PdfDocument::create_sample(PathBuf::from("/t.pdf"), 3);
        let mut s = SearchState::new();
        s.query = "Lorem".to_string();
        s.search(&doc);
        s.prev_match(); // Wrap to last
        assert_eq!(s.current_match, Some(2));
        s.prev_match();
        assert_eq!(s.current_match, Some(1));
    }

    #[test]
    fn test_search_match_count_label() {
        let mut s = SearchState::new();
        assert!(s.match_count_label().is_empty());
        s.query = "xyz".to_string();
        assert_eq!(s.match_count_label(), "No matches");
        s.results.push(SearchResult {
            page_index: 0,
            rect: PageRect::new(0.0, 0.0, 10.0, 10.0),
            context: String::new(),
        });
        s.current_match = Some(0);
        assert_eq!(s.match_count_label(), "1 of 1");
    }

    #[test]
    fn test_search_clear() {
        let doc = PdfDocument::create_sample(PathBuf::from("/t.pdf"), 2);
        let mut s = SearchState::new();
        s.query = "Lorem".to_string();
        s.active = true;
        s.search(&doc);
        s.clear();
        assert!(s.query.is_empty());
        assert!(s.results.is_empty());
        assert!(!s.active);
    }

    // -- IdGenerator tests ----------------------------------------------------

    #[test]
    fn test_id_generator() {
        let mut id_gen = IdGenerator::new();
        assert_eq!(id_gen.next_id(), 1);
        assert_eq!(id_gen.next_id(), 2);
        assert_eq!(id_gen.next_id(), 3);
    }

    // -- PdfViewerApp tests ---------------------------------------------------

    #[test]
    fn test_app_new() {
        let app = PdfViewerApp::new(1280.0, 720.0);
        assert_eq!(app.tabs.len(), 1);
        assert_eq!(app.active_tab, 0);
        assert!(app.dark_mode);
        assert_eq!(app.window_width, 1280.0);
    }

    #[test]
    fn test_app_new_tab() {
        let mut app = PdfViewerApp::new(1280.0, 720.0);
        let id = app.new_tab();
        assert!(id > 0);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.active_tab, 1);
    }

    #[test]
    fn test_app_close_tab() {
        let mut app = PdfViewerApp::new(1280.0, 720.0);
        app.new_tab();
        app.new_tab();
        assert_eq!(app.tabs.len(), 3);
        app.close_tab(1);
        assert_eq!(app.tabs.len(), 2);
    }

    #[test]
    fn test_app_close_last_tab() {
        let mut app = PdfViewerApp::new(1280.0, 720.0);
        app.close_tab(0); // Should keep at least 1
        assert_eq!(app.tabs.len(), 1);
    }

    #[test]
    fn test_app_switch_tab() {
        let mut app = PdfViewerApp::new(1280.0, 720.0);
        app.new_tab();
        app.switch_tab(0);
        assert_eq!(app.active_tab, 0);
        app.switch_tab(1);
        assert_eq!(app.active_tab, 1);
        app.switch_tab(100); // Out of range, no change
        assert_eq!(app.active_tab, 1);
    }

    #[test]
    fn test_app_load_document() {
        let mut app = PdfViewerApp::new(1280.0, 720.0);
        let doc = PdfDocument::create_sample(PathBuf::from("/test.pdf"), 3);
        app.load_document(doc);
        assert!(app.active_tab().unwrap().document.is_some());
        assert_eq!(app.active_tab().unwrap().page_count(), 3);
        assert_eq!(app.recent_files.entries.len(), 1);
    }

    #[test]
    fn test_app_add_highlight() {
        let mut app = PdfViewerApp::new(1280.0, 720.0);
        let doc = PdfDocument::create_sample(PathBuf::from("/test.pdf"), 2);
        app.load_document(doc);
        let rect = PageRect::new(50.0, 50.0, 100.0, 20.0);
        let id = app.add_highlight(rect, YELLOW);
        assert!(id.is_some());
        let page = &app.active_tab().unwrap().document.as_ref().unwrap().pages[0];
        assert_eq!(page.annotations.len(), 1);
    }

    #[test]
    fn test_app_add_note() {
        let mut app = PdfViewerApp::new(1280.0, 720.0);
        let doc = PdfDocument::create_sample(PathBuf::from("/test.pdf"), 1);
        app.load_document(doc);
        let rect = PageRect::new(10.0, 10.0, 20.0, 20.0);
        let id = app.add_note(rect, "A note".to_string());
        assert!(id.is_some());
    }

    #[test]
    fn test_app_add_freehand() {
        let mut app = PdfViewerApp::new(1280.0, 720.0);
        let doc = PdfDocument::create_sample(PathBuf::from("/test.pdf"), 1);
        app.load_document(doc);
        let rect = PageRect::new(0.0, 0.0, 100.0, 100.0);
        let pts = vec![(10.0, 10.0), (50.0, 50.0), (90.0, 10.0)];
        let id = app.add_freehand(rect, pts, RED, 2.0);
        assert!(id.is_some());
    }

    #[test]
    fn test_app_remove_annotation() {
        let mut app = PdfViewerApp::new(1280.0, 720.0);
        let doc = PdfDocument::create_sample(PathBuf::from("/test.pdf"), 1);
        app.load_document(doc);
        let rect = PageRect::new(10.0, 10.0, 50.0, 20.0);
        let id = app.add_highlight(rect, YELLOW).unwrap();
        assert!(app.remove_annotation(id));
        let page = &app.active_tab().unwrap().document.as_ref().unwrap().pages[0];
        assert!(page.annotations.is_empty());
    }

    #[test]
    fn test_app_remove_annotation_not_found() {
        let mut app = PdfViewerApp::new(1280.0, 720.0);
        let doc = PdfDocument::create_sample(PathBuf::from("/test.pdf"), 1);
        app.load_document(doc);
        assert!(!app.remove_annotation(9999));
    }

    #[test]
    fn test_app_content_area_with_sidebar() {
        let mut app = PdfViewerApp::new(1280.0, 720.0);
        let doc = PdfDocument::create_sample(PathBuf::from("/test.pdf"), 1);
        app.load_document(doc);
        let (x, y, w, _h) = app.content_area();
        assert_eq!(x, SIDEBAR_WIDTH); // Sidebar visible by default
        assert!(w < 1280.0);
        assert!(y > 0.0);
    }

    #[test]
    fn test_app_content_area_without_sidebar() {
        let mut app = PdfViewerApp::new(1280.0, 720.0);
        let doc = PdfDocument::create_sample(PathBuf::from("/test.pdf"), 1);
        app.load_document(doc);
        app.active_tab_mut().unwrap().sidebar_visible = false;
        let (x, _y, _w, _h) = app.content_area();
        assert_eq!(x, 0.0);
    }

    #[test]
    fn test_app_render_no_doc() {
        let app = PdfViewerApp::new(1280.0, 720.0);
        let tree = app.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_app_render_with_doc() {
        let mut app = PdfViewerApp::new(1280.0, 720.0);
        let doc = PdfDocument::create_sample(PathBuf::from("/test.pdf"), 3);
        app.load_document(doc);
        let tree = app.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_app_render_continuous_scroll() {
        let mut app = PdfViewerApp::new(1280.0, 720.0);
        let doc = PdfDocument::create_sample(PathBuf::from("/test.pdf"), 5);
        app.load_document(doc);
        app.active_tab_mut().unwrap().toggle_view_mode();
        let tree = app.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_app_render_with_search_active() {
        let mut app = PdfViewerApp::new(1280.0, 720.0);
        let doc = PdfDocument::create_sample(PathBuf::from("/test.pdf"), 2);
        app.load_document(doc);
        app.search.active = true;
        app.search.query = "Lorem".to_string();
        if let Some(doc) = &app.active_tab().unwrap().document {
            let results = doc.search("Lorem");
            app.search.results = results;
            app.search.current_match = Some(0);
        }
        let tree = app.render();
        assert!(!tree.is_empty());
    }

    // -- Utility function tests -----------------------------------------------

    #[test]
    fn test_next_zoom_preset() {
        assert_eq!(next_zoom_preset(1.0), 1.25);
        assert_eq!(next_zoom_preset(0.5), 0.75);
        assert_eq!(next_zoom_preset(3.5), MAX_ZOOM);
    }

    #[test]
    fn test_prev_zoom_preset() {
        assert_eq!(prev_zoom_preset(1.0), 0.75);
        assert_eq!(prev_zoom_preset(0.5), 0.25);
        assert_eq!(prev_zoom_preset(0.25), MIN_ZOOM);
    }

    #[test]
    fn test_total_document_height() {
        let doc = PdfDocument::create_sample(PathBuf::from("/t.pdf"), 3);
        let height = total_document_height(&doc, 1.0);
        // 3 pages * 792 + 2 gaps * 12 + 2 margins * 24
        let expected = PAGE_MARGIN + 3.0 * 792.0 + 2.0 * PAGE_GAP + PAGE_MARGIN;
        assert!((height - expected).abs() < 0.1);
    }

    #[test]
    fn test_total_document_height_empty() {
        let doc = PdfDocument::new(PathBuf::from("/t.pdf"));
        let height = total_document_height(&doc, 1.0);
        assert_eq!(height, PAGE_MARGIN);
    }

    #[test]
    fn test_page_at_offset() {
        let doc = PdfDocument::create_sample(PathBuf::from("/t.pdf"), 5);
        assert_eq!(page_at_offset(&doc, 0.0, 1.0), 0);
        // After first page: MARGIN + 792 + GAP = 828
        assert_eq!(page_at_offset(&doc, 900.0, 1.0), 1);
    }

    #[test]
    fn test_page_at_offset_past_end() {
        let doc = PdfDocument::create_sample(PathBuf::from("/t.pdf"), 3);
        assert_eq!(page_at_offset(&doc, 100000.0, 1.0), 2);
    }

    #[test]
    fn test_close_active_tab_adjusts_index() {
        let mut app = PdfViewerApp::new(1280.0, 720.0);
        app.new_tab();
        app.new_tab();
        // Active is now tab 2 (index 2)
        app.switch_tab(2);
        app.close_tab(0); // Remove first tab
        // Active tab index should adjust
        assert!(app.active_tab < app.tabs.len());
    }

    #[test]
    fn test_sidebar_panel_default() {
        assert_eq!(SidebarPanel::default(), SidebarPanel::Thumbnails);
    }

    #[test]
    fn test_render_with_annotations_on_page() {
        let mut app = PdfViewerApp::new(1280.0, 720.0);
        let doc = PdfDocument::create_sample(PathBuf::from("/test.pdf"), 1);
        app.load_document(doc);
        app.add_highlight(
            PageRect::new(50.0, 100.0, 200.0, 14.0),
            YELLOW,
        );
        app.add_note(PageRect::new(300.0, 200.0, 20.0, 20.0), "Test note".to_string());
        app.add_freehand(
            PageRect::new(0.0, 0.0, 100.0, 100.0),
            vec![(10.0, 10.0), (50.0, 50.0)],
            RED,
            2.0,
        );
        let tree = app.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_zoom_in_from_fit_width() {
        let mut tab = DocumentTab::new(1);
        tab.zoom = ZoomMode::FitWidth;
        tab.zoom_in();
        match tab.zoom {
            ZoomMode::Fixed(z) => assert!(z > 1.0),
            _ => panic!("expected Fixed after zoom in from FitWidth"),
        }
    }

    #[test]
    fn test_zoom_out_from_fit_page() {
        let mut tab = DocumentTab::new(1);
        tab.zoom = ZoomMode::FitPage;
        tab.zoom_out();
        match tab.zoom {
            ZoomMode::Fixed(z) => assert!(z < 1.0),
            _ => panic!("expected Fixed after zoom out from FitPage"),
        }
    }
}
