//! Slate OS Font Manager — Graphical Font Management and Preview
//!
//! A graphical application for managing system and user fonts. Provides
//! font browsing by category, family, and style; live previews at multiple
//! sizes; install/uninstall operations; and global font rendering settings
//! (hinting, antialiasing, subpixel order).
//!
//! Uses the guitk library for rendering. Dark theme (Catppuccin Mocha).

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;

// ============================================================================
// Catppuccin Mocha theme colors
// ============================================================================

/// Background (base)
const COL_BASE: Color = Color::from_hex(0x1E1E2E);
/// Surface layer 0
const COL_SURFACE0: Color = Color::from_hex(0x313244);
/// Surface layer 1 (sidebar)
const COL_SURFACE1: Color = Color::from_hex(0x45475A);
/// Surface layer 2 (hover)
#[allow(dead_code)]
const COL_SURFACE2: Color = Color::from_hex(0x585B70);
/// Overlay 0
#[allow(dead_code)]
const COL_OVERLAY0: Color = Color::from_hex(0x6C7086);
/// Main text
const COL_TEXT: Color = Color::from_hex(0xCDD6F4);
/// Subtext (dimmer)
const COL_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
/// Subtext (dimmest)
#[allow(dead_code)]
const COL_SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
/// Accent (blue)
const COL_ACCENT: Color = Color::from_hex(0x89B4FA);
/// Green
#[allow(dead_code)]
const COL_GREEN: Color = Color::from_hex(0xA6E3A1);
/// Red (for destructive actions)
const COL_RED: Color = Color::from_hex(0xF38BA8);
/// Peach
#[allow(dead_code)]
const COL_PEACH: Color = Color::from_hex(0xFAB387);
/// Lavender
#[allow(dead_code)]
const COL_LAVENDER: Color = Color::from_hex(0xB4BEFE);
/// Teal
#[allow(dead_code)]
const COL_TEAL: Color = Color::from_hex(0x94E2D5);
/// Mauve
#[allow(dead_code)]
const COL_MAUVE: Color = Color::from_hex(0xCBA6F7);
/// Crust (darkest)
const COL_CRUST: Color = Color::from_hex(0x11111B);
/// Mantle (between crust and base)
const COL_MANTLE: Color = Color::from_hex(0x181825);

// ============================================================================
// Layout constants
// ============================================================================

const SIDEBAR_WIDTH: f32 = 200.0;
const PREVIEW_PANEL_WIDTH: f32 = 300.0;
const TOOLBAR_HEIGHT: f32 = 48.0;
const CATEGORY_ITEM_HEIGHT: f32 = 36.0;
const FONT_LIST_ITEM_HEIGHT: f32 = 56.0;
const SIDEBAR_PADDING: f32 = 8.0;
const CONTENT_PADDING: f32 = 16.0;
const PREVIEW_SIZE_LABELS: &[f32] = &[12.0, 18.0, 24.0, 36.0, 48.0];

const DEFAULT_WINDOW_WIDTH: f32 = 1000.0;
const DEFAULT_WINDOW_HEIGHT: f32 = 700.0;

// ============================================================================
// Domain types
// ============================================================================

/// Font style variant within a family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontStyle {
    Regular,
    Bold,
    Italic,
    BoldItalic,
    Light,
    Medium,
    SemiBold,
}

impl FontStyle {
    /// Display label for this style.
    fn label(self) -> &'static str {
        match self {
            Self::Regular => "Regular",
            Self::Bold => "Bold",
            Self::Italic => "Italic",
            Self::BoldItalic => "Bold Italic",
            Self::Light => "Light",
            Self::Medium => "Medium",
            Self::SemiBold => "SemiBold",
        }
    }
}

/// Font classification category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontCategory {
    SansSerif,
    Serif,
    Monospace,
    Display,
    Handwriting,
    Symbol,
}

impl FontCategory {
    const ALL: &[Self] = &[
        Self::SansSerif,
        Self::Serif,
        Self::Monospace,
        Self::Display,
        Self::Handwriting,
        Self::Symbol,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::SansSerif => "Sans Serif",
            Self::Serif => "Serif",
            Self::Monospace => "Monospace",
            Self::Display => "Display",
            Self::Handwriting => "Handwriting",
            Self::Symbol => "Symbol",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::SansSerif => "Aa",
            Self::Serif => "Tt",
            Self::Monospace => ">_",
            Self::Display => "Ab",
            Self::Handwriting => "Hh",
            Self::Symbol => "#",
        }
    }
}

/// Font file format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontFormat {
    TrueType,
    OpenType,
    Woff2,
    Bitmap,
}

impl FontFormat {
    fn label(self) -> &'static str {
        match self {
            Self::TrueType => "TrueType",
            Self::OpenType => "OpenType",
            Self::Woff2 => "WOFF2",
            Self::Bitmap => "Bitmap",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::TrueType => ".ttf",
            Self::OpenType => ".otf",
            Self::Woff2 => ".woff2",
            Self::Bitmap => ".bdf",
        }
    }
}

/// Error type for font operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontError {
    /// Font with the given ID was not found.
    NotFound,
    /// Cannot uninstall or modify a system font.
    SystemFont,
    /// A font with the same family/style is already installed.
    AlreadyInstalled,
    /// The font file format is unsupported or corrupt.
    InvalidFormat,
    /// An I/O error occurred reading/writing font files.
    IoError(String),
}

impl core::fmt::Display for FontError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotFound => write!(f, "Font not found"),
            Self::SystemFont => write!(f, "Cannot modify system font"),
            Self::AlreadyInstalled => write!(f, "Font already installed"),
            Self::InvalidFormat => write!(f, "Invalid font format"),
            Self::IoError(msg) => write!(f, "I/O error: {msg}"),
        }
    }
}

// ============================================================================
// FontInfo — describes a single installed font
// ============================================================================

/// Metadata for a single installed font face.
#[derive(Clone, Debug)]
pub struct FontInfo {
    /// Unique identifier for this font face.
    pub id: u64,
    /// Font family name (e.g., "Inter", "JetBrains Mono").
    pub family: String,
    /// Style variant within the family.
    pub style: FontStyle,
    /// File format.
    pub format: FontFormat,
    /// Classification category.
    pub category: FontCategory,
    /// Path to the font file on disk.
    pub path: String,
    /// Font version string.
    pub version: String,
    /// Whether this is a system font (cannot be uninstalled).
    pub system: bool,
    /// Whether the font is currently enabled for use.
    pub enabled: bool,
    /// Number of glyphs in the font.
    pub glyph_count: u32,
}

// ============================================================================
// FontCollection — manages the set of installed fonts
// ============================================================================

/// Manages the collection of installed fonts.
pub struct FontCollection {
    /// All installed fonts.
    pub fonts: Vec<FontInfo>,
    /// Counter for generating unique IDs.
    next_id: u64,
}

impl Default for FontCollection {
    fn default() -> Self {
        Self::new()
    }
}

impl FontCollection {
    /// Create an empty font collection.
    pub fn new() -> Self {
        Self {
            fonts: Vec::new(),
            next_id: 1,
        }
    }

    /// Create a font collection pre-populated with common default fonts.
    pub fn new_with_defaults() -> Self {
        let mut coll = Self::new();
        coll.add_default_fonts();
        coll
    }

    /// Install a new font from the given path.
    ///
    /// Infers family name and format from the path. Returns the new font's ID.
    pub fn install(&mut self, path: &str) -> Result<u64, FontError> {
        let format = Self::detect_format(path)?;
        let family = Self::family_from_path(path);

        // Check for duplicate installation.
        if self
            .fonts
            .iter()
            .any(|f| f.family == family && f.style == FontStyle::Regular)
        {
            return Err(FontError::AlreadyInstalled);
        }

        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).unwrap_or(self.next_id);

        self.fonts.push(FontInfo {
            id,
            family,
            style: FontStyle::Regular,
            format,
            category: FontCategory::SansSerif,
            path: path.to_string(),
            version: String::from("1.0"),
            system: false,
            enabled: true,
            glyph_count: 200,
        });

