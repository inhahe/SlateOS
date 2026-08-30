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
//!
//! # Where the controls come from
//!
//! Every clickable thing is a [`Target`], and the *only* place a target's
//! rectangle is written down is the renderer, which records it with
//! [`Frame::hit`] at the moment it draws it. [`PdfViewerApp::target_at`] then
//! answers "what is under this point?" by drawing a frame and asking it.
//!
//! That matters here more than in most of these apps, because this toolbar's
//! geometry is *accumulated*: `btn_x` walks left to right across fourteen
//! buttons, three separators and two variable-width text readouts, one of which
//! (the zoom label) is only drawn when a tab exists. A second, hand-summed copy
//! of that walk would be wrong the first time anyone inserted a button — which
//! is exactly how `credmanager` ended up drawing sidebar rows that clicked to
//! nothing; see `known-issues.md` ->
//! `C-RENDERER-AND-HIT-TEST-DERIVE-THE-SAME-LAYOUT-SEPARATELY`.

use std::process::ExitCode;

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
#[cfg(test)]
use guitk::probe;
use guitk::probe::Probe;
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
#[allow(unused_imports)]
use guitk::style::CornerRadii;
use guitk::text;
use guitk::textfind;
use guitk::wheel;
use oswindow::app::{self, App, Response};

use std::path::{Path, PathBuf};

// ============================================================================
// Catppuccin Mocha theme colors
// ============================================================================

// Full palette is defined for use across the application. Not all colors are
// referenced yet but they are part of the theme and available for future use.
#[allow(dead_code)]
const BASE: Color = Color::rgb(30, 30, 46);
#[allow(dead_code)]
const MANTLE: Color = Color::rgb(24, 24, 37);
#[allow(dead_code)]
const CRUST: Color = Color::rgb(17, 17, 27);
#[allow(dead_code)]
const SURFACE0: Color = Color::rgb(49, 50, 68);
#[allow(dead_code)]
const SURFACE1: Color = Color::rgb(69, 71, 90);
#[allow(dead_code)]
const SURFACE2: Color = Color::rgb(88, 91, 112);
#[allow(dead_code)]
const OVERLAY0: Color = Color::rgb(108, 112, 134);
#[allow(dead_code)]
const TEXT_COLOR: Color = Color::rgb(205, 214, 244);
#[allow(dead_code)]
const SUBTEXT1: Color = Color::rgb(186, 194, 222);
#[allow(dead_code)]
const SUBTEXT0: Color = Color::rgb(166, 173, 200);
#[allow(dead_code)]
const BLUE: Color = Color::rgb(137, 180, 250);
#[allow(dead_code)]
const LAVENDER: Color = Color::rgb(180, 190, 254);
#[allow(dead_code)]
const SAPPHIRE: Color = Color::rgb(116, 199, 236);
#[allow(dead_code)]
const GREEN: Color = Color::rgb(166, 227, 161);
#[allow(dead_code)]
const YELLOW: Color = Color::rgb(249, 226, 175);
#[allow(dead_code)]
const PEACH: Color = Color::rgb(250, 179, 135);
#[allow(dead_code)]
const RED: Color = Color::rgb(243, 139, 168);
#[allow(dead_code)]
const MAUVE: Color = Color::rgb(203, 166, 247);
#[allow(dead_code)]
const ROSEWATER: Color = Color::rgb(245, 224, 220);
#[allow(dead_code)]
const FLAMINGO: Color = Color::rgb(242, 205, 205);
#[allow(dead_code)]
const TEAL: Color = Color::rgb(148, 226, 213);

// ============================================================================
// Layout constants
// ============================================================================

const TOOLBAR_HEIGHT: f32 = 44.0;
const TAB_BAR_HEIGHT: f32 = 36.0;
/// Left inset of a tab's title text within its tab.
const TAB_TEXT_INSET: f32 = 10.0;
/// Distance from a tab's right edge to the close glyph. Doubles as the right
/// bound on the title, which must stop before the glyph rather than at the tab
/// edge — the two used to disagree by 2px and the title drew under the `x`.
const TAB_CLOSE_INSET: f32 = 22.0;
const TAB_TITLE_SIZE: f32 = 12.0;
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

/// The window this app asks for when it opens.
const WINDOW_WIDTH: f32 = 1100.0;
const WINDOW_HEIGHT: f32 = 780.0;

/// How far one scrolled *row* moves the document, in points at 1x zoom.
///
/// A notch is `guitk::wheel::ROWS_PER_NOTCH` rows, so a notch moves three times
/// this -- the same three-lines-per-notch the rest of the desktop uses, rather
/// than a distance this app picked for itself.
const SCROLL_ROW_HEIGHT: f32 = 20.0;

// ============================================================================
// What can be clicked
// ============================================================================

/// Every clickable thing in the window.
///
/// One variant per control, carrying whatever index the handler needs. The
/// renderer records the rectangle; nothing else knows one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// First / previous / next / last page.
    Nav(Nav),
    ZoomOut,
    ZoomIn,
    /// Fit-width and fit-page, which set [`ZoomMode`] rather than a factor.
    Fit(Fit),
    RotateCcw,
    RotateCw,
    /// Single-page vs. continuous scroll.
    ViewModeToggle,
    SidebarToggle,
    SearchToggle,
    Print,
    /// A document tab in the tab strip.
    Tab(usize),
    /// The `x` on a document tab. Recorded *after* the tab it sits on, so that
    /// `hit_test` -- which answers with the last box containing the point --
    /// closes the tab rather than merely selecting it.
    TabClose(usize),
    NewTab,
    /// One of the three sidebar panel selectors.
    Panel(SidebarPanel),
    /// A page thumbnail in the sidebar, by page index.
    Thumbnail(usize),
    /// A bookmark row, by index into the *flattened* outline.
    Bookmark(usize),
    /// The expand/collapse arrow on a bookmark that has children. Recorded
    /// after the row, so the arrow wins where the two overlap.
    BookmarkArrow(usize),
    /// A recent-file entry on the welcome screen.
    RecentFile(usize),
    /// The search field, previous-match and next-match buttons.
    SearchField,
    SearchPrev,
    SearchNext,
    /// The document viewport. Not a button -- it exists so a click or a wheel
    /// notch over the pages can be told apart from one over the sidebar.
    Document,
}

/// The four page-navigation buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    First,
    Prev,
    Next,
    Last,
}

/// The two fit buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    Width,
    Page,
}

/// The frame this app draws into: a render tree that also remembers where each
/// [`Target`] was put.
pub type Frame = guitk::frame::Frame<Target>;

// ============================================================================
// Layout
// ============================================================================

/// A finite, non-negative version of `v`.
///
/// A window size arrives from outside this program, and NaN or a negative width
/// would otherwise propagate into every rectangle in the frame.
fn sane(v: f32) -> f32 {
    if v.is_finite() { v.max(0.0) } else { 0.0 }
}

/// Take a band of height `want` off the top of what is left below `y`, and
/// advance `y` past it.
///
/// The band **shrinks** to what remains rather than being clamped away or
/// clipped afterwards, and -- the part that matters -- it keeps its `y` even
/// when it shrinks to nothing. An earlier version derived each band by
/// intersecting it with the window and taking `Rect::EMPTY` when that failed,
/// which is correct for the *size* and silently wrong for the *position*:
/// `Rect::EMPTY` sits at the origin, so a zero-height tab strip reported
/// `bottom() == 0` and the sidebar below it was laid out on top of the toolbar,
/// where its controls were both invisible and clickable. A band with no height
/// still has a place.
fn take_top(y: &mut f32, limit: f32, width: f32, want: f32) -> Rect {
    let h = want.min((limit - *y).max(0.0));
    let band = Rect::new(0.0, *y, width, h);
    *y += h;
    band
}

/// Where the bands of the window are, for one frame at one size.
///
/// Recomputed on every frame from the size the caller supplies and never
/// remembered -- see [`PdfViewerApp::frame`] for why.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub window: Rect,
    pub toolbar: Rect,
    pub tab_bar: Rect,
    /// Empty when the sidebar is hidden, which is also what makes every
    /// sidebar hit box vanish: `Frame::hit` drops a box trimmed to nothing.
    pub sidebar: Rect,
    pub content: Rect,
    pub status: Rect,
}

impl Layout {
    /// Derive the bands for a `width` x `height` window.
    ///
    /// `sidebar_visible` is app state rather than geometry, but the content
    /// area's left edge depends on it, so it has to come in here rather than be
    /// applied afterwards.
    #[must_use]
    pub fn new(width: f32, height: f32, sidebar_visible: bool) -> Self {
        let width = sane(width);
        let height = sane(height);
        let window = Rect::new(0.0, 0.0, width, height);

        // The status bar is taken off the bottom first, so a window too short
        // for everything loses document area rather than losing the bar off the
        // bottom edge.
        let status_h = STATUS_BAR_HEIGHT.min(height);
        let body_bottom = (height - status_h).max(0.0);

        let mut y = 0.0;
        let toolbar = take_top(&mut y, body_bottom, width, TOOLBAR_HEIGHT);
        let tab_bar = take_top(&mut y, body_bottom, width, TAB_BAR_HEIGHT);
        // Whatever is left between the strips and the status bar.
        let body = take_top(&mut y, body_bottom, width, f32::INFINITY);

        let sidebar_w = if sidebar_visible {
            SIDEBAR_WIDTH.min(width)
        } else {
            0.0
        };
        let sidebar = Rect::new(0.0, body.y, sidebar_w, body.h);
        let content = Rect::new(sidebar_w, body.y, (width - sidebar_w).max(0.0), body.h);
        let status = Rect::new(0.0, body_bottom, width, status_h);

        Self {
            window,
            toolbar,
            tab_bar,
            sidebar,
            content,
            status,
        }
    }
}

// ============================================================================
// Document model
// ============================================================================

