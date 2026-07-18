//! Font Preview — font browsing and comparison.
//!
//! Provides font preview, comparison, and metadata display
//! for installed system fonts.
//!
//! ## Architecture
//!
//! ```text
//! User browses fonts
//!   → fontpreview::list_fonts() → available fonts
//!   → fontpreview::preview(font, text) → rendered sample
//!   → fontpreview::compare(fonts) → side-by-side
//!
//! Integration:
//!   → fontmgr (font management)
//!   → fontsettings (font settings)
//!   → theme (system theme)
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

/// Font style.
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
}

impl FontStyle {
    pub fn label(self) -> &'static str {
        match self {
            Self::Regular => "Regular",
            Self::Bold => "Bold",
            Self::Italic => "Italic",
            Self::BoldItalic => "Bold Italic",
            Self::Light => "Light",
            Self::Medium => "Medium",
            Self::SemiBold => "SemiBold",
            Self::ExtraBold => "ExtraBold",
        }
    }
}

/// Font category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontCategory {
    SansSerif,
    Serif,
    Monospace,
    Display,
    Handwriting,
    Symbol,
}

impl FontCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::SansSerif => "Sans Serif",
            Self::Serif => "Serif",
            Self::Monospace => "Monospace",
            Self::Display => "Display",
            Self::Handwriting => "Handwriting",
            Self::Symbol => "Symbol",
        }
    }
}

/// A font entry.
#[derive(Debug, Clone)]
pub struct FontEntry {
    pub id: u32,
    pub family: String,
    pub style: FontStyle,
    pub category: FontCategory,
    pub file_path: String,
    pub version: String,
    pub glyph_count: u32,
    pub preview_count: u64,
}

/// A preview result.
#[derive(Debug, Clone)]
pub struct PreviewResult {
    pub font_id: u32,
    pub family: String,
    pub style: FontStyle,
    pub sample_text: String,
    pub size_pt: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_FONTS: usize = 500;

struct State {
    fonts: Vec<FontEntry>,
    next_id: u32,
    default_sample: String,
    total_previews: u64,
    total_comparisons: u64,
    ops: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    f(state)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialise the font-preview state.
///
/// Seeds only CONFIGURATION — the default preview sample text (a pangram), which
/// is a legitimate compiled-in default. It seeds NO font records.
///
/// A FontEntry carries OBSERVED metadata (`glyph_count`, `version`, an on-disk
/// `file_path` assumed to exist) that cannot be known without parsing the actual
/// font file. The previous implementation fabricated five fonts (Inter, JetBrains
/// Mono, Noto Serif) with invented glyph counts (2548/1086/3400) and version
/// strings — surfaced via the `fontpreview` shell view / `/proc` as REAL
/// installed fonts. Tellingly, `fontmgr` fabricated the SAME families with
/// DIFFERENT invented numbers, which is what exposed both as made-up. So the
/// font list now starts EMPTY; entries are added via `add_font()`.
///
/// DEFERRED PROPER FIX (tech debt): `fontpreview` should NOT keep its own font
/// registry at all — it should read through to `fontmgr` (the single source of
/// truth for installed fonts) and track only its preview-specific state
/// (`preview_count`, sample text). Unifying the two registries is a larger
/// refactor (type mapping between `fontmgr::FontInfo` and the local `FontEntry`,
/// and deciding where `preview_count` lives) and is deferred until the font
/// subsystem is consolidated. Until then both registries are honestly empty.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        fonts: Vec::new(),
        next_id: 1,
        default_sample: String::from("The quick brown fox jumps over the lazy dog"),
        total_previews: 0,
        total_comparisons: 0,
        ops: 0,
    });
}

/// Add a font.
pub fn add_font(family: &str, style: FontStyle, category: FontCategory, path: &str, version: &str, glyphs: u32) -> KernelResult<u32> {
    with_state(|state| {
        if state.fonts.len() >= MAX_FONTS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.fonts.push(FontEntry {
            id, family: String::from(family), style, category,
            file_path: String::from(path), version: String::from(version),
            glyph_count: glyphs, preview_count: 0,
        });
        Ok(id)
    })
}