        Ok(id)
    }

    /// Uninstall a font by ID. System fonts cannot be uninstalled.
    pub fn uninstall(&mut self, id: u64) -> Result<(), FontError> {
        let idx = self
            .fonts
            .iter()
            .position(|f| f.id == id)
            .ok_or(FontError::NotFound)?;
        // `.get` rather than `[idx]`: the index came from `position` on this
        // same vector so it is in range, but stating that in the operation
        // survives an edit that moves the two apart, and a comment does not.
        if self.fonts.get(idx).is_some_and(|f| f.system) {
            return Err(FontError::SystemFont);
        }
        self.fonts.remove(idx);
        Ok(())
    }

    /// Toggle the enabled/disabled state of a font by ID.
    pub fn toggle_enabled(&mut self, id: u64) {
        if let Some(font) = self.fonts.iter_mut().find(|f| f.id == id) {
            font.enabled = !font.enabled;
        }
    }

    /// Return a sorted list of unique family names.
    pub fn families(&self) -> Vec<String> {
        let mut names: Vec<String> = self.fonts.iter().map(|f| f.family.clone()).collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    /// Return all fonts belonging to a given family.
    pub fn by_family(&self, family: &str) -> Vec<&FontInfo> {
        self.fonts.iter().filter(|f| f.family == family).collect()
    }

    /// Return all fonts in a given category.
    pub fn by_category(&self, cat: FontCategory) -> Vec<&FontInfo> {
        self.fonts.iter().filter(|f| f.category == cat).collect()
    }

    /// Search fonts by family name (case-insensitive substring match).
    pub fn search(&self, query: &str) -> Vec<&FontInfo> {
        let query_lower = query.to_lowercase();
        self.fonts
            .iter()
            .filter(|f| f.family.to_lowercase().contains(&query_lower))
            .collect()
    }

    /// Look up a font by ID.
    pub fn get(&self, id: u64) -> Option<&FontInfo> {
        self.fonts.iter().find(|f| f.id == id)
    }

    /// Detect font format from file extension.
    fn detect_format(path: &str) -> Result<FontFormat, FontError> {
        if path.ends_with(".ttf") || path.ends_with(".ttc") {
            Ok(FontFormat::TrueType)
        } else if path.ends_with(".otf") {
            Ok(FontFormat::OpenType)
        } else if path.ends_with(".woff2") {
            Ok(FontFormat::Woff2)
        } else if path.ends_with(".bdf") || path.ends_with(".pcf") {
            Ok(FontFormat::Bitmap)
        } else {
            Err(FontError::InvalidFormat)
        }
    }

    /// Extract a plausible family name from a file path.
    fn family_from_path(path: &str) -> String {
        // Take the filename stem, strip extension, replace hyphens/underscores with spaces.
        let filename = path
            .rsplit('/')
            .next()
            .or_else(|| path.rsplit('\\').next())
            .unwrap_or(path);
        let stem = filename
            .rsplit_once('.')
            .map(|(s, _)| s)
            .unwrap_or(filename);
        stem.replace(['-', '_'], " ")
    }

    /// Populate with default system fonts covering all categories.
    fn add_default_fonts(&mut self) {
        let defaults: &[(&str, FontStyle, FontFormat, FontCategory, u32)] = &[
            (
                "Inter",
                FontStyle::Regular,
                FontFormat::OpenType,
                FontCategory::SansSerif,
                2548,
            ),
            (
                "Inter",
                FontStyle::Bold,
                FontFormat::OpenType,
                FontCategory::SansSerif,
                2548,
            ),
            (
                "Inter",
                FontStyle::Italic,
                FontFormat::OpenType,
                FontCategory::SansSerif,
                2548,
            ),
            (
                "Roboto",
                FontStyle::Regular,
                FontFormat::TrueType,
                FontCategory::SansSerif,
                1294,
            ),
            (
                "Roboto",
                FontStyle::Light,
                FontFormat::TrueType,
                FontCategory::SansSerif,
                1294,
            ),
            (
                "Noto Sans",
                FontStyle::Regular,
                FontFormat::TrueType,
                FontCategory::SansSerif,
                3440,
            ),
            (
                "Noto Sans",
                FontStyle::Bold,
                FontFormat::TrueType,
                FontCategory::SansSerif,
                3440,
            ),
            (
                "Noto Serif",
                FontStyle::Regular,
                FontFormat::TrueType,
                FontCategory::Serif,
                3200,
            ),
            (
                "Noto Serif",
                FontStyle::Italic,
                FontFormat::TrueType,
                FontCategory::Serif,
                3200,
            ),
            (
                "Libre Baskerville",
                FontStyle::Regular,
                FontFormat::OpenType,
                FontCategory::Serif,
                820,
            ),
            (
                "JetBrains Mono",
                FontStyle::Regular,
                FontFormat::TrueType,
                FontCategory::Monospace,
                1036,
            ),
            (
                "JetBrains Mono",
                FontStyle::Bold,
                FontFormat::TrueType,
                FontCategory::Monospace,
                1036,
            ),
            (
                "Fira Code",
                FontStyle::Regular,
                FontFormat::TrueType,
                FontCategory::Monospace,
                1588,
            ),
            (
                "Source Code Pro",
                FontStyle::Regular,
                FontFormat::OpenType,
                FontCategory::Monospace,
                974,
            ),
            (
                "Source Code Pro",
                FontStyle::Medium,
                FontFormat::OpenType,
                FontCategory::Monospace,
                974,
            ),
            (
                "Lobster",
                FontStyle::Regular,
                FontFormat::TrueType,
                FontCategory::Display,
                490,
            ),
            (
                "Pacifico",
                FontStyle::Regular,
                FontFormat::TrueType,
                FontCategory::Handwriting,
                370,
            ),
            (
                "Dancing Script",
                FontStyle::Regular,
                FontFormat::TrueType,
                FontCategory::Handwriting,
                534,
            ),
            (
                "Noto Emoji",
                FontStyle::Regular,
                FontFormat::TrueType,
                FontCategory::Symbol,
                3610,
            ),
        ];

        for (family, style, format, category, glyph_count) in defaults {
            let id = self.next_id;
            self.next_id = self.next_id.checked_add(1).unwrap_or(self.next_id);

            let ext = format.extension();
            let style_suffix = match style {
                FontStyle::Regular => "",
                FontStyle::Bold => "-Bold",
                FontStyle::Italic => "-Italic",
                FontStyle::BoldItalic => "-BoldItalic",
                FontStyle::Light => "-Light",
                FontStyle::Medium => "-Medium",
                FontStyle::SemiBold => "-SemiBold",
            };
            let safe_name = family.replace(' ', "");
            let path = format!("/usr/share/fonts/{safe_name}{style_suffix}{ext}");

            self.fonts.push(FontInfo {
                id,
                family: family.to_string(),
                style: *style,
                format: *format,
                category: *category,
                path,
                version: String::from("1.0"),
                system: true,
                enabled: true,
                glyph_count: *glyph_count,
            });
        }
    }
}

// ============================================================================
// Hint mode and antialiasing settings
// ============================================================================

/// Font hinting mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HintMode {
    /// No hinting; glyph outlines used as-is.
    None,
    /// Light hinting for improved readability without distortion.
    Slight,
    /// Medium hinting (balanced).
    Medium,
    /// Full hinting; snaps outlines to pixel grid.
    Full,
}

impl HintMode {
    const ALL: &[Self] = &[Self::None, Self::Slight, Self::Medium, Self::Full];

    fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Slight => "Slight",
            Self::Medium => "Medium",
            Self::Full => "Full",
        }
    }
}

/// Antialiasing mode for font rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AntialiasingMode {
    /// No antialiasing (aliased text).
    None,
    /// Grayscale antialiasing.
    Grayscale,
    /// Subpixel antialiasing (LCD rendering).
    Subpixel,
}

impl AntialiasingMode {
    const ALL: &[Self] = &[Self::None, Self::Grayscale, Self::Subpixel];

    fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Grayscale => "Grayscale",
            Self::Subpixel => "Subpixel",
        }
    }
}

/// Subpixel layout order for LCD rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubpixelOrder {
    /// Horizontal RGB.
    Rgb,
    /// Horizontal BGR.
    Bgr,
    /// Vertical RGB.
    VRgb,
    /// Vertical BGR.
    VBgr,
}

impl SubpixelOrder {
    const ALL: &[Self] = &[Self::Rgb, Self::Bgr, Self::VRgb, Self::VBgr];

    fn label(self) -> &'static str {
        match self {
            Self::Rgb => "RGB",
            Self::Bgr => "BGR",
            Self::VRgb => "Vertical RGB",
            Self::VBgr => "Vertical BGR",
        }
    }
}

/// Global font rendering configuration.
#[derive(Clone, Debug)]
pub struct RenderSettings {
    /// Default font size in points.
    pub default_size_pt: f32,
    /// Hinting mode.
    pub hinting: HintMode,
    /// Antialiasing mode.
    pub antialiasing: AntialiasingMode,
    /// Subpixel layout order (only relevant when antialiasing is Subpixel).
    pub subpixel_order: SubpixelOrder,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            default_size_pt: 11.0,
            hinting: HintMode::Slight,
            antialiasing: AntialiasingMode::Subpixel,
            subpixel_order: SubpixelOrder::Rgb,
        }
    }
}

// ============================================================================
// Font preview state
// ============================================================================

/// State for the font preview panel.
#[derive(Clone, Debug)]
pub struct FontPreview {
    /// Sample text to display.
    pub text: String,
    /// Preview size in points.
    pub size_pt: f32,
    /// Whether the user has typed custom preview text.
    pub custom_text: bool,
}

impl Default for FontPreview {
    fn default() -> Self {
        Self {
            text: String::from("The quick brown fox jumps over the lazy dog"),
            size_pt: 24.0,
            custom_text: false,
        }
    }
}

// ============================================================================
// Filter mode for the font list
// ============================================================================

/// One clickable row of the sidebar.
///
/// Exists so the renderer and the hit test can walk one list instead of two
/// copies of the same arithmetic. See `FontManagerState::sidebar_rows`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SidebarRow {
    /// One of the three filter rows at the top.
    Filter(FilterMode),
    /// One of the category rows below the separator.
    Category(FontCategory),
}

