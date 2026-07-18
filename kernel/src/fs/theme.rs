//! Desktop theme — color scheme, accent colors, and visual style management.
//!
//! Provides the system-wide theme API that applications query for consistent
//! styling. Supports light/dark mode, custom color schemes, accent colors,
//! and per-user theme customization.
//!
//! ## Design Reference
//!
//! design.txt lines 706-707:
//! "light and dark mode, maybe a few other color schemes available"
//! "gui api functions to retrieve current theme colors"
//!
//! design.txt lines 1249-1250:
//! "desktop theme - light, dark, anything else"
//! "individual desktop colors? (make your own theme)"
//!
//! design.txt line 1358: "theme selection"
//!
//! ## Architecture
//!
//! ```text
//! Application
//!   → theme::current()              → active ThemeColors
//!   → theme::color(ColorRole::*)    → specific color
//!   → theme::on_change(callback)    → notified when theme changes
//!
//! Settings panel
//!   → theme::set_mode(Dark)         → switches all colors
//!   → theme::set_accent(color)      → updates accent-derived colors
//!   → theme::apply_custom(...)      → per-color overrides
//! ```

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum custom themes.
const MAX_CUSTOM_THEMES: usize = 64;

/// Maximum color overrides per custom theme.
const MAX_OVERRIDES: usize = 128;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// RGBA color (0-255 per channel).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// Create an opaque color.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// Create a color with alpha.
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Parse a hex color string (e.g., "#FF5500" or "FF5500").
    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.strip_prefix('#').unwrap_or(s);
        if s.len() != 6 && s.len() != 8 {
            return None;
        }
        let r = u8::from_str_radix(s.get(0..2)?, 16).ok()?;
        let g = u8::from_str_radix(s.get(2..4)?, 16).ok()?;
        let b = u8::from_str_radix(s.get(4..6)?, 16).ok()?;
        let a = if s.len() == 8 {
            u8::from_str_radix(s.get(6..8)?, 16).ok()?
        } else {
            255
        };
        Some(Self { r, g, b, a })
    }

    /// Convert to hex string.
    pub fn to_hex(self) -> String {
        if self.a == 255 {
            alloc::format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
        } else {
            alloc::format!("#{:02X}{:02X}{:02X}{:02X}", self.r, self.g, self.b, self.a)
        }
    }
}

/// Theme mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    /// Light color scheme.
    Light,
    /// Dark color scheme.
    Dark,
    /// High contrast (accessibility).
    HighContrast,
}

impl ThemeMode {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
            Self::HighContrast => "High Contrast",
        }
    }

    /// Parse from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            "highcontrast" | "high-contrast" | "hc" => Some(Self::HighContrast),
            _ => None,
        }
    }
}

/// Semantic color roles that applications query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ColorRole {
    // --- Background ---
    /// Window/surface background.
    Background,
    /// Slightly raised surface (cards, panels).
    Surface,
    /// Strong surface (selected list item background).
    SurfaceHighlight,
    /// Sidebar background.
    SidebarBackground,
    /// Titlebar background.
    TitlebarBackground,
    /// Taskbar background.
    TaskbarBackground,
    /// Tooltip background.
    TooltipBackground,

    // --- Text ---
    /// Primary text on background.
    TextPrimary,
    /// Secondary/dimmed text.
    TextSecondary,
    /// Disabled text.
    TextDisabled,
    /// Text on accent-colored surface.
    TextOnAccent,
    /// Titlebar text.
    TitlebarText,
    /// Tooltip text.
    TooltipText,

    // --- Accent ---
    /// Primary accent color (buttons, links, focus rings).
    Accent,
    /// Accent hover state.
    AccentHover,
    /// Accent pressed state.
    AccentPressed,

    // --- Border / Divider ---
    /// Default border color.
    Border,
    /// Strong border (focused inputs).
    BorderStrong,
    /// Divider line (between sections).
    Divider,

    // --- Semantic ---
    /// Error/danger.
    Error,
    /// Warning/caution.
    Warning,
    /// Success/positive.
    Success,
    /// Information.
    Info,

    // --- Interactive ---
    /// Button default background.
    ButtonBackground,
    /// Button hover background.
    ButtonHover,
    /// Button text.
    ButtonText,
    /// Input field background.
    InputBackground,
    /// Input field border.
    InputBorder,
    /// Scrollbar thumb.
    ScrollbarThumb,
    /// Scrollbar track.
    ScrollbarTrack,
    /// Selection highlight (text selection, file selection).
    Selection,
}

