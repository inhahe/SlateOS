//! Font Settings — font rendering and antialiasing configuration.
//!
//! Controls system-wide font rendering: antialiasing method, hinting,
//! subpixel rendering order, default font sizes, and DPI-aware scaling.
//!
//! ## Architecture
//!
//! ```text
//! Text rendering
//!   → fontsettings::get_config() → rendering parameters
//!   → fontsettings::set_antialiasing(method)
//!   → fontsettings::set_hinting(level)
//!
//! Integration:
//!   → fontmgr (font management)
//!   → dpiscaling (DPI-aware sizing)
//!   → theme (theme font preferences)
//!   → a11y (accessibility font sizes)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Antialiasing method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Antialiasing {
    None,
    Grayscale,
    Subpixel,
    SubpixelBgr,
}

impl Antialiasing {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Grayscale => "Grayscale",
            Self::Subpixel => "Subpixel (RGB)",
            Self::SubpixelBgr => "Subpixel (BGR)",
        }
    }
}

/// Font hinting level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hinting {
    None,
    Slight,
    Medium,
    Full,
}

impl Hinting {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Slight => "Slight",
            Self::Medium => "Medium",
            Self::Full => "Full",
        }
    }
}

/// Font rendering configuration.
#[derive(Debug, Clone)]
pub struct FontConfig {
    pub antialiasing: Antialiasing,
    pub hinting: Hinting,
    /// Default UI font family.
    pub default_family: String,
    /// Default monospace font family.
    pub monospace_family: String,
    /// Default document font family.
    pub document_family: String,
    /// Default UI font size in points * 10 (e.g., 100 = 10pt).
    pub default_size_dp: u32,
    /// Monospace font size in points * 10.
    pub monospace_size_dp: u32,
    /// Minimum font size in points * 10.
    pub min_size_dp: u32,
    /// Text scaling factor in percent (100 = normal).
    pub text_scale_percent: u32,
    /// Enable ligatures in monospace fonts.
    pub ligatures: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    config: FontConfig,
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

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        config: FontConfig {
            antialiasing: Antialiasing::Subpixel,
            hinting: Hinting::Slight,
            default_family: String::from("Sans"),
            monospace_family: String::from("Monospace"),
            document_family: String::from("Serif"),
            default_size_dp: 100,
            monospace_size_dp: 100,
            min_size_dp: 60,
            text_scale_percent: 100,
            ligatures: false,
        },
        total_changes: 0,
        ops: 0,
    });
}

/// Set antialiasing method.
pub fn set_antialiasing(method: Antialiasing) -> KernelResult<()> {
    with_state(|state| {
        state.config.antialiasing = method;
        state.total_changes += 1;
        Ok(())
    })
}

/// Set hinting level.
pub fn set_hinting(level: Hinting) -> KernelResult<()> {
    with_state(|state| {
        state.config.hinting = level;
        state.total_changes += 1;
        Ok(())
    })
}

/// Set default font family.
pub fn set_default_font(family: &str) -> KernelResult<()> {
    with_state(|state| {
        state.config.default_family = String::from(family);
        state.total_changes += 1;
        Ok(())
    })
}

/// Set monospace font family.
pub fn set_monospace_font(family: &str) -> KernelResult<()> {
    with_state(|state| {
        state.config.monospace_family = String::from(family);
        state.total_changes += 1;
        Ok(())
    })
}

/// Set default font size (in decipoints, e.g., 120 = 12pt).
pub fn set_default_size(size_dp: u32) -> KernelResult<()> {
    with_state(|state| {
        state.config.default_size_dp = size_dp.clamp(60, 400);
        state.total_changes += 1;
        Ok(())
    })
}

/// Set text scaling factor (percent).
pub fn set_text_scale(percent: u32) -> KernelResult<()> {
    with_state(|state| {
        state.config.text_scale_percent = percent.clamp(50, 300);
        state.total_changes += 1;
        Ok(())
    })
}

/// Set ligatures.
pub fn set_ligatures(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.ligatures = enabled;
        state.total_changes += 1;
        Ok(())
    })
}

/// Get current config.
pub fn get_config() -> Option<FontConfig> {
    STATE.lock().as_ref().map(|s| s.config.clone())
}

/// Statistics: (total_changes, ops).
pub fn stats() -> (u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.total_changes, s.ops),
        None => (0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("fontsettings::self_test() — running tests...");
    init_defaults();

    // 1: Default config.
    let cfg = get_config().expect("config");
    assert_eq!(cfg.antialiasing, Antialiasing::Subpixel);
    assert_eq!(cfg.hinting, Hinting::Slight);
    assert_eq!(cfg.default_size_dp, 100);
    crate::serial_println!("  [1/8] default config: OK");

    // 2: Set antialiasing.
    set_antialiasing(Antialiasing::Grayscale).expect("aa");
    let cfg = get_config().expect("config2");
    assert_eq!(cfg.antialiasing, Antialiasing::Grayscale);
    crate::serial_println!("  [2/8] antialiasing: OK");

    // 3: Set hinting.
    set_hinting(Hinting::Full).expect("hint");
    let cfg = get_config().expect("config3");
    assert_eq!(cfg.hinting, Hinting::Full);
    crate::serial_println!("  [3/8] hinting: OK");

    // 4: Set font family.
    set_default_font("Noto Sans").expect("font");
    let cfg = get_config().expect("config4");
    assert_eq!(cfg.default_family, "Noto Sans");
    crate::serial_println!("  [4/8] default font: OK");

    // 5: Set size.
    set_default_size(120).expect("size");
    let cfg = get_config().expect("config5");
    assert_eq!(cfg.default_size_dp, 120);
    crate::serial_println!("  [5/8] font size: OK");

    // 6: Clamp size.
    set_default_size(500).expect("clamp");
    let cfg = get_config().expect("config6");
    assert_eq!(cfg.default_size_dp, 400);
    crate::serial_println!("  [6/8] clamp size: OK");

    // 7: Text scaling.
    set_text_scale(125).expect("scale");
    let cfg = get_config().expect("config7");
    assert_eq!(cfg.text_scale_percent, 125);
    crate::serial_println!("  [7/8] text scale: OK");

    // 8: Stats.
    let (changes, ops) = stats();
    assert!(changes >= 6);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("fontsettings::self_test() — all 8 tests passed");
}