/// PDF page rotation in degrees.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
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
        Self {
            x,
            y,
            width,
            height,
        }
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
    Freehand {
        points: Vec<(f32, f32)>,
        color: Color,
        width: f32,
    },
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
        self.children
            .iter()
            .map(Bookmark::total_count)
            .fold(1, usize::saturating_add)
    }

    /// Flatten the bookmark tree into a list of (depth, bookmark_ref).
    pub fn flatten(&self, depth: usize) -> Vec<(usize, &Bookmark)> {
        let mut result = vec![(depth, self)];
        if self.expanded {
            for child in &self.children {
                result.extend(child.flatten(depth.saturating_add(1)));
            }
        }
        result
    }

    /// Find the `index`-th entry of this subtree's flattened order, mutably.
    ///
    /// `index` counts down as the walk consumes entries, and the walk stops at
    /// the first hit -- so on return `index` is the *remaining* count if this
    /// subtree was too short. Callers pass the same running counter to each
    /// root in turn, which is how a flat row number reached through the
    /// renderer's list finds the tree node it was drawn from.
    ///
    /// The order here must be the same order [`Bookmark::flatten`] produces,
    /// collapsed children included -- a row the user cannot see is a row they
    /// cannot have clicked, so descending into a collapsed subtree would shift
    /// every index below it.
    fn nth_mut(&mut self, index: &mut usize) -> Option<&mut Bookmark> {
        if *index == 0 {
            return Some(self);
        }
        *index = index.saturating_sub(1);
        if !self.expanded {
            return None;
        }
        // Not a `find_map`: the closure would need `&mut index` while the
        // iterator holds `&mut self.children`, and the loop says it plainly.
        for child in &mut self.children {
            if let Some(found) = child.nth_mut(index) {
                return Some(found);
            }
        }
        None
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
        doc.metadata.file_size_bytes = (page_count as u64).saturating_mul(4096);

        for i in 0..page_count {
            let mut page = PdfPage::new(DEFAULT_PAGE_WIDTH, DEFAULT_PAGE_HEIGHT);
            page.text_spans.push(TextSpan {
                text: format!("Page {}", i.saturating_add(1)),
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
            .unwrap_or_else(|| format!("{}", index.saturating_add(1)))
    }

    /// Search for text across all pages. Returns (page_index, rect) for each match.
    pub fn search(&self, query: &str) -> Vec<SearchResult> {
        if query.is_empty() {
            return Vec::new();
        }
        let mut results = Vec::new();
        for (page_idx, page) in self.pages.iter().enumerate() {
            for span in &page.text_spans {
                // The span's width comes from the document, so the honest
                // approximation is to spread it over the span's *characters*.
                // Spreading it over its bytes made the cell too narrow for any
                // span containing an accent.
                let char_count = span.text.chars().count();
                let char_width = if span.rect.width > 0.0 && char_count > 0 {
                    span.rect.width / char_count as f32
                } else {
                    8.0
                };
                // `textfind` reports offsets into `span.text` itself. This used
                // to search `span.text.to_lowercase()` and count characters in
                // *that*, which puts the highlight in the wrong place for any
                // span containing a character that changes length when folded —
                // and then advanced by the *unfolded* needle's byte length
                // inside the folded copy, which for a query of `İ` lands inside
                // a character and panics.
                for (start, end) in
                    textfind::matches(&span.text, query, textfind::Case::Insensitive)
                {
                    let chars_before = span.text.get(..start).map_or(0, |p| p.chars().count());
                    let match_chars = span.text.get(start..end).map_or(0, |m| m.chars().count());
                    let highlight_rect = PageRect::new(
                        span.rect.x + chars_before as f32 * char_width,
                        span.rect.y,
                        match_chars as f32 * char_width,
                        span.rect.height.max(span.font_size),
                    );
                    results.push(SearchResult {
                        page_index: page_idx,
                        rect: highlight_rect,
                        context: span.text.clone(),
                    });
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

    /// Expand or collapse the `index`-th row of the flattened outline.
    ///
    /// Answers whether anything changed, so a caller can tell a click that hit
    /// a childless row (where an arrow is not drawn and toggling is
    /// meaningless) from one that did something.
    pub fn toggle_bookmark(&mut self, index: usize) -> bool {
        let mut remaining = index;
        for bm in &mut self.bookmarks {
            if let Some(found) = bm.nth_mut(&mut remaining) {
                if found.children.is_empty() {
                    return false;
                }
                found.expanded = !found.expanded;
                return true;
            }
        }
        false
    }

    /// The page the `index`-th row of the flattened outline points at.
    pub fn bookmark_page(&self, index: usize) -> Option<usize> {
        self.flatten_bookmarks()
            .get(index)
            .map(|(_, bm)| bm.page_index)
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
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
#[derive(Clone, Debug, PartialEq, Default)]
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
                let mut pages: Vec<usize> = ranges
                    .iter()
                    // A range that begins past the last page names no pages.
                    //
                    // For a document that *has* pages the `end.min(..)` below
                    // already handled this by accident -- `49..=9` is empty --
                    // but on a document with **no** pages the clamp saturates
                    // to `0`, so every range became `start..=0` and a range
                    // starting at zero yielded `[0]`: a page index into an
                    // empty document, handed to a caller with every reason to
                    // trust it. `All` and `CurrentPage` both return nothing
                    // there; this arm was the odd one out. Saying the
                    // precondition outright is better than relying on an empty
                    // `RangeInclusive` to express it, because that reasoning is
                    // exactly what stopped holding at zero pages.
                    .filter(|&&(start, _)| start < page_count)
                    .flat_map(|&(start, end)| start..=end.min(page_count.saturating_sub(1)))
                    .collect();
                // Sort-then-dedup rather than a `contains` check per page: the
                // list is sorted on the way out regardless, and the linear scan
                // made overlapping ranges over a long document quadratic.
                pages.sort_unstable();
                pages.dedup();
                pages
            }
        }
    }

    /// Parse a user-entered page range string (e.g. "1-3, 5, 7-9").
    ///
    /// Returns 0-based, inclusive ranges. Ranges that begin past the end of the
    /// document are dropped; ranges that merely *extend* past it are kept whole
    /// and clamped later by [`Self::resolve_pages`], which is the only place
    /// that knows the page count at the moment of printing rather than at the
    /// moment of typing. Clamping in both places is the same bound written
    /// twice, and the two copies disagreed: this one used to clamp the range's
    /// *start* as well, which turned "50-60" of a ten-page document into
    /// "page 10" instead of into nothing.
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
                // Convert from 1-based to 0-based.
                let s = start.saturating_sub(1);
                let e = end.saturating_sub(1);
                if s <= e && s < page_count {
                    ranges.push((s, e));
                }
            } else if let Ok(n) = part.parse::<usize>() {
                // `checked_sub` rather than a `n >= 1` guard: page "0" does not
                // exist in the 1-based numbering the user is typing in, and
                // failing the subtraction *is* that case.
                if let Some(idx) = n.checked_sub(1)
                    && idx < page_count
                {
                    ranges.push((idx, idx));
                }
            }
        }
        // `Custom(vec![])`, deliberately, and not `All`.
        //
        // Blank input and the word "all" are already handled above, so reaching
        // here means the user named specific pages. If none of them exist, the
        // honest answer is "no pages", which prints nothing and shows as such in
        // the dialog. Falling back to `All` here meant that typing "50-60" into
        // a ten-page document -- a plain typo -- printed the entire document.
        // Of the two ways to be wrong about a range nobody asked for, printing
        // everything is the expensive one.
        if ranges.is_empty() {
            PrintPageRange::Custom(Vec::new())
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

    /// The index of the last page, or `None` when no document is loaded.
    ///
    /// Every navigation method below is a clamp against this, and each of them
    /// used to write the clamp out itself as `if count > 0 { … count - 1 }` --
    /// the emptiness test and the subtraction stated separately, four times
    /// over, when `checked_sub(1)` failing *is* the empty case.
    fn last_page_index(&self) -> Option<usize> {
        self.page_count().checked_sub(1)
    }

    /// Navigate to a specific page.
    pub fn go_to_page(&mut self, page: usize) {
        if let Some(last) = self.last_page_index() {
            self.current_page = page.min(last);
        }
    }

    /// Go to the next page.
    pub fn next_page(&mut self) {
        if let Some(last) = self.last_page_index()
            && self.current_page < last
        {
            self.current_page = self.current_page.saturating_add(1);
        }
    }

    /// Go to the previous page.
    pub fn prev_page(&mut self) {
        self.current_page = self.current_page.saturating_sub(1);
    }

    /// Go to the first page.
    pub fn first_page(&mut self) {
        self.current_page = 0;
    }

    /// Go to the last page.
    pub fn last_page(&mut self) {
        if let Some(last) = self.last_page_index() {
            self.current_page = last;
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

    /// Navigate to the next search result, wrapping at the end.
    ///
    /// `checked_sub(1)` failing on the length *is* the empty-list case, so the
    /// emptiness test and the last-index it guards are one expression rather
    /// than a separate `is_empty` guard that a later edit could move away from
    /// the arithmetic it protects.
    pub fn next_match(&mut self) {
        let Some(last) = self.results.len().checked_sub(1) else {
            return;
        };
        // `i >= last` rather than `i == last`: a cursor left over from a search
        // that returned more hits than this one wraps to the start instead of
        // stepping to an index the list does not reach.
        self.current_match = Some(match self.current_match {
            None => 0,
            Some(i) if i >= last => 0,
            Some(i) => i.saturating_add(1),
        });
    }

    /// Navigate to the previous search result, wrapping at the start.
    pub fn prev_match(&mut self) {
        let Some(last) = self.results.len().checked_sub(1) else {
            return;
        };
        self.current_match = Some(match self.current_match {
            None => 0,
            Some(0) => last,
            Some(i) => i.saturating_sub(1).min(last),
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
                Some(i) => format!("{} of {}", i.saturating_add(1), self.results.len()),
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
        // Saturating rather than wrapping: a wrapped id would collide with a
        // live tab and make "close tab 3" close a different one. Sticking at
        // `u64::MAX` hands out one id forever instead, which is a stuck feature
        // rather than a mix-up -- and 2^64 tabs is a proof it cannot happen,
        // not a case anyone will meet.
        self.next = self.next.saturating_add(1);
        id
    }
}

impl Default for IdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// Turn a path into a document, if something in this build knows how.
///
/// Nothing does yet -- there is no PDF parser in the tree -- so the field
/// holding one of these is `None` in a shipped build, and the controls that
/// would need it are drawn disabled and record no hit box. See `known-issues.md`
/// -> `C-PDFVIEWER-HAS-NO-PDF-BACKEND`.
///
/// The seam exists rather than the call being written inline and stubbed out
/// because a stub is a lie the type system stops checking: the moment
/// `open_recent` "succeeds" by conjuring a sample document, every test above it
/// is testing the conjurer. A `None` opener makes the absence a value the
/// renderer can *see*, which is what keeps the button honest.
pub type OpenFn = fn(&Path) -> Option<PdfDocument>;

/// Send the named pages of a document to a spooler, answering whether it took.
///
/// `None` for the same reason as [`OpenFn`]: there is no print service to talk
/// to, and a Print button that silently does nothing is worse than one that is
/// visibly greyed out.
pub type PrintFn = fn(&PdfDocument, &[usize]) -> bool;

/// The complete PDF viewer application state.
pub struct PdfViewerApp {
    pub tabs: Vec<DocumentTab>,
    pub active_tab: usize,
    pub search: SearchState,
    /// Whether keystrokes go to the search query rather than the document.
    ///
    /// Distinct from `search.active`, which is whether the bar is *shown*: a
    /// click on the page leaves the bar up (so the match count stays readable)
    /// but takes the caret away, exactly as a browser's find bar does.
    pub search_focused: bool,
    pub recent_files: RecentFilesList,
    pub print_settings: PrintSettings,
    pub dark_mode: bool,
    pub window_width: f32,
    pub window_height: f32,
    pub id_gen: IdGenerator,
    pub next_annotation_id: u64,
    /// See [`OpenFn`]. `None` in a shipped build.
    open: Option<OpenFn>,
    /// See [`PrintFn`]. `None` in a shipped build.
    print: Option<PrintFn>,
    /// Fractional wheel notches not yet worth a scroll step.
    ///
    /// A touchpad delivers a stream of small fractions; dropping each one
    /// because it rounds to zero rows makes a slow drag scroll nothing at all,
    /// so the remainder is carried between events.
    wheel: wheel::Accumulator,
}

impl std::fmt::Debug for PdfViewerApp {
    // Hand-written because `fn` pointers are not `Debug`, and the two seams are
    // not state anyone debugging this wants printed -- only whether they are
    // wired at all, which is what the two booleans say.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PdfViewerApp")
            .field("tabs", &self.tabs)
            .field("active_tab", &self.active_tab)
            .field("search", &self.search)
            .field("search_focused", &self.search_focused)
            .field("recent_files", &self.recent_files)
            .field("print_settings", &self.print_settings)
            .field("dark_mode", &self.dark_mode)
            .field("window_width", &self.window_width)
            .field("window_height", &self.window_height)
            .field("id_gen", &self.id_gen)
            .field("next_annotation_id", &self.next_annotation_id)
            .field("can_open", &self.open.is_some())
            .field("can_print", &self.print.is_some())
            .field("wheel", &self.wheel)
            .finish()
    }
}

impl PdfViewerApp {
    pub fn new(width: f32, height: f32) -> Self {
        let mut id_gen = IdGenerator::new();
        let initial_tab = DocumentTab::new(id_gen.next_id());
        Self {
            tabs: vec![initial_tab],
            active_tab: 0,
            search: SearchState::new(),
            search_focused: false,
            recent_files: RecentFilesList::default(),
            print_settings: PrintSettings::default(),
            dark_mode: true,
            window_width: width,
            window_height: height,
            id_gen,
            next_annotation_id: 1,
            open: None,
            print: None,
            wheel: wheel::Accumulator::default(),
        }
    }

    /// Install the thing that turns a path into a document. See [`OpenFn`].
    pub fn set_opener(&mut self, open: OpenFn) {
        self.open = Some(open);
    }

    /// Install the thing that spools pages. See [`PrintFn`].
    pub fn set_printer(&mut self, print: PrintFn) {
        self.print = Some(print);
    }

    /// Whether a document can be opened at all in this build.
    #[must_use]
    pub fn can_open(&self) -> bool {
        self.open.is_some()
    }

    /// Whether the active tab holds something a printer could take.
    #[must_use]
    pub fn can_print(&self) -> bool {
        self.print.is_some() && self.active_tab().is_some_and(|t| t.document.is_some())
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
        // The push guarantees a last element, so the saturation is unreachable
        // -- but it states the bound once instead of asserting non-emptiness a
        // line after relying on it.
        self.active_tab = self.tabs.len().saturating_sub(1);
        id
    }

    /// Close a tab by index.
    pub fn close_tab(&mut self, index: usize) {
        if self.tabs.len() <= 1 {
            return; // Keep at least one tab
        }
        if index < self.tabs.len() {
            self.tabs.remove(index);
            // Order matters and is not obvious: a tab to the *left* of the
            // active one shifts the active one down by one, whereas closing the
            // last tab leaves the index past the end. Checking the past-the-end
            // case first means the shift never needs to consider it.
            if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len().saturating_sub(1);
            } else if self.active_tab > index {
                self.active_tab = self.active_tab.saturating_sub(1);
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

    /// Attach an annotation to the current page of the active tab.
    ///
    /// The one place that adds an annotation, because the three public wrappers
    /// below differ only in which [`AnnotationType`] they build. Written out
    /// three times, they each took an id from the counter *before* looking for
    /// a page to put it on, so every call on a tab with no document -- an empty
    /// tab, or a click that arrived while one was closing -- returned `None`
    /// having already burnt an id. The ids stayed unique, so nothing broke
    /// visibly; they simply developed gaps, which is the kind of thing that is
    /// only ever noticed by whoever later assumes they are dense.
    ///
    /// Here the id is allocated after the page is in hand, so a failure costs
    /// nothing.
    fn add_annotation(&mut self, rect: PageRect, annotation_type: AnnotationType) -> Option<u64> {
        let ann_id = self.next_annotation_id;
        let tab = self.active_tab_mut()?;
        let page_idx = tab.current_page;
        let page = tab.document.as_mut()?.pages.get_mut(page_idx)?;
        page.annotations.push(Annotation {
            id: ann_id,
            page_index: page_idx,
            rect,
            annotation_type,
            author: String::new(),
            created_timestamp: 0,
        });
        // Saturating rather than wrapping: an id that wraps would collide with
        // a live annotation and make `remove_annotation` delete the wrong one.
        // Sticking at `u64::MAX` instead refuses to issue new ids, which is a
        // stuck feature rather than a corrupted document -- and it takes 2^64
        // annotations to reach, so it is a proof of impossibility rather than a
        // behaviour anyone will meet.
        self.next_annotation_id = self.next_annotation_id.saturating_add(1);
        Some(ann_id)
    }

    /// Add a highlight annotation to the current page of the active tab.
    pub fn add_highlight(&mut self, rect: PageRect, color: Color) -> Option<u64> {
        self.add_annotation(rect, AnnotationType::Highlight { color })
    }

    /// Add a sticky note annotation.
    pub fn add_note(&mut self, rect: PageRect, content: String) -> Option<u64> {
        self.add_annotation(rect, AnnotationType::Note { content })
    }

    /// Add a freehand annotation.
    pub fn add_freehand(
        &mut self,
        rect: PageRect,
        points: Vec<(f32, f32)>,
        color: Color,
        width: f32,
    ) -> Option<u64> {
        self.add_annotation(
            rect,
            AnnotationType::Freehand {
                points,
                color,
                width,
            },
        )
    }

    /// Remove an annotation by id from the active tab's current page.
    pub fn remove_annotation(&mut self, annotation_id: u64) -> bool {
        if let Some(tab) = self.active_tab_mut() {
            let page_idx = tab.current_page;
            if let Some(doc) = &mut tab.document
                && let Some(page) = doc.pages.get_mut(page_idx)
            {
                let before = page.annotations.len();
                page.annotations.retain(|a| a.id != annotation_id);
                return page.annotations.len() < before;
            }
        }
        false
    }

    /// Compute the content area dimensions (accounting for toolbar, tabs, status, sidebar).
    ///
    /// Derived from [`Layout`] rather than re-summing the band heights, so
    /// there is one description of where the document area is and callers of
    /// this cannot drift away from what the renderer drew.
    pub fn content_area(&self) -> (f32, f32, f32, f32) {
        let c = self.layout(self.window_width, self.window_height).content;
        (c.x, c.y, c.w, c.h)
    }

    /// The bands of the window at `width` x `height`, with the sidebar's
    /// visibility taken from the active tab.
    fn layout(&self, width: f32, height: f32) -> Layout {
        Layout::new(
            width,
            height,
            self.active_tab().is_some_and(|t| t.sidebar_visible),
        )
    }

    /// Draw the whole window at `width` x `height`, recording as it goes where
    /// every control ended up.
    ///
    /// The size is an argument and not a field on purpose. A layout cached from
    /// the last resize is wrong for exactly one frame -- the first one after the
    /// window changes size -- and that is the frame in which a click lands on
    /// whatever used to be there.
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let layout = self.layout(width, height);
        let mut frame = Frame::new(layout.window.w, layout.window.h);

        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: layout.window.w,
            height: layout.window.h,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_toolbar(&mut frame, &layout);
        self.render_tab_bar(&mut frame, &layout);

        if let Some(tab) = self.active_tab() {
            if tab.sidebar_visible {
                self.render_sidebar(&mut frame, &layout, tab);
            }
            self.render_document_area(&mut frame, &layout, tab);
        }

        self.render_status_bar(&mut frame, &layout);

        if self.search.active {
            self.render_search_bar(&mut frame, &layout);
        }

        frame
    }

    /// Render the toolbar.
    fn render_toolbar(&self, frame: &mut Frame, layout: &Layout) {
        let bar = layout.toolbar;

        // Toolbar background
        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: bar.w,
            height: bar.h,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Bottom border
        frame.push(RenderCommand::Line {
            x1: 0.0,
            y1: bar.bottom(),
            x2: bar.w,
            y2: bar.bottom(),
            color: SURFACE0,
            width: 1.0,
        });

        // Clipped to the toolbar band, which is what makes the buttons in a
        // window too short to have a toolbar not merely invisible but
        // unclickable: `Frame::hit` trims each box to the innermost clip and
        // drops what is left of nothing.
        frame.clip(bar);

        let mut btn_x: f32 = 8.0;
        let btn_y: f32 = 6.0;
        let btn_h: f32 = 32.0;

        // Navigation buttons
        let nav_buttons = [
            ("<<", Nav::First),
            ("<", Nav::Prev),
            (">", Nav::Next),
            (">>", Nav::Last),
        ];
        for (label, which) in &nav_buttons {
            let btn_w: f32 = 36.0;
            self.render_toolbar_button(
                frame,
                btn_x,
                btn_y,
                btn_w,
                btn_h,
                label,
                Target::Nav(*which),
            );
            btn_x += btn_w + 4.0;
        }

        btn_x += 8.0;

        // Page indicator
        if let Some(tab) = self.active_tab() {
            let page_text = format!(
                "Page {} / {}",
                tab.current_page.saturating_add(1),
                tab.page_count()
            );
            frame.push(RenderCommand::Text {
                x: btn_x,
                y: btn_y + 9.0,
                text: page_text,
                color: TEXT_COLOR,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(120.0),
                overflow: TextOverflow::Ellipsis,
            });
            btn_x += 128.0;
        }

        // Separator
        frame.push(RenderCommand::Line {
            x1: btn_x,
            y1: btn_y + 2.0,
            x2: btn_x,
            y2: btn_y + btn_h - 2.0,
            color: SURFACE1,
            width: 1.0,
        });
        btn_x += 12.0;

        // Zoom buttons
        let zoom_buttons = [("-", Target::ZoomOut), ("+", Target::ZoomIn)];
        for (label, target) in &zoom_buttons {
            let btn_w: f32 = 32.0;
            self.render_toolbar_button(frame, btn_x, btn_y, btn_w, btn_h, label, *target);
            btn_x += btn_w + 4.0;
        }

        // Zoom indicator
        if let Some(tab) = self.active_tab() {
            let (vw, vh) = (layout.content.w, layout.content.h);
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
            frame.push(RenderCommand::Text {
                x: btn_x,
                y: btn_y + 9.0,
                text: zoom_label,
                color: SUBTEXT1,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(140.0),
                overflow: TextOverflow::Ellipsis,
            });
            btn_x += 148.0;
        }

        // Separator
        frame.push(RenderCommand::Line {
            x1: btn_x,
            y1: btn_y + 2.0,
            x2: btn_x,
            y2: btn_y + btn_h - 2.0,
            color: SURFACE1,
            width: 1.0,
        });
        btn_x += 12.0;

        // Fit buttons
        let fit_buttons = [
            ("FW", Target::Fit(Fit::Width)),
            ("FP", Target::Fit(Fit::Page)),
        ];
        for (label, target) in &fit_buttons {
            let btn_w: f32 = 36.0;
            self.render_toolbar_button(frame, btn_x, btn_y, btn_w, btn_h, label, *target);
            btn_x += btn_w + 4.0;
        }

        btn_x += 8.0;

        // Rotation buttons
        let rot_buttons = [("CCW", Target::RotateCcw), ("CW", Target::RotateCw)];
        for (label, target) in &rot_buttons {
            let btn_w: f32 = 40.0;
            self.render_toolbar_button(frame, btn_x, btn_y, btn_w, btn_h, label, *target);
            btn_x += btn_w + 4.0;
        }

        btn_x += 8.0;

        // View mode button
        let vm_label = match self.active_tab().map(|t| t.view_mode) {
            Some(ViewMode::SinglePage) => "1pg",
            Some(ViewMode::ContinuousScroll) => "Scr",
            None => "1pg",
        };
        self.render_toolbar_button(
            frame,
            btn_x,
            btn_y,
            36.0,
            btn_h,
            vm_label,
            Target::ViewModeToggle,
        );
        btn_x += 44.0;

        // Sidebar toggle
        self.render_toolbar_button(
            frame,
            btn_x,
            btn_y,
            36.0,
            btn_h,
            "SB",
            Target::SidebarToggle,
        );

        // Right-side buttons (search, print). Positioned from the right edge, so
        // in a window narrow enough for them to collide with the row above they
        // are drawn last and therefore win the hit test -- which is the right
        // answer, since they are the ones still fully visible.
        let right_x = bar.w - 90.0;
        self.render_toolbar_button(
            frame,
            right_x,
            btn_y,
            36.0,
            btn_h,
            "Srch",
            Target::SearchToggle,
        );
        // Print is only a control when something can be printed. With no
        // spooler wired (the shipped case, see `OpenFn`) or no document in the
        // tab, it is drawn greyed and records no target -- so a click lands on
        // whatever is behind it, which is nothing, rather than on a button that
        // takes the press and swallows it.
        if self.can_print() {
            self.render_toolbar_button(
                frame,
                right_x + 44.0,
                btn_y,
                36.0,
                btn_h,
                "Prt",
                Target::Print,
            );
        } else {
            self.render_disabled_button(frame, right_x + 44.0, btn_y, 36.0, btn_h, "Prt");
        }

        frame.unclip();
    }

    /// Render a toolbar button that cannot be pressed, recording no hit box.
    ///
    /// Deliberately not `render_toolbar_button` with a flag: the enabled path
    /// *always* records a target and the disabled path *never* does, and a
    /// boolean parameter is how that invariant becomes a runtime question
    /// somebody eventually gets backwards.
    fn render_disabled_button(
        &self,
        frame: &mut Frame,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        label: &str,
    ) {
        frame.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: MANTLE,
            corner_radii: CornerRadii::all(4.0),
        });
        frame.push(RenderCommand::StrokeRect {
            x,
            y,
            width: w,
            height: h,
            color: SURFACE0,
            line_width: 1.0,
            corner_radii: CornerRadii::all(4.0),
        });
        frame.push(RenderCommand::Text {
            x: x + 4.0,
            y: y + (h - 12.0) / 2.0,
            text: label.to_string(),
            color: OVERLAY0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(w - 8.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// Render a toolbar button, and record that `target` is what a click inside
    /// it means.
    ///
    /// The hit box is the button's own rectangle, taken from the same
    /// parameters the fill uses -- not recomputed. That is the entire reason
    /// this takes a target at all rather than the caller recording the box:
    /// there is no arithmetic here for the two to disagree about.
    // self + frame + rect (x,y,w,h) + label + target. Grouping the rect into a
    // struct would not read better at eighteen call sites.
    #[allow(clippy::too_many_arguments)]
    fn render_toolbar_button(
        &self,
        frame: &mut Frame,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        label: &str,
        target: Target,
    ) {
        frame.hit(target, Rect::new(x, y, w, h));
        frame.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        frame.push(RenderCommand::StrokeRect {
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
        frame.push(RenderCommand::Text {
            x: text_x,
            y: text_y,
            text: label.to_string(),
            color: TEXT_COLOR,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(w - 8.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// Render the tab bar.
    fn render_tab_bar(&self, frame: &mut Frame, layout: &Layout) {
        let strip = layout.tab_bar;
        let y = strip.y;

        // Tab bar background
        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: strip.w,
            height: strip.h,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        frame.push(RenderCommand::Line {
            x1: 0.0,
            y1: strip.bottom(),
            x2: strip.w,
            y2: strip.bottom(),
            color: SURFACE0,
            width: 1.0,
        });

        // Clipped to the strip, so tabs that have run off the right-hand edge
        // are unclickable rather than clickable-but-invisible.
        frame.clip(strip);

        let mut tab_x: f32 = 4.0;
        let tab_w: f32 = 180.0;
        let tab_h: f32 = TAB_BAR_HEIGHT - 4.0;

        for (i, tab) in self.tabs.iter().enumerate() {
            let is_active = i == self.active_tab;
            let bg = if is_active { BASE } else { CRUST };
            let fg = if is_active { TEXT_COLOR } else { SUBTEXT0 };

            frame.hit(Target::Tab(i), Rect::new(tab_x, y + 2.0, tab_w, tab_h));

            // Tab background
            frame.push(RenderCommand::FillRect {
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
                frame.push(RenderCommand::Line {
                    x1: tab_x,
                    y1: y + 2.0,
                    x2: tab_x + tab_w,
                    y2: y + 2.0,
                    color: BLUE,
                    width: 2.0,
                });
            }

            // Tab title. Elided by measured width, not by byte index: the
            // title comes from the PDF's own `/Title` metadata or from the
            // file name, so it is document-controlled and routinely non-ASCII.
            // `&title[..17]` aborts whenever byte 17 lands inside a multi-byte
            // character, and the `len() > 20` guard made that *more* likely —
            // a seven-character Japanese title is 21 bytes and so always took
            // the truncating branch.
            //
            // The room is measured to the close button rather than to the tab
            // edge: the title starts `TAB_TEXT_INSET` in and the `x` glyph
            // starts `TAB_CLOSE_INSET` from the far edge, so anything wider
            // than the difference runs underneath it.
            let title_room = (tab_w - TAB_TEXT_INSET - TAB_CLOSE_INSET).max(0.0);
            let display_title = text::elide(
                &tab.title(),
                title_room,
                "...",
                TAB_TITLE_SIZE,
                FontWeightHint::Regular,
            );
            frame.push(RenderCommand::Text {
                x: tab_x + TAB_TEXT_INSET,
                y: y + 10.0,
                text: display_title,
                color: fg,
                font_size: TAB_TITLE_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(title_room),
                overflow: TextOverflow::Ellipsis,
            });

            // Close button (x) on each tab. Recorded *after* the tab, because
            // `hit_test` answers with the last box containing the point: where
            // the cross overlaps the tab it sits on, closing wins over
            // selecting, which is what a user aiming at a cross means.
            //
            // The box is a finger-sized square around the glyph rather than the
            // glyph's own ink -- an 11px `x` is about six pixels wide, which is
            // not a target anyone can hit.
            let close_x = tab_x + tab_w - TAB_CLOSE_INSET;
            let close_y = y + 10.0;
            frame.hit(
                Target::TabClose(i),
                Rect::new(close_x - 5.0, y + 6.0, 20.0, 20.0),
            );
            frame.push(RenderCommand::Text {
                x: close_x,
                y: close_y,
                text: "x".to_string(),
                color: OVERLAY0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            tab_x += tab_w + 2.0;
        }

        // New tab button (+)
        frame.hit(Target::NewTab, Rect::new(tab_x, y + 6.0, 28.0, 24.0));
        frame.push(RenderCommand::FillRect {
            x: tab_x,
            y: y + 6.0,
            width: 28.0,
            height: 24.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        frame.push(RenderCommand::Text {
            x: tab_x + 8.0,
            y: y + 10.0,
            text: "+".to_string(),
            color: SUBTEXT1,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        frame.unclip();
    }

    /// Render the sidebar.
    ///
    /// Everything here is measured from `layout.sidebar`, which is empty when
    /// the sidebar is hidden or the window is too small to hold it. That is
    /// what makes the panel tabs stop being clickable in those cases without a
    /// second visibility test: the `frame.clip` below trims every hit box to
    /// the band, and [`Frame::hit`] drops a box that trims to nothing.
    fn render_sidebar(&self, frame: &mut Frame, layout: &Layout, tab: &DocumentTab) {
        let band = layout.sidebar;
        let sidebar_y = band.y;
        let sidebar_h = band.h;
        let sidebar_w = band.w;

        frame.clip(band);

        // Sidebar background
        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y: sidebar_y,
            width: sidebar_w,
            height: sidebar_h,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Right border
        frame.push(RenderCommand::Line {
            x1: sidebar_w,
            y1: sidebar_y,
            x2: sidebar_w,
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
        let panel_tab_w = sidebar_w / panels.len() as f32;
        for (i, (panel, label)) in panels.iter().enumerate() {
            let px = i as f32 * panel_tab_w;
            let is_active = tab.sidebar_panel == *panel;
            let bg = if is_active { BASE } else { MANTLE };
            let fg = if is_active { BLUE } else { SUBTEXT0 };

            frame.hit(
                Target::Panel(*panel),
                Rect::new(px, sidebar_y, panel_tab_w, 28.0),
            );

            frame.push(RenderCommand::FillRect {
                x: px,
                y: sidebar_y,
                width: panel_tab_w,
                height: 28.0,
                color: bg,
                corner_radii: CornerRadii::ZERO,
            });

            frame.push(RenderCommand::Text {
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
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Panel content
        let content_y = sidebar_y + 32.0;
        let content_h = (sidebar_h - 32.0).max(0.0);

        frame.clip(Rect::new(0.0, content_y, sidebar_w, content_h));

        match tab.sidebar_panel {
            SidebarPanel::Thumbnails => {
                self.render_thumbnail_strip(frame, tab, content_y, content_h);
            }
            SidebarPanel::Bookmarks => {
                self.render_bookmarks_panel(frame, tab, content_y, content_h);
            }
            SidebarPanel::Annotations => {
                self.render_annotations_panel(frame, tab, content_y, content_h);
            }
        }

        frame.unclip();
        frame.unclip();
    }

    /// Render the thumbnail strip in the sidebar.
    fn render_thumbnail_strip(
        &self,
        frame: &mut Frame,
        tab: &DocumentTab,
        start_y: f32,
        _height: f32,
    ) {
        let Some(doc) = &tab.document else {
            frame.push(RenderCommand::Text {
                x: 16.0,
                y: start_y + 20.0,
                text: "No document loaded".to_string(),
                color: SUBTEXT0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - 32.0),
                overflow: TextOverflow::Ellipsis,
            });
            return;
        };

        let thumb_w = SIDEBAR_WIDTH - 32.0;
        let thumb_h = THUMBNAIL_HEIGHT;
        let mut y = start_y + 8.0;

        for (i, _page) in doc.pages.iter().enumerate() {
            let is_current = i == tab.current_page;

            // The whole cell -- page image *and* its number label -- is the
            // target, because the label is what a user reads to decide which
            // page they want, and a strip where the words below a thumbnail do
            // nothing is a strip where half the aimed-at pixels are dead.
            frame.hit(
                Target::Thumbnail(i),
                Rect::new(14.0, y - 2.0, thumb_w + 4.0, thumb_h + 22.0),
            );

            // Thumbnail border highlight
            if is_current {
                frame.push(RenderCommand::StrokeRect {
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
            frame.push(RenderCommand::FillRect {
                x: 16.0,
                y,
                width: thumb_w,
                height: thumb_h,
                color: Color::rgb(240, 240, 240),
                corner_radii: CornerRadii::all(2.0),
            });

            // Page number label below thumbnail
            let label = doc.page_label(i);
            frame.push(RenderCommand::Text {
                x: 16.0,
                y: y + thumb_h + 2.0,
                text: label,
                color: if is_current { TEXT_COLOR } else { SUBTEXT0 },
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(thumb_w),
                overflow: TextOverflow::Ellipsis,
            });

            y += thumb_h + 24.0;
        }
    }

    /// Render the bookmarks/outline panel.
    fn render_bookmarks_panel(
        &self,
        frame: &mut Frame,
        tab: &DocumentTab,
        start_y: f32,
        _height: f32,
    ) {
        let Some(doc) = &tab.document else {
            frame.push(RenderCommand::Text {
                x: 16.0,
                y: start_y + 20.0,
                text: "No document loaded".to_string(),
                color: SUBTEXT0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - 32.0),
                overflow: TextOverflow::Ellipsis,
            });
            return;
        };

        if doc.bookmarks.is_empty() {
            frame.push(RenderCommand::Text {
                x: 16.0,
                y: start_y + 20.0,
                text: "No bookmarks".to_string(),
                color: SUBTEXT0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - 32.0),
                overflow: TextOverflow::Ellipsis,
            });
            return;
        }

        let entries = doc.flatten_bookmarks();
        let mut y = start_y + 8.0;
        let line_h: f32 = 24.0;

        for (i, (depth, bm)) in entries.iter().enumerate() {
            let indent = 16.0 + (*depth as f32) * 16.0;
            let is_on_current_page = bm.page_index == tab.current_page;

            // The row spans the full sidebar width, not just the title's ink,
            // so the blank space to the right of a short title still jumps to
            // the page -- which is what every outline pane in every reader
            // does, and what a user aiming below a long title expects.
            frame.hit(
                Target::Bookmark(i),
                Rect::new(0.0, y, SIDEBAR_WIDTH, line_h),
            );

            // Expand/collapse indicator
            if !bm.children.is_empty() {
                let arrow = if bm.expanded { "v" } else { ">" };

                // Recorded *after* the row it sits inside, because `hit_test`
                // answers with the last box containing the point: on the
                // triangle, folding wins over navigating. A childless row
                // records no arrow at all, so its whole width navigates.
                frame.hit(
                    Target::BookmarkArrow(i),
                    Rect::new(indent - 16.0, y, 16.0, line_h),
                );

                frame.push(RenderCommand::Text {
                    x: indent - 12.0,
                    y: y + 4.0,
                    text: arrow.to_string(),
                    color: OVERLAY0,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }

            // Bookmark title
            frame.push(RenderCommand::Text {
                x: indent,
                y: y + 4.0,
                text: bm.title.clone(),
                color: if is_on_current_page { BLUE } else { TEXT_COLOR },
                font_size: 12.0,
                font_weight: if is_on_current_page {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(SIDEBAR_WIDTH - indent - 8.0),
                overflow: TextOverflow::Ellipsis,
            });

            y += line_h;
        }
    }

    /// Render the annotations panel.
    fn render_annotations_panel(
        &self,
        frame: &mut Frame,
        tab: &DocumentTab,
        start_y: f32,
        _height: f32,
    ) {
        let Some(doc) = &tab.document else {
            frame.push(RenderCommand::Text {
                x: 16.0,
                y: start_y + 20.0,
                text: "No document loaded".to_string(),
                color: SUBTEXT0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - 32.0),
                overflow: TextOverflow::Ellipsis,
            });
            return;
        };

        let annotations: Vec<&Annotation> = doc
            .pages
            .iter()
            .flat_map(|p| p.annotations.iter())
            .collect();

        if annotations.is_empty() {
            frame.push(RenderCommand::Text {
                x: 16.0,
                y: start_y + 20.0,
                text: "No annotations".to_string(),
                color: SUBTEXT0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - 32.0),
                overflow: TextOverflow::Ellipsis,
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
            frame.push(RenderCommand::FillRect {
                x: 16.0,
                y: y + 4.0,
                width: 8.0,
                height: 8.0,
                color: type_color,
                corner_radii: CornerRadii::all(4.0),
            });

            // Annotation type and page
            frame.push(RenderCommand::Text {
                x: 30.0,
                y: y + 2.0,
                text: format!("{} - Page {}", type_label, ann.page_index.saturating_add(1)),
                color: TEXT_COLOR,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - 46.0),
                overflow: TextOverflow::Ellipsis,
            });

            y += 22.0;
        }
    }

    /// Render the main document viewing area.
    fn render_document_area(&self, frame: &mut Frame, layout: &Layout, tab: &DocumentTab) {
        let area = layout.content;
        let (area_x, area_y, area_w, area_h) = (area.x, area.y, area.w, area.h);

        // Clip to content area
        frame.clip(area);

        // The page itself is one target covering the whole viewport, recorded
        // first so anything drawn into the area later (a recent-file link on
        // the welcome screen) overrides it. It exists so a click on the page
        // can take focus away from the search field -- not so it can navigate.
        frame.hit(Target::Document, area);

        // Dark background
        frame.push(RenderCommand::FillRect {
            x: area_x,
            y: area_y,
            width: area_w,
            height: area_h,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        let Some(doc) = &tab.document else {
            // No document — show welcome message
            self.render_welcome(frame, area_x, area_y, area_w, area_h);
            frame.unclip();
            return;
        };

        match tab.view_mode {
            ViewMode::SinglePage => {
                self.render_single_page(frame, doc, tab, area_x, area_y, area_w, area_h);
            }
            ViewMode::ContinuousScroll => {
                self.render_continuous_scroll(frame, doc, tab, area_x, area_y, area_w, area_h);
            }
        }

        frame.unclip();
    }

    /// Render welcome message when no document is loaded.
    fn render_welcome(&self, frame: &mut Frame, x: f32, y: f32, w: f32, h: f32) {
        let cx = x + w / 2.0 - 100.0;
        let cy = y + h / 2.0 - 60.0;

        frame.push(RenderCommand::Text {
            x: cx,
            y: cy,
            text: "PDF Viewer".to_string(),
            color: TEXT_COLOR,
            font_size: 24.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });

        frame.push(RenderCommand::Text {
            x: cx,
            y: cy + 36.0,
            text: "Open a PDF to begin".to_string(),
            color: SUBTEXT0,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Recent files
        if !self.recent_files.entries.is_empty() {
            frame.push(RenderCommand::Text {
                x: cx,
                y: cy + 72.0,
                text: "Recent Files:".to_string(),
                color: SUBTEXT1,
                font_size: 13.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(300.0),
                overflow: TextOverflow::Ellipsis,
            });

            let mut ry = cy + 94.0;
            for (i, entry) in self.recent_files.entries.iter().take(5).enumerate() {
                let name = entry
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "unknown".to_string());

                // A link only when this build can follow it. With no parser
                // wired there is no target and no link colour -- the list is
                // still shown, because "these are the files you had open" is
                // true and useful, but it does not pretend to be clickable.
                if self.can_open() {
                    // Recorded after `Target::Document`, which covers the whole
                    // viewport: `hit_test` takes the last match, so the link
                    // wins over the page beneath it.
                    frame.hit(Target::RecentFile(i), Rect::new(cx, ry - 2.0, 296.0, 20.0));
                }

                frame.push(RenderCommand::Text {
                    x: cx + 8.0,
                    y: ry,
                    text: format!("{}. {}", i.saturating_add(1), name),
                    color: if self.can_open() { BLUE } else { OVERLAY0 },
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(280.0),
                    overflow: TextOverflow::Ellipsis,
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
        frame: &mut Frame,
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

        self.render_page_box(
            frame,
            page,
            tab.current_page,
            page_x,
            page_y,
            rendered_w,
            rendered_h,
            zoom,
        );

        // Render search highlights on this page
        self.render_search_highlights(frame, tab.current_page, page_x, page_y, zoom);
    }

    /// Render continuous scroll mode.
    // Same shape as render_single_page; both are render driver entry points.
    #[allow(clippy::too_many_arguments)]
    fn render_continuous_scroll(
        &self,
        frame: &mut Frame,
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
        let zoom = tab
            .zoom
            .effective_zoom(area_w, area_h, ref_page_w, ref_page_h);

        let mut y_offset = area_y + PAGE_MARGIN - tab.scroll_offset_y;

        for (i, page) in doc.pages.iter().enumerate() {
            let pw = page.display_width() * zoom;
            let ph = page.display_height() * zoom;

            // Only render pages that are visible
            if y_offset + ph >= area_y && y_offset <= area_y + area_h {
                let page_x = area_x + (area_w - pw) / 2.0;
                self.render_page_box(frame, page, i, page_x, y_offset, pw, ph, zoom);
                self.render_search_highlights(frame, i, page_x, y_offset, zoom);
            }

            y_offset += ph + PAGE_GAP;
        }
    }

    /// Render a page box with shadow, background, and content.
    // self + tree + page model + page index + rect (x,y,w,h) + zoom; all needed.
    #[allow(clippy::too_many_arguments)]
    fn render_page_box(
        &self,
        frame: &mut Frame,
        page: &PdfPage,
        _page_index: usize,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        zoom: f32,
    ) {
        // Page shadow
        frame.push(RenderCommand::BoxShadow {
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
        frame.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: page_bg,
            corner_radii: CornerRadii::all(2.0),
        });

        // Page border
        frame.push(RenderCommand::StrokeRect {
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

            frame.push(RenderCommand::Text {
                x: sx,
                y: sy,
                text: span.text.clone(),
                color: text_color,
                font_size: font_sz,
                font_weight: FontWeightHint::Regular,
                max_width: Some(max_w),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Render annotations
        for ann in &page.annotations {
            self.render_annotation(frame, ann, x, y, zoom);
        }
    }

    /// Render an annotation overlay on a page.
    fn render_annotation(
        &self,
        frame: &mut Frame,
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
                frame.push(RenderCommand::FillRect {
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
                frame.push(RenderCommand::FillRect {
                    x: ax,
                    y: ay,
                    width: 20.0 * zoom,
                    height: 20.0 * zoom,
                    color: YELLOW,
                    corner_radii: CornerRadii::all(3.0),
                });
                frame.push(RenderCommand::Text {
                    x: ax + 3.0 * zoom,
                    y: ay + 3.0 * zoom,
                    text: "N".to_string(),
                    color: CRUST,
                    font_size: 12.0 * zoom,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }
            AnnotationType::Freehand {
                points,
                color,
                width,
            } => {
                // Draw line segments between points
                for pair in points.windows(2) {
                    if let [p1, p2] = pair {
                        frame.push(RenderCommand::Line {
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
                frame.push(RenderCommand::Line {
                    x1: ax,
                    y1: ay + ah,
                    x2: ax + aw,
                    y2: ay + ah,
                    color: *color,
                    width: 1.5 * zoom,
                });
            }
            AnnotationType::Strikethrough { color } => {
                frame.push(RenderCommand::Line {
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
        frame: &mut Frame,
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

            frame.push(RenderCommand::FillRect {
                x: hx,
                y: hy,
                width: hw,
                height: hh,
                color,
                corner_radii: CornerRadii::all(2.0),
            });

            if is_current {
                frame.push(RenderCommand::StrokeRect {
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
    ///
    /// This floats over the document rather than displacing it, and it is drawn
    /// last, so its hit boxes are recorded after the ones underneath and win
    /// the overlap -- but only where it actually covers them. A click beside
    /// the bar still reaches the page, which is what an overlay (as opposed to
    /// a modal) means.
    fn render_search_bar(&self, frame: &mut Frame, layout: &Layout) {
        let bar_w: f32 = 360.0_f32.min(layout.window.w);
        let bar_h: f32 = 44.0;
        let bar_x = (layout.window.w - bar_w - 16.0).max(0.0);
        let bar_y = layout.content.y + 8.0;

        // A window too short to hold the bar below the tab strip gets no search
        // bar at all rather than one drawn over the status bar: the clip trims
        // its boxes to the content band, and `Frame::hit` drops what is left of
        // nothing.
        frame.clip(layout.content);

        // Shadow
        frame.push(RenderCommand::BoxShadow {
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
        frame.push(RenderCommand::FillRect {
            x: bar_x,
            y: bar_y,
            width: bar_w,
            height: bar_h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });

        // The focus ring is the only thing that distinguishes "typing goes
        // here" from "typing pages the document", and both states are reachable
        // with the bar on screen, so it has to be visible rather than implied.
        frame.push(RenderCommand::StrokeRect {
            x: bar_x,
            y: bar_y,
            width: bar_w,
            height: bar_h,
            color: if self.search_focused { BLUE } else { SURFACE1 },
            line_width: if self.search_focused { 2.0 } else { 1.0 },
            corner_radii: CornerRadii::all(8.0),
        });

        // The field is everything left of the two nav buttons: the icon, the
        // query text and the match count all put the caret in the query, which
        // is the only thing a click in a search box can sensibly mean.
        frame.hit(
            Target::SearchField,
            Rect::new(bar_x, bar_y, (bar_w - 68.0).max(0.0), bar_h),
        );

        // Search icon placeholder
        frame.push(RenderCommand::Text {
            x: bar_x + 12.0,
            y: bar_y + 13.0,
            text: "S".to_string(),
            color: OVERLAY0,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
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
        frame.push(RenderCommand::Text {
            x: bar_x + 32.0,
            y: bar_y + 14.0,
            text: query_display,
            color: query_color,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(180.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Match count
        let count_label = self.search.match_count_label();
        if !count_label.is_empty() {
            frame.push(RenderCommand::Text {
                x: bar_x + 220.0,
                y: bar_y + 14.0,
                text: count_label,
                color: SUBTEXT0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(80.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Nav buttons (prev/next match)
        let btn_y = bar_y + 8.0;
        self.render_toolbar_button(
            frame,
            bar_x + bar_w - 64.0,
            btn_y,
            26.0,
            28.0,
            "<",
            Target::SearchPrev,
        );
        self.render_toolbar_button(
            frame,
            bar_x + bar_w - 34.0,
            btn_y,
            26.0,
            28.0,
            ">",
            Target::SearchNext,
        );

        frame.unclip();
    }

    /// Render the status bar.
    fn render_status_bar(&self, frame: &mut Frame, layout: &Layout) {
        let band = layout.status;
        let y = band.y;

        frame.clip(band);

        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: band.w,
            height: band.h,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Top border
        frame.push(RenderCommand::Line {
            x1: 0.0,
            y1: y,
            x2: band.w,
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
                frame.push(RenderCommand::Text {
                    x: sx,
                    y: y + 7.0,
                    text: name,
                    color: SUBTEXT1,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(200.0),
                    overflow: TextOverflow::Ellipsis,
                });
                sx += 210.0;
            }

            // Page info
            frame.push(RenderCommand::Text {
                x: sx,
                y: y + 7.0,
                text: format!(
                    "Page {} / {}",
                    tab.current_page.saturating_add(1),
                    tab.page_count()
                ),
                color: SUBTEXT0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
                overflow: TextOverflow::Ellipsis,
            });
            sx += 110.0;

            // View mode
            let mode_str = match tab.view_mode {
                ViewMode::SinglePage => "Single Page",
                ViewMode::ContinuousScroll => "Continuous",
            };
            frame.push(RenderCommand::Text {
                x: sx,
                y: y + 7.0,
                text: mode_str.to_string(),
                color: SUBTEXT0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
                overflow: TextOverflow::Ellipsis,
            });
            sx += 110.0;

            // Rotation
            if tab.rotation != Rotation::Deg0 {
                frame.push(RenderCommand::Text {
                    x: sx,
                    y: y + 7.0,
                    text: format!("{}deg", tab.rotation.degrees()),
                    color: SUBTEXT0,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(60.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            // Right side: zoom percentage
            let (vw, vh) = (layout.content.w, layout.content.h);
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
            frame.push(RenderCommand::Text {
                x: band.w - 80.0,
                y: y + 7.0,
                text: format!("{}%", zoom_pct as u32),
                color: SUBTEXT0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(60.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        frame.unclip();
    }

    // ========================================================================
    // Input
    // ========================================================================

    /// Remember the size the window last reported.
    ///
    /// The size is *only* stored here; every rectangle is recomputed from it on
    /// each frame. Nothing caches a [`Layout`], because a cached layout is a
    /// second copy of the geometry that a resize can leave stale -- which is
    /// how a click lands on where a button used to be.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.window_width = sane(width);
        self.window_height = sane(height);
    }

    /// What is under this point, according to the frame that was last drawn.
    ///
    /// Drawing a whole frame to answer a click looks wasteful and is not: it is
    /// the only way the answer cannot disagree with what the user sees, because
    /// it *is* what the user sees. The alternative -- a second pass that
    /// re-derives the toolbar's accumulated `btn_x` walk -- is the bug this
    /// design exists to make impossible.
    #[must_use]
    pub fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        self.frame(self.window_width, self.window_height)
            .hit_test(x, y)
    }

    /// Open the `index`-th recent file, answering whether a document arrived.
    ///
    /// `false` covers three different failures on purpose -- no opener wired,
    /// no such entry, the opener declined -- because every one of them means
    /// the same thing to the caller: the tab still holds what it held.
    pub fn open_recent(&mut self, index: usize) -> bool {
        let Some(open) = self.open else {
            return false;
        };
        let Some(entry) = self.recent_files.entries.get(index) else {
            return false;
        };
        // Cloned before the call because `open` may (and in a real build will)
        // want to hand back a document that borrows nothing from us, and
        // `load_document` needs `&mut self` while `entry` borrows `self`.
        let path = entry.path.clone();
        let Some(doc) = open(&path) else {
            return false;
        };
        self.load_document(doc);
        true
    }

    /// Spool the pages the print settings name, answering whether it took.
    pub fn print_active(&mut self) -> bool {
        let Some(print) = self.print else {
            return false;
        };
        let Some(tab) = self.active_tab() else {
            return false;
        };
        let Some(doc) = tab.document.as_ref() else {
            return false;
        };
        let pages = self
            .print_settings
            .resolve_pages(doc.page_count(), tab.current_page);
        if pages.is_empty() {
            return false;
        }
        print(doc, &pages)
    }

    /// Re-run the search against the active document.
    ///
    /// Called on every edit to the query rather than on Enter, because the
    /// match count in the bar is only truthful if it describes the text that is
    /// currently in the box.
    fn refresh_search(&mut self) {
        let Some(doc) = self.active_tab().and_then(|t| t.document.clone()) else {
            self.search.results.clear();
            self.search.current_match = None;
            return;
        };
        self.search.search(&doc);
        self.follow_current_match();
    }

    /// Move the view to the page holding the current search hit.
    ///
    /// A highlight the user cannot see is not a search result, so stepping
    /// through matches has to page the document as well as move the cursor.
    fn follow_current_match(&mut self) {
        let Some(i) = self.search.current_match else {
            return;
        };
        let Some(page) = self.search.results.get(i).map(|r| r.page_index) else {
            return;
        };
        self.go_to_page(page);
    }

    /// Navigate the active tab to a page, keeping continuous scroll in step.
    ///
    /// In continuous mode the page number is a *consequence* of the scroll
    /// offset -- the renderer lays pages out from `scroll_offset_y` and never
    /// reads `current_page` -- so setting the page without moving the offset
    /// changes the status bar and nothing else. This moves both.
    fn go_to_page(&mut self, page: usize) {
        let content = self.layout(self.window_width, self.window_height).content;
        let area = (content.w, content.h);
        let Some(tab) = self.tabs.get_mut(self.active_tab) else {
            return;
        };
        tab.go_to_page(page);
        if tab.view_mode == ViewMode::ContinuousScroll {
            Self::scroll_to_current_page(tab, area);
        }
    }

    /// Put the top of `tab.current_page` at the top of the viewport.
    fn scroll_to_current_page(tab: &mut DocumentTab, area: (f32, f32)) {
        let Some(doc) = tab.document.as_ref() else {
            return;
        };
        let zoom = Self::continuous_zoom(tab, doc, area);
        let mut y = PAGE_MARGIN;
        for page in doc.pages.iter().take(tab.current_page) {
            y += page.display_height() * zoom + PAGE_GAP;
        }
        // Clamped to the same extent a wheel scroll is: the last page of a
        // document cannot be dragged to the top of the window if that would
        // leave blank space below it, so "go to the last page" and "scroll to
        // the bottom" have to agree about where the bottom is.
        tab.scroll_offset_y = y.clamp(0.0, Self::max_scroll(doc, zoom, area.1));
    }

    /// The zoom continuous mode lays pages out at.
    ///
    /// This has to agree with `render_continuous_scroll` exactly -- it is the
    /// scale the scroll extent is measured in, so a disagreement of even a few
    /// percent makes the bottom stop land short of, or past, the last page. It
    /// is a separate function rather than a call into the renderer because the
    /// renderer's copy lives in a `&self` method and this is reached from the
    /// `&mut DocumentTab` paths; the two are written the same way on purpose.
    fn continuous_zoom(tab: &DocumentTab, doc: &PdfDocument, area: (f32, f32)) -> f32 {
        let ref_w = doc
            .pages
            .first()
            .map_or(DEFAULT_PAGE_WIDTH, |p| p.display_width());
        let ref_h = doc
            .pages
            .first()
            .map_or(DEFAULT_PAGE_HEIGHT, |p| p.display_height());
        tab.zoom.effective_zoom(area.0, area.1, ref_w, ref_h)
    }

    /// How far the document can scroll before its last page is fully shown.
    ///
    /// Zero when the whole document already fits, which is why this is a `max`
    /// rather than a subtraction: a document shorter than the window must not
    /// scroll *up* past its own top.
    fn max_scroll(doc: &PdfDocument, zoom: f32, area_h: f32) -> f32 {
        (total_document_height(doc, zoom) - area_h).max(0.0)
    }

    /// Handle a click on whatever `target_at` says is under the pointer.
    ///
    /// Answers whether anything changed, which is what lets the window skip a
    /// repaint for a click that landed on scenery.
    #[allow(clippy::too_many_lines)]
    pub fn handle_target(&mut self, target: Target) -> bool {
        // A click anywhere outside the search bar takes the caret out of it, so
        // that (say) pressing Right after clicking a thumbnail pages the
        // document instead of moving a caret the user has forgotten about.
        let keeps_focus = matches!(
            target,
            Target::SearchField | Target::SearchPrev | Target::SearchNext
        );
        let focus_changed = self.search_focused && !keeps_focus;
        if focus_changed {
            self.search_focused = false;
        }

        let acted = match target {
            Target::Nav(which) => {
                let Some(tab) = self.active_tab_mut() else {
                    return focus_changed;
                };
                match which {
                    Nav::First => tab.first_page(),
                    Nav::Prev => tab.prev_page(),
                    Nav::Next => tab.next_page(),
                    Nav::Last => tab.last_page(),
                }
                let page = tab.current_page;
                self.go_to_page(page);
                true
            }
            Target::ZoomOut => self.with_tab(DocumentTab::zoom_out),
            Target::ZoomIn => self.with_tab(DocumentTab::zoom_in),
            Target::Fit(Fit::Width) => self.set_zoom_mode(ZoomMode::FitWidth),
            Target::Fit(Fit::Page) => self.set_zoom_mode(ZoomMode::FitPage),
            Target::RotateCcw => self.with_tab(DocumentTab::rotate_ccw),
            Target::RotateCw => self.with_tab(DocumentTab::rotate_cw),
            Target::ViewModeToggle => self.with_tab(DocumentTab::toggle_view_mode),
            Target::SidebarToggle => self.with_tab(DocumentTab::toggle_sidebar),
            Target::SearchToggle => {
                self.search.active = !self.search.active;
                // Opening the bar focuses it -- a find bar you have to click
                // after summoning is a find bar that wasted the summon. Closing
                // it drops both the caret and the highlights, because leaving
                // highlights on screen with no bar to explain them is worse
                // than losing them.
                self.search_focused = self.search.active;
                if !self.search.active {
                    self.search.clear();
                }
                true
            }
            Target::Print => self.print_active(),
            Target::Tab(i) => {
                self.switch_tab(i);
                true
            }
            Target::TabClose(i) => {
                self.close_tab(i);
                true
            }
            Target::NewTab => {
                self.new_tab();
                true
            }
            Target::Panel(panel) => {
                if let Some(tab) = self.active_tab_mut() {
                    tab.sidebar_panel = panel;
                }
                true
            }
            Target::Thumbnail(i) => {
                self.go_to_page(i);
                true
            }
            Target::Bookmark(i) => {
                let page = self
                    .active_tab()
                    .and_then(|t| t.document.as_ref())
                    .and_then(|d| d.bookmark_page(i));
                if let Some(page) = page {
                    self.go_to_page(page);
                    true
                } else {
                    false
                }
            }
            Target::BookmarkArrow(i) => self
                .active_tab_mut()
                .and_then(|t| t.document.as_mut())
                .is_some_and(|d| d.toggle_bookmark(i)),
            Target::RecentFile(i) => self.open_recent(i),
            Target::SearchField => {
                self.search_focused = true;
                true
            }
            Target::SearchPrev => {
                self.search.prev_match();
                self.follow_current_match();
                true
            }
            Target::SearchNext => {
                self.search.next_match();
                self.follow_current_match();
                true
            }
            // The page takes the click only to drop the search caret, which the
            // block above already did.
            Target::Document => false,
        };

        acted || focus_changed
    }

    /// Apply a mutation to the active tab, answering whether there was one.
    fn with_tab(&mut self, f: fn(&mut DocumentTab)) -> bool {
        match self.active_tab_mut() {
            Some(tab) => {
                f(tab);
                true
            }
            None => false,
        }
    }

    /// Switch the active tab to a fit mode.
    fn set_zoom_mode(&mut self, mode: ZoomMode) -> bool {
        match self.active_tab_mut() {
            Some(tab) => {
                tab.zoom = mode;
                true
            }
            None => false,
        }
    }

    /// Scroll the document, in wheel notches.
    ///
    /// `notches` is the raw wheel delta, which is positive for a push *away*
    /// from the user. That is the opposite of the direction the document moves,
    /// and the sign flip belongs to `guitk::wheel` rather than to each app --
    /// so everything below reads the wheel helpers' output as a distance along
    /// the document, where positive means "towards the last page".
    ///
    /// The two view modes read a notch differently and both readings are right
    /// for their mode: continuous scroll is a continuous surface, so a notch is
    /// a distance; single-page mode shows one centred page that cannot pan, so
    /// there is nothing for a distance to mean and a notch is a page.
    pub fn scroll_by(&mut self, notches: f32) -> bool {
        let content = self.layout(self.window_width, self.window_height).content;
        let area = (content.w, content.h);
        // Only single-page mode goes through the accumulator, and the split is
        // deliberate. A page turn is quantised, so a touchpad's fractional
        // notches have to be banked until they add up to one -- otherwise a
        // slow drag rounds to zero every frame and turns no pages at all. A
        // continuous scroll is not quantised: a fraction of a notch is a real
        // distance, and putting it through a whole-row accumulator would throw
        // away exactly the movement the user is making.
        let Some(mode) = self.tabs.get(self.active_tab).map(|t| t.view_mode) else {
            return false;
        };
        match mode {
            ViewMode::SinglePage => {
                // One page per notch rather than `ROWS_PER_NOTCH` pages: a
                // flick reporting six notches at once should not skip eighteen
                // pages past what the user saw go by.
                let pages = self.wheel.rows_at(notches, 1.0);
                if pages == 0 {
                    return false;
                }
                let Some(tab) = self.tabs.get_mut(self.active_tab) else {
                    return false;
                };
                if pages > 0 {
                    tab.next_page();
                } else {
                    tab.prev_page();
                }
                true
            }
            ViewMode::ContinuousScroll => {
                let delta = wheel::pixels(notches, SCROLL_ROW_HEIGHT);
                let Some(tab) = self.tabs.get_mut(self.active_tab) else {
                    return false;
                };
                let Some(doc) = tab.document.as_ref() else {
                    return false;
                };
                let zoom = Self::continuous_zoom(tab, doc, area);
                let limit = Self::max_scroll(doc, zoom, area.1);
                let before = tab.scroll_offset_y;
                tab.scroll_offset_y = (before + delta).clamp(0.0, limit);
                // The page number is derived from where we ended up, so a
                // scroll that hit the top or bottom stop still reports the
                // right page rather than the one it was aiming at.
                tab.current_page = page_at_offset(doc, tab.scroll_offset_y, zoom);
                #[allow(clippy::float_cmp)]
                let moved = tab.scroll_offset_y != before;
                moved
            }
        }
    }

    /// Handle a keystroke, answering whether anything changed.
    #[allow(clippy::too_many_lines)]
    pub fn handle_key(&mut self, event: &KeyEvent) -> bool {
        if !event.pressed {
            return false;
        }

        // Escape closes the search bar from anywhere, focused or not, because
        // the whole point of Escape on an overlay is that you do not have to
        // find it first.
        if event.key == Key::Escape && self.search.active {
            self.search.clear();
            self.search_focused = false;
            return true;
        }

        if self.search_focused {
            return self.handle_search_key(event);
        }

        match event.key {
            Key::Right | Key::Down | Key::PageDown | Key::Space => {
                self.step_page(true);
                true
            }
            Key::Left | Key::Up | Key::PageUp => {
                self.step_page(false);
                true
            }
            Key::Home => {
                self.go_to_page(0);
                true
            }
            Key::End => {
                let last = self
                    .active_tab()
                    .map_or(0, |t| t.page_count().saturating_sub(1));
                self.go_to_page(last);
                true
            }
            _ => false,
        }
    }

    /// Page forward or back, keeping continuous scroll in step.
    fn step_page(&mut self, forward: bool) {
        let Some(tab) = self.active_tab_mut() else {
            return;
        };
        if forward {
            tab.next_page();
        } else {
            tab.prev_page();
        }
        let page = tab.current_page;
        self.go_to_page(page);
    }

    /// Handle a keystroke while the search field holds the caret.
    fn handle_search_key(&mut self, event: &KeyEvent) -> bool {
        match event.key {
            Key::Enter => {
                self.search.next_match();
                self.follow_current_match();
                true
            }
            Key::Backspace => {
                // `pop` removes a `char`, not a byte, so a query ending in a
                // multi-byte character loses the character rather than half of
                // it -- which would panic the next time the string was sliced.
                if self.search.query.pop().is_none() {
                    return false;
                }
                self.refresh_search();
                true
            }
            _ => {
                // `typed()` rather than `single_char()`: a compose sequence or
                // an IME commit hands over several characters in one event, and
                // taking only the first silently drops the rest. It also drops
                // control characters, which is what keeps Tab and Enter from
                // being typed into the box.
                let typed: String = event.typed().collect();
                if typed.is_empty() {
                    return false;
                }
                self.search.query.push_str(&typed);
                self.refresh_search();
                true
            }
        }
    }

    /// Route one window event.
    pub fn handle_event(&mut self, event: &Event) -> bool {
        match event {
            Event::Resize { width, height } => {
                // `as f32` on a window dimension: exact for every size a
                // display can be, and `sane` in `resize` catches the rest.
                #[allow(clippy::cast_precision_loss)]
                self.resize(*width as f32, *height as f32);
                true
            }
            Event::Mouse(MouseEvent { kind, x, y, .. }) => match kind {
                MouseEventKind::Press(MouseButton::Left) => match self.target_at(*x, *y) {
                    Some(target) => self.handle_target(target),
                    None => {
                        // A press on bare background is still a press away from
                        // the search field.
                        let had_focus = self.search_focused;
                        self.search_focused = false;
                        had_focus
                    }
                },
                // `dy` only: there is no horizontal scroll offset to move, and
                // silently treating a sideways flick as a vertical one is worse
                // than ignoring it.
                MouseEventKind::Scroll { dy, .. } => self.scroll_by(*dy),
                _ => false,
            },
            Event::Key(key) => self.handle_key(key),
            _ => false,
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
// Window
// ============================================================================

impl App for PdfViewerApp {
    fn title(&self) -> String {
        match self.active_tab() {
            Some(tab) if tab.document.is_some() => format!("{} - PDF Viewer", tab.title()),
            _ => "PDF Viewer".to_string(),
        }
    }

    fn initial_size(&self) -> (u32, u32) {
        // `as u32` on two positive constants; the values are written above.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    fn on_event(&mut self, event: &Event) -> Response {
        // Ctrl+Q and the window's own close button are the two ways out, and
        // they are checked before anything else so a modal state can never
        // swallow the quit.
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        if let Event::Key(key) = event {
            if key.pressed && key.modifiers.ctrl {
                match key.key {
                    Key::Q => return Response::Exit,
                    // Ctrl+F is the find shortcut everywhere else, and going
                    // through `handle_target` rather than setting the flags
                    // here means the shortcut and the button cannot drift.
                    Key::F => {
                        if !self.search.active {
                            self.handle_target(Target::SearchToggle);
                        } else {
                            self.search_focused = true;
                        }
                        return Response::Redraw;
                    }
                    Key::T => {
                        self.handle_target(Target::NewTab);
                        return Response::Redraw;
                    }
                    Key::W => {
                        let i = self.active_tab;
                        self.handle_target(Target::TabClose(i));
                        return Response::Redraw;
                    }
                    _ => {}
                }
            }
        }

        if self.handle_event(event) {
            Response::Redraw
        } else {
            Response::Idle
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // The size the compositor is about to draw at is the size every
        // rectangle -- and therefore every hit box -- is derived from, so it is
        // recorded before the frame is built rather than waiting for a `Resize`
        // event that may not arrive before the first paint.
        self.resize(width, height);
        self.frame(self.window_width, self.window_height)
            .into_tree()
    }
}

impl Probe for PdfViewerApp {
    type Target = Target;
    type Outcome = bool;

    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> guitk::frame::Frame<Self::Target> {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        self.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Press(button),
            x,
            y,
        }))
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        self.handle_key(key)
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() -> ExitCode {
    app::launch(
        "pdfviewer",
        &mut PdfViewerApp::new(WINDOW_WIDTH, WINDOW_HEIGHT),
    )
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the line
    // that did it -- that is the diagnosis. The defensive lints exist to keep
    // panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    // -- Search highlight placement -------------------------------------------

    /// A one-page document whose only span is `text`, occupying `width` points.
    fn doc_with_span(text: &str, width: f32) -> PdfDocument {
        let mut doc = PdfDocument::new(PathBuf::from("test.pdf"));
        let mut page = PdfPage::new(600.0, 800.0);
        page.text_spans.push(TextSpan {
            text: text.to_string(),
            rect: PageRect::new(0.0, 0.0, width, 12.0),
            font_size: 12.0,
        });
        doc.pages.push(page);
        doc
    }

    #[test]
    fn a_highlight_covers_the_match_it_found() {
        // Ten characters across 100 points is 10 points per character, so the
        // match at characters 5..10 must start at 50 and be 50 wide.
        let doc = doc_with_span("abcdefghij", 100.0);
        let hits = doc.search("fghij");
        assert_eq!(hits.len(), 1);
        let hit = hits.first().expect("one hit");
        assert!((hit.rect.x - 50.0).abs() < 0.01, "starts at {}", hit.rect.x);
        assert!(
            (hit.rect.width - 50.0).abs() < 0.01,
            "is {} wide",
            hit.rect.width
        );
    }

    #[test]
    fn a_highlight_is_placed_by_characters_not_bytes() {
        // Five characters across 100 points, two of which are two bytes each.
        // The match is the last character, so it must start at 80 -- placing it
        // by byte offset would put it at 100, past the end of the span.
        let doc = doc_with_span("\u{e9}\u{e9}abc", 100.0);
        let hits = doc.search("c");
        assert_eq!(hits.len(), 1);
        let hit = hits.first().expect("one hit");
        assert!((hit.rect.x - 80.0).abs() < 0.01, "starts at {}", hit.rect.x);
        assert!(
            (hit.rect.width - 20.0).abs() < 0.01,
            "is {} wide",
            hit.rect.width
        );
    }

    /// A highlight is placed by counting characters of the *span*, not of a
    /// lowercased copy of it.
    ///
    /// Turkish `İ` (U+0130) is one character but folds to two (`i` plus a
    /// combining dot). Counting in the folded copy therefore put every
    /// highlight after one a whole cell to the right.
    #[test]
    fn a_highlight_is_placed_by_the_span_not_by_a_folded_copy_of_it() {
        // Four characters across 100 points is 25 points each; the match is
        // the last one, so it starts at 75.
        let doc = doc_with_span("\u{130}abc", 100.0);
        let hits = doc.search("C");
        assert_eq!(hits.len(), 1);
        let hit = hits.first().expect("one hit");
        assert!((hit.rect.x - 75.0).abs() < 0.01, "starts at {}", hit.rect.x);
        assert!(
            (hit.rect.width - 25.0).abs() < 0.01,
            "is {} wide",
            hit.rect.width
        );
    }

    /// Searching for a character whose folded form is longer than itself does
    /// not walk off a character boundary. The scan used to advance by the
    /// *unfolded* query's byte length inside the *folded* copy: for a query of
    /// `İ` that is 2 bytes into a 3-byte folded character, and re-slicing
    /// there panics.
    #[test]
    fn a_query_that_grows_when_folded_does_not_split_a_character() {
        let doc = doc_with_span("a\u{130}b\u{130}c", 100.0);
        let hits = doc.search("\u{130}");
        assert_eq!(hits.len(), 2);
    }

    /// Matches do not overlap: `aa` occurs twice in `aaaa`, not three times.
    #[test]
    fn highlights_do_not_overlap_one_another() {
        let doc = doc_with_span("aaaa", 100.0);
        assert_eq!(doc.search("aa").len(), 2);
    }

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

    // -- Print ranges that name pages the document does not have --------------

    /// A document with no pages has no page zero.
    ///
    /// `All` and `CurrentPage` both returned nothing here already; the `Custom`
    /// arm clamped its range's end to `page_count - 1`, which saturates to `0`,
    /// so every range collapsed to `0..=0` and it returned `[0]` -- a page
    /// index handed to a caller with every reason to trust it.
    #[test]
    fn a_custom_range_over_an_empty_document_names_no_pages() {
        let ps = PrintSettings {
            page_range: PrintPageRange::Custom(vec![(0, 4)]),
            ..Default::default()
        };
        assert!(ps.resolve_pages(0, 0).is_empty());
    }

    /// A range that begins past the last page prints nothing, rather than
    /// being dragged down onto the last page: "print 50-60" of a ten-page
    /// document is a typo, and the answer to it is not "print page ten".
    #[test]
    fn a_range_beyond_the_end_prints_nothing() {
        let ps = PrintSettings {
            page_range: PrintPageRange::Custom(vec![(49, 59)]),
            ..Default::default()
        };
        assert!(ps.resolve_pages(10, 0).is_empty());

        // A range that merely *extends* past the end still prints the part
        // that exists -- the two cases are different and used to be conflated.
        let ps = PrintSettings {
            page_range: PrintPageRange::Custom(vec![(8, 59)]),
            ..Default::default()
        };
        assert_eq!(ps.resolve_pages(10, 0), vec![8, 9]);
    }

    /// Overlapping ranges name each page once, in order.
    #[test]
    fn overlapping_ranges_are_merged() {
        let ps = PrintSettings {
            page_range: PrintPageRange::Custom(vec![(4, 6), (0, 2), (5, 8)]),
            ..Default::default()
        };
        assert_eq!(ps.resolve_pages(10, 0), vec![0, 1, 2, 4, 5, 6, 7, 8]);
    }

    /// Typing a page range that names nothing must not print the whole
    /// document. It used to: the parser dropped the out-of-range parts, found
    /// itself with an empty list, and fell back to `All`. Of the two ways to
    /// be wrong about a range nobody asked for, printing everything is the
    /// expensive one.
    #[test]
    fn a_page_range_naming_nothing_does_not_print_everything() {
        let parsed = PrintSettings::parse_page_range("50-60", 10);
        let ps = PrintSettings {
            page_range: parsed,
            ..Default::default()
        };
        assert!(
            ps.resolve_pages(10, 0).is_empty(),
            "a typo in the page box printed the whole document"
        );
    }

    /// Page "0" does not exist in the 1-based numbering the user types in.
    #[test]
    fn page_zero_is_not_a_page() {
        let ps = PrintSettings {
            page_range: PrintSettings::parse_page_range("0", 10),
            ..Default::default()
        };
        assert!(ps.resolve_pages(10, 0).is_empty());
    }

    /// The end of a range is clamped when the document is printed, not when the
    /// range is typed, because only the first of those knows how many pages the
    /// document has *now*.
    #[test]
    fn the_end_of_a_range_is_clamped_at_print_time() {
        let parsed = PrintSettings::parse_page_range("1-100", 10);
        assert_eq!(parsed, PrintPageRange::Custom(vec![(0, 99)]));
        let ps = PrintSettings {
            page_range: parsed,
            ..Default::default()
        };
        assert_eq!(ps.resolve_pages(3, 0), vec![0, 1, 2]);
        assert_eq!(ps.resolve_pages(10, 0), (0..10).collect::<Vec<_>>());
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

    /// An annotation that could not be placed must not consume an id.
    ///
    /// The three `add_*` methods each took an id from the counter before
    /// looking for a page to put the annotation on, so every failed call --
    /// an empty tab, or a click arriving while a document was closing -- burnt
    /// one. The ids stayed unique, so nothing broke visibly; they just grew
    /// gaps, which is the sort of thing only noticed by whoever later assumes
    /// they are dense.
    #[test]
    fn a_failed_annotation_does_not_consume_an_id() {
        let mut app = PdfViewerApp::new(1280.0, 720.0);
        // A fresh app has one tab and no document in it.
        assert!(app.active_tab().unwrap().document.is_none());
        let rect = PageRect::new(10.0, 10.0, 20.0, 20.0);
        assert!(app.add_highlight(rect, YELLOW).is_none());
        assert!(app.add_note(rect, "n".to_string()).is_none());
        assert!(app.add_freehand(rect, vec![(0.0, 0.0)], RED, 1.0).is_none());

        // Now give it a document: the first annotation that lands should get
        // the first id, not the fourth.
        app.load_document(PdfDocument::create_sample(PathBuf::from("/t.pdf"), 1));
        assert_eq!(app.add_highlight(rect, YELLOW), Some(1));
    }

    /// The three wrappers share one counter, so their ids interleave without
    /// colliding -- which is what makes `remove_annotation(id)` unambiguous.
    #[test]
    fn annotation_ids_are_unique_across_the_three_kinds() {
        let mut app = PdfViewerApp::new(1280.0, 720.0);
        app.load_document(PdfDocument::create_sample(PathBuf::from("/t.pdf"), 1));
        let rect = PageRect::new(10.0, 10.0, 20.0, 20.0);
        let ids = [
            app.add_highlight(rect, YELLOW),
            app.add_note(rect, "n".to_string()),
            app.add_freehand(rect, vec![(0.0, 0.0)], RED, 1.0),
        ];
        assert_eq!(ids, [Some(1), Some(2), Some(3)]);
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
        let tree = app.frame(1280.0, 720.0).into_tree();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_app_render_with_doc() {
        let mut app = PdfViewerApp::new(1280.0, 720.0);
        let doc = PdfDocument::create_sample(PathBuf::from("/test.pdf"), 3);
        app.load_document(doc);
        let tree = app.frame(1280.0, 720.0).into_tree();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_app_render_continuous_scroll() {
        let mut app = PdfViewerApp::new(1280.0, 720.0);
        let doc = PdfDocument::create_sample(PathBuf::from("/test.pdf"), 5);
        app.load_document(doc);
        app.active_tab_mut().unwrap().toggle_view_mode();
        let tree = app.frame(1280.0, 720.0).into_tree();
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
        let tree = app.frame(1280.0, 720.0).into_tree();
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
        app.add_highlight(PageRect::new(50.0, 100.0, 200.0, 14.0), YELLOW);
        app.add_note(
            PageRect::new(300.0, 200.0, 20.0, 20.0),
            "Test note".to_string(),
        );
        app.add_freehand(
            PageRect::new(0.0, 0.0, 100.0, 100.0),
            vec![(10.0, 10.0), (50.0, 50.0)],
            RED,
            2.0,
        );
        let tree = app.frame(1280.0, 720.0).into_tree();
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

    // -- Tab titles: non-ASCII safety and width discipline --------------------

    /// Titles chosen so byte 17 — the offset the old code cut at — falls inside
    /// a multi-byte character. The last one pins it exactly: 16 ASCII bytes
    /// then a two-byte `é`, so byte 17 is that character's continuation byte.
    fn adversarial_titles() -> Vec<String> {
        vec![
            "日本語の報告書の草案".to_string(),
            "Ελληνικό έγγραφο αναφοράς".to_string(),
            "Отчёт о состоянии дел за квартал".to_string(),
            "quarterly 📊 results 🚀 summary".to_string(),
            format!("{}é{}", "a".repeat(16), "b".repeat(30)),
        ]
    }

    /// The tab bar with one tab per title, each title supplied as the
    /// document's own `/Title` metadata — the document-controlled path.
    fn tab_bar_with_titles(titles: &[String]) -> RenderTree {
        let mut app = PdfViewerApp::new(1280.0, 720.0);
        app.tabs.clear();
        for (i, title) in titles.iter().enumerate() {
            let mut tab = DocumentTab::new(i as u64 + 1);
            let mut doc = PdfDocument::new(PathBuf::from("doc.pdf"));
            doc.metadata.title = Some(title.clone());
            tab.document = Some(doc);
            app.tabs.push(tab);
        }
        app.active_tab = 0;
        // Through the whole frame rather than `render_tab_bar` alone, because
        // the strip is positioned by `Layout` and calling the band renderer
        // with a hand-made layout would test a geometry the app never draws.
        // But the whole frame also holds the toolbar, whose button labels are
        // drawn at TAB_TITLE_SIZE too -- so a caller that told tab titles apart
        // by font size alone would measure fourteen toolbar buttons and believe
        // it had measured the tabs. Cutting to the strip's own band keeps the
        // real geometry and still hands back only the strip.
        let band = app.layout(1280.0, 720.0).tab_bar;
        let mut tree = app.frame(1280.0, 720.0).into_tree();
        tree.commands
            .retain(|cmd| command_y(cmd).is_some_and(|y| y >= band.y && y < band.bottom()));
        tree
    }

    /// Where a drawing command puts what it draws, or `None` for the structural
    /// commands (clip and transform bookkeeping) that put nothing anywhere.
    fn command_y(cmd: &RenderCommand) -> Option<f32> {
        match *cmd {
            RenderCommand::FillRect { y, .. }
            | RenderCommand::StrokeRect { y, .. }
            | RenderCommand::Text { y, .. }
            | RenderCommand::RichText { y, .. }
            | RenderCommand::Image { y, .. }
            | RenderCommand::BoxShadow { y, .. }
            | RenderCommand::PushClip { y, .. } => Some(y),
            RenderCommand::Line { y1, .. } => Some(y1),
            RenderCommand::PopClip
            | RenderCommand::PushTranslate { .. }
            | RenderCommand::PopTranslate
            | RenderCommand::PushFont { .. }
            | RenderCommand::PopFont => None,
        }
    }

    #[test]
    fn a_non_ascii_tab_title_does_not_abort_the_tab_bar() {
        // Regression: the title was `&title[..17]` behind a `len() > 20` guard.
        // Byte 17 lands inside a multi-byte character for most non-Latin text,
        // and slicing there aborts — the guard made that *more* likely, not
        // less, since a seven-character Japanese title is 21 bytes and so
        // always took the truncating branch. The title comes from the PDF's own
        // `/Title` metadata, so this is document-controlled input.
        let tree = tab_bar_with_titles(&adversarial_titles());
        assert!(!tree.commands.is_empty(), "the tab bar drew nothing");
    }

    #[test]
    fn a_tab_title_stops_before_the_close_button() {
        // A byte budget says nothing about the room available. The title starts
        // TAB_TEXT_INSET into the tab and the close glyph starts
        // TAB_CLOSE_INSET from its far edge, so anything wider than the
        // difference draws underneath the `x`.
        let titles = adversarial_titles();
        let room = 180.0 - TAB_TEXT_INSET - TAB_CLOSE_INSET;
        let mut checked = 0usize;
        for cmd in tab_bar_with_titles(&titles).commands {
            let RenderCommand::Text {
                x,
                text,
                font_size,
                font_weight,
                ..
            } = cmd
            else {
                continue;
            };
            if font_size != TAB_TITLE_SIZE {
                continue;
            }
            let drawn = text::measure(&text, font_size, font_weight);
            assert!(
                drawn <= room + 0.5,
                "the tab title {text:?} draws {drawn} wide from {x}, past the \
                 {room} available before the close button"
            );
            checked += 1;
        }
        assert_eq!(
            checked,
            titles.len(),
            "expected one title per tab; the filter matched {checked}, so this \
             test would pass without measuring what it claims to"
        );
    }

    // -- Window wiring --------------------------------------------------------
    //
    // The tests below are about the seam between "the app has a method that
    // changes something" and "a user can reach it". Every one of the eighteen
    // state-changing methods above was already tested before this app opened a
    // window, and every one of them was unreachable -- `main` was empty. What
    // these test is the part that was missing: that a click at a place the
    // renderer drew a control arrives at the method that control names.

    /// A viewer with a five-page document, so bookmarks nest and the thumbnail
    /// strip has more than one entry.
    fn wired() -> PdfViewerApp {
        let mut app = PdfViewerApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.load_document(PdfDocument::create_sample(PathBuf::from("/report.pdf"), 5));
        app
    }

    // The `Option` is not redundant even though this arm always answers: the
    // signature has to be `OpenFn`, and a real parser declines a file it cannot
    // read -- which `refuses` below is the test double for.
    #[allow(clippy::unnecessary_wraps)]
    fn a_document(_path: &Path) -> Option<PdfDocument> {
        Some(PdfDocument::create_sample(PathBuf::from("/opened.pdf"), 2))
    }

    fn refuses(_path: &Path) -> Option<PdfDocument> {
        None
    }

    fn accepts(_doc: &PdfDocument, _pages: &[usize]) -> bool {
        true
    }

    #[test]
    fn every_control_the_renderer_draws_can_be_clicked() {
        let mut app = wired();
        app.search.active = true;
        let frame = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);

        assert!(
            !frame.hits().is_empty(),
            "a fully populated viewer recorded no hit boxes at all"
        );

        for (target, rect) in frame.hits() {
            let (cx, cy) = rect.centre();
            assert_eq!(
                frame.hit_test(cx, cy),
                Some(*target),
                "{target:?} was drawn at {rect:?} but the centre of its own box \
                 does not click to it -- something recorded later covers it"
            );
        }
    }

    #[test]
    fn no_control_is_drawn_outside_the_window() {
        let mut app = wired();
        app.search.active = true;
        let frame = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let window = Rect::new(0.0, 0.0, WINDOW_WIDTH, WINDOW_HEIGHT);

        for (target, rect) in frame.hits() {
            assert!(
                rect.x >= window.x
                    && rect.y >= window.y
                    && rect.right() <= window.right() + 0.5
                    && rect.bottom() <= window.bottom() + 0.5,
                "{target:?} is at {rect:?}, which leaves the {window:?} window"
            );
        }
    }

    #[test]
    fn a_window_too_small_for_a_band_drops_its_controls_rather_than_stacking_them() {
        let mut app = wired();
        app.search.active = true;
        // Tall enough for the toolbar and nothing else.
        let frame = app.frame(400.0, TOOLBAR_HEIGHT);
        let window = Rect::new(0.0, 0.0, 400.0, TOOLBAR_HEIGHT);

        for (target, rect) in frame.hits() {
            assert!(
                rect.intersect(window).is_some(),
                "{target:?} at {rect:?} survived into a window it does not touch"
            );
        }
        assert!(
            frame.rect_of(|t| matches!(t, Target::Panel(_))).is_none(),
            "the sidebar has no room at all, but its panel tabs are still clickable"
        );
    }

    #[test]
    fn a_zero_sized_window_draws_nothing_clickable() {
        let app = wired();
        let frame = app.frame(0.0, 0.0);
        assert!(
            frame.hits().is_empty(),
            "a window with no area still recorded {} hit boxes",
            frame.hits().len()
        );
    }

    #[test]
    fn resizing_moves_the_boxes_the_next_click_is_tested_against() {
        let mut app = wired();

        // Deliberately not `probe::rect_of`, which draws at the fixed
        // `Probe::SIZE` and therefore cannot observe a resize at all. What is
        // under test is what `target_at` will match a click against, and that
        // is the frame at the size the window last reported.
        let live = |app: &PdfViewerApp| {
            app.frame(app.window_width, app.window_height)
                .rect_of(|t| *t == Target::SearchToggle)
        };

        let before = live(&app).expect("the search button is drawn at the opening size");
        app.resize(WINDOW_WIDTH + 300.0, WINDOW_HEIGHT);
        let after = live(&app).expect("the search button survives a widening");

        assert!(
            after.x > before.x,
            "the toolbar's right-hand buttons are measured from the right edge, \
             so widening the window must move them: {before:?} -> {after:?}"
        );
        assert_eq!(
            app.target_at(after.centre().0, after.centre().1),
            Some(Target::SearchToggle),
            "the click test still uses the old geometry after a resize"
        );
    }

    // -- Toolbar --------------------------------------------------------------

    #[test]
    fn the_nav_buttons_page_the_document() {
        let mut app = wired();
        assert_eq!(app.active_tab().unwrap().current_page, 0);

        probe::click(&mut app, Target::Nav(Nav::Next));
        assert_eq!(app.active_tab().unwrap().current_page, 1);

        probe::click(&mut app, Target::Nav(Nav::Last));
        assert_eq!(app.active_tab().unwrap().current_page, 4);

        probe::click(&mut app, Target::Nav(Nav::Prev));
        assert_eq!(app.active_tab().unwrap().current_page, 3);

        probe::click(&mut app, Target::Nav(Nav::First));
        assert_eq!(app.active_tab().unwrap().current_page, 0);
    }

    #[test]
    fn the_zoom_buttons_reach_the_tab() {
        let mut app = wired();
        app.active_tab_mut().unwrap().zoom = ZoomMode::Fixed(1.0);

        probe::click(&mut app, Target::ZoomIn);
        match app.active_tab().unwrap().zoom {
            ZoomMode::Fixed(z) => assert!(z > 1.0, "zoom in left the factor at {z}"),
            other => panic!("expected Fixed after zoom in, got {other:?}"),
        }

        probe::click(&mut app, Target::ZoomOut);
        probe::click(&mut app, Target::ZoomOut);
        match app.active_tab().unwrap().zoom {
            ZoomMode::Fixed(z) => assert!(z < 1.0, "two zoom-outs left the factor at {z}"),
            other => panic!("expected Fixed after zoom out, got {other:?}"),
        }
    }

    #[test]
    fn the_fit_buttons_reach_the_tab() {
        let mut app = wired();

        probe::click(&mut app, Target::Fit(Fit::Width));
        assert_eq!(app.active_tab().unwrap().zoom, ZoomMode::FitWidth);

        probe::click(&mut app, Target::Fit(Fit::Page));
        assert_eq!(app.active_tab().unwrap().zoom, ZoomMode::FitPage);
    }

    #[test]
    fn the_rotate_buttons_turn_the_page_both_ways() {
        let mut app = wired();

        probe::click(&mut app, Target::RotateCw);
        assert_eq!(app.active_tab().unwrap().rotation, Rotation::Deg90);

        probe::click(&mut app, Target::RotateCcw);
        probe::click(&mut app, Target::RotateCcw);
        assert_eq!(app.active_tab().unwrap().rotation, Rotation::Deg270);
    }

    #[test]
    fn the_view_mode_button_switches_between_the_two_layouts() {
        let mut app = wired();
        assert_eq!(app.active_tab().unwrap().view_mode, ViewMode::SinglePage);

        probe::click(&mut app, Target::ViewModeToggle);
        assert_eq!(
            app.active_tab().unwrap().view_mode,
            ViewMode::ContinuousScroll
        );

        probe::click(&mut app, Target::ViewModeToggle);
        assert_eq!(app.active_tab().unwrap().view_mode, ViewMode::SinglePage);
    }

    #[test]
    fn hiding_the_sidebar_takes_its_controls_with_it() {
        let mut app = wired();
        assert!(
            probe::rect_of(&app, Target::Panel(SidebarPanel::Bookmarks)).is_some(),
            "the panel tabs should be clickable while the sidebar is shown"
        );

        probe::click(&mut app, Target::SidebarToggle);
        assert!(!app.active_tab().unwrap().sidebar_visible);
        assert!(
            probe::rect_of(&app, Target::Panel(SidebarPanel::Bookmarks)).is_none(),
            "the sidebar is hidden but its panel tabs still take clicks"
        );
        assert!(
            probe::rect_of(&app, Target::Thumbnail(0)).is_none(),
            "the sidebar is hidden but its thumbnails still take clicks"
        );

        // And the document takes the space back, which is the visible half of
        // the same change.
        let content = app.layout(WINDOW_WIDTH, WINDOW_HEIGHT).content;
        assert_eq!(content.x, 0.0);
        assert_eq!(content.w, WINDOW_WIDTH);
    }

    // -- Tabs -----------------------------------------------------------------

    #[test]
    fn the_new_tab_button_opens_a_tab_and_switches_to_it() {
        let mut app = wired();
        assert_eq!(app.tabs.len(), 1);

        probe::click(&mut app, Target::NewTab);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.active_tab, 1);
        assert!(
            app.active_tab().unwrap().document.is_none(),
            "a new tab should be empty"
        );
    }

    #[test]
    fn clicking_a_tab_switches_to_it() {
        let mut app = wired();
        probe::click(&mut app, Target::NewTab);
        assert_eq!(app.active_tab, 1);

        probe::click(&mut app, Target::Tab(0));
        assert_eq!(app.active_tab, 0);
        assert!(app.active_tab().unwrap().document.is_some());
    }

    #[test]
    fn the_close_cross_beats_the_tab_it_sits_on() {
        let mut app = wired();
        probe::click(&mut app, Target::NewTab);
        assert_eq!(app.tabs.len(), 2);

        // The point that matters is the cross's own centre: it lies inside the
        // tab's rectangle, so whichever box was recorded last decides. Closing
        // must win, or the cross is decoration.
        let cross = probe::rect_of(&app, Target::TabClose(1)).expect("tab 1 has a close cross");
        let tab = probe::rect_of(&app, Target::Tab(1)).expect("tab 1 is drawn");
        assert!(
            tab.intersect(cross).is_some(),
            "this test is vacuous unless the cross sits on the tab: {tab:?} vs {cross:?}"
        );

        let (cx, cy) = cross.centre();
        assert_eq!(app.target_at(cx, cy), Some(Target::TabClose(1)));

        app.click_at(cx, cy, MouseButton::Left, <PdfViewerApp as Probe>::SIZE);
        assert_eq!(app.tabs.len(), 1, "the cross did not close the tab");
    }

    #[test]
    fn the_last_tab_cannot_be_closed_away() {
        let mut app = wired();
        assert_eq!(app.tabs.len(), 1);
        probe::click(&mut app, Target::TabClose(0));
        assert_eq!(
            app.tabs.len(),
            1,
            "closing the only tab left the window with nothing in it"
        );
    }

    // -- Sidebar --------------------------------------------------------------

    #[test]
    fn the_panel_tabs_switch_which_panel_is_shown() {
        let mut app = wired();
        assert_eq!(
            app.active_tab().unwrap().sidebar_panel,
            SidebarPanel::Thumbnails
        );

        probe::click(&mut app, Target::Panel(SidebarPanel::Bookmarks));
        assert_eq!(
            app.active_tab().unwrap().sidebar_panel,
            SidebarPanel::Bookmarks
        );
        assert!(
            probe::rect_of(&app, Target::Bookmark(0)).is_some(),
            "switching to the bookmarks panel should make bookmarks clickable"
        );
        assert!(
            probe::rect_of(&app, Target::Thumbnail(0)).is_none(),
            "the thumbnail strip is not shown, so it must not take clicks"
        );

        probe::click(&mut app, Target::Panel(SidebarPanel::Annotations));
        assert_eq!(
            app.active_tab().unwrap().sidebar_panel,
            SidebarPanel::Annotations
        );
    }

    #[test]
    fn clicking_a_thumbnail_goes_to_that_page() {
        let mut app = wired();
        probe::click(&mut app, Target::Thumbnail(3));
        assert_eq!(app.active_tab().unwrap().current_page, 3);
    }

    #[test]
    fn a_thumbnail_scrolled_out_of_the_strip_is_not_clickable() {
        let mut app = PdfViewerApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        // Far more pages than the strip is tall, so the last ones are drawn
        // below the sidebar's clip and must not be reachable.
        app.load_document(PdfDocument::create_sample(PathBuf::from("/long.pdf"), 60));
        assert!(
            probe::rect_of(&app, Target::Thumbnail(0)).is_some(),
            "the first thumbnail is on screen"
        );
        assert!(
            probe::rect_of(&app, Target::Thumbnail(59)).is_none(),
            "a thumbnail drawn past the bottom of the strip is still taking clicks"
        );
    }

    #[test]
    fn clicking_a_bookmark_goes_to_its_page() {
        let mut app = wired();
        app.active_tab_mut().unwrap().sidebar_panel = SidebarPanel::Bookmarks;

        // Row 1 is "Chapter 2: Methods" on page 2 -- row 0's children are
        // collapsed, so the flattened list is the two chapter headings.
        let flat = app
            .active_tab()
            .unwrap()
            .document
            .as_ref()
            .unwrap()
            .flatten_bookmarks();
        assert_eq!(flat.len(), 2, "the sample's chapters start collapsed");
        assert_eq!(flat[1].1.page_index, 2);

        probe::click(&mut app, Target::Bookmark(1));
        assert_eq!(app.active_tab().unwrap().current_page, 2);
    }

    #[test]
    fn the_bookmark_arrow_folds_the_row_rather_than_navigating() {
        let mut app = wired();
        app.active_tab_mut().unwrap().sidebar_panel = SidebarPanel::Bookmarks;
        assert_eq!(app.active_tab().unwrap().current_page, 0);

        probe::click(&mut app, Target::BookmarkArrow(0));

        let doc = app.active_tab().unwrap().document.as_ref().unwrap();
        assert_eq!(
            doc.flatten_bookmarks().len(),
            4,
            "expanding chapter 1 should reveal its two children"
        );
        assert_eq!(
            app.active_tab().unwrap().current_page,
            0,
            "the fold arrow moved the document as well as folding"
        );

        probe::click(&mut app, Target::BookmarkArrow(0));
        assert_eq!(
            app.active_tab()
                .unwrap()
                .document
                .as_ref()
                .unwrap()
                .flatten_bookmarks()
                .len(),
            2,
            "the arrow does not fold back"
        );
    }

    #[test]
    fn a_childless_bookmark_has_no_arrow_and_its_whole_row_navigates() {
        let mut app = wired();
        app.active_tab_mut().unwrap().sidebar_panel = SidebarPanel::Bookmarks;
        probe::click(&mut app, Target::BookmarkArrow(0));

        // Row 1 is now "1.1 Background", a leaf.
        assert!(
            probe::rect_of(&app, Target::BookmarkArrow(1)).is_none(),
            "a leaf bookmark should not record a fold arrow"
        );
        let row = probe::rect_of(&app, Target::Bookmark(1)).expect("the leaf row is drawn");
        assert_eq!(
            app.target_at(row.x + 2.0, row.centre().1),
            Some(Target::Bookmark(1)),
            "the left edge of a leaf row, where an arrow would be, must navigate"
        );
    }

    #[test]
    fn folding_a_bookmark_renumbers_the_rows_below_it() {
        let mut app = wired();
        app.active_tab_mut().unwrap().sidebar_panel = SidebarPanel::Bookmarks;
        probe::click(&mut app, Target::BookmarkArrow(0));

        // Expanded: [Ch1, 1.1(p0), 1.2(p1), Ch2(p2)]. Row 3 is chapter 2.
        probe::click(&mut app, Target::Bookmark(3));
        assert_eq!(app.active_tab().unwrap().current_page, 2);

        // Collapsed again, row 1 is chapter 2 -- the indices the renderer draws
        // and the ones `toggle_bookmark`/`bookmark_page` walk must agree.
        probe::click(&mut app, Target::BookmarkArrow(0));
        probe::click(&mut app, Target::Nav(Nav::First));
        probe::click(&mut app, Target::Bookmark(1));
        assert_eq!(app.active_tab().unwrap().current_page, 2);
    }

    // -- The two absent backends ----------------------------------------------

    #[test]
    fn print_is_not_a_control_without_a_spooler() {
        let app = wired();
        assert!(!app.can_print());
        assert!(
            probe::rect_of(&app, Target::Print).is_none(),
            "the Print button takes clicks with nothing behind it -- see \
             known-issues.md -> C-PDFVIEWER-HAS-NO-PDF-BACKEND"
        );
    }

    #[test]
    fn print_is_not_a_control_with_a_spooler_but_no_document() {
        let mut app = PdfViewerApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.set_printer(accepts);
        assert!(!app.can_print(), "an empty tab has nothing to print");
        assert!(probe::rect_of(&app, Target::Print).is_none());
    }

    #[test]
    fn wiring_a_spooler_makes_print_clickable_and_it_spools() {
        let mut app = wired();
        app.set_printer(accepts);
        assert!(app.can_print());
        assert!(
            probe::rect_of(&app, Target::Print).is_some(),
            "with a spooler wired, Print must become a control"
        );
        assert!(
            probe::click(&mut app, Target::Print),
            "the click did nothing"
        );
    }

    #[test]
    fn a_recent_file_is_not_a_link_without_a_parser() {
        let app = wired();
        assert!(!app.can_open());
        // `load_document` put the sample in the recent list, and the welcome
        // screen is what draws it -- so an empty tab is needed to see it.
        let mut app = app;
        probe::click(&mut app, Target::NewTab);
        assert!(!app.recent_files.entries.is_empty());
        assert!(
            probe::rect_of(&app, Target::RecentFile(0)).is_none(),
            "the recent-files list is offering links this build cannot follow"
        );
    }

    #[test]
    fn wiring_a_parser_makes_a_recent_file_open() {
        let mut app = wired();
        app.set_opener(a_document);
        probe::click(&mut app, Target::NewTab);
        assert!(app.active_tab().unwrap().document.is_none());

        assert!(
            probe::rect_of(&app, Target::RecentFile(0)).is_some(),
            "with a parser wired, a recent file must become a link"
        );
        assert!(probe::click(&mut app, Target::RecentFile(0)));
        assert_eq!(app.active_tab().unwrap().page_count(), 2);
    }

    #[test]
    fn an_opener_that_declines_leaves_the_tab_alone() {
        let mut app = wired();
        app.set_opener(refuses);
        probe::click(&mut app, Target::NewTab);
        assert!(!probe::click(&mut app, Target::RecentFile(0)));
        assert!(
            app.active_tab().unwrap().document.is_none(),
            "a refused open must not leave a half-loaded tab"
        );
    }

    #[test]
    fn opening_a_recent_file_that_is_not_there_is_not_a_panic() {
        let mut app = wired();
        app.set_opener(a_document);
        assert!(!app.open_recent(99));
    }

    // -- Scrolling ------------------------------------------------------------
    //
    // A wheel delta is positive when the wheel is pushed *away* from the user,
    // which walks the view back towards page 1. So throughout this section a
    // **negative** `dy` is the gesture that scrolls *down* the document, and
    // these tests are written that way on purpose: reading them as "negative
    // means down" is the thing that would have caught the sign being applied
    // twice.

    #[test]
    fn scrolling_in_continuous_mode_moves_the_offset() {
        let mut app = wired();
        probe::click(&mut app, Target::ViewModeToggle);
        assert_eq!(app.active_tab().unwrap().scroll_offset_y, 0.0);

        assert!(app.scroll_by(-3.0));
        let after = app.active_tab().unwrap().scroll_offset_y;
        assert!(after > 0.0, "three notches down moved nothing");

        assert!(app.scroll_by(3.0));
        assert_eq!(
            app.active_tab().unwrap().scroll_offset_y,
            0.0,
            "three notches back up did not return to the top"
        );
    }

    #[test]
    fn a_continuous_scroll_stops_at_the_end_of_the_document() {
        let mut app = wired();
        probe::click(&mut app, Target::ViewModeToggle);

        // Far more than the document is long.
        for _ in 0..200 {
            app.scroll_by(-10.0);
        }
        let tab = app.active_tab().unwrap();
        let doc = tab.document.as_ref().unwrap();
        let area = app.layout(WINDOW_WIDTH, WINDOW_HEIGHT).content;
        let zoom = PdfViewerApp::continuous_zoom(tab, doc, (area.w, area.h));
        let limit = PdfViewerApp::max_scroll(doc, zoom, area.h);

        assert!(
            (tab.scroll_offset_y - limit).abs() < 0.5,
            "scrolled to {} but the document ends at {limit}",
            tab.scroll_offset_y
        );
        assert!(
            !app.scroll_by(-10.0),
            "a scroll at the bottom stop should report that nothing moved"
        );
    }

    #[test]
    fn scrolling_never_takes_the_document_above_its_own_top() {
        let mut app = wired();
        probe::click(&mut app, Target::ViewModeToggle);
        for _ in 0..20 {
            app.scroll_by(10.0);
        }
        assert_eq!(app.active_tab().unwrap().scroll_offset_y, 0.0);
    }

    #[test]
    fn the_page_number_follows_a_continuous_scroll() {
        let mut app = wired();
        probe::click(&mut app, Target::ViewModeToggle);
        assert_eq!(app.active_tab().unwrap().current_page, 0);

        for _ in 0..40 {
            app.scroll_by(-3.0);
        }
        assert!(
            app.active_tab().unwrap().current_page > 0,
            "the status bar would still read page 1 after scrolling past it"
        );
    }

    #[test]
    fn scrolling_in_single_page_mode_pages_one_page_at_a_time() {
        let mut app = wired();
        assert_eq!(app.active_tab().unwrap().view_mode, ViewMode::SinglePage);

        // Six notches in one gesture -- a flick, not six deliberate clicks.
        assert!(app.scroll_by(-6.0));
        assert_eq!(
            app.active_tab().unwrap().current_page,
            1,
            "one gesture skipped past the pages the user watched go by"
        );

        assert!(app.scroll_by(6.0));
        assert_eq!(app.active_tab().unwrap().current_page, 0);
    }

    #[test]
    fn a_slow_touchpad_drag_scrolls_rather_than_rounding_to_nothing() {
        let mut app = wired();
        probe::click(&mut app, Target::ViewModeToggle);

        // A touchpad drag: ten reports, each a tenth of a notch. Quantised to
        // whole rows one at a time, every one of these rounds to zero and the
        // document never moves -- which is why continuous scrolling takes the
        // distance directly instead of going through the row accumulator.
        let mut moved = false;
        for _ in 0..10 {
            moved |= app.scroll_by(-0.1);
        }
        assert!(moved, "a slow touchpad drag scrolled nothing");
        assert!(app.active_tab().unwrap().scroll_offset_y > 0.0);
    }

    #[test]
    fn a_slow_touchpad_drag_still_eventually_turns_a_page() {
        let mut app = wired();
        assert_eq!(app.active_tab().unwrap().view_mode, ViewMode::SinglePage);

        // The same drag in single-page mode, where a page turn *is* quantised.
        // Each tenth of a notch is too small to turn a page on its own, so this
        // only works if the leftovers are banked rather than discarded.
        let mut moved = false;
        for _ in 0..10 {
            moved |= app.scroll_by(-0.1);
        }
        assert!(moved, "a slow touchpad drag turned no pages at all");
        assert_eq!(app.active_tab().unwrap().current_page, 1);
    }

    #[test]
    fn a_scroll_wheel_event_reaches_the_document() {
        let mut app = wired();
        probe::click(&mut app, Target::ViewModeToggle);
        assert!(app.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Scroll { dx: 0.0, dy: -3.0 },
            x: WINDOW_WIDTH / 2.0,
            y: WINDOW_HEIGHT / 2.0,
        })));
        assert!(app.active_tab().unwrap().scroll_offset_y > 0.0);
    }

    #[test]
    fn a_sideways_flick_does_not_scroll_the_document_downwards() {
        let mut app = wired();
        probe::click(&mut app, Target::ViewModeToggle);
        assert!(!app.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Scroll { dx: 5.0, dy: 0.0 },
            x: WINDOW_WIDTH / 2.0,
            y: WINDOW_HEIGHT / 2.0,
        })));
        assert_eq!(app.active_tab().unwrap().scroll_offset_y, 0.0);
    }

    // -- Search ---------------------------------------------------------------

    #[test]
    fn the_search_button_opens_the_bar_focused() {
        let mut app = wired();
        assert!(probe::rect_of(&app, Target::SearchField).is_none());

        probe::click(&mut app, Target::SearchToggle);
        assert!(app.search.active);
        assert!(
            app.search_focused,
            "a find bar you have to click after summoning wasted the summon"
        );
        assert!(probe::rect_of(&app, Target::SearchField).is_some());
    }

    #[test]
    fn typing_reaches_the_query_only_while_the_field_is_focused() {
        let mut app = wired();
        probe::click(&mut app, Target::SearchToggle);
        probe::type_str(&mut app, "Lorem");
        assert_eq!(app.search.query, "Lorem");
        assert!(
            !app.search.results.is_empty(),
            "the query matches the sample text, so the count must not read zero"
        );

        // Clicking the page takes the caret away; further typing pages the
        // document instead of editing the query.
        probe::click(&mut app, Target::Document);
        assert!(!app.search_focused);
        probe::type_str(&mut app, "xyz");
        assert_eq!(
            app.search.query, "Lorem",
            "typing leaked into an unfocused box"
        );
    }

    #[test]
    fn clicking_the_field_takes_the_caret_back() {
        let mut app = wired();
        probe::click(&mut app, Target::SearchToggle);
        probe::click(&mut app, Target::Document);
        assert!(!app.search_focused);

        probe::click(&mut app, Target::SearchField);
        assert!(app.search_focused);
        probe::type_str(&mut app, "ip");
        assert_eq!(app.search.query, "ip");
    }

    #[test]
    fn the_match_buttons_do_not_steal_the_caret() {
        let mut app = wired();
        probe::click(&mut app, Target::SearchToggle);
        probe::type_str(&mut app, "Lorem");
        probe::click(&mut app, Target::SearchNext);
        assert!(
            app.search_focused,
            "stepping to the next match must not close the caret out of the box"
        );
    }

    #[test]
    fn backspace_removes_a_whole_character_not_a_byte() {
        let mut app = wired();
        probe::click(&mut app, Target::SearchToggle);
        probe::type_str(&mut app, "Отчёт");
        probe::key(&mut app, &probe::press(Key::Backspace));
        assert_eq!(
            app.search.query, "Отчё",
            "backspace cut a multi-byte character in half"
        );
    }

    #[test]
    fn backspace_on_an_empty_query_is_not_a_panic() {
        let mut app = wired();
        probe::click(&mut app, Target::SearchToggle);
        assert!(!probe::key(&mut app, &probe::press(Key::Backspace)));
        assert!(app.search.query.is_empty());
    }

    #[test]
    fn stepping_through_matches_pages_the_document_to_them() {
        let mut app = wired();
        probe::click(&mut app, Target::SearchToggle);
        // "Page 4" appears only in the fourth page's own text span.
        probe::type_str(&mut app, "Page 4");
        assert!(
            !app.search.results.is_empty(),
            "the query found nothing to step to"
        );
        assert_eq!(
            app.active_tab().unwrap().current_page,
            3,
            "a highlight the user cannot see is not a search result"
        );
    }

    #[test]
    fn enter_steps_to_the_next_match() {
        let mut app = wired();
        probe::click(&mut app, Target::SearchToggle);
        probe::type_str(&mut app, "Lorem");
        let first = app.search.current_match;
        probe::key(&mut app, &probe::press(Key::Enter));
        assert_ne!(
            app.search.current_match, first,
            "Enter in a find bar must advance the match"
        );
    }

    #[test]
    fn the_match_buttons_wrap_in_both_directions() {
        let mut app = wired();
        probe::click(&mut app, Target::SearchToggle);
        probe::type_str(&mut app, "Lorem");
        let count = app.search.results.len();
        assert!(
            count > 1,
            "this test needs more than one match, got {count}"
        );

        assert_eq!(app.search.current_match, Some(0));
        probe::click(&mut app, Target::SearchPrev);
        assert_eq!(
            app.search.current_match,
            Some(count - 1),
            "stepping back from the first match should wrap to the last"
        );
        probe::click(&mut app, Target::SearchNext);
        assert_eq!(app.search.current_match, Some(0));
    }

    #[test]
    fn escape_closes_the_search_bar_from_anywhere() {
        let mut app = wired();
        probe::click(&mut app, Target::SearchToggle);
        probe::type_str(&mut app, "Lorem");
        probe::click(&mut app, Target::Document); // unfocused, bar still up

        assert!(probe::key(&mut app, &probe::press(Key::Escape)));
        assert!(!app.search.active);
        assert!(app.search.query.is_empty());
        assert!(probe::rect_of(&app, Target::SearchField).is_none());
    }

    #[test]
    fn closing_the_bar_with_the_button_clears_the_highlights_too() {
        let mut app = wired();
        probe::click(&mut app, Target::SearchToggle);
        probe::type_str(&mut app, "Lorem");
        assert!(!app.search.results.is_empty());

        probe::click(&mut app, Target::SearchToggle);
        assert!(!app.search.active);
        assert!(
            app.search.results.is_empty(),
            "highlights outlived the bar that explained them"
        );
    }

    #[test]
    fn escape_with_no_search_bar_is_not_swallowed_as_a_change() {
        let mut app = wired();
        assert!(!probe::key(&mut app, &probe::press(Key::Escape)));
    }

    // -- Keyboard -------------------------------------------------------------

    #[test]
    fn the_arrow_keys_page_the_document() {
        let mut app = wired();
        probe::key(&mut app, &probe::press(Key::Right));
        assert_eq!(app.active_tab().unwrap().current_page, 1);
        probe::key(&mut app, &probe::press(Key::Down));
        assert_eq!(app.active_tab().unwrap().current_page, 2);
        probe::key(&mut app, &probe::press(Key::Left));
        assert_eq!(app.active_tab().unwrap().current_page, 1);
        probe::key(&mut app, &probe::press(Key::PageUp));
        assert_eq!(app.active_tab().unwrap().current_page, 0);
    }

    #[test]
    fn home_and_end_reach_the_ends_of_the_document() {
        let mut app = wired();
        probe::key(&mut app, &probe::press(Key::End));
        assert_eq!(app.active_tab().unwrap().current_page, 4);
        probe::key(&mut app, &probe::press(Key::Home));
        assert_eq!(app.active_tab().unwrap().current_page, 0);
    }

    #[test]
    fn a_key_release_is_not_a_second_keypress() {
        let mut app = wired();
        let mut release = probe::press(Key::Right);
        release.pressed = false;
        assert!(!app.handle_key(&release));
        assert_eq!(app.active_tab().unwrap().current_page, 0);
    }

    #[test]
    fn paging_in_continuous_mode_moves_the_scroll_offset_too() {
        let mut app = wired();
        probe::click(&mut app, Target::ViewModeToggle);
        probe::key(&mut app, &probe::press(Key::End));

        let tab = app.active_tab().unwrap();
        assert_eq!(tab.current_page, 4);
        assert!(
            tab.scroll_offset_y > 0.0,
            "the status bar reads the last page but the view never moved"
        );
    }

    // -- The window strap -----------------------------------------------------

    #[test]
    fn the_close_button_exits() {
        let mut app = wired();
        assert_eq!(app.on_event(&Event::CloseRequested), Response::Exit);
    }

    #[test]
    fn ctrl_q_exits() {
        let mut app = wired();
        assert_eq!(
            app.on_event(&Event::Key(probe::ctrl(Key::Q))),
            Response::Exit
        );
    }

    #[test]
    fn a_bare_q_types_rather_than_quitting() {
        let mut app = wired();
        assert_ne!(
            app.on_event(&Event::Key(probe::typing("q"))),
            Response::Exit,
            "an unmodified q must not close the window"
        );
    }

    #[test]
    fn ctrl_f_opens_the_search_bar_focused() {
        let mut app = wired();
        assert_eq!(
            app.on_event(&Event::Key(probe::ctrl(Key::F))),
            Response::Redraw
        );
        assert!(app.search.active);
        assert!(app.search_focused);

        // Pressed again with the bar already up, it takes the caret back rather
        // than closing what the user just asked for.
        probe::click(&mut app, Target::Document);
        app.on_event(&Event::Key(probe::ctrl(Key::F)));
        assert!(app.search.active);
        assert!(app.search_focused);
    }

    #[test]
    fn ctrl_t_and_ctrl_w_open_and_close_a_tab() {
        let mut app = wired();
        app.on_event(&Event::Key(probe::ctrl(Key::T)));
        assert_eq!(app.tabs.len(), 2);
        app.on_event(&Event::Key(probe::ctrl(Key::W)));
        assert_eq!(app.tabs.len(), 1);
    }

    #[test]
    fn a_click_on_scenery_asks_for_no_repaint() {
        let mut app = wired();
        // The very top-left corner is toolbar background, not a button.
        assert_eq!(
            app.on_event(&Event::Mouse(MouseEvent {
                kind: MouseEventKind::Press(MouseButton::Left),
                x: 1.0,
                y: 1.0,
            })),
            Response::Idle
        );
    }

    #[test]
    fn render_uses_the_size_it_was_handed() {
        let mut app = wired();
        let tree = app.render(900.0, 600.0);
        assert!(!tree.is_empty());
        assert_eq!(app.window_width, 900.0);
        assert_eq!(app.window_height, 600.0);
        assert_eq!(
            app.target_at(1.0, 1.0),
            app.frame(900.0, 600.0).hit_test(1.0, 1.0),
            "the click test and the paint disagree about the window size"
        );
    }

    #[test]
    fn a_resize_event_is_what_moves_the_controls() {
        let mut app = wired();
        let before = probe::rect_of(&app, Target::SearchToggle).unwrap();
        app.handle_event(&Event::Resize {
            width: 1500,
            height: 780,
        });
        let after = app
            .frame(app.window_width, app.window_height)
            .rect_of(|t| *t == Target::SearchToggle)
            .unwrap();
        assert!(after.x > before.x);
    }

    #[test]
    fn a_nonsense_window_size_does_not_reach_the_geometry() {
        let mut app = wired();
        app.resize(f32::NAN, -5.0);
        assert_eq!(app.window_width, 0.0);
        assert_eq!(app.window_height, 0.0);
        // And drawing at it is still a well-formed, empty frame.
        let frame = app.frame(app.window_width, app.window_height);
        assert!(
            frame.is_balanced(),
            "a degenerate frame left a clip unclosed"
        );
        assert!(frame.hits().is_empty());
    }

    #[test]
    fn the_title_names_the_open_document() {
        let mut app = wired();
        assert!(
            app.title().contains("Sample Document"),
            "the title bar should say what is open, got {:?}",
            app.title()
        );

        probe::click(&mut app, Target::NewTab);
        assert_eq!(
            app.title(),
            "PDF Viewer",
            "an empty tab should not claim to be showing a document"
        );
    }

    #[test]
    fn every_frame_closes_the_clips_it_opens() {
        let sizes = [
            (WINDOW_WIDTH, WINDOW_HEIGHT),
            (320.0, 240.0),
            (0.0, 0.0),
            (WINDOW_WIDTH, TOOLBAR_HEIGHT),
            (2400.0, 1600.0),
        ];
        for panel in [
            SidebarPanel::Thumbnails,
            SidebarPanel::Bookmarks,
            SidebarPanel::Annotations,
        ] {
            for (w, h) in sizes {
                let mut app = wired();
                app.search.active = true;
                app.active_tab_mut().unwrap().sidebar_panel = panel;
                assert!(
                    app.frame(w, h).is_balanced(),
                    "the frame at {w}x{h} with the {panel:?} panel left a clip open"
                );
            }
        }
    }

    #[test]
    fn a_short_tab_title_is_drawn_verbatim() {
        // Otherwise "it fits" would be satisfiable by drawing nothing.
        let tree = tab_bar_with_titles(&["Отчёт".to_string()]);
        let drawn: Vec<String> = tree
            .commands
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        assert!(
            drawn.iter().any(|t| t == "Отчёт"),
            "the short title was cut; got {drawn:?}"
        );
    }
}