impl ColorRole {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Background => "background",
            Self::Surface => "surface",
            Self::SurfaceHighlight => "surface-highlight",
            Self::SidebarBackground => "sidebar-bg",
            Self::TitlebarBackground => "titlebar-bg",
            Self::TaskbarBackground => "taskbar-bg",
            Self::TooltipBackground => "tooltip-bg",
            Self::TextPrimary => "text-primary",
            Self::TextSecondary => "text-secondary",
            Self::TextDisabled => "text-disabled",
            Self::TextOnAccent => "text-on-accent",
            Self::TitlebarText => "titlebar-text",
            Self::TooltipText => "tooltip-text",
            Self::Accent => "accent",
            Self::AccentHover => "accent-hover",
            Self::AccentPressed => "accent-pressed",
            Self::Border => "border",
            Self::BorderStrong => "border-strong",
            Self::Divider => "divider",
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Success => "success",
            Self::Info => "info",
            Self::ButtonBackground => "button-bg",
            Self::ButtonHover => "button-hover",
            Self::ButtonText => "button-text",
            Self::InputBackground => "input-bg",
            Self::InputBorder => "input-border",
            Self::ScrollbarThumb => "scrollbar-thumb",
            Self::ScrollbarTrack => "scrollbar-track",
            Self::Selection => "selection",
        }
    }

    /// Parse from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "background" | "bg" => Some(Self::Background),
            "surface" => Some(Self::Surface),
            "surface-highlight" | "surface-hl" => Some(Self::SurfaceHighlight),
            "sidebar-bg" | "sidebar" => Some(Self::SidebarBackground),
            "titlebar-bg" | "titlebar" => Some(Self::TitlebarBackground),
            "taskbar-bg" | "taskbar" => Some(Self::TaskbarBackground),
            "tooltip-bg" | "tooltip" => Some(Self::TooltipBackground),
            "text-primary" | "text" => Some(Self::TextPrimary),
            "text-secondary" => Some(Self::TextSecondary),
            "text-disabled" => Some(Self::TextDisabled),
            "text-on-accent" => Some(Self::TextOnAccent),
            "titlebar-text" => Some(Self::TitlebarText),
            "tooltip-text" => Some(Self::TooltipText),
            "accent" => Some(Self::Accent),
            "accent-hover" => Some(Self::AccentHover),
            "accent-pressed" => Some(Self::AccentPressed),
            "border" => Some(Self::Border),
            "border-strong" => Some(Self::BorderStrong),
            "divider" => Some(Self::Divider),
            "error" => Some(Self::Error),
            "warning" => Some(Self::Warning),
            "success" => Some(Self::Success),
            "info" => Some(Self::Info),
            "button-bg" | "button" => Some(Self::ButtonBackground),
            "button-hover" => Some(Self::ButtonHover),
            "button-text" => Some(Self::ButtonText),
            "input-bg" | "input" => Some(Self::InputBackground),
            "input-border" => Some(Self::InputBorder),
            "scrollbar-thumb" | "scrollbar" => Some(Self::ScrollbarThumb),
            "scrollbar-track" => Some(Self::ScrollbarTrack),
            "selection" => Some(Self::Selection),
            _ => None,
        }
    }

    /// All color roles.
    pub fn all() -> &'static [ColorRole] {
        &[
            Self::Background, Self::Surface, Self::SurfaceHighlight,
            Self::SidebarBackground, Self::TitlebarBackground,
            Self::TaskbarBackground, Self::TooltipBackground,
            Self::TextPrimary, Self::TextSecondary, Self::TextDisabled,
            Self::TextOnAccent, Self::TitlebarText, Self::TooltipText,
            Self::Accent, Self::AccentHover, Self::AccentPressed,
            Self::Border, Self::BorderStrong, Self::Divider,
            Self::Error, Self::Warning, Self::Success, Self::Info,
            Self::ButtonBackground, Self::ButtonHover, Self::ButtonText,
            Self::InputBackground, Self::InputBorder,
            Self::ScrollbarThumb, Self::ScrollbarTrack, Self::Selection,
        ]
    }
}

