//! SlateOS Font Manager — Graphical Font Management and Preview
//!
//! A graphical application for managing system and user fonts. Provides
//! font browsing by category, family, and style; live previews at multiple
//! sizes; install/uninstall operations; and global font rendering settings
//! (hinting, antialiasing, subpixel order).
//!
//! Uses the guitk library for rendering. Dark theme (Catppuccin Mocha).

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
#[allow(unused_imports)]
use guitk::layout::{FlexAlign, FlexDirection, FlexWrap, Size};
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::{CornerRadii, Edges};

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
        if self.fonts.iter().any(|f| f.family == family && f.style == FontStyle::Regular) {
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
        let idx = self.fonts.iter().position(|f| f.id == id)
            .ok_or(FontError::NotFound)?;
        if self.fonts[idx].system {
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
        let mut names: Vec<String> = self.fonts.iter()
            .map(|f| f.family.clone())
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// Return all fonts belonging to a given family.
    pub fn by_family(&self, family: &str) -> Vec<&FontInfo> {
        self.fonts.iter()
            .filter(|f| f.family == family)
            .collect()
    }

    /// Return all fonts in a given category.
    pub fn by_category(&self, cat: FontCategory) -> Vec<&FontInfo> {
        self.fonts.iter()
            .filter(|f| f.category == cat)
            .collect()
    }

    /// Search fonts by family name (case-insensitive substring match).
    pub fn search(&self, query: &str) -> Vec<&FontInfo> {
        let query_lower = query.to_lowercase();
        self.fonts.iter()
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
        let filename = path.rsplit('/').next()
            .or_else(|| path.rsplit('\\').next())
            .unwrap_or(path);
        let stem = filename.rsplit_once('.')
            .map(|(s, _)| s)
            .unwrap_or(filename);
        stem.replace(['-', '_'], " ")
    }

    /// Populate with default system fonts covering all categories.
    fn add_default_fonts(&mut self) {
        let defaults: &[(&str, FontStyle, FontFormat, FontCategory, u32)] = &[
            ("Inter", FontStyle::Regular, FontFormat::OpenType, FontCategory::SansSerif, 2548),
            ("Inter", FontStyle::Bold, FontFormat::OpenType, FontCategory::SansSerif, 2548),
            ("Inter", FontStyle::Italic, FontFormat::OpenType, FontCategory::SansSerif, 2548),
            ("Roboto", FontStyle::Regular, FontFormat::TrueType, FontCategory::SansSerif, 1294),
            ("Roboto", FontStyle::Light, FontFormat::TrueType, FontCategory::SansSerif, 1294),
            ("Noto Sans", FontStyle::Regular, FontFormat::TrueType, FontCategory::SansSerif, 3440),
            ("Noto Sans", FontStyle::Bold, FontFormat::TrueType, FontCategory::SansSerif, 3440),
            ("Noto Serif", FontStyle::Regular, FontFormat::TrueType, FontCategory::Serif, 3200),
            ("Noto Serif", FontStyle::Italic, FontFormat::TrueType, FontCategory::Serif, 3200),
            ("Libre Baskerville", FontStyle::Regular, FontFormat::OpenType, FontCategory::Serif, 820),
            ("JetBrains Mono", FontStyle::Regular, FontFormat::TrueType, FontCategory::Monospace, 1036),
            ("JetBrains Mono", FontStyle::Bold, FontFormat::TrueType, FontCategory::Monospace, 1036),
            ("Fira Code", FontStyle::Regular, FontFormat::TrueType, FontCategory::Monospace, 1588),
            ("Source Code Pro", FontStyle::Regular, FontFormat::OpenType, FontCategory::Monospace, 974),
            ("Source Code Pro", FontStyle::Medium, FontFormat::OpenType, FontCategory::Monospace, 974),
            ("Lobster", FontStyle::Regular, FontFormat::TrueType, FontCategory::Display, 490),
            ("Pacifico", FontStyle::Regular, FontFormat::TrueType, FontCategory::Handwriting, 370),
            ("Dancing Script", FontStyle::Regular, FontFormat::TrueType, FontCategory::Handwriting, 534),
            ("Noto Emoji", FontStyle::Regular, FontFormat::TrueType, FontCategory::Symbol, 3610),
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
        let mut names: Vec<String> = self.visible_fonts().iter()
            .map(|f| f.family.clone())
            .collect();
        names.sort();
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
            _ => EventResult::Ignored,
        }
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
        if let Some(current_id) = self.selected_font {
            if let Some(pos) = visible.iter().position(|f| f.id == current_id) {
                let next_pos = if pos + 1 < visible.len() { pos + 1 } else { 0 };
                self.selected_font = Some(visible[next_pos].id);
            } else {
                self.selected_font = Some(visible[0].id);
            }
        } else {
            self.selected_font = Some(visible[0].id);
        }
    }

    fn select_prev_font(&mut self) {
        let visible = self.visible_fonts();
        if visible.is_empty() {
            return;
        }
        if let Some(current_id) = self.selected_font {
            if let Some(pos) = visible.iter().position(|f| f.id == current_id) {
                let prev_pos = if pos > 0 { pos - 1 } else { visible.len() - 1 };
                self.selected_font = Some(visible[prev_pos].id);
            } else {
                self.selected_font = Some(visible[0].id);
            }
        } else if let Some(last) = visible.last() {
            self.selected_font = Some(last.id);
        }
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the complete Font Manager UI frame.
    pub fn render(&self) -> RenderTree {
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
        fill_rounded(tree, search_x, search_y, search_w, search_h, COL_SURFACE0, 6.0);
        if self.search_query.is_empty() {
            tree.text(search_x + 10.0, search_y + 7.0, "Search fonts...", COL_SUBTEXT0, 13.0);
        } else {
            tree.text(search_x + 10.0, search_y + 7.0, &self.search_query, COL_TEXT, 13.0);
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

        let mut y = sidebar_y + SIDEBAR_PADDING;

        // Section: Filter
        text_bold(tree, SIDEBAR_PADDING + 4.0, y + 4.0, "FILTER", COL_SUBTEXT0, 10.0);
        y += 24.0;

        // All Fonts
        let all_selected = self.filter_mode == FilterMode::All;
        render_sidebar_item(tree, y, "All Fonts", all_selected);
        y += CATEGORY_ITEM_HEIGHT;

        // System
        let sys_selected = self.filter_mode == FilterMode::System;
        render_sidebar_item(tree, y, "System", sys_selected);
        y += CATEGORY_ITEM_HEIGHT;

        // User
        let usr_selected = self.filter_mode == FilterMode::User;
        render_sidebar_item(tree, y, "User", usr_selected);
        y += CATEGORY_ITEM_HEIGHT;

        // Separator
        y += 8.0;
        tree.push(RenderCommand::Line {
            x1: SIDEBAR_PADDING,
            y1: y,
            x2: SIDEBAR_WIDTH - SIDEBAR_PADDING,
            y2: y,
            color: COL_SURFACE0,
            width: 1.0,
        });
        y += 12.0;

        // Section: Categories
        text_bold(tree, SIDEBAR_PADDING + 4.0, y + 4.0, "CATEGORIES", COL_SUBTEXT0, 10.0);
        y += 24.0;

        for cat in FontCategory::ALL {
            let is_selected = self.filter_mode == FilterMode::Category
                && self.selected_category == Some(*cat);
            let label = cat.label();
            let icon = cat.icon();
            let display = format!("{icon}  {label}");
            render_sidebar_item(tree, y, &display, is_selected);
            y += CATEGORY_ITEM_HEIGHT;
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
        let mut y = list_y + CONTENT_PADDING - self.list_scroll_y;

        // Font count header
        let count_str = format!("{} families", families.len());
        tree.text(list_x + CONTENT_PADDING, y, &count_str, COL_SUBTEXT0, 12.0);
        y += 24.0;

        for family in &families {
            // Background highlight for selected
            let variants = self.visible_fonts().into_iter()
                .filter(|f| &f.family == family)
                .collect::<Vec<_>>();
            let is_selected = self.selected_font
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
            text_bold(tree, list_x + CONTENT_PADDING, y + 8.0, family, name_color, 15.0);

            // Style variants and metadata
            let styles: Vec<&str> = variants.iter().map(|v| v.style.label()).collect();
            let styles_str = styles.join(", ");
            let enabled_count = variants.iter().filter(|v| v.enabled).count();
            let meta = format!("{styles_str}  |  {enabled_count}/{} enabled", variants.len());
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
                tree.text(disabled_x, y + 16.0, "partially disabled", COL_SUBTEXT0, 10.0);
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
            text_bold(tree, panel_x + CONTENT_PADDING, y, &font.family, COL_TEXT, 18.0);
            y += 28.0;

            // Style and format
            let info_str = format!("{} -- {}", font.style.label(), font.format.label());
            tree.text(panel_x + CONTENT_PADDING, y, &info_str, COL_SUBTEXT0, 12.0);
            y += 20.0;

            // Version and glyph count
            let detail_str = format!("v{}  |  {} glyphs", font.version, font.glyph_count);
            tree.text(panel_x + CONTENT_PADDING, y, &detail_str, COL_SUBTEXT0, 11.0);
            y += 20.0;

            // System/User badge
            let badge = if font.system { "System Font" } else { "User Font" };
            let badge_color = if font.system { COL_ACCENT } else { COL_TEAL };
            let badge_w = badge.len() as f32 * 7.0 + 16.0;
            fill_rounded(tree, panel_x + CONTENT_PADDING, y, badge_w, 22.0, badge_color, 4.0);
            tree.text(panel_x + CONTENT_PADDING + 8.0, y + 4.0, badge, COL_CRUST, 11.0);
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
            text_bold(tree, panel_x + CONTENT_PADDING, y, "Preview", COL_TEXT, 14.0);
            y += 24.0;

            // Render preview at multiple sizes
            for size in PREVIEW_SIZE_LABELS {
                let size_label = format!("{size}pt");
                tree.text(panel_x + CONTENT_PADDING, y, &size_label, COL_SUBTEXT0, 10.0);
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
        tree.fill_rect(0.0, 0.0, self.window_width, self.window_height,
            Color::rgba(0, 0, 0, 160));

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
            tree, value_x, y,
            HintMode::ALL,
            self.render_settings.hinting,
            |m| m.label(),
        );
        y += 32.0;

        // Antialiasing
        tree.text(label_x, y, "Antialiasing", COL_TEXT, 13.0);
        render_setting_options(
            tree, value_x, y,
            AntialiasingMode::ALL,
            self.render_settings.antialiasing,
            |m| m.label(),
        );
        y += 32.0;

        // Subpixel order
        tree.text(label_x, y, "Subpixel Order", COL_TEXT, 13.0);
        render_setting_options(
            tree, value_x, y,
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
    });
}

/// Render a toolbar button.
fn render_toolbar_button(tree: &mut RenderTree, x: f32, y: f32, label: &str, color: Color) {
    let w = label.len() as f32 * 8.0 + 20.0;
    let h = 28.0;
    fill_rounded(tree, x, y, w, h, color, 6.0);
    tree.text(x + 10.0, y + 7.0, label, COL_CRUST, 12.0);
}

/// Render a sidebar item.
fn render_sidebar_item(tree: &mut RenderTree, y: f32, label: &str, selected: bool) {
    let item_x = SIDEBAR_PADDING;
    let item_w = SIDEBAR_WIDTH - SIDEBAR_PADDING * 2.0;

    if selected {
        fill_rounded(tree, item_x, y, item_w, CATEGORY_ITEM_HEIGHT, COL_SURFACE1, 6.0);
        // Left accent bar
        tree.fill_rect(item_x, y + 6.0, 3.0, CATEGORY_ITEM_HEIGHT - 12.0, COL_ACCENT);
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
        ox += lbl.len() as f32 * 7.0 + 12.0;
    }
}

// ============================================================================
// Application entry point
// ============================================================================

fn main() {
    let state = FontManagerState::new();

    // In a real SlateOS environment, this would enter the compositor event loop.
    // For now, render one frame to verify the UI builds correctly.
    let tree = state.render();

    // The render tree would be submitted to the compositor.
    assert!(!tree.is_empty(), "Font Manager UI must produce render commands");
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ====================================================================
    // FontCollection basics
    // ====================================================================

    #[test]
    fn test_default_collection_is_populated() {
        let coll = FontCollection::new_with_defaults();
        assert!(coll.fonts.len() >= 15, "Should have at least 15 default fonts");
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
        assert!(families.len() < coll.fonts.len(), "families() should deduplicate");
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
        let tree = state.render();
        assert!(!tree.is_empty());
        assert!(tree.len() > 30, "Should produce many render commands");
    }

    #[test]
    fn test_render_with_settings_panel() {
        let mut state = FontManagerState::new();
        state.show_settings = true;
        let tree = state.render();
        assert!(!tree.is_empty());
        // Settings panel adds many more commands.
        let base_tree = FontManagerState::new().render();
        assert!(tree.len() > base_tree.len(), "Settings overlay adds commands");
    }

    #[test]
    fn test_render_after_resize() {
        let mut state = FontManagerState::new();
        let ev = Event::Resize { width: 1400, height: 900 };
        let result = state.handle_event(&ev);
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.window_width, 1400.0);
        assert_eq!(state.window_height, 900.0);
        let tree = state.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_no_selection() {
        let mut state = FontManagerState::new();
        state.selected_font = None;
        let tree = state.render();
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
            text: None,
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
            text: None,
        });
        state.handle_event(&down);
        let up = Event::Key(KeyEvent {
            key: Key::Up,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
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
            text: None,
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
            text: None,
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
            modifiers: Modifiers { shift: false, ctrl: true, alt: false, super_key: false },
            text: None,
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
        assert_eq!(FontError::SystemFont.to_string(), "Cannot modify system font");
        assert_eq!(FontError::AlreadyInstalled.to_string(), "Font already installed");
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
        unique_ids.sort();
        unique_ids.dedup();
        assert_eq!(ids.len(), unique_ids.len(), "All IDs must be unique");
    }
}
