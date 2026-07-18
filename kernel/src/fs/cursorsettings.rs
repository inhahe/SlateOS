//! Mouse pointer and cursor configuration.
//!
//! Manages cursor themes, pointer speed, acceleration, and per-cursor-type
//! appearance. Applications and the compositor query these settings when
//! rendering cursors.
//!
//! ## Design Reference
//!
//! design.txt lines 1270-1271: "mouse pointer" and "mouse pointer speed".
//! Also covers general input settings for pointer devices.
//!
//! ## Architecture
//!
//! ```text
//! Compositor / window manager
//!   → cursorsettings::current_theme() → CursorTheme
//!   → cursorsettings::pointer_speed() → i8
//!   → cursorsettings::cursor_for(CursorShape) → CursorImage
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum cursor themes.
const MAX_THEMES: usize = 32;

/// Maximum cursor shapes per theme.
const MAX_CURSORS: usize = 64;

/// Maximum custom cursor size options.
const MAX_SIZES: usize = 8;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Standard cursor shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    Default,
    Pointer,       // Hand/link
    Text,          // I-beam
    Crosshair,
    Move,
    ResizeNS,
    ResizeEW,
    ResizeNWSE,
    ResizeNESW,
    Wait,
    Progress,      // Arrow + spinner
    NotAllowed,
    Help,          // Arrow + question mark
    Grab,
    Grabbing,
    ZoomIn,
    ZoomOut,
    Cell,          // Spreadsheet cell selection
    ContextMenu,
    Custom,
}

impl CursorShape {
    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Pointer => "pointer",
            Self::Text => "text",
            Self::Crosshair => "crosshair",
            Self::Move => "move",
            Self::ResizeNS => "ns-resize",
            Self::ResizeEW => "ew-resize",
            Self::ResizeNWSE => "nwse-resize",
            Self::ResizeNESW => "nesw-resize",
            Self::Wait => "wait",
            Self::Progress => "progress",
            Self::NotAllowed => "not-allowed",
            Self::Help => "help",
            Self::Grab => "grab",
            Self::Grabbing => "grabbing",
            Self::ZoomIn => "zoom-in",
            Self::ZoomOut => "zoom-out",
            Self::Cell => "cell",
            Self::ContextMenu => "context-menu",
            Self::Custom => "custom",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "default" | "arrow" => Some(Self::Default),
            "pointer" | "hand" => Some(Self::Pointer),
            "text" | "ibeam" => Some(Self::Text),
            "crosshair" | "cross" => Some(Self::Crosshair),
            "move" => Some(Self::Move),
            "ns-resize" | "ns" => Some(Self::ResizeNS),
            "ew-resize" | "ew" => Some(Self::ResizeEW),
            "nwse-resize" | "nwse" => Some(Self::ResizeNWSE),
            "nesw-resize" | "nesw" => Some(Self::ResizeNESW),
            "wait" | "busy" => Some(Self::Wait),
            "progress" | "working" => Some(Self::Progress),
            "not-allowed" | "forbidden" => Some(Self::NotAllowed),
            "help" => Some(Self::Help),
            "grab" => Some(Self::Grab),
            "grabbing" => Some(Self::Grabbing),
            "zoom-in" | "zoomin" => Some(Self::ZoomIn),
            "zoom-out" | "zoomout" => Some(Self::ZoomOut),
            "cell" => Some(Self::Cell),
            "context-menu" | "context" => Some(Self::ContextMenu),
            _ => None,
        }
    }

    /// All standard shapes.
    pub const ALL: &'static [CursorShape] = &[
        Self::Default, Self::Pointer, Self::Text, Self::Crosshair,
        Self::Move, Self::ResizeNS, Self::ResizeEW, Self::ResizeNWSE,
        Self::ResizeNESW, Self::Wait, Self::Progress, Self::NotAllowed,
        Self::Help, Self::Grab, Self::Grabbing, Self::ZoomIn,
        Self::ZoomOut, Self::Cell, Self::ContextMenu,
    ];
}

/// A cursor image entry (metadata only — actual bitmap would be loaded from
/// the theme's resource directory at runtime).
#[derive(Debug, Clone)]
pub struct CursorImage {
    pub shape: CursorShape,
    /// Hotspot X (pixels from left).
    pub hotspot_x: u32,
    /// Hotspot Y (pixels from top).
    pub hotspot_y: u32,
    /// Animation frame count (1 = static).
    pub frames: u32,
    /// Animation interval (ms per frame, 0 = static).
    pub frame_interval_ms: u32,
}