/// A named custom theme.
#[derive(Debug, Clone)]
pub struct CustomTheme {
    /// Theme name.
    pub name: String,
    /// Base mode (overrides build on top of this).
    pub base_mode: ThemeMode,
    /// Color overrides.
    pub overrides: BTreeMap<ColorRole, Color>,
}

/// Full resolved color set for the active theme.
#[derive(Debug, Clone)]
pub struct ThemeColors {
    /// Active mode.
    pub mode: ThemeMode,
    /// All resolved colors.
    pub colors: BTreeMap<ColorRole, Color>,
}

// ---------------------------------------------------------------------------
// Built-in palettes
// ---------------------------------------------------------------------------

/// Get the default color for a role in a given mode.
fn default_color(role: ColorRole, mode: ThemeMode) -> Color {
    match mode {
        ThemeMode::Light => light_palette(role),
        ThemeMode::Dark => dark_palette(role),
        ThemeMode::HighContrast => high_contrast_palette(role),
    }
}

fn light_palette(role: ColorRole) -> Color {
    match role {
        ColorRole::Background => Color::rgb(255, 255, 255),
        ColorRole::Surface => Color::rgb(245, 245, 245),
        ColorRole::SurfaceHighlight => Color::rgb(229, 236, 246),
        ColorRole::SidebarBackground => Color::rgb(240, 240, 240),
        ColorRole::TitlebarBackground => Color::rgb(235, 235, 235),
        ColorRole::TaskbarBackground => Color::rgba(240, 240, 240, 230),
        ColorRole::TooltipBackground => Color::rgb(50, 50, 50),
        ColorRole::TextPrimary => Color::rgb(30, 30, 30),
        ColorRole::TextSecondary => Color::rgb(100, 100, 100),
        ColorRole::TextDisabled => Color::rgb(170, 170, 170),
        ColorRole::TextOnAccent => Color::rgb(255, 255, 255),
        ColorRole::TitlebarText => Color::rgb(30, 30, 30),
        ColorRole::TooltipText => Color::rgb(240, 240, 240),
        ColorRole::Accent => Color::rgb(0, 120, 212),
        ColorRole::AccentHover => Color::rgb(0, 100, 190),
        ColorRole::AccentPressed => Color::rgb(0, 84, 163),
        ColorRole::Border => Color::rgb(200, 200, 200),
        ColorRole::BorderStrong => Color::rgb(0, 120, 212),
        ColorRole::Divider => Color::rgb(220, 220, 220),
        ColorRole::Error => Color::rgb(196, 43, 28),
        ColorRole::Warning => Color::rgb(255, 185, 0),
        ColorRole::Success => Color::rgb(16, 124, 16),
        ColorRole::Info => Color::rgb(0, 120, 212),
        ColorRole::ButtonBackground => Color::rgb(243, 243, 243),
        ColorRole::ButtonHover => Color::rgb(230, 230, 230),
        ColorRole::ButtonText => Color::rgb(30, 30, 30),
        ColorRole::InputBackground => Color::rgb(255, 255, 255),
        ColorRole::InputBorder => Color::rgb(180, 180, 180),
        ColorRole::ScrollbarThumb => Color::rgb(180, 180, 180),
        ColorRole::ScrollbarTrack => Color::rgb(240, 240, 240),
        ColorRole::Selection => Color::rgba(0, 120, 212, 80),
    }
}

