//! Font manager — system font registry, discovery, and configuration.
//!
//! Manages installed fonts for the compositor, text rendering, and
//! applications.  Provides font enumeration, family/style lookup,
//! default font selection, and per-application font overrides.
//!
//! ## Design Reference
//!
//! design.txt line 773: "colors, borders, padding, margins, fonts, ..."
//! design.txt line 812-814: text views use single font or rich text fonts
//! design.txt line 883: "configurable colors and font" for terminal
//! design.txt line 1339: "detect monitor DPI and set default font scaling"
//!
//! ## Architecture
//!
//! ```text
//! Text rendering / compositor
//!   → fontmgr::find_font(family, style) → FontInfo
//!   → fontmgr::default_fonts() → DefaultFonts
//!
//! Settings panel → Appearance → Fonts
//!   → fontmgr::list_fonts() → all installed fonts
//!   → fontmgr::set_default(role, family)
//!   → fontmgr::set_global_size(pt)
//!
//! Font installer
//!   → fontmgr::install_font(path, data)
//!   → fontmgr::uninstall_font(font_id)
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Font style/weight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontStyle {
    Regular,
    Bold,
    Italic,
    BoldItalic,
    Light,
    Medium,
    SemiBold,
    ExtraBold,
    Thin,
}

/// Font format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontFormat {
    TrueType,
    OpenType,
    Woff,
    Woff2,
    Bitmap,
}

/// Font category for browsing/filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontCategory {
    SansSerif,
    Serif,
    Monospace,
    Display,
    Handwriting,
    Symbol,
}

/// Semantic role for default font selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontRole {
    /// Default UI font (menus, labels, buttons).
    Ui,
    /// Document/body text.
    Document,
    /// Monospace (terminal, code editor).
    Monospace,
    /// Window title bars.
    Titlebar,
    /// Fallback for missing glyphs.
    Fallback,
}

/// Hinting preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintMode {
    None,
    Slight,
    Medium,
    Full,
}

/// Antialiasing mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AntialiasMode {
    None,
    Grayscale,
    Subpixel,
}

/// Subpixel layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubpixelOrder {
    Rgb,
    Bgr,
    VRgb,
    VBgr,
}

/// An installed font face.
#[derive(Debug, Clone)]
pub struct FontInfo {
    /// Unique ID.
    pub id: u64,
    /// Font family name (e.g., "Noto Sans").
    pub family: String,
    /// Style variant.
    pub style: FontStyle,
    /// Format.
    pub format: FontFormat,
    /// Category.
    pub category: FontCategory,
    /// Path to font file.
    pub path: String,
    /// Font version string.
    pub version: String,
    /// Whether this is a system font (cannot be uninstalled).
    pub system: bool,
    /// Whether this font is enabled.
    pub enabled: bool,
    /// Number of glyphs.
    pub glyph_count: u32,
    /// Supported Unicode ranges (simplified: count of ranges).
    pub unicode_range_count: u32,
}

/// Default font assignments.
#[derive(Debug, Clone)]
pub struct DefaultFonts {
    pub ui: String,
    pub document: String,
    pub monospace: String,
    pub titlebar: String,
    pub fallback: String,
}

/// Global font rendering settings.
#[derive(Debug, Clone)]
pub struct RenderSettings {
    pub global_size_pt: u32,
    pub hint_mode: HintMode,
    pub antialias: AntialiasMode,
    pub subpixel_order: SubpixelOrder,
    pub dpi: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    fonts: Vec<FontInfo>,
    defaults: DefaultFonts,
    render: RenderSettings,
    changes: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    fonts: Vec::new(),
    defaults: DefaultFonts {
        ui: String::new(),
        document: String::new(),
        monospace: String::new(),
        titlebar: String::new(),
        fallback: String::new(),
    },
    render: RenderSettings {
        global_size_pt: 10,
        hint_mode: HintMode::Slight,
        antialias: AntialiasMode::Subpixel,
        subpixel_order: SubpixelOrder::Rgb,
        dpi: 96,
    },
    changes: 0,
});

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static OP_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Font management
// ---------------------------------------------------------------------------