/// A cursor theme.
#[derive(Debug, Clone)]
pub struct CursorTheme {
    pub name: String,
    pub description: String,
    /// Base size in pixels (e.g., 24, 32, 48).
    pub base_size: u32,
    /// Whether this theme is built-in (non-removable).
    pub builtin: bool,
    /// Per-shape cursor definitions.
    pub cursors: Vec<CursorImage>,
}

/// Pointer acceleration profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccelProfile {
    /// No acceleration (constant speed).
    Flat,
    /// Adaptive acceleration (faster movement = more distance).
    Adaptive,
    /// Custom acceleration curve.
    Custom,
}

impl AccelProfile {
    pub fn label(self) -> &'static str {
        match self {
            Self::Flat => "flat",
            Self::Adaptive => "adaptive",
            Self::Custom => "custom",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "flat" | "none" => Some(Self::Flat),
            "adaptive" | "accel" => Some(Self::Adaptive),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }
}

/// Mouse button configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonLayout {
    /// Right-handed (default): left=primary, right=secondary.
    RightHanded,
    /// Left-handed: right=primary, left=secondary.
    LeftHanded,
}

impl ButtonLayout {
    pub fn label(self) -> &'static str {
        match self {
            Self::RightHanded => "right-handed",
            Self::LeftHanded => "left-handed",
        }
    }
}

/// Configuration.
#[derive(Debug, Clone)]
pub struct PointerConfig {
    /// Pointer speed (-10 to +10, 0 = system default).
    pub speed: i8,
    /// Acceleration profile.
    pub accel_profile: AccelProfile,
    /// Custom acceleration factor (1-100, used when accel_profile=Custom).
    pub accel_factor: u8,
    /// Button layout (handedness).
    pub button_layout: ButtonLayout,
    /// Double-click interval (ms).
    pub double_click_ms: u32,
    /// Scroll speed multiplier (1-20, 10 = default).
    pub scroll_speed: u8,
    /// Natural scrolling (reverse direction).
    pub natural_scroll: bool,
    /// Cursor size (pixels).
    pub cursor_size: u32,
    /// Active cursor theme name.
    pub active_theme: String,
    /// Whether to show cursor trail.
    pub show_trail: bool,
    /// Trail length (1-10, only when trail enabled).
    pub trail_length: u8,
    /// Whether to highlight cursor on Ctrl press.
    pub locate_on_ctrl: bool,
    /// Whether to hide cursor while typing.
    pub hide_while_typing: bool,
}

