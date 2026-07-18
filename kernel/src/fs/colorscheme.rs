//! Color Scheme — UI color scheme management.
//!
//! Manages system-wide color schemes including light/dark mode,
//! accent colors, and custom color palettes.
//!
//! ## Architecture
//!
//! ```text
//! UI renders elements
//!   → colorscheme::get_color(role) → color value
//!
//! Configuration
//!   → colorscheme::set_mode(light/dark)
//!   → colorscheme::set_accent(color)
//!   → colorscheme::set_scheme(name)
//!
//! Integration:
//!   → theme (theme management)
//!   → displaycolor (display color profiles)
//!   → nightlight (color temperature)
//!   → a11y (high contrast modes)
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

/// Color mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Light,
    Dark,
    Auto,
    HighContrast,
}

impl ColorMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
            Self::Auto => "Auto",
            Self::HighContrast => "High Contrast",
        }
    }
}

/// Color role in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorRole {
    Background,
    Surface,
    Primary,
    Secondary,
    Accent,
    TextPrimary,
    TextSecondary,
    Border,
    Error,
    Warning,
    Success,
    Link,
}

impl ColorRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::Background => "Background",
            Self::Surface => "Surface",
            Self::Primary => "Primary",
            Self::Secondary => "Secondary",
            Self::Accent => "Accent",
            Self::TextPrimary => "Text Primary",
            Self::TextSecondary => "Text Secondary",
            Self::Border => "Border",
            Self::Error => "Error",
            Self::Warning => "Warning",
            Self::Success => "Success",
            Self::Link => "Link",
        }
    }
}

/// A color value as RGB hex string.
#[derive(Debug, Clone)]
pub struct ColorEntry {
    pub role: ColorRole,
    pub hex: String,
}

/// A complete color scheme.
#[derive(Debug, Clone)]
pub struct Scheme {
    pub id: u32,
    pub name: String,
    pub mode: ColorMode,
    pub colors: Vec<ColorEntry>,
    pub accent: String,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_SCHEMES: usize = 20;

struct State {
    schemes: Vec<Scheme>,
    active_scheme_id: u32,
    next_id: u32,
    total_changes: u64,
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

fn light_colors() -> Vec<ColorEntry> {
    alloc::vec![
        ColorEntry { role: ColorRole::Background, hex: String::from("#ffffff") },
        ColorEntry { role: ColorRole::Surface, hex: String::from("#f5f5f5") },
        ColorEntry { role: ColorRole::Primary, hex: String::from("#1976d2") },
        ColorEntry { role: ColorRole::Secondary, hex: String::from("#424242") },
        ColorEntry { role: ColorRole::Accent, hex: String::from("#ff4081") },
        ColorEntry { role: ColorRole::TextPrimary, hex: String::from("#212121") },
        ColorEntry { role: ColorRole::TextSecondary, hex: String::from("#757575") },
        ColorEntry { role: ColorRole::Border, hex: String::from("#e0e0e0") },
        ColorEntry { role: ColorRole::Error, hex: String::from("#d32f2f") },
        ColorEntry { role: ColorRole::Warning, hex: String::from("#f57c00") },
        ColorEntry { role: ColorRole::Success, hex: String::from("#388e3c") },
        ColorEntry { role: ColorRole::Link, hex: String::from("#1565c0") },
    ]
}

fn dark_colors() -> Vec<ColorEntry> {
    alloc::vec![
        ColorEntry { role: ColorRole::Background, hex: String::from("#121212") },
        ColorEntry { role: ColorRole::Surface, hex: String::from("#1e1e1e") },
        ColorEntry { role: ColorRole::Primary, hex: String::from("#90caf9") },
        ColorEntry { role: ColorRole::Secondary, hex: String::from("#b0bec5") },
        ColorEntry { role: ColorRole::Accent, hex: String::from("#ff80ab") },
        ColorEntry { role: ColorRole::TextPrimary, hex: String::from("#e0e0e0") },
        ColorEntry { role: ColorRole::TextSecondary, hex: String::from("#9e9e9e") },
        ColorEntry { role: ColorRole::Border, hex: String::from("#333333") },
        ColorEntry { role: ColorRole::Error, hex: String::from("#ef5350") },
        ColorEntry { role: ColorRole::Warning, hex: String::from("#ffa726") },
        ColorEntry { role: ColorRole::Success, hex: String::from("#66bb6a") },
        ColorEntry { role: ColorRole::Link, hex: String::from("#42a5f5") },
    ]
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        schemes: alloc::vec![
            Scheme { id: 1, name: String::from("Light"), mode: ColorMode::Light, colors: light_colors(), accent: String::from("#1976d2") },
            Scheme { id: 2, name: String::from("Dark"), mode: ColorMode::Dark, colors: dark_colors(), accent: String::from("#90caf9") },
        ],
        active_scheme_id: 1,
        next_id: 3,
        total_changes: 0,
        ops: 0,
    });
}