fn dark_palette(role: ColorRole) -> Color {
    match role {
        ColorRole::Background => Color::rgb(32, 32, 32),
        ColorRole::Surface => Color::rgb(44, 44, 44),
        ColorRole::SurfaceHighlight => Color::rgb(55, 65, 80),
        ColorRole::SidebarBackground => Color::rgb(38, 38, 38),
        ColorRole::TitlebarBackground => Color::rgb(32, 32, 32),
        ColorRole::TaskbarBackground => Color::rgba(38, 38, 38, 230),
        ColorRole::TooltipBackground => Color::rgb(60, 60, 60),
        ColorRole::TextPrimary => Color::rgb(240, 240, 240),
        ColorRole::TextSecondary => Color::rgb(160, 160, 160),
        ColorRole::TextDisabled => Color::rgb(90, 90, 90),
        ColorRole::TextOnAccent => Color::rgb(255, 255, 255),
        ColorRole::TitlebarText => Color::rgb(240, 240, 240),
        ColorRole::TooltipText => Color::rgb(230, 230, 230),
        ColorRole::Accent => Color::rgb(96, 205, 255),
        ColorRole::AccentHover => Color::rgb(120, 215, 255),
        ColorRole::AccentPressed => Color::rgb(76, 185, 235),
        ColorRole::Border => Color::rgb(60, 60, 60),
        ColorRole::BorderStrong => Color::rgb(96, 205, 255),
        ColorRole::Divider => Color::rgb(50, 50, 50),
        ColorRole::Error => Color::rgb(255, 99, 71),
        ColorRole::Warning => Color::rgb(255, 200, 60),
        ColorRole::Success => Color::rgb(80, 200, 80),
        ColorRole::Info => Color::rgb(96, 205, 255),
        ColorRole::ButtonBackground => Color::rgb(55, 55, 55),
        ColorRole::ButtonHover => Color::rgb(70, 70, 70),
        ColorRole::ButtonText => Color::rgb(240, 240, 240),
        ColorRole::InputBackground => Color::rgb(44, 44, 44),
        ColorRole::InputBorder => Color::rgb(80, 80, 80),
        ColorRole::ScrollbarThumb => Color::rgb(80, 80, 80),
        ColorRole::ScrollbarTrack => Color::rgb(38, 38, 38),
        ColorRole::Selection => Color::rgba(96, 205, 255, 80),
    }
}