impl Default for PointerConfig {
    fn default() -> Self {
        Self {
            speed: 0,
            accel_profile: AccelProfile::Adaptive,
            accel_factor: 50,
            button_layout: ButtonLayout::RightHanded,
            double_click_ms: 400,
            scroll_speed: 10,
            natural_scroll: false,
            cursor_size: 24,
            active_theme: String::from("Default"),
            show_trail: false,
            trail_length: 3,
            locate_on_ctrl: false,
            hide_while_typing: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct State {
    config: PointerConfig,
    themes: Vec<CursorTheme>,
    /// Available cursor sizes.
    available_sizes: Vec<u32>,
}

impl State {
    const fn new() -> Self {
        Self {
            config: PointerConfig {
                speed: 0,
                accel_profile: AccelProfile::Adaptive,
                accel_factor: 50,
                button_layout: ButtonLayout::RightHanded,
                double_click_ms: 400,
                scroll_speed: 10,
                natural_scroll: false,
                cursor_size: 24,
                active_theme: String::new(),
                show_trail: false,
                trail_length: 3,
                locate_on_ctrl: false,
                hide_while_typing: false,
            },
            themes: Vec::new(),
            available_sizes: Vec::new(),
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());
static CHANGE_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Theme management
// ---------------------------------------------------------------------------

/// Register a cursor theme.
pub fn register_theme(name: &str, desc: &str, base_size: u32, builtin: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.themes.len() >= MAX_THEMES {
        return Err(KernelError::ResourceExhausted);
    }
    if state.themes.iter().any(|t| t.name == name) {
        return Err(KernelError::AlreadyExists);
    }
    state.themes.push(CursorTheme {
        name: String::from(name),
        description: String::from(desc),
        base_size,
        builtin,
        cursors: Vec::new(),
    });
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Unregister a cursor theme (built-in themes cannot be removed).
pub fn unregister_theme(name: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    if let Some(t) = state.themes.iter().find(|t| t.name == name) {
        if t.builtin {
            return Err(KernelError::PermissionDenied);
        }
    }
    let len = state.themes.len();
    state.themes.retain(|t| t.name != name);
    if state.themes.len() == len {
        return Err(KernelError::NotFound);
    }
    // If active theme was removed, fall back to first available.
    if state.config.active_theme == name {
        state.config.active_theme = state.themes.first()
            .map(|t| t.name.clone())
            .unwrap_or_default();
    }
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Add a cursor image to a theme.
pub fn add_cursor(theme: &str, shape: CursorShape, hotspot_x: u32, hotspot_y: u32, frames: u32, interval_ms: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let t = state.themes.iter_mut().find(|t| t.name == theme)
        .ok_or(KernelError::NotFound)?;
    if t.cursors.len() >= MAX_CURSORS {
        return Err(KernelError::ResourceExhausted);
    }
    // Replace existing shape if present.
    t.cursors.retain(|c| c.shape != shape);
    t.cursors.push(CursorImage {
        shape,
        hotspot_x,
        hotspot_y,
        frames: frames.max(1),
        frame_interval_ms: interval_ms,
    });
    Ok(())
}

/// List themes.
pub fn list_themes() -> Vec<CursorTheme> {
    STATE.lock().themes.clone()
}

/// Get a theme by name.
pub fn get_theme(name: &str) -> KernelResult<CursorTheme> {
    STATE.lock().themes.iter().find(|t| t.name == name).cloned().ok_or(KernelError::NotFound)
}

/// Set the active cursor theme.
pub fn set_active_theme(name: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    if !state.themes.iter().any(|t| t.name == name) {
        return Err(KernelError::NotFound);
    }
    state.config.active_theme = String::from(name);
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Get the currently active theme.
pub fn active_theme() -> KernelResult<CursorTheme> {
    let state = STATE.lock();
    let name = &state.config.active_theme;
    state.themes.iter().find(|t| t.name == *name).cloned().ok_or(KernelError::NotFound)
}

/// Get cursor image for a specific shape from the active theme.
pub fn cursor_for(shape: CursorShape) -> Option<CursorImage> {
    let state = STATE.lock();
    let name = &state.config.active_theme;
    state.themes.iter()
        .find(|t| t.name == *name)
        .and_then(|t| t.cursors.iter().find(|c| c.shape == shape).cloned())
}

// ---------------------------------------------------------------------------
// Pointer settings
// ---------------------------------------------------------------------------

pub fn config() -> PointerConfig { STATE.lock().config.clone() }

pub fn set_speed(speed: i8) {
    STATE.lock().config.speed = speed.clamp(-10, 10);
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_accel_profile(profile: AccelProfile) {
    STATE.lock().config.accel_profile = profile;
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_accel_factor(factor: u8) {
    STATE.lock().config.accel_factor = factor.clamp(1, 100);
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_button_layout(layout: ButtonLayout) {
    STATE.lock().config.button_layout = layout;
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_double_click_ms(ms: u32) {
    STATE.lock().config.double_click_ms = ms.clamp(100, 2000);
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_scroll_speed(speed: u8) {
    STATE.lock().config.scroll_speed = speed.clamp(1, 20);
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_natural_scroll(v: bool) {
    STATE.lock().config.natural_scroll = v;
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_cursor_size(size: u32) {
    STATE.lock().config.cursor_size = size.clamp(16, 128);
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_show_trail(v: bool) {
    STATE.lock().config.show_trail = v;
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_trail_length(len: u8) {
    STATE.lock().config.trail_length = len.clamp(1, 10);
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_locate_on_ctrl(v: bool) {
    STATE.lock().config.locate_on_ctrl = v;
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_hide_while_typing(v: bool) {
    STATE.lock().config.hide_while_typing = v;
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Available cursor sizes.
pub fn available_sizes() -> Vec<u32> {
    STATE.lock().available_sizes.clone()
}

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

/// Initialize default themes and settings.
pub fn init_defaults() {
    let mut state = STATE.lock();
    if !state.themes.is_empty() { return; }

    // Default theme with standard cursors.
    let mut default_cursors = Vec::new();
    for &shape in CursorShape::ALL {
        default_cursors.push(CursorImage {
            shape,
            hotspot_x: if shape == CursorShape::Default { 1 } else { 12 },
            hotspot_y: if shape == CursorShape::Default { 1 } else { 12 },
            frames: if shape == CursorShape::Wait || shape == CursorShape::Progress { 8 } else { 1 },
            frame_interval_ms: if shape == CursorShape::Wait || shape == CursorShape::Progress { 100 } else { 0 },
        });
    }
    state.themes.push(CursorTheme {
        name: String::from("Default"),
        description: String::from("System default cursor theme"),
        base_size: 24,
        builtin: true,
        cursors: default_cursors,
    });

    // Dark theme variant.
    let mut dark_cursors = Vec::new();
    for &shape in CursorShape::ALL {
        dark_cursors.push(CursorImage {
            shape,
            hotspot_x: if shape == CursorShape::Default { 1 } else { 16 },
            hotspot_y: if shape == CursorShape::Default { 1 } else { 16 },
            frames: if shape == CursorShape::Wait || shape == CursorShape::Progress { 12 } else { 1 },
            frame_interval_ms: if shape == CursorShape::Wait || shape == CursorShape::Progress { 80 } else { 0 },
        });
    }
    state.themes.push(CursorTheme {
        name: String::from("Dark"),
        description: String::from("Dark cursor theme for light backgrounds"),
        base_size: 24,
        builtin: true,
        cursors: dark_cursors,
    });

    state.config.active_theme = String::from("Default");
    state.available_sizes = alloc::vec![16, 24, 32, 48, 64, 96];
}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

/// Returns (theme_count, changes).
pub fn stats() -> (usize, u64) {
    let state = STATE.lock();
    (state.themes.len(), CHANGE_COUNT.load(Ordering::Relaxed))
}

pub fn reset_stats() { CHANGE_COUNT.store(0, Ordering::Relaxed); }

pub fn clear_all() {
    let mut state = STATE.lock();
    state.themes.clear();
    state.config = PointerConfig::default();
    state.available_sizes.clear();
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;
    clear_all();
    reset_stats();

    // Test 1: Init defaults.
    serial_println!("  cursorsettings::self_test 1: init defaults");
    init_defaults();
    let themes = list_themes();
    assert!(themes.len() >= 2);
    assert!(themes.iter().any(|t| t.name == "Default"));
    assert!(themes.iter().any(|t| t.name == "Dark"));

    // Test 2: Active theme and cursor lookup.
    serial_println!("  cursorsettings::self_test 2: cursor lookup");
    let at = active_theme()?;
    assert_eq!(at.name, "Default");
    let def = cursor_for(CursorShape::Default);
    assert!(def.is_some());
    assert_eq!(def.unwrap().hotspot_x, 1);
    let wait = cursor_for(CursorShape::Wait);
    assert!(wait.is_some());
    assert!(wait.unwrap().frames > 1);

    // Test 3: Switch theme.
    serial_println!("  cursorsettings::self_test 3: switch theme");
    set_active_theme("Dark")?;
    let at2 = active_theme()?;
    assert_eq!(at2.name, "Dark");
    set_active_theme("Default")?;

    // Test 4: Pointer speed.
    serial_println!("  cursorsettings::self_test 4: pointer speed");
    set_speed(5);
    let cfg = config();
    assert_eq!(cfg.speed, 5);
    set_speed(-15); // Clamped to -10.
    let cfg2 = config();
    assert_eq!(cfg2.speed, -10);

    // Test 5: Button layout and scroll.
    serial_println!("  cursorsettings::self_test 5: button layout");
    set_button_layout(ButtonLayout::LeftHanded);
    assert_eq!(config().button_layout, ButtonLayout::LeftHanded);
    set_natural_scroll(true);
    assert!(config().natural_scroll);
    set_scroll_speed(15);
    assert_eq!(config().scroll_speed, 15);

    // Test 6: Custom theme.
    serial_println!("  cursorsettings::self_test 6: custom theme");
    register_theme("MyTheme", "Custom cursors", 32, false)?;
    add_cursor("MyTheme", CursorShape::Default, 0, 0, 1, 0)?;
    add_cursor("MyTheme", CursorShape::Pointer, 5, 2, 1, 0)?;
    let mt = get_theme("MyTheme")?;
    assert_eq!(mt.cursors.len(), 2);
    unregister_theme("MyTheme")?;
    // Built-in can't be removed.
    assert!(unregister_theme("Default").is_err());

    // Test 7: Cursor size and trail.
    serial_println!("  cursorsettings::self_test 7: cursor size and trail");
    set_cursor_size(48);
    assert_eq!(config().cursor_size, 48);
    set_show_trail(true);
    set_trail_length(7);
    assert!(config().show_trail);
    assert_eq!(config().trail_length, 7);
    set_locate_on_ctrl(true);
    assert!(config().locate_on_ctrl);
    set_hide_while_typing(true);
    assert!(config().hide_while_typing);

    let (tc, changes) = stats();
    assert!(tc >= 2);
    assert!(changes > 0);

    clear_all();
    reset_stats();
    serial_println!("  cursorsettings: all tests passed");
    Ok(())
}