/// Remove a font.
pub fn remove_font(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.fonts.len();
        state.fonts.retain(|f| f.id != id);
        if state.fonts.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Preview a font with sample text.
pub fn preview(font_id: u32, sample: Option<&str>, size_pt: u32) -> KernelResult<PreviewResult> {
    with_state(|state| {
        let font = state.fonts.iter_mut().find(|f| f.id == font_id)
            .ok_or(KernelError::NotFound)?;
        font.preview_count += 1;
        state.total_previews += 1;
        let text = sample.map(String::from).unwrap_or_else(|| state.default_sample.clone());
        Ok(PreviewResult {
            font_id: font.id,
            family: font.family.clone(),
            style: font.style,
            sample_text: text,
            size_pt,
        })
    })
}

/// Compare multiple fonts.
pub fn compare(font_ids: &[u32], sample: Option<&str>, size_pt: u32) -> KernelResult<Vec<PreviewResult>> {
    with_state(|state| {
        state.total_comparisons += 1;
        let text = sample.map(String::from).unwrap_or_else(|| state.default_sample.clone());
        let mut results = Vec::new();
        for &id in font_ids {
            if let Some(font) = state.fonts.iter_mut().find(|f| f.id == id) {
                font.preview_count += 1;
                state.total_previews += 1;
                results.push(PreviewResult {
                    font_id: font.id,
                    family: font.family.clone(),
                    style: font.style,
                    sample_text: text.clone(),
                    size_pt,
                });
            }
        }
        Ok(results)
    })
}

/// List all fonts.
pub fn list_fonts() -> Vec<FontEntry> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.fonts.clone())
}

/// Search fonts by family name.
pub fn search(query: &str) -> Vec<FontEntry> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let q = query.to_lowercase();
        s.fonts.iter()
            .filter(|f| f.family.to_lowercase().contains(&q))
            .cloned()
            .collect()
    })
}

/// List fonts by category.
pub fn by_category(category: FontCategory) -> Vec<FontEntry> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.fonts.iter().filter(|f| f.category == category).cloned().collect()
    })
}

/// Set default sample text.
pub fn set_sample(text: &str) -> KernelResult<()> {
    with_state(|state| {
        state.default_sample = String::from(text);
        Ok(())
    })
}

/// Statistics: (font_count, total_previews, total_comparisons, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.fonts.len(), s.total_previews, s.total_comparisons, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("fontpreview::self_test() — running tests...");

    // Residue-free: start from a known-empty state.
    *STATE.lock() = None;
    init_defaults();

    // init_defaults seeds NO fonts (we never fabricate installed fonts); only the
    // default sample text is configured. Build the test fixtures explicitly via
    // add_font(). add_font assigns ids from next_id (1), so these land on 1..=5.
    assert!(list_fonts().is_empty());
    let _ = add_font("Inter", FontStyle::Regular, FontCategory::SansSerif, "/fonts/inter-regular.ttf", "0.0", 0).expect("fixture 1");
    let _ = add_font("Inter", FontStyle::Bold, FontCategory::SansSerif, "/fonts/inter-bold.ttf", "0.0", 0).expect("fixture 2");
    let _ = add_font("JetBrains Mono", FontStyle::Regular, FontCategory::Monospace, "/fonts/jetbrainsmono-regular.ttf", "0.0", 0).expect("fixture 3");
    let _ = add_font("Noto Serif", FontStyle::Regular, FontCategory::Serif, "/fonts/notoserif-regular.ttf", "0.0", 0).expect("fixture 4");
    let _ = add_font("Noto Serif", FontStyle::Italic, FontCategory::Serif, "/fonts/notoserif-italic.ttf", "0.0", 0).expect("fixture 5");

    // 1: Fixtures present.
    assert_eq!(list_fonts().len(), 5);
    crate::serial_println!("  [1/8] fixtures: OK");

    // 2: Preview a font.
    let p = preview(1, None, 16).expect("preview");
    assert_eq!(p.family, "Inter");
    assert_eq!(p.size_pt, 16);
    assert!(p.sample_text.contains("quick brown"));
    crate::serial_println!("  [2/8] preview: OK");

    // 3: Custom sample text.
    let p = preview(3, Some("Hello World"), 24).expect("preview2");
    assert_eq!(p.sample_text, "Hello World");
    assert_eq!(p.family, "JetBrains Mono");
    crate::serial_println!("  [3/8] custom sample: OK");

    // 4: Compare fonts.
    let results = compare(&[1, 3, 4], None, 14).expect("compare");
    assert_eq!(results.len(), 3);
    crate::serial_println!("  [4/8] compare: OK");

    // 5: Search fonts.
    let results = search("noto");
    assert_eq!(results.len(), 2);
    crate::serial_println!("  [5/8] search: OK");

    // 6: By category.
    let mono = by_category(FontCategory::Monospace);
    assert_eq!(mono.len(), 1);
    assert_eq!(mono[0].family, "JetBrains Mono");
    crate::serial_println!("  [6/8] category: OK");

    // 7: Add font.
    let _id = add_font("Roboto", FontStyle::Regular, FontCategory::SansSerif, "/fonts/roboto.ttf", "3.0", 1500).expect("add");
    assert_eq!(list_fonts().len(), 6);
    crate::serial_println!("  [7/8] add: OK");

    // 8: Stats.
    let (fonts, previews, comparisons, ops) = stats();
    assert_eq!(fonts, 6);
    assert!(previews >= 5); // 1 + 1 + 3.
    assert_eq!(comparisons, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Residue-free: leave no fixtures behind.
    *STATE.lock() = None;

    crate::serial_println!("fontpreview::self_test() — all 8 tests passed");
}