/// Set active scheme.
pub fn set_scheme(id: u32) -> KernelResult<()> {
    with_state(|state| {
        if !state.schemes.iter().any(|s| s.id == id) {
            return Err(KernelError::NotFound);
        }
        state.active_scheme_id = id;
        state.total_changes += 1;
        Ok(())
    })
}

/// Set color mode (finds matching scheme).
pub fn set_mode(mode: ColorMode) -> KernelResult<()> {
    with_state(|state| {
        if let Some(s) = state.schemes.iter().find(|s| s.mode == mode) {
            state.active_scheme_id = s.id;
            state.total_changes += 1;
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    })
}

/// Set accent color for active scheme.
pub fn set_accent(hex: &str) -> KernelResult<()> {
    with_state(|state| {
        let scheme = state.schemes.iter_mut().find(|s| s.id == state.active_scheme_id)
            .ok_or(KernelError::NotFound)?;
        scheme.accent = String::from(hex);
        if let Some(c) = scheme.colors.iter_mut().find(|c| c.role == ColorRole::Accent) {
            c.hex = String::from(hex);
        }
        state.total_changes += 1;
        Ok(())
    })
}

/// Set a specific color in the active scheme.
pub fn set_color(role: ColorRole, hex: &str) -> KernelResult<()> {
    with_state(|state| {
        let scheme = state.schemes.iter_mut().find(|s| s.id == state.active_scheme_id)
            .ok_or(KernelError::NotFound)?;
        if let Some(c) = scheme.colors.iter_mut().find(|c| c.role == role) {
            c.hex = String::from(hex);
        }
        state.total_changes += 1;
        Ok(())
    })
}

/// Get a color value for a role.
pub fn get_color(role: ColorRole) -> Option<String> {
    STATE.lock().as_ref().and_then(|s| {
        s.schemes.iter().find(|sc| sc.id == s.active_scheme_id)
            .and_then(|sc| sc.colors.iter().find(|c| c.role == role))
            .map(|c| c.hex.clone())
    })
}

/// Get active scheme.
pub fn get_active() -> Option<Scheme> {
    STATE.lock().as_ref().and_then(|s| {
        s.schemes.iter().find(|sc| sc.id == s.active_scheme_id).cloned()
    })
}

/// List all schemes.
pub fn list_schemes() -> Vec<Scheme> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.schemes.clone())
}

/// Statistics: (scheme_count, total_changes, ops).
pub fn stats() -> (usize, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.schemes.len(), s.total_changes, s.ops),
        None => (0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("colorscheme::self_test() — running tests...");
    init_defaults();

    // 1: Default schemes.
    let schemes = list_schemes();
    assert_eq!(schemes.len(), 2);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Active scheme is Light.
    let active = get_active().expect("active");
    assert_eq!(active.mode, ColorMode::Light);
    crate::serial_println!("  [2/8] active: OK");

    // 3: Get color.
    let bg = get_color(ColorRole::Background).expect("color");
    assert_eq!(bg, "#ffffff");
    crate::serial_println!("  [3/8] get color: OK");

    // 4: Switch to dark.
    set_mode(ColorMode::Dark).expect("dark");
    let bg = get_color(ColorRole::Background).expect("color2");
    assert_eq!(bg, "#121212");
    crate::serial_println!("  [4/8] dark mode: OK");

    // 5: Set accent.
    set_accent("#ff5722").expect("accent");
    let active = get_active().expect("active2");
    assert_eq!(active.accent, "#ff5722");
    crate::serial_println!("  [5/8] accent: OK");

    // 6: Set custom color.
    set_color(ColorRole::Background, "#1a1a2e").expect("custom");
    let bg = get_color(ColorRole::Background).expect("color3");
    assert_eq!(bg, "#1a1a2e");
    crate::serial_println!("  [6/8] custom color: OK");

    // 7: Switch by id.
    set_scheme(1).expect("id");
    let active = get_active().expect("active3");
    assert_eq!(active.mode, ColorMode::Light);
    crate::serial_println!("  [7/8] switch by id: OK");

    // 8: Stats.
    let (schemes, changes, ops) = stats();
    assert_eq!(schemes, 2);
    assert!(changes >= 4);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("colorscheme::self_test() — all 8 tests passed");
}
