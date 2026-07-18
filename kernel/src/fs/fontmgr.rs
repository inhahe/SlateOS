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

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

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
        if f.enabled && !families.contains(&f.family) {
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

/// Initialise font configuration defaults.
///
/// This seeds only CONFIGURATION — the preferred default font family per role
/// and the global render settings (size, hinting, antialiasing, DPI). These are
/// legitimate compiled-in policy defaults, analogous to a default fontconfig:
/// they express what the OS WANTS to use, not an observation of what is present.
///
/// It deliberately seeds NO font records. A FontInfo carries OBSERVED metadata —
/// `glyph_count`, `unicode_range_count`, `version`, and an on-disk `path` that is
/// assumed to exist — none of which can be known without parsing the actual
/// `.ttf`/`.otf` file. The previous implementation fabricated nine "system"
/// fonts (Noto Sans/Serif, JetBrains Mono, Noto Color Emoji) with invented glyph
/// counts (3500/3200/800/3600), a placeholder version "1.0", and paths under
/// `/usr/share/fonts/...` that the kernel never verifies. `fontpreview` did the
/// same with DIFFERENT invented numbers for the same families (Inter 2548,
/// JetBrains Mono 1086, Noto Serif 3400) — proof these were made up, not read.
/// The `fontmgr` shell command and any `/proc` view surfaced those as REAL
/// installed fonts. So the registry now starts EMPTY; real fonts are added via
/// `install_font()` or a font-directory scanner once one exists.
///
/// DEFERRED PROPER FIX: add a font-directory scanner that walks the real font
/// path, parses each face (TrueType/OpenType tables) for its actual glyph count,
/// Unicode coverage, version, and family/style, and registers it as a system
/// font. Until that parser exists there is no honest source for the observed
/// metadata, so no fonts are seeded. NOTE (tech debt): `fontpreview` keeps its
/// OWN parallel, conflicting fabricated font list — the two should be unified so
/// `fontpreview` reads through to `fontmgr` as the single source of truth rather
/// than maintaining a second registry.
pub fn init_defaults() {
    let mut state = STATE.lock();

    // Preferred default family per role (config policy, not an observation).
    state.defaults = DefaultFonts {
        ui: String::from("Noto Sans"),
        document: String::from("Noto Serif"),
        monospace: String::from("JetBrains Mono"),
        titlebar: String::from("Noto Sans"),
        fallback: String::from("Noto Color Emoji"),
    };

    // Global render settings (config defaults).
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
            if !fams.contains(&f.family.as_str()) {
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
    let _f1 = install_font("TestSans", FontStyle::Regular, FontFormat::TrueType, FontCategory::SansSerif, "/fonts/test.ttf", "1.0", 500)?;
    let _f2 = install_font("TestSans", FontStyle::Bold, FontFormat::TrueType, FontCategory::SansSerif, "/fonts/test-bold.ttf", "1.0", 500)?;
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

    // Test 8: init_defaults seeds CONFIG ONLY, never fabricated font records.
    // After init the registry must be EMPTY (no fonts are invented), while the
    // policy defaults and render settings are populated.
    serial_println!("fontmgr::self_test 8: init_defaults seeds config, no fonts");
    clear_all();
    init_defaults();
    let (total, families, system, _) = stats();
    assert_eq!(total, 0);
    assert_eq!(families, 0);
    assert_eq!(system, 0);
    let defs = default_fonts();
    assert_eq!(defs.ui, "Noto Sans");
    assert_eq!(defs.monospace, "JetBrains Mono");
    let rs = render_settings();
    assert_eq!(rs.global_size_pt, 10);
    assert_eq!(rs.dpi, 96);

    // Test 9: system fonts are uninstall-protected. There is no public API to
    // create one yet (a real font-directory scanner will, once it exists), so
    // install a deterministic system-font fixture directly into STATE to verify
    // the protection path still rejects the removal.
    serial_println!("fontmgr::self_test 9: system font uninstall protection");
    let sys_id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    {
        let mut state = STATE.lock();
        state.fonts.push(FontInfo {
            id: sys_id,
            family: String::from("FixtureSystem"),
            style: FontStyle::Regular,
            format: FontFormat::TrueType,
            category: FontCategory::SansSerif,
            path: String::from("/fixture/system.ttf"),
            version: String::from("0.0"),
            system: true,
            enabled: true,
            glyph_count: 0,
            unicode_range_count: 0,
        });
    }
    assert!(uninstall_font(sys_id).is_err());

    clear_all();
    serial_println!("fontmgr::self_test: all 9 tests passed");
    Ok(())
}