/// How the font list is filtered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterMode {
    /// Show all fonts.
    All,
    /// Show only system-provided fonts.
    System,
    /// Show only user-installed fonts.
    User,
    /// Show fonts in a specific category.
    Category,
}

// ============================================================================
// FontManagerState — full application state
// ============================================================================

/// Complete state for the Font Manager application.
pub struct FontManagerState {
    /// The font collection (all installed fonts).
    pub collection: FontCollection,
    /// Global font rendering settings.
    pub render_settings: RenderSettings,
    /// Preview panel state.
    pub preview: FontPreview,
    /// Currently selected font ID.
    pub selected_font: Option<u64>,
    /// Selected category in the sidebar (when filter is Category).
    pub selected_category: Option<FontCategory>,
    /// Current filter mode.
    pub filter_mode: FilterMode,
    /// Search query string.
    pub search_query: String,
    /// Whether the rendering settings panel is visible.
    pub show_settings: bool,
    /// Scroll offset for the font list (vertical pixels).
    pub list_scroll_y: f32,
    /// Window dimensions.
    pub window_width: f32,
    pub window_height: f32,
}

impl Default for FontManagerState {
    fn default() -> Self {
        Self::new()
    }
}

impl FontManagerState {
    /// Create a new font manager state with default fonts and settings.
    pub fn new() -> Self {
        let collection = FontCollection::new_with_defaults();
        // Select the first font by default.
        let first_id = collection.fonts.first().map(|f| f.id);
        Self {
            collection,
            render_settings: RenderSettings::default(),
            preview: FontPreview::default(),
            selected_font: first_id,
            selected_category: None,
            filter_mode: FilterMode::All,
            search_query: String::new(),
            show_settings: false,
            list_scroll_y: 0.0,
            window_width: DEFAULT_WINDOW_WIDTH,
            window_height: DEFAULT_WINDOW_HEIGHT,
        }
    }

    /// Return the fonts that should be displayed given the current filter/search.
    pub fn visible_fonts(&self) -> Vec<&FontInfo> {
        let base: Vec<&FontInfo> = match self.filter_mode {
            FilterMode::All => self.collection.fonts.iter().collect(),
            FilterMode::System => self.collection.fonts.iter().filter(|f| f.system).collect(),
            FilterMode::User => self.collection.fonts.iter().filter(|f| !f.system).collect(),
            FilterMode::Category => {
                if let Some(cat) = self.selected_category {
                    self.collection.by_category(cat)
                } else {
                    self.collection.fonts.iter().collect()
                }
            }
        };

        if self.search_query.is_empty() {
            base
        } else {
            let query_lower = self.search_query.to_lowercase();
            base.into_iter()
                .filter(|f| f.family.to_lowercase().contains(&query_lower))
                .collect()
        }
    }