fn high_contrast_palette(role: ColorRole) -> Color {
    match role {
        ColorRole::Background => Color::rgb(0, 0, 0),
        ColorRole::Surface => Color::rgb(20, 20, 20),
        ColorRole::SurfaceHighlight => Color::rgb(0, 80, 160),
        ColorRole::SidebarBackground => Color::rgb(0, 0, 0),
        ColorRole::TitlebarBackground => Color::rgb(0, 0, 128),
        ColorRole::TaskbarBackground => Color::rgb(0, 0, 0),
        ColorRole::TooltipBackground => Color::rgb(255, 255, 225),
        ColorRole::TextPrimary => Color::rgb(255, 255, 255),
        ColorRole::TextSecondary => Color::rgb(200, 200, 200),
        ColorRole::TextDisabled => Color::rgb(128, 128, 128),
        ColorRole::TextOnAccent => Color::rgb(0, 0, 0),
        ColorRole::TitlebarText => Color::rgb(255, 255, 255),
        ColorRole::TooltipText => Color::rgb(0, 0, 0),
        ColorRole::Accent => Color::rgb(0, 255, 255),
        ColorRole::AccentHover => Color::rgb(100, 255, 255),
        ColorRole::AccentPressed => Color::rgb(0, 200, 200),
        ColorRole::Border => Color::rgb(255, 255, 255),
        ColorRole::BorderStrong => Color::rgb(0, 255, 255),
        ColorRole::Divider => Color::rgb(128, 128, 128),
        ColorRole::Error => Color::rgb(255, 0, 0),
        ColorRole::Warning => Color::rgb(255, 255, 0),
        ColorRole::Success => Color::rgb(0, 255, 0),
        ColorRole::Info => Color::rgb(0, 255, 255),
        ColorRole::ButtonBackground => Color::rgb(0, 0, 0),
        ColorRole::ButtonHover => Color::rgb(0, 80, 160),
        ColorRole::ButtonText => Color::rgb(255, 255, 255),
        ColorRole::InputBackground => Color::rgb(0, 0, 0),
        ColorRole::InputBorder => Color::rgb(255, 255, 255),
        ColorRole::ScrollbarThumb => Color::rgb(128, 128, 128),
        ColorRole::ScrollbarTrack => Color::rgb(40, 40, 40),
        ColorRole::Selection => Color::rgba(0, 80, 160, 200),
    }
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

struct ThemeState {
    /// Active mode.
    mode: ThemeMode,
    /// Custom accent color (overrides default accent).
    accent_override: Option<Color>,
    /// Per-role color overrides.
    role_overrides: BTreeMap<ColorRole, Color>,
    /// Saved custom themes.
    custom_themes: BTreeMap<String, CustomTheme>,
    /// Name of active custom theme (None = built-in mode).
    active_custom: Option<String>,
}

impl ThemeState {
    const fn new() -> Self {
        Self {
            mode: ThemeMode::Light,
            accent_override: None,
            role_overrides: BTreeMap::new(),
            custom_themes: BTreeMap::new(),
            active_custom: None,
        }
    }
}

static THEME: Mutex<ThemeState> = Mutex::new(ThemeState::new());
static QUERY_COUNT: AtomicU64 = AtomicU64::new(0);
static CHANGE_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Core API
// ---------------------------------------------------------------------------

/// Get the current theme mode.
pub fn mode() -> ThemeMode {
    let t = THEME.lock();
    t.mode
}

/// Set the theme mode (light/dark/high-contrast).
pub fn set_mode(mode: ThemeMode) {
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut t = THEME.lock();
    t.mode = mode;
    // Clear custom theme when switching modes.
    t.active_custom = None;
}

/// Get a color for a specific role.
pub fn color(role: ColorRole) -> Color {
    QUERY_COUNT.fetch_add(1, Ordering::Relaxed);
    let t = THEME.lock();
    resolve_color(role, &t)
}

/// Get all resolved theme colors.
pub fn current() -> ThemeColors {
    QUERY_COUNT.fetch_add(1, Ordering::Relaxed);
    let t = THEME.lock();
    let mut colors = BTreeMap::new();
    for &role in ColorRole::all() {
        colors.insert(role, resolve_color(role, &t));
    }
    ThemeColors {
        mode: t.mode,
        colors,
    }
}

/// Resolve a color considering overrides.
fn resolve_color(role: ColorRole, state: &ThemeState) -> Color {
    // 1. Check custom theme overrides.
    if let Some(ref name) = state.active_custom {
        if let Some(theme) = state.custom_themes.get(name) {
            if let Some(&c) = theme.overrides.get(&role) {
                return c;
            }
        }
    }

    // 2. Check per-role overrides.
    if let Some(&c) = state.role_overrides.get(&role) {
        return c;
    }

    // 3. Check accent override for accent-derived roles.
    if let Some(accent) = state.accent_override {
        match role {
            ColorRole::Accent => return accent,
            ColorRole::AccentHover => {
                return Color::rgb(
                    accent.r.saturating_add(20),
                    accent.g.saturating_add(20),
                    accent.b.saturating_add(20),
                );
            }
            ColorRole::AccentPressed => {
                return Color::rgb(
                    accent.r.saturating_sub(20),
                    accent.g.saturating_sub(20),
                    accent.b.saturating_sub(20),
                );
            }
            ColorRole::BorderStrong | ColorRole::Info => return accent,
            ColorRole::Selection => return Color::rgba(accent.r, accent.g, accent.b, 80),
            _ => {}
        }
    }

    // 4. Fall back to built-in palette.
    default_color(role, state.mode)
}

// ---------------------------------------------------------------------------
// Accent color
// ---------------------------------------------------------------------------

/// Set a custom accent color.
pub fn set_accent(color: Color) {
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut t = THEME.lock();
    t.accent_override = Some(color);
}

/// Clear the accent override (use mode default).
pub fn clear_accent() {
    let mut t = THEME.lock();
    t.accent_override = None;
}

/// Get the current accent color.
pub fn accent() -> Color {
    let t = THEME.lock();
    t.accent_override.unwrap_or_else(|| default_color(ColorRole::Accent, t.mode))
}

// ---------------------------------------------------------------------------
// Per-role overrides
// ---------------------------------------------------------------------------

/// Override a specific color role.
pub fn set_override(role: ColorRole, c: Color) {
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut t = THEME.lock();
    if t.role_overrides.len() < MAX_OVERRIDES || t.role_overrides.contains_key(&role) {
        t.role_overrides.insert(role, c);
    }
}

/// Clear an override for a role.
pub fn clear_override(role: ColorRole) {
    let mut t = THEME.lock();
    t.role_overrides.remove(&role);
}

/// Clear all overrides.
pub fn clear_all_overrides() {
    let mut t = THEME.lock();
    t.role_overrides.clear();
    t.accent_override = None;
}

/// List all active overrides.
pub fn list_overrides() -> Vec<(ColorRole, Color)> {
    let t = THEME.lock();
    t.role_overrides.iter().map(|(&r, &c)| (r, c)).collect()
}

// ---------------------------------------------------------------------------
// Custom themes
// ---------------------------------------------------------------------------

/// Save a custom theme.
pub fn save_custom(name: &str, base: ThemeMode, overrides: BTreeMap<ColorRole, Color>) -> KernelResult<()> {
    if name.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let mut t = THEME.lock();
    if !t.custom_themes.contains_key(name) && t.custom_themes.len() >= MAX_CUSTOM_THEMES {
        return Err(KernelError::ResourceExhausted);
    }
    t.custom_themes.insert(String::from(name), CustomTheme {
        name: String::from(name),
        base_mode: base,
        overrides,
    });
    Ok(())
}

/// Delete a custom theme.
pub fn delete_custom(name: &str) -> KernelResult<()> {
    let mut t = THEME.lock();
    t.custom_themes.remove(name).ok_or(KernelError::NotFound)?;
    if t.active_custom.as_deref() == Some(name) {
        t.active_custom = None;
    }
    Ok(())
}

/// Apply a saved custom theme.
pub fn apply_custom(name: &str) -> KernelResult<()> {
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut t = THEME.lock();
    if !t.custom_themes.contains_key(name) {
        return Err(KernelError::NotFound);
    }
    let base = t.custom_themes.get(name).map(|ct| ct.base_mode)
        .unwrap_or(ThemeMode::Light);
    t.mode = base;
    t.active_custom = Some(String::from(name));
    Ok(())
}

/// List custom theme names.
pub fn list_custom() -> Vec<String> {
    let t = THEME.lock();
    t.custom_themes.keys().cloned().collect()
}

/// Get a custom theme by name.
pub fn get_custom(name: &str) -> Option<CustomTheme> {
    let t = THEME.lock();
    t.custom_themes.get(name).cloned()
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (mode, custom_themes, overrides, query_ops, change_ops).
pub fn stats() -> (ThemeMode, usize, usize, u64, u64) {
    let t = THEME.lock();
    (
        t.mode,
        t.custom_themes.len(),
        t.role_overrides.len(),
        QUERY_COUNT.load(Ordering::Relaxed),
        CHANGE_COUNT.load(Ordering::Relaxed),
    )
}

/// Reset statistics.
pub fn reset_stats() {
    QUERY_COUNT.store(0, Ordering::Relaxed);
    CHANGE_COUNT.store(0, Ordering::Relaxed);
}

/// Clear all data (reset to defaults).
pub fn clear_all() {
    let mut t = THEME.lock();
    t.mode = ThemeMode::Light;
    t.accent_override = None;
    t.role_overrides.clear();
    t.custom_themes.clear();
    t.active_custom = None;
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the theme system.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();
    reset_stats();

    // Test 1: default mode and colors.
    {
        assert_eq!(mode(), ThemeMode::Light);
        let bg = color(ColorRole::Background);
        assert_eq!(bg, Color::rgb(255, 255, 255));
        let text = color(ColorRole::TextPrimary);
        assert_eq!(text, Color::rgb(30, 30, 30));
        serial_println!("[theme] test 1 passed: light mode defaults");
    }

    // Test 2: switch to dark mode.
    {
        set_mode(ThemeMode::Dark);
        assert_eq!(mode(), ThemeMode::Dark);
        let bg = color(ColorRole::Background);
        assert_eq!(bg, Color::rgb(32, 32, 32));
        let text = color(ColorRole::TextPrimary);
        assert_eq!(text, Color::rgb(240, 240, 240));
        serial_println!("[theme] test 2 passed: dark mode");
    }

    // Test 3: accent color override.
    {
        set_accent(Color::rgb(255, 100, 0));
        let a = accent();
        assert_eq!(a, Color::rgb(255, 100, 0));
        let c = color(ColorRole::Accent);
        assert_eq!(c, Color::rgb(255, 100, 0));
        clear_accent();
        serial_println!("[theme] test 3 passed: accent override");
    }

    // Test 4: per-role override.
    {
        set_override(ColorRole::Background, Color::rgb(10, 20, 30));
        let bg = color(ColorRole::Background);
        assert_eq!(bg, Color::rgb(10, 20, 30));
        clear_override(ColorRole::Background);
        let bg = color(ColorRole::Background);
        // Should be dark mode default now.
        assert_eq!(bg, Color::rgb(32, 32, 32));
        serial_println!("[theme] test 4 passed: role override");
    }

    // Test 5: Color::from_hex.
    {
        let c = Color::from_hex("#FF5500").unwrap();
        assert_eq!(c, Color::rgb(255, 85, 0));
        let c = Color::from_hex("AABBCC").unwrap();
        assert_eq!(c, Color::rgb(170, 187, 204));
        assert!(Color::from_hex("xyz").is_none());
        serial_println!("[theme] test 5 passed: hex parsing");
    }

    // Test 6: custom theme.
    {
        let mut overrides = BTreeMap::new();
        overrides.insert(ColorRole::Background, Color::rgb(40, 0, 40));
        overrides.insert(ColorRole::Accent, Color::rgb(200, 100, 200));
        save_custom("purple", ThemeMode::Dark, overrides)?;
        apply_custom("purple")?;

        let bg = color(ColorRole::Background);
        assert_eq!(bg, Color::rgb(40, 0, 40));
        let acc = color(ColorRole::Accent);
        assert_eq!(acc, Color::rgb(200, 100, 200));

        // Unoverridden roles fall through to dark palette.
        let text = color(ColorRole::TextPrimary);
        assert_eq!(text, Color::rgb(240, 240, 240));
        serial_println!("[theme] test 6 passed: custom theme");
    }

    // Test 7: full theme snapshot.
    {
        let theme = current();
        assert_eq!(theme.mode, ThemeMode::Dark);
        assert_eq!(theme.colors.len(), ColorRole::all().len());
        assert!(theme.colors.contains_key(&ColorRole::Accent));
        serial_println!("[theme] test 7 passed: full snapshot");
    }

    clear_all();
    reset_stats();

    serial_println!("[theme] all 7 self-tests passed");
    Ok(())
}