/// Install a font.
pub fn install_font(
    family: &str,
    style: FontStyle,
    format: FontFormat,
    category: FontCategory,
    path: &str,
    version: &str,
    glyph_count: u32,
) -> KernelResult<u64> {
    let mut state = STATE.lock();
    if state.fonts.len() >= 2048 {
        return Err(KernelError::ResourceExhausted);
    }
    // Check for duplicate family+style.
    if state.fonts.iter().any(|f| f.family == family && f.style == style && f.path == path) {
        return Err(KernelError::AlreadyExists);
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    state.fonts.push(FontInfo {
        id,
        family: String::from(family),
        style,
        format,
        category,
        path: String::from(path),
        version: String::from(version),
        system: false,
        enabled: true,
        glyph_count,
        unicode_range_count: 1,
    });
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(id)
}

/// Uninstall a font (system fonts cannot be uninstalled).
pub fn uninstall_font(font_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let font = state.fonts.iter().find(|f| f.id == font_id)
        .ok_or(KernelError::NotFound)?;
    if font.system {
        return Err(KernelError::PermissionDenied);
    }
    state.fonts.retain(|f| f.id != font_id);
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Get font by ID.
pub fn get_font(font_id: u64) -> KernelResult<FontInfo> {
    STATE.lock().fonts.iter().find(|f| f.id == font_id).cloned()
        .ok_or(KernelError::NotFound)
}

/// List all fonts, optionally filtered by category.
pub fn list_fonts(category: Option<FontCategory>) -> Vec<FontInfo> {
    let state = STATE.lock();
    match category {
        Some(cat) => state.fonts.iter().filter(|f| f.category == cat).cloned().collect(),
        None => state.fonts.clone(),
    }
}

/// Find fonts matching a family name.
pub fn find_family(family: &str) -> Vec<FontInfo> {
    STATE.lock().fonts.iter().filter(|f| f.family == family && f.enabled).cloned().collect()
}

/// Find a specific font by family and style.
pub fn find_font(family: &str, style: FontStyle) -> KernelResult<FontInfo> {
    STATE.lock().fonts.iter()
        .find(|f| f.family == family && f.style == style && f.enabled)
        .cloned()
        .ok_or(KernelError::NotFound)
}

/// List unique family names.
pub fn list_families() -> Vec<String> {
    let state = STATE.lock();
    let mut families: Vec<String> = Vec::new();
    for f in &state.fonts {
        if f.enabled && !families.iter().any(|fam| *fam == f.family) {
            families.push(f.family.clone());
        }
    }
    families
}

/// Enable or disable a font.
pub fn set_enabled(font_id: u64, enabled: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let font = state.fonts.iter_mut().find(|f| f.id == font_id)
        .ok_or(KernelError::NotFound)?;
    font.enabled = enabled;
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// Default font assignments
// ---------------------------------------------------------------------------

/// Set the default font for a role.
pub fn set_default(role: FontRole, family: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    // Verify font exists.
    if !state.fonts.iter().any(|f| f.family == family && f.enabled) {
        return Err(KernelError::NotFound);
    }
    match role {
        FontRole::Ui => state.defaults.ui = String::from(family),
        FontRole::Document => state.defaults.document = String::from(family),
        FontRole::Monospace => state.defaults.monospace = String::from(family),
        FontRole::Titlebar => state.defaults.titlebar = String::from(family),
        FontRole::Fallback => state.defaults.fallback = String::from(family),
    }
    state.changes += 1;
    Ok(())
}

/// Get current default fonts.
pub fn default_fonts() -> DefaultFonts {
    STATE.lock().defaults.clone()
}

// ---------------------------------------------------------------------------
// Rendering settings
// ---------------------------------------------------------------------------

/// Set global font size in points.
pub fn set_global_size(pt: u32) -> KernelResult<()> {
    if pt < 4 || pt > 72 {
        return Err(KernelError::InvalidArgument);
    }
    STATE.lock().render.global_size_pt = pt;
    Ok(())
}

/// Set hinting mode.
pub fn set_hint_mode(mode: HintMode) {
    STATE.lock().render.hint_mode = mode;
}

/// Set antialiasing mode.
pub fn set_antialias(mode: AntialiasMode) {
    STATE.lock().render.antialias = mode;
}

/// Set subpixel order.
pub fn set_subpixel_order(order: SubpixelOrder) {
    STATE.lock().render.subpixel_order = order;
}

/// Set rendering DPI.
pub fn set_dpi(dpi: u32) -> KernelResult<()> {
    if dpi < 48 || dpi > 600 {
        return Err(KernelError::InvalidArgument);
    }
    STATE.lock().render.dpi = dpi;
    Ok(())
}

/// Get current rendering settings.
pub fn render_settings() -> RenderSettings {
    STATE.lock().render.clone()
}

// ---------------------------------------------------------------------------
// Init / stats
// ---------------------------------------------------------------------------

fn add_system_font(state: &mut State, family: &str, style: FontStyle, format: FontFormat,
    category: FontCategory, path: &str, glyphs: u32)
{
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    state.fonts.push(FontInfo {
        id,
        family: String::from(family),
        style,
        format,
        category,
        path: String::from(path),
        version: String::from("1.0"),
        system: true,
        enabled: true,
        glyph_count: glyphs,
        unicode_range_count: 4,
    });
}

/// Initialise with system fonts.
pub fn init_defaults() {
    let mut state = STATE.lock();
    if !state.fonts.is_empty() {
        return;
    }

    // System sans-serif family (Noto Sans — good Unicode coverage).
    add_system_font(&mut state, "Noto Sans", FontStyle::Regular, FontFormat::TrueType, FontCategory::SansSerif, "/usr/share/fonts/noto/NotoSans-Regular.ttf", 3500);
    add_system_font(&mut state, "Noto Sans", FontStyle::Bold, FontFormat::TrueType, FontCategory::SansSerif, "/usr/share/fonts/noto/NotoSans-Bold.ttf", 3500);
    add_system_font(&mut state, "Noto Sans", FontStyle::Italic, FontFormat::TrueType, FontCategory::SansSerif, "/usr/share/fonts/noto/NotoSans-Italic.ttf", 3500);
    add_system_font(&mut state, "Noto Sans", FontStyle::BoldItalic, FontFormat::TrueType, FontCategory::SansSerif, "/usr/share/fonts/noto/NotoSans-BoldItalic.ttf", 3500);

    // System serif family.
    add_system_font(&mut state, "Noto Serif", FontStyle::Regular, FontFormat::TrueType, FontCategory::Serif, "/usr/share/fonts/noto/NotoSerif-Regular.ttf", 3200);
    add_system_font(&mut state, "Noto Serif", FontStyle::Bold, FontFormat::TrueType, FontCategory::Serif, "/usr/share/fonts/noto/NotoSerif-Bold.ttf", 3200);

    // System monospace family.
    add_system_font(&mut state, "JetBrains Mono", FontStyle::Regular, FontFormat::TrueType, FontCategory::Monospace, "/usr/share/fonts/jetbrains/JetBrainsMono-Regular.ttf", 800);
    add_system_font(&mut state, "JetBrains Mono", FontStyle::Bold, FontFormat::TrueType, FontCategory::Monospace, "/usr/share/fonts/jetbrains/JetBrainsMono-Bold.ttf", 800);

    // Emoji/symbol font.
    add_system_font(&mut state, "Noto Color Emoji", FontStyle::Regular, FontFormat::OpenType, FontCategory::Symbol, "/usr/share/fonts/noto/NotoColorEmoji.ttf", 3600);

    // Set defaults.
    state.defaults = DefaultFonts {
        ui: String::from("Noto Sans"),
        document: String::from("Noto Serif"),
        monospace: String::from("JetBrains Mono"),
        titlebar: String::from("Noto Sans"),
        fallback: String::from("Noto Color Emoji"),
    };

    state.render = RenderSettings {
        global_size_pt: 10,
        hint_mode: HintMode::Slight,
        antialias: AntialiasMode::Subpixel,
        subpixel_order: SubpixelOrder::Rgb,
        dpi: 96,
    };

    state.changes += 1;
}

/// Return (font_count, family_count, system_count, ops).
pub fn stats() -> (usize, usize, usize, u64) {
    let state = STATE.lock();
    let total = state.fonts.len();
    let families = {
        let mut fams: Vec<&str> = Vec::new();
        for f in &state.fonts {
            if !fams.iter().any(|fam| *fam == f.family.as_str()) {
                fams.push(&f.family);
            }
        }
        fams.len()
    };
    let system = state.fonts.iter().filter(|f| f.system).count();
    let ops = OP_COUNT.load(Ordering::Relaxed);
    (total, families, system, ops)
}

pub fn reset_stats() {
    OP_COUNT.store(0, Ordering::Relaxed);
}

pub fn clear_all() {
    let mut state = STATE.lock();
    state.fonts.clear();
    state.defaults = DefaultFonts {
        ui: String::new(), document: String::new(),
        monospace: String::new(), titlebar: String::new(),
        fallback: String::new(),
    };
    state.render = RenderSettings {
        global_size_pt: 10, hint_mode: HintMode::Slight,
        antialias: AntialiasMode::Subpixel, subpixel_order: SubpixelOrder::Rgb, dpi: 96,
    };
    state.changes = 0;
    NEXT_ID.store(1, Ordering::Relaxed);
    OP_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();

    // Test 1: install fonts.
    serial_println!("fontmgr::self_test 1: install fonts");
    let f1 = install_font("TestSans", FontStyle::Regular, FontFormat::TrueType, FontCategory::SansSerif, "/fonts/test.ttf", "1.0", 500)?;
    let f2 = install_font("TestSans", FontStyle::Bold, FontFormat::TrueType, FontCategory::SansSerif, "/fonts/test-bold.ttf", "1.0", 500)?;
    let f3 = install_font("TestMono", FontStyle::Regular, FontFormat::OpenType, FontCategory::Monospace, "/fonts/mono.otf", "2.0", 300)?;
    assert_eq!(list_fonts(None).len(), 3);

    // Test 2: find fonts.
    serial_println!("fontmgr::self_test 2: find fonts");
    let family = find_family("TestSans");
    assert_eq!(family.len(), 2);
    let mono = find_font("TestMono", FontStyle::Regular)?;
    assert_eq!(mono.glyph_count, 300);
    assert!(find_font("TestMono", FontStyle::Bold).is_err());

    // Test 3: list families and categories.
    serial_println!("fontmgr::self_test 3: families and categories");
    let fams = list_families();
    assert_eq!(fams.len(), 2);
    let sans = list_fonts(Some(FontCategory::SansSerif));
    assert_eq!(sans.len(), 2);
    let mono_list = list_fonts(Some(FontCategory::Monospace));
    assert_eq!(mono_list.len(), 1);

    // Test 4: default fonts.
    serial_println!("fontmgr::self_test 4: default fonts");
    set_default(FontRole::Ui, "TestSans")?;
    set_default(FontRole::Monospace, "TestMono")?;
    let defs = default_fonts();
    assert_eq!(defs.ui, "TestSans");
    assert_eq!(defs.monospace, "TestMono");
    // Non-existent family rejected.
    assert!(set_default(FontRole::Ui, "NonExistent").is_err());

    // Test 5: enable/disable.
    serial_println!("fontmgr::self_test 5: enable/disable");
    set_enabled(f3, false)?;
    assert!(find_font("TestMono", FontStyle::Regular).is_err()); // disabled
    set_enabled(f3, true)?;
    assert!(find_font("TestMono", FontStyle::Regular).is_ok());

    // Test 6: rendering settings.
    serial_println!("fontmgr::self_test 6: rendering settings");
    set_global_size(12)?;
    set_hint_mode(HintMode::Full);
    set_antialias(AntialiasMode::Grayscale);
    set_dpi(144)?;
    let rs = render_settings();
    assert_eq!(rs.global_size_pt, 12);
    assert_eq!(rs.dpi, 144);
    assert!(set_global_size(3).is_err()); // too small
    assert!(set_dpi(30).is_err()); // too low

    // Test 7: uninstall (user fonts only).
    serial_println!("fontmgr::self_test 7: uninstall");
    uninstall_font(f3)?;
    assert_eq!(list_fonts(None).len(), 2);
    // Test init_defaults creates system fonts that can't be removed.
    clear_all();
    init_defaults();
    let (total, _, system, _) = stats();
    assert!(total >= 9);
    assert!(system >= 9);
    // System font cannot be uninstalled.
    let first = list_fonts(None)[0].id;
    assert!(uninstall_font(first).is_err());

    clear_all();
    serial_println!("fontmgr::self_test: all 7 tests passed");
    Ok(())
}