    /// Return unique family names from the currently visible fonts.
    pub fn visible_families(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .visible_fonts()
            .iter()
            .map(|f| f.family.clone())
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    // ========================================================================
    // Event handling
    // ========================================================================

    /// Handle an input event. Returns whether it was consumed.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Resize { width, height } => {
                self.window_width = *width as f32;
                self.window_height = *height as f32;
                EventResult::Consumed
            }
            Event::Key(key_ev) if key_ev.pressed => self.handle_key(key_ev),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            _ => EventResult::Ignored,
        }
    }

    /// What a sidebar row selects when it is clicked.
    ///
    /// The renderer and the hit test walk the same list, so a row cannot be
    /// drawn in one place and clicked in another. Writing the y positions out
    /// twice is the defect `apps/mixer` shipped — its slider handler took a
    /// column index and a fraction that nothing in the program computed — and
    /// the cheapest way not to have it is to have one list.
    fn sidebar_rows() -> Vec<(f32, SidebarRow)> {
        let mut rows = Vec::new();
        // Filter heading.
        let mut y = TOOLBAR_HEIGHT + SIDEBAR_PADDING + 24.0;
        for row in [
            SidebarRow::Filter(FilterMode::All),
            SidebarRow::Filter(FilterMode::System),
            SidebarRow::Filter(FilterMode::User),
        ] {
            rows.push((y, row));
            y += CATEGORY_ITEM_HEIGHT;
        }
        // Separator (8 above, a line, 12 below) then the Categories heading.
        y += 8.0 + 12.0 + 24.0;
        for cat in FontCategory::ALL {
            rows.push((y, SidebarRow::Category(*cat)));
            y += CATEGORY_ITEM_HEIGHT;
        }
        rows
    }

    /// The y at which the first font family is drawn, scroll included.
    ///
    /// The count header sits above it and is 24px tall; both the renderer and
    /// the hit test start from here.
    fn font_list_top(&self) -> f32 {
        TOOLBAR_HEIGHT + CONTENT_PADDING - self.list_scroll_y + 24.0
    }

    /// The sidebar row a point is over, if it is over one.
    fn sidebar_row_at(x: f32, y: f32) -> Option<SidebarRow> {
        if x < 0.0 || x >= SIDEBAR_WIDTH || y < TOOLBAR_HEIGHT {
            return None;
        }
        Self::sidebar_rows()
            .into_iter()
            .find(|(top, _)| y >= *top && y < top + CATEGORY_ITEM_HEIGHT)
            .map(|(_, row)| row)
    }

    /// The index into [`Self::visible_families`] a point is over.
    fn font_row_at(&self, x: f32, y: f32) -> Option<usize> {
        let list_right = self.window_width - PREVIEW_PANEL_WIDTH;
        if x < SIDEBAR_WIDTH || x >= list_right || y < TOOLBAR_HEIGHT {
            return None;
        }
        let offset = y - self.font_list_top();
        if offset < 0.0 {
            return None;
        }
        // `as usize` on a non-negative finite f32 is the floor, which is the
        // row containing the point.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let index = (offset / FONT_LIST_ITEM_HEIGHT) as usize;
        (index < self.visible_families().len()).then_some(index)
    }

    /// Handle a mouse event.
    ///
    /// **This app had no mouse handling at all until 2026-09-03.** `MouseEvent`,
    /// `MouseButton` and `MouseEventKind` were imported and never named again,
    /// which the file-wide `#[allow(unused_imports)]` is what kept quiet — a
    /// font manager whose list of fonts could not be clicked, only arrowed
    /// through. It was invisible because nothing had ever delivered it an
    /// event: `main` rendered one frame and asserted it was non-empty. See
    /// known-issues.md -> `TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR`, whose whole
    /// point is that an app nothing drives is an app whose gaps nobody meets.
    fn handle_mouse(&mut self, mouse: &MouseEvent) -> EventResult {
        if !matches!(mouse.kind, MouseEventKind::Press(MouseButton::Left)) {
            return EventResult::Ignored;
        }
        if let Some(row) = Self::sidebar_row_at(mouse.x, mouse.y) {
            match row {
                SidebarRow::Filter(mode) => {
                    self.filter_mode = mode;
                    self.selected_category = None;
                }
                SidebarRow::Category(cat) => {
                    self.filter_mode = FilterMode::Category;
                    self.selected_category = Some(cat);
                }
            }
            // The visible list has just changed under the selection, so a font
            // that is no longer in it must not stay selected: the preview panel
            // would go on showing a font the list does not offer.
            let still_visible = self
                .selected_font
                .is_some_and(|id| self.visible_fonts().iter().any(|f| f.id == id));
            if !still_visible {
                self.selected_font = self.visible_fonts().first().map(|f| f.id);
            }
            return EventResult::Consumed;
        }
        if let Some(index) = self.font_row_at(mouse.x, mouse.y) {
            let families = self.visible_families();
            let Some(family) = families.get(index) else {
                return EventResult::Ignored;
            };
            // The list is drawn by family and the selection is a font id, so
            // clicking a family selects its first visible variant — which is
            // the one whose name is under the pointer.
            if let Some(font) = self
                .visible_fonts()
                .into_iter()
                .find(|f| &f.family == family)
            {
                self.selected_font = Some(font.id);
                return EventResult::Consumed;
            }
        }
        EventResult::Ignored
    }

    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Up => {
                self.select_prev_font();
                EventResult::Consumed
            }
            Key::Down => {
                self.select_next_font();
                EventResult::Consumed
            }
            Key::Escape => {
                if self.show_settings {
                    self.show_settings = false;
                } else if !self.search_query.is_empty() {
                    self.search_query.clear();
                }
                EventResult::Consumed
            }
            Key::F if key.modifiers.ctrl => {
                // Ctrl+F: focus search (toggle for now)
                self.search_query.clear();
                EventResult::Consumed
            }
            Key::S if key.modifiers.ctrl => {
                // Ctrl+S: toggle settings panel
                self.show_settings = !self.show_settings;
                EventResult::Consumed
            }
            Key::Delete => {
                // Delete: uninstall selected font
                if let Some(id) = self.selected_font {
                    let _ = self.collection.uninstall(id);
                    // If the font was removed, clear selection.
                    if self.collection.get(id).is_none() {
                        self.selected_font = self.collection.fonts.first().map(|f| f.id);
                    }
                }
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn select_next_font(&mut self) {
        let visible = self.visible_fonts();
        if visible.is_empty() {
            return;
        }
        // The whole body is written against `first`/`get` rather than `[0]`
        // and `+ 1`: the emptiness check above makes every index below valid
        // *today*, and that is exactly the kind of guarantee that survives
        // until someone moves the check.
        let next = self
            .selected_font
            .and_then(|id| visible.iter().position(|f| f.id == id))
            .and_then(|pos| {
                let after = pos.checked_add(1).filter(|n| *n < visible.len());
                visible.get(after.unwrap_or(0))
            })
            .or_else(|| visible.first());
        if let Some(font) = next {
            self.selected_font = Some(font.id);
        }
    }

    fn select_prev_font(&mut self) {
        let visible = self.visible_fonts();
        if visible.is_empty() {
            return;
        }
        let prev = self
            .selected_font
            .and_then(|id| visible.iter().position(|f| f.id == id))
            .and_then(|pos| match pos.checked_sub(1) {
                Some(before) => visible.get(before),
                // Wrapping off the front lands on the last, which `last()`
                // gives without a `len() - 1` that would underflow on empty.
                None => visible.last(),
            })
            .or_else(|| visible.first());
        if let Some(font) = prev {
            self.selected_font = Some(font.id);
        }
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the complete Font Manager UI frame.
    ///
    /// Named `render_tree` and not `render`: at equal arity an inherent method
    /// silently wins method lookup over `oswindow::app::App::render`, so every
    /// existing call would keep compiling while testing the other function.
    pub fn render_tree(&self) -> RenderTree {
        let mut tree = RenderTree::new();

        // Window background
        tree.fill_rect(0.0, 0.0, self.window_width, self.window_height, COL_BASE);

        // Layout regions
        self.render_toolbar(&mut tree);
        self.render_sidebar(&mut tree);
        self.render_font_list(&mut tree);
        self.render_preview_panel(&mut tree);

        // Settings overlay (if visible)
        if self.show_settings {
            self.render_settings_panel(&mut tree);
        }

        tree
    }

    /// Render the top toolbar with action buttons and search.
    fn render_toolbar(&self, tree: &mut RenderTree) {
        // Toolbar background
        tree.fill_rect(0.0, 0.0, self.window_width, TOOLBAR_HEIGHT, COL_MANTLE);

        // Divider line below toolbar
        tree.push(RenderCommand::Line {
            x1: 0.0,
            y1: TOOLBAR_HEIGHT,
            x2: self.window_width,
            y2: TOOLBAR_HEIGHT,
            color: COL_SURFACE0,
            width: 1.0,
        });

        // Title
        text_bold(tree, 16.0, 14.0, "Font Manager", COL_TEXT, 18.0);

        // Action buttons (right-aligned)
        let btn_y = 10.0;
        let mut btn_x = self.window_width - 16.0;

        // Settings button
        btn_x -= 80.0;
        render_toolbar_button(tree, btn_x, btn_y, "Settings", COL_SURFACE1);

        // Uninstall button
        btn_x -= 90.0;
        render_toolbar_button(tree, btn_x, btn_y, "Uninstall", COL_RED);

        // Install button
        btn_x -= 80.0;
        render_toolbar_button(tree, btn_x, btn_y, "Install", COL_ACCENT);

        // Search box
        let search_x = 180.0;
        let search_w = 250.0;
        let search_y = 10.0;
        let search_h = 28.0;
        fill_rounded(
            tree,
            search_x,
            search_y,
            search_w,
            search_h,
            COL_SURFACE0,
            6.0,
        );
        if self.search_query.is_empty() {
            tree.text(
                search_x + 10.0,
                search_y + 7.0,
                "Search fonts...",
                COL_SUBTEXT0,
                13.0,
            );
        } else {
            tree.text(
                search_x + 10.0,
                search_y + 7.0,
                &self.search_query,
                COL_TEXT,
                13.0,
            );
        }
    }

    /// Render the left sidebar with filter categories.
    fn render_sidebar(&self, tree: &mut RenderTree) {
        let sidebar_y = TOOLBAR_HEIGHT;
        let sidebar_h = self.window_height - TOOLBAR_HEIGHT;

        // Sidebar background
        tree.fill_rect(0.0, sidebar_y, SIDEBAR_WIDTH, sidebar_h, COL_CRUST);

        // Divider line
        tree.push(RenderCommand::Line {
            x1: SIDEBAR_WIDTH,
            y1: sidebar_y,
            x2: SIDEBAR_WIDTH,
            y2: self.window_height,
            color: COL_SURFACE0,
            width: 1.0,
        });

        // The two headings and the separator are drawn relative to the rows
        // themselves, so the labels cannot drift away from the things they
        // label — `sidebar_rows` is the single source of both positions.
        let rows = Self::sidebar_rows();
        let filter_top = rows.first().map_or(sidebar_y, |(y, _)| *y);
        text_bold(
            tree,
            SIDEBAR_PADDING + 4.0,
            filter_top - 24.0 + 4.0,
            "FILTER",
            COL_SUBTEXT0,
            10.0,
        );

        let category_top = rows
            .iter()
            .find(|(_, row)| matches!(row, SidebarRow::Category(_)))
            .map_or(sidebar_y, |(y, _)| *y);
        // The separator sits 12px above the Categories heading, which is 24px
        // above the first category row.
        let separator_y = category_top - 24.0 - 12.0;
        tree.push(RenderCommand::Line {
            x1: SIDEBAR_PADDING,
            y1: separator_y,
            x2: SIDEBAR_WIDTH - SIDEBAR_PADDING,
            y2: separator_y,
            color: COL_SURFACE0,
            width: 1.0,
        });
        text_bold(
            tree,
            SIDEBAR_PADDING + 4.0,
            category_top - 24.0 + 4.0,
            "CATEGORIES",
            COL_SUBTEXT0,
            10.0,
        );

        for (y, row) in rows {
            match row {
                SidebarRow::Filter(mode) => {
                    let label = match mode {
                        FilterMode::All => "All Fonts",
                        FilterMode::System => "System",
                        FilterMode::User => "User",
                        FilterMode::Category => continue,
                    };
                    render_sidebar_item(tree, y, label, self.filter_mode == mode);
                }
                SidebarRow::Category(cat) => {
                    let is_selected = self.filter_mode == FilterMode::Category
                        && self.selected_category == Some(cat);
                    let display = format!("{}  {}", cat.icon(), cat.label());
                    render_sidebar_item(tree, y, &display, is_selected);
                }
            }
        }
    }

    /// Render the scrollable font list in the center area.
    fn render_font_list(&self, tree: &mut RenderTree) {
        let list_x = SIDEBAR_WIDTH;
        let list_y = TOOLBAR_HEIGHT;
        let list_w = self.window_width - SIDEBAR_WIDTH - PREVIEW_PANEL_WIDTH;
        let list_h = self.window_height - TOOLBAR_HEIGHT;

        // Clip to the list region
        tree.clip(list_x, list_y, list_w, list_h);

        let families = self.visible_families();
        // `- 24.0` because `font_list_top` is where the first *family* goes and
        // the count header is drawn 24px above it; one number, two readers.
        let mut y = self.font_list_top() - 24.0;

        // Font count header
        let count_str = format!("{} families", families.len());
        tree.text(list_x + CONTENT_PADDING, y, &count_str, COL_SUBTEXT0, 12.0);
        y += 24.0;

        for family in &families {
            // Background highlight for selected
            let variants = self
                .visible_fonts()
                .into_iter()
                .filter(|f| &f.family == family)
                .collect::<Vec<_>>();
            let is_selected = self
                .selected_font
                .and_then(|id| self.collection.get(id))
                .map(|f| &f.family == family)
                .unwrap_or(false);

            if is_selected {
                fill_rounded(
                    tree,
                    list_x + 8.0,
                    y,
                    list_w - 16.0,
                    FONT_LIST_ITEM_HEIGHT,
                    COL_SURFACE0,
                    6.0,
                );
            }

            // Family name
            let name_color = if is_selected { COL_ACCENT } else { COL_TEXT };
            text_bold(
                tree,
                list_x + CONTENT_PADDING,
                y + 8.0,
                family,
                name_color,
                15.0,
            );

            // Style variants and metadata
            let styles: Vec<&str> = variants.iter().map(|v| v.style.label()).collect();
            let styles_str = styles.join(", ");
            let enabled_count = variants.iter().filter(|v| v.enabled).count();
            let meta = format!(
                "{styles_str}  |  {enabled_count}/{} enabled",
                variants.len()
            );
            tree.text(
                list_x + CONTENT_PADDING,
                y + 28.0,
                &meta,
                COL_SUBTEXT0,
                11.0,
            );

            // Disabled indicator
            if variants.iter().any(|v| !v.enabled) {
                let disabled_x = list_x + list_w - 80.0;
                tree.text(
                    disabled_x,
                    y + 16.0,
                    "partially disabled",
                    COL_SUBTEXT0,
                    10.0,
                );
            }

            y += FONT_LIST_ITEM_HEIGHT + 4.0;
        }

        tree.unclip();

        // Right divider
        let divider_x = list_x + list_w;
        tree.push(RenderCommand::Line {
            x1: divider_x,
            y1: list_y,
            x2: divider_x,
            y2: self.window_height,
            color: COL_SURFACE0,
            width: 1.0,
        });
    }

    /// Render the right-hand preview panel.
    fn render_preview_panel(&self, tree: &mut RenderTree) {
        let panel_x = self.window_width - PREVIEW_PANEL_WIDTH;
        let panel_y = TOOLBAR_HEIGHT;
        let panel_h = self.window_height - TOOLBAR_HEIGHT;

        // Panel background
        tree.fill_rect(panel_x, panel_y, PREVIEW_PANEL_WIDTH, panel_h, COL_MANTLE);

        // Clip to the panel region
        tree.clip(panel_x, panel_y, PREVIEW_PANEL_WIDTH, panel_h);

        let mut y = panel_y + CONTENT_PADDING;

        if let Some(font) = self.selected_font.and_then(|id| self.collection.get(id)) {
            // Font family name
            text_bold(
                tree,
                panel_x + CONTENT_PADDING,
                y,
                &font.family,
                COL_TEXT,
                18.0,
            );
            y += 28.0;

            // Style and format
            let info_str = format!("{} -- {}", font.style.label(), font.format.label());
            tree.text(panel_x + CONTENT_PADDING, y, &info_str, COL_SUBTEXT0, 12.0);
            y += 20.0;

            // Version and glyph count
            let detail_str = format!("v{}  |  {} glyphs", font.version, font.glyph_count);
            tree.text(
                panel_x + CONTENT_PADDING,
                y,
                &detail_str,
                COL_SUBTEXT0,
                11.0,
            );
            y += 20.0;

            // System/User badge
            let badge = if font.system {
                "System Font"
            } else {
                "User Font"
            };
            let badge_color = if font.system { COL_ACCENT } else { COL_TEAL };
            let badge_w = text::padded_width(badge, 8.0, 11.0, FontWeightHint::Regular);
            fill_rounded(
                tree,
                panel_x + CONTENT_PADDING,
                y,
                badge_w,
                22.0,
                badge_color,
                4.0,
            );
            tree.text(
                panel_x + CONTENT_PADDING + 8.0,
                y + 4.0,
                badge,
                COL_CRUST,
                11.0,
            );
            y += 36.0;

            // Separator
            tree.push(RenderCommand::Line {
                x1: panel_x + CONTENT_PADDING,
                y1: y,
                x2: panel_x + PREVIEW_PANEL_WIDTH - CONTENT_PADDING,
                y2: y,
                color: COL_SURFACE0,
                width: 1.0,
            });
            y += 16.0;

            // Preview heading
            text_bold(
                tree,
                panel_x + CONTENT_PADDING,
                y,
                "Preview",
                COL_TEXT,
                14.0,
            );
            y += 24.0;

            // Render preview at multiple sizes
            for size in PREVIEW_SIZE_LABELS {
                let size_label = format!("{size}pt");
                tree.text(
                    panel_x + CONTENT_PADDING,
                    y,
                    &size_label,
                    COL_SUBTEXT0,
                    10.0,
                );
                y += 14.0;

                // Sample text at this size (clamped to panel width)
                tree.push(RenderCommand::Text {
                    x: panel_x + CONTENT_PADDING,
                    y,
                    text: self.preview.text.clone(),
                    color: COL_TEXT,
                    font_size: *size,
                    font_weight: match font.style {
                        FontStyle::Bold | FontStyle::BoldItalic | FontStyle::SemiBold => {
                            FontWeightHint::Bold
                        }
                        FontStyle::Light => FontWeightHint::Light,
                        _ => FontWeightHint::Regular,
                    },
                    max_width: Some(PREVIEW_PANEL_WIDTH - CONTENT_PADDING * 2.0),
                    overflow: TextOverflow::Ellipsis,
                });

                y += size + 12.0;
            }
        } else {
            // No font selected
            tree.text(
                panel_x + CONTENT_PADDING,
                y + 40.0,
                "Select a font to preview",
                COL_SUBTEXT0,
                14.0,
            );
        }

        tree.unclip();
    }

    /// Render the rendering-settings overlay panel.
    fn render_settings_panel(&self, tree: &mut RenderTree) {
        let panel_w = 360.0;
        let panel_h = 340.0;
        let panel_x = (self.window_width - panel_w) / 2.0;
        let panel_y = (self.window_height - panel_h) / 2.0;

        // Dim overlay behind the panel
        tree.fill_rect(
            0.0,
            0.0,
            self.window_width,
            self.window_height,
            Color::rgba(0, 0, 0, 160),
        );

        // Panel shadow
        tree.push(RenderCommand::BoxShadow {
            x: panel_x,
            y: panel_y,
            width: panel_w,
            height: panel_h,
            offset_x: 0.0,
            offset_y: 4.0,
            blur: 24.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 120),
            corner_radii: CornerRadii::all(12.0),
        });

        // Panel background
        fill_rounded(tree, panel_x, panel_y, panel_w, panel_h, COL_SURFACE0, 12.0);

        let mut y = panel_y + 20.0;
        let label_x = panel_x + 24.0;
        let value_x = panel_x + 200.0;

        // Title
        text_bold(tree, label_x, y, "Font Rendering Settings", COL_TEXT, 16.0);
        y += 36.0;

        // Default size
        tree.text(label_x, y, "Default Size", COL_TEXT, 13.0);
        let size_str = format!("{:.1} pt", self.render_settings.default_size_pt);
        tree.text(value_x, y, &size_str, COL_ACCENT, 13.0);
        y += 32.0;

        // Hinting
        tree.text(label_x, y, "Hinting", COL_TEXT, 13.0);
        render_setting_options(
            tree,
            value_x,
            y,
            HintMode::ALL,
            self.render_settings.hinting,
            |m| m.label(),
        );
        y += 32.0;

        // Antialiasing
        tree.text(label_x, y, "Antialiasing", COL_TEXT, 13.0);
        render_setting_options(
            tree,
            value_x,
            y,
            AntialiasingMode::ALL,
            self.render_settings.antialiasing,
            |m| m.label(),
        );
        y += 32.0;

        // Subpixel order
        tree.text(label_x, y, "Subpixel Order", COL_TEXT, 13.0);
        render_setting_options(
            tree,
            value_x,
            y,
            SubpixelOrder::ALL,
            self.render_settings.subpixel_order,
            |m| m.label(),
        );
        y += 40.0;

        // Close button
        let close_w = 60.0;
        let close_x = panel_x + (panel_w - close_w) / 2.0;
        fill_rounded(tree, close_x, y, close_w, 28.0, COL_ACCENT, 6.0);
        tree.text(close_x + 14.0, y + 7.0, "Close", COL_CRUST, 12.0);
    }
}

// ============================================================================
// Rendering helpers
// ============================================================================

/// Push a rounded rectangle fill command.
fn fill_rounded(tree: &mut RenderTree, x: f32, y: f32, w: f32, h: f32, color: Color, radius: f32) {
    tree.fill_rounded_rect(x, y, w, h, color, CornerRadii::all(radius));
}

/// Push a text command with bold weight.
fn text_bold(tree: &mut RenderTree, x: f32, y: f32, content: &str, color: Color, size: f32) {
    tree.push(RenderCommand::Text {
        x,
        y,
        text: content.to_string(),
        color,
        font_size: size,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
}

/// Render a toolbar button.
fn render_toolbar_button(tree: &mut RenderTree, x: f32, y: f32, label: &str, color: Color) -> f32 {
    let w = text::padded_width(label, 10.0, 12.0, FontWeightHint::Regular);
    let h = 28.0;
    fill_rounded(tree, x, y, w, h, color, 6.0);
    tree.text(x + 10.0, y + 7.0, label, COL_CRUST, 12.0);
    w
}

/// Render a sidebar item.
fn render_sidebar_item(tree: &mut RenderTree, y: f32, label: &str, selected: bool) {
    let item_x = SIDEBAR_PADDING;
    let item_w = SIDEBAR_WIDTH - SIDEBAR_PADDING * 2.0;

    if selected {
        fill_rounded(
            tree,
            item_x,
            y,
            item_w,
            CATEGORY_ITEM_HEIGHT,
            COL_SURFACE1,
            6.0,
        );
        // Left accent bar
        tree.fill_rect(
            item_x,
            y + 6.0,
            3.0,
            CATEGORY_ITEM_HEIGHT - 12.0,
            COL_ACCENT,
        );
    }

    let text_color = if selected { COL_ACCENT } else { COL_TEXT };
    tree.text(item_x + 14.0, y + 10.0, label, text_color, 13.0);
}

/// Render a row of selectable option labels (for settings panel).
fn render_setting_options<T: PartialEq + Copy>(
    tree: &mut RenderTree,
    x: f32,
    y: f32,
    options: &[T],
    selected: T,
    label_fn: impl Fn(T) -> &'static str,
) {
    let mut ox = x;
    for opt in options {
        let lbl = label_fn(*opt);
        let is_sel = *opt == selected;
        let color = if is_sel { COL_ACCENT } else { COL_SUBTEXT0 };
        tree.text(ox, y, lbl, color, 12.0);
        ox += text::measure(lbl, 12.0, FontWeightHint::Regular) + 12.0;
    }
}

// ============================================================================
// Application entry point
// ============================================================================

impl App for FontManagerState {
    fn title(&self) -> String {
        "Font Manager".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        (DEFAULT_WINDOW_WIDTH as u32, DEFAULT_WINDOW_HEIGHT as u32)
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match self.handle_event(event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // Reconciled with the size we are *handed*, not the one we remember: a
        // compositor may grant a size that was never requested, and the first
        // frame is drawn before any `Resize` event arrives. Without this the
        // opening frame is laid out for 1000x700 whatever the window actually
        // is.
        self.window_width = width;
        self.window_height = height;
        self.render_tree()
    }

    // No `tick_interval`: this app ages nothing. There is no beat, blink,
    // countdown or toast in it — checked, not assumed, by grepping for
    // `Event::Tick` and `elapsed_ms` and finding neither. The default `None`
    // is therefore right here, and returning one would wake an idle desktop
    // to redraw an unchanged frame.
}

fn main() -> ExitCode {
    app::launch("fontmanager", &mut FontManagerState::new())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the line
    // that did it — that is the diagnosis. The defensive lints exist to keep
    // panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    use super::*;
    // Used only by the tests — the app itself reads modifiers off the
    // `KeyEvent` it is handed and never builds one, so importing this at crate
    // root left the binary with an unused import.
    use guitk::event::Modifiers;

    // ====================================================================
    // Measured widths
    // ====================================================================

    #[test]
    fn a_toolbar_button_reports_the_width_it_drew() {
        let mut tree = RenderTree::new();
        for label in ["Install", "Remove", "Schriftart installieren"] {
            let w = render_toolbar_button(&mut tree, 0.0, 0.0, label, COL_ACCENT);
            assert!(
                w >= text::measure(label, 12.0, FontWeightHint::Regular) + 20.0,
                "{label} overflows its button"
            );
        }
    }

    #[test]
    fn the_system_badge_fits_its_label() {
        for label in ["System Font", "User Font"] {
            let w = text::padded_width(label, 8.0, 11.0, FontWeightHint::Regular);
            assert!(
                w >= text::measure(label, 11.0, FontWeightHint::Regular) + 16.0,
                "{label} overflows its badge"
            );
        }
    }

    #[test]
    fn setting_options_do_not_overlap() {
        // The row advances by the width of the label it just drew, so no two
        // labels can land on top of each other however long they are.
        let mut tree = RenderTree::new();
        let opts = [0_usize, 1, 2];
        render_setting_options(&mut tree, 0.0, 0.0, &opts, 0, |i| match i {
            0 => "Alphabetisch",
            1 => "Nach Familie",
            _ => "Zuletzt hinzugef\u{fc}gt",
        });
        let drawn: Vec<(f32, String)> = tree
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { x, text, .. } => Some((*x, text.clone())),
                _ => None,
            })
            .collect();
        for pair in drawn.windows(2) {
            let (Some((x0, label)), Some((x1, _))) = (pair.first(), pair.get(1)) else {
                unreachable!("windows(2) yields pairs");
            };
            let end = x0 + text::measure(label, 12.0, FontWeightHint::Regular);
            assert!(*x1 >= end, "{label} runs into the option after it");
        }
    }

    // ====================================================================
    // FontCollection basics
    // ====================================================================

    #[test]
    fn test_default_collection_is_populated() {
        let coll = FontCollection::new_with_defaults();
        assert!(
            coll.fonts.len() >= 15,
            "Should have at least 15 default fonts"
        );
    }

    #[test]
    fn test_empty_collection() {
        let coll = FontCollection::new();
        assert!(coll.fonts.is_empty());
        assert!(coll.families().is_empty());
    }

    #[test]
    fn test_families_are_unique_and_sorted() {
        let coll = FontCollection::new_with_defaults();
        let families = coll.families();
        // Check sorted order.
        for pair in families.windows(2) {
            assert!(pair[0] <= pair[1], "Families must be sorted");
        }
        // Check no duplicates.
        let mut deduped = families.clone();
        deduped.dedup();
        assert_eq!(families.len(), deduped.len(), "Families must be unique");
    }

    #[test]
    fn test_default_fonts_are_system() {
        let coll = FontCollection::new_with_defaults();
        for font in &coll.fonts {
            assert!(font.system, "Default fonts should be system fonts");
        }
    }

    #[test]
    fn test_default_fonts_are_enabled() {
        let coll = FontCollection::new_with_defaults();
        for font in &coll.fonts {
            assert!(font.enabled, "Default fonts should be enabled");
        }
    }

    // ====================================================================
    // Install / Uninstall
    // ====================================================================

    #[test]
    fn test_install_ttf() {
        let mut coll = FontCollection::new();
        let result = coll.install("/home/user/fonts/MyFont.ttf");
        assert!(result.is_ok());
        let id = result.unwrap();
        let font = coll.get(id).unwrap();
        assert_eq!(font.family, "MyFont");
        assert_eq!(font.format, FontFormat::TrueType);
        assert!(!font.system);
        assert!(font.enabled);
    }

    #[test]
    fn test_install_otf() {
        let mut coll = FontCollection::new();
        let id = coll.install("/fonts/Fancy-Script.otf").unwrap();
        let font = coll.get(id).unwrap();
        assert_eq!(font.family, "Fancy Script");
        assert_eq!(font.format, FontFormat::OpenType);
    }

    #[test]
    fn test_install_woff2() {
        let mut coll = FontCollection::new();
        let id = coll.install("/fonts/WebFont.woff2").unwrap();
        let font = coll.get(id).unwrap();
        assert_eq!(font.format, FontFormat::Woff2);
    }

    #[test]
    fn test_install_bitmap() {
        let mut coll = FontCollection::new();
        let id = coll.install("/fonts/Terminal.bdf").unwrap();
        let font = coll.get(id).unwrap();
        assert_eq!(font.format, FontFormat::Bitmap);
    }

    #[test]
    fn test_install_invalid_format() {
        let mut coll = FontCollection::new();
        let result = coll.install("/fonts/not_a_font.png");
        assert_eq!(result, Err(FontError::InvalidFormat));
    }

    #[test]
    fn test_install_duplicate() {
        let mut coll = FontCollection::new();
        coll.install("/fonts/TestFont.ttf").unwrap();
        let result = coll.install("/other/TestFont.ttf");
        assert_eq!(result, Err(FontError::AlreadyInstalled));
    }

    #[test]
    fn test_uninstall_user_font() {
        let mut coll = FontCollection::new();
        let id = coll.install("/fonts/Temp.ttf").unwrap();
        assert_eq!(coll.fonts.len(), 1);
        coll.uninstall(id).unwrap();
        assert!(coll.fonts.is_empty());
    }

    #[test]
    fn test_uninstall_nonexistent() {
        let mut coll = FontCollection::new();
        assert_eq!(coll.uninstall(9999), Err(FontError::NotFound));
    }

    // ====================================================================
    // System font protection
    // ====================================================================

    #[test]
    fn test_cannot_uninstall_system_font() {
        let mut coll = FontCollection::new_with_defaults();
        let system_id = coll.fonts[0].id;
        assert!(coll.fonts[0].system);
        assert_eq!(coll.uninstall(system_id), Err(FontError::SystemFont));
    }

    #[test]
    fn test_system_fonts_persist_after_failed_uninstall() {
        let mut coll = FontCollection::new_with_defaults();
        let count_before = coll.fonts.len();
        let system_id = coll.fonts[0].id;
        let _ = coll.uninstall(system_id);
        assert_eq!(coll.fonts.len(), count_before);
    }

    // ====================================================================
    // Category filtering
    // ====================================================================

    #[test]
    fn test_by_category_sans_serif() {
        let coll = FontCollection::new_with_defaults();
        let sans = coll.by_category(FontCategory::SansSerif);
        assert!(!sans.is_empty());
        for font in &sans {
            assert_eq!(font.category, FontCategory::SansSerif);
        }
    }

    #[test]
    fn test_by_category_monospace() {
        let coll = FontCollection::new_with_defaults();
        let mono = coll.by_category(FontCategory::Monospace);
        assert!(!mono.is_empty());
        for font in &mono {
            assert_eq!(font.category, FontCategory::Monospace);
        }
    }

    #[test]
    fn test_by_category_symbol() {
        let coll = FontCollection::new_with_defaults();
        let symbols = coll.by_category(FontCategory::Symbol);
        assert!(!symbols.is_empty());
        assert!(symbols.iter().any(|f| f.family == "Noto Emoji"));
    }

    #[test]
    fn test_all_categories_covered() {
        let coll = FontCollection::new_with_defaults();
        for cat in FontCategory::ALL {
            let fonts = coll.by_category(*cat);
            assert!(!fonts.is_empty(), "Category {:?} should have fonts", cat);
        }
    }

    // ====================================================================
    // Family grouping
    // ====================================================================

    #[test]
    fn test_by_family_inter() {
        let coll = FontCollection::new_with_defaults();
        let inter = coll.by_family("Inter");
        assert!(inter.len() >= 3, "Inter should have Regular, Bold, Italic");
        for font in &inter {
            assert_eq!(font.family, "Inter");
        }
    }

    #[test]
    fn test_by_family_nonexistent() {
        let coll = FontCollection::new_with_defaults();
        let result = coll.by_family("Nonexistent Family");
        assert!(result.is_empty());
    }

    #[test]
    fn test_families_count() {
        let coll = FontCollection::new_with_defaults();
        let families = coll.families();
        // We have multiple styles per family; families() deduplicates.
        assert!(
            families.len() < coll.fonts.len(),
            "families() should deduplicate"
        );
    }

    // ====================================================================
    // Search
    // ====================================================================

    #[test]
    fn test_search_exact() {
        let coll = FontCollection::new_with_defaults();
        let results = coll.search("JetBrains Mono");
        assert!(!results.is_empty());
        for font in &results {
            assert!(font.family.contains("JetBrains"));
        }
    }

    #[test]
    fn test_search_case_insensitive() {
        let coll = FontCollection::new_with_defaults();
        let results = coll.search("jetbrains");
        assert!(!results.is_empty());
    }

    #[test]
    fn test_search_partial() {
        let coll = FontCollection::new_with_defaults();
        let results = coll.search("Noto");
        // Should match Noto Sans, Noto Serif, Noto Emoji
        assert!(results.len() >= 3);
    }

    #[test]
    fn test_search_no_results() {
        let coll = FontCollection::new_with_defaults();
        let results = coll.search("zzz_nonexistent_zzz");
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_empty_query() {
        let coll = FontCollection::new_with_defaults();
        let results = coll.search("");
        assert_eq!(results.len(), coll.fonts.len(), "Empty query returns all");
    }

    // ====================================================================
    // Enable/disable toggle
    // ====================================================================

    #[test]
    fn test_toggle_enabled() {
        let mut coll = FontCollection::new_with_defaults();
        let id = coll.fonts[0].id;
        assert!(coll.fonts[0].enabled);
        coll.toggle_enabled(id);
        assert!(!coll.get(id).unwrap().enabled);
        coll.toggle_enabled(id);
        assert!(coll.get(id).unwrap().enabled);
    }

    #[test]
    fn test_toggle_nonexistent_is_noop() {
        let mut coll = FontCollection::new_with_defaults();
        let count_before = coll.fonts.len();
        coll.toggle_enabled(99999);
        assert_eq!(coll.fonts.len(), count_before);
    }

    // ====================================================================
    // RenderSettings defaults
    // ====================================================================

    #[test]
    fn test_render_settings_defaults() {
        let rs = RenderSettings::default();
        assert!((rs.default_size_pt - 11.0).abs() < f32::EPSILON);
        assert_eq!(rs.hinting, HintMode::Slight);
        assert_eq!(rs.antialiasing, AntialiasingMode::Subpixel);
        assert_eq!(rs.subpixel_order, SubpixelOrder::Rgb);
    }

    // ====================================================================
    // FontPreview defaults
    // ====================================================================

    #[test]
    fn test_preview_defaults() {
        let pv = FontPreview::default();
        assert!(pv.text.contains("quick brown fox"));
        assert!((pv.size_pt - 24.0).abs() < f32::EPSILON);
        assert!(!pv.custom_text);
    }

    // ====================================================================
    // FontManagerState
    // ====================================================================

    #[test]
    fn test_initial_state() {
        let state = FontManagerState::new();
        assert_eq!(state.filter_mode, FilterMode::All);
        assert!(state.search_query.is_empty());
        assert!(!state.show_settings);
        assert!(state.selected_font.is_some());
        assert_eq!(state.window_width, DEFAULT_WINDOW_WIDTH);
        assert_eq!(state.window_height, DEFAULT_WINDOW_HEIGHT);
    }

    #[test]
    fn test_visible_fonts_all_mode() {
        let state = FontManagerState::new();
        let visible = state.visible_fonts();
        assert_eq!(visible.len(), state.collection.fonts.len());
    }

    #[test]
    fn test_visible_fonts_search_filter() {
        let mut state = FontManagerState::new();
        state.search_query = String::from("Inter");
        let visible = state.visible_fonts();
        assert!(!visible.is_empty());
        for font in &visible {
            assert!(font.family.to_lowercase().contains("inter"));
        }
    }

    #[test]
    fn test_visible_fonts_category_filter() {
        let mut state = FontManagerState::new();
        state.filter_mode = FilterMode::Category;
        state.selected_category = Some(FontCategory::Monospace);
        let visible = state.visible_fonts();
        for font in &visible {
            assert_eq!(font.category, FontCategory::Monospace);
        }
    }

    #[test]
    fn test_visible_fonts_system_filter() {
        let state = FontManagerState::new();
        // All defaults are system fonts, so System filter shows all.
        let mut sys_state = FontManagerState::new();
        sys_state.filter_mode = FilterMode::System;
        let visible = sys_state.visible_fonts();
        assert_eq!(visible.len(), state.collection.fonts.len());
    }

    #[test]
    fn test_visible_fonts_user_filter() {
        let mut state = FontManagerState::new();
        state.filter_mode = FilterMode::User;
        let visible = state.visible_fonts();
        assert!(visible.is_empty(), "No user fonts installed by default");
    }

    // ====================================================================
    // Rendering
    // ====================================================================

    #[test]
    fn test_render_produces_commands() {
        let state = FontManagerState::new();
        let tree = state.render_tree();
        assert!(!tree.is_empty());
        assert!(tree.len() > 30, "Should produce many render commands");
    }

    #[test]
    fn test_render_with_settings_panel() {
        let mut state = FontManagerState::new();
        state.show_settings = true;
        let tree = state.render_tree();
        assert!(!tree.is_empty());
        // Settings panel adds many more commands.
        let base_tree = FontManagerState::new().render_tree();
        assert!(
            tree.len() > base_tree.len(),
            "Settings overlay adds commands"
        );
    }

    #[test]
    fn test_render_after_resize() {
        let mut state = FontManagerState::new();
        let ev = Event::Resize {
            width: 1400,
            height: 900,
        };
        let result = state.handle_event(&ev);
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.window_width, 1400.0);
        assert_eq!(state.window_height, 900.0);
        let tree = state.render_tree();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_no_selection() {
        let mut state = FontManagerState::new();
        state.selected_font = None;
        let tree = state.render_tree();
        assert!(!tree.is_empty(), "Should render even with no selection");
    }

    // ====================================================================
    // Event handling
    // ====================================================================

    #[test]
    fn test_key_down_selects_next() {
        let mut state = FontManagerState::new();
        let first_id = state.selected_font.unwrap();
        let ev = Event::Key(KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        state.handle_event(&ev);
        assert_ne!(state.selected_font.unwrap(), first_id);
    }

    #[test]
    fn test_key_up_selects_prev() {
        let mut state = FontManagerState::new();
        // Move down first, then up to get back.
        let first_id = state.selected_font.unwrap();
        let down = Event::Key(KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        state.handle_event(&down);
        let up = Event::Key(KeyEvent {
            key: Key::Up,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        state.handle_event(&up);
        assert_eq!(state.selected_font.unwrap(), first_id);
    }

    #[test]
    fn test_escape_closes_settings() {
        let mut state = FontManagerState::new();
        state.show_settings = true;
        let ev = Event::Key(KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        state.handle_event(&ev);
        assert!(!state.show_settings);
    }

    #[test]
    fn test_escape_clears_search() {
        let mut state = FontManagerState::new();
        state.search_query = String::from("test");
        let ev = Event::Key(KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        state.handle_event(&ev);
        assert!(state.search_query.is_empty());
    }

    #[test]
    fn test_ctrl_s_toggles_settings() {
        let mut state = FontManagerState::new();
        assert!(!state.show_settings);
        let ev = Event::Key(KeyEvent {
            key: Key::S,
            pressed: true,
            modifiers: Modifiers {
                shift: false,
                ctrl: true,
                alt: false,
                super_key: false,
            },
            text: String::new(),
        });
        state.handle_event(&ev);
        assert!(state.show_settings);
        state.handle_event(&ev);
        assert!(!state.show_settings);
    }

    // ====================================================================
    // Error types
    // ====================================================================

    #[test]
    fn test_font_error_display() {
        assert_eq!(FontError::NotFound.to_string(), "Font not found");
        assert_eq!(
            FontError::SystemFont.to_string(),
            "Cannot modify system font"
        );
        assert_eq!(
            FontError::AlreadyInstalled.to_string(),
            "Font already installed"
        );
        assert_eq!(FontError::InvalidFormat.to_string(), "Invalid font format");
        assert_eq!(
            FontError::IoError(String::from("disk full")).to_string(),
            "I/O error: disk full"
        );
    }

    // ====================================================================
    // FontStyle / FontCategory / FontFormat labels
    // ====================================================================

    #[test]
    fn test_font_style_labels() {
        assert_eq!(FontStyle::Regular.label(), "Regular");
        assert_eq!(FontStyle::BoldItalic.label(), "Bold Italic");
        assert_eq!(FontStyle::SemiBold.label(), "SemiBold");
    }

    #[test]
    fn test_font_category_labels() {
        assert_eq!(FontCategory::SansSerif.label(), "Sans Serif");
        assert_eq!(FontCategory::Monospace.label(), "Monospace");
        assert_eq!(FontCategory::Handwriting.label(), "Handwriting");
    }

    #[test]
    fn test_font_format_labels_and_extensions() {
        assert_eq!(FontFormat::TrueType.label(), "TrueType");
        assert_eq!(FontFormat::TrueType.extension(), ".ttf");
        assert_eq!(FontFormat::OpenType.extension(), ".otf");
        assert_eq!(FontFormat::Woff2.extension(), ".woff2");
        assert_eq!(FontFormat::Bitmap.extension(), ".bdf");
    }

    // ====================================================================
    // Hint mode / AA / Subpixel labels
    // ====================================================================

    #[test]
    fn test_hint_mode_labels() {
        assert_eq!(HintMode::None.label(), "None");
        assert_eq!(HintMode::Slight.label(), "Slight");
        assert_eq!(HintMode::Full.label(), "Full");
    }

    #[test]
    fn test_antialiasing_labels() {
        assert_eq!(AntialiasingMode::None.label(), "None");
        assert_eq!(AntialiasingMode::Grayscale.label(), "Grayscale");
        assert_eq!(AntialiasingMode::Subpixel.label(), "Subpixel");
    }

    #[test]
    fn test_subpixel_order_labels() {
        assert_eq!(SubpixelOrder::Rgb.label(), "RGB");
        assert_eq!(SubpixelOrder::Bgr.label(), "BGR");
        assert_eq!(SubpixelOrder::VRgb.label(), "Vertical RGB");
        assert_eq!(SubpixelOrder::VBgr.label(), "Vertical BGR");
    }

    // ====================================================================
    // Font path parsing
    // ====================================================================

    #[test]
    fn test_family_from_path_simple() {
        let name = FontCollection::family_from_path("/fonts/MyFont.ttf");
        assert_eq!(name, "MyFont");
    }

    #[test]
    fn test_family_from_path_with_hyphens() {
        let name = FontCollection::family_from_path("/fonts/Source-Code-Pro.otf");
        assert_eq!(name, "Source Code Pro");
    }

    #[test]
    fn test_family_from_path_with_underscores() {
        let name = FontCollection::family_from_path("/fonts/my_custom_font.ttf");
        assert_eq!(name, "my custom font");
    }

    #[test]
    fn test_family_from_path_no_directory() {
        let name = FontCollection::family_from_path("Standalone.woff2");
        assert_eq!(name, "Standalone");
    }

    // ====================================================================
    // Visible families
    // ====================================================================

    #[test]
    fn test_visible_families_sorted_and_deduped() {
        let state = FontManagerState::new();
        let families = state.visible_families();
        for pair in families.windows(2) {
            assert!(pair[0] <= pair[1]);
        }
        let mut deduped = families.clone();
        deduped.dedup();
        assert_eq!(families.len(), deduped.len());
    }

    // ====================================================================
    // ID generation
    // ====================================================================

    #[test]
    fn test_font_ids_are_unique() {
        let coll = FontCollection::new_with_defaults();
        let ids: Vec<u64> = coll.fonts.iter().map(|f| f.id).collect();
        let mut unique_ids = ids.clone();
        unique_ids.sort_unstable();
        unique_ids.dedup();
        assert_eq!(ids.len(), unique_ids.len(), "All IDs must be unique");
    }

    // ====================================================================
    // Mouse — the layer this app did not have until it was wired
    // ====================================================================

    fn click(state: &mut FontManagerState, x: f32, y: f32) -> EventResult {
        state.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        }))
    }

    /// A click on a sidebar filter row selects that filter.
    ///
    /// **This app had no `Event::Mouse` arm at all until 2026-09-03** —
    /// `MouseEvent` and friends were imported and never named again, hidden by
    /// a file-wide `#[allow(unused_imports)]`, and nothing noticed because
    /// `main` rendered one frame and asserted it was non-empty rather than
    /// delivering an event. A font manager that can only be driven with the
    /// arrow keys.
    #[test]
    fn a_click_on_a_sidebar_filter_selects_it() {
        let mut state = FontManagerState::new();
        assert_eq!(state.filter_mode, FilterMode::All, "fixture starts on All");

        // Rows are taken from the same list the renderer walks, so this cannot
        // pass against geometry the user never sees.
        let rows = FontManagerState::sidebar_rows();
        let (y, _) = rows
            .iter()
            .find(|(_, r)| matches!(r, SidebarRow::Filter(FilterMode::System)))
            .expect("a System row is drawn");

        assert_eq!(
            click(&mut state, 40.0, y + 4.0),
            EventResult::Consumed,
            "the click was not handled"
        );
        assert_eq!(state.filter_mode, FilterMode::System);
        assert_eq!(state.selected_category, None);
    }

    /// A click on a category row selects the category *and* switches the mode.
    ///
    /// Both, because a category with the filter left on `All` would highlight
    /// a row and change nothing in the list beside it.
    #[test]
    fn a_click_on_a_category_selects_it_and_switches_the_filter() {
        let mut state = FontManagerState::new();
        let rows = FontManagerState::sidebar_rows();
        let (y, row) = rows
            .iter()
            .find(|(_, r)| matches!(r, SidebarRow::Category(_)))
            .expect("category rows are drawn");
        let SidebarRow::Category(expected) = row else {
            panic!("found a non-category row");
        };

        assert_eq!(click(&mut state, 40.0, y + 4.0), EventResult::Consumed);
        assert_eq!(state.filter_mode, FilterMode::Category);
        assert_eq!(state.selected_category, Some(*expected));
    }

    /// Every drawn sidebar row is clickable, and each selects a different thing.
    ///
    /// A sweep rather than a sample: the rows are laid out by an accumulator,
    /// and an off-by-one in it would leave exactly one row unreachable —
    /// which a test that clicks two of them would not notice.
    #[test]
    fn every_sidebar_row_can_be_clicked() {
        for (y, row) in FontManagerState::sidebar_rows() {
            let mut state = FontManagerState::new();
            assert_eq!(
                click(&mut state, 40.0, y + CATEGORY_ITEM_HEIGHT / 2.0),
                EventResult::Consumed,
                "the row at y={y} ({row:?}) is drawn but not clickable"
            );
            match row {
                SidebarRow::Filter(mode) => assert_eq!(state.filter_mode, mode),
                SidebarRow::Category(cat) => {
                    assert_eq!(state.filter_mode, FilterMode::Category);
                    assert_eq!(state.selected_category, Some(cat));
                }
            }
        }
    }

    /// A click on a font row selects that family.
    #[test]
    fn a_click_on_a_font_row_selects_that_family() {
        let mut state = FontManagerState::new();
        let families = state.visible_families();
        assert!(
            families.len() >= 2,
            "the fixture needs two families to tell a selection from a default"
        );
        let target = families[1].clone();

        let y = state.font_list_top() + FONT_LIST_ITEM_HEIGHT + 4.0;
        assert_eq!(
            click(&mut state, SIDEBAR_WIDTH + 40.0, y),
            EventResult::Consumed
        );
        let selected = state
            .selected_font
            .and_then(|id| state.collection.get(id))
            .expect("something is selected");
        assert_eq!(selected.family, target);
    }

    /// A click in the gutter above the first row, or past the last, selects
    /// nothing.
    ///
    /// The failure this guards is a hit test that clamps: clicking the header
    /// would select the first family and clicking empty space below the list
    /// would select the last, both of which look like the program choosing for
    /// you.
    #[test]
    fn a_click_outside_the_rows_selects_nothing() {
        let mut state = FontManagerState::new();
        let before = state.selected_font;

        // Above the first family, where the count header is drawn.
        let above = state.font_list_top() - 6.0;
        assert_eq!(
            click(&mut state, SIDEBAR_WIDTH + 40.0, above),
            EventResult::Ignored
        );
        assert_eq!(state.selected_font, before);

        // Below the last family.
        let past = state.font_list_top()
            + FONT_LIST_ITEM_HEIGHT * (state.visible_families().len() as f32 + 2.0);
        assert_eq!(
            click(&mut state, SIDEBAR_WIDTH + 40.0, past),
            EventResult::Ignored
        );
        assert_eq!(state.selected_font, before);
    }

    /// Narrowing the filter does not leave a font selected that the list no
    /// longer offers.
    ///
    /// Otherwise the preview panel goes on showing a font the user cannot see
    /// in the list beside it, which reads as the list being wrong rather than
    /// the selection being stale.
    #[test]
    fn narrowing_the_filter_does_not_leave_an_invisible_font_selected() {
        for (y, row) in FontManagerState::sidebar_rows() {
            let mut s = FontManagerState::new();
            click(&mut s, 40.0, y + 4.0);
            if let Some(id) = s.selected_font {
                assert!(
                    s.visible_fonts().iter().any(|f| f.id == id),
                    "after selecting {row:?} the selected font is not in the visible list"
                );
            }
        }
    }

    /// A press of a button other than the left one does nothing.
    #[test]
    fn a_right_click_does_not_select() {
        let mut state = FontManagerState::new();
        let before = state.filter_mode;
        let (y, _) = FontManagerState::sidebar_rows()[1];
        let result = state.handle_event(&Event::Mouse(MouseEvent {
            x: 40.0,
            y: y + 4.0,
            kind: MouseEventKind::Press(MouseButton::Right),
        }));
        assert_eq!(result, EventResult::Ignored);
        assert_eq!(state.filter_mode, before);
    }
}
