//! Hotkey Manager — global keyboard shortcut management for the desktop shell.
//!
//! Provides a registry-based system for mapping key combinations (key + modifiers)
//! to desktop actions. The registry supports conflict detection, configuration
//! persistence (key=value text format), and a default binding set that mirrors
//! common desktop OS conventions.
//!
//! # Usage from the desktop shell
//!
//! ```ignore
//! let mut registry = HotkeyRegistry::defaults();
//!
//! // Register a custom binding:
//! registry.register(
//!     Hotkey::new(Key::T, Modifiers::ctrl()),
//!     HotkeyAction::LaunchApp("terminal".into()),
//! )?;
//!
//! // Look up a key event:
//! if let Some(action) = registry.lookup(Key::F4, &mods) {
//!     match action {
//!         HotkeyAction::CloseWindow => { /* close focused window */ }
//!         _ => {}
//!     }
//! }
//!
//! // Persist to/from text:
//! let config = HotkeyConfig::from_registry(&registry);
//! let text = config.save();
//! let loaded = HotkeyConfig::load(&text)?;
//! let restored = loaded.into_registry();
//! ```

use guitk::event::{Key, Modifiers};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

use std::collections::BTreeMap;
use std::fmt;

// ============================================================================
// Theme — Catppuccin Mocha palette (consistent with other desktop modules)
// ============================================================================

mod theme {
    use guitk::color::Color;

    pub const BASE: Color = Color::rgba(30, 30, 46, 240);
    pub const MANTLE: Color = Color::from_hex(0x181825);
    pub const SURFACE0: Color = Color::from_hex(0x313244);
    pub const SURFACE1: Color = Color::from_hex(0x45475A);
    pub const SURFACE2: Color = Color::from_hex(0x585B70);
    pub const TEXT: Color = Color::from_hex(0xCDD6F4);
    pub const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
    pub const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
    pub const OVERLAY0: Color = Color::from_hex(0x6C7086);
    pub const BLUE: Color = Color::from_hex(0x89B4FA);
    pub const RED: Color = Color::from_hex(0xF38BA8);
    pub const GREEN: Color = Color::from_hex(0xA6E3A1);
    pub const PEACH: Color = Color::from_hex(0xFAB387);
    pub const SHADOW: Color = Color::rgba(0, 0, 0, 100);
}

// ============================================================================
// Rendering constants
// ============================================================================

const PANEL_WIDTH: f32 = 560.0;
const PANEL_RADIUS: f32 = 10.0;
const PADDING: f32 = 16.0;
const HEADER_HEIGHT: f32 = 44.0;
const ROW_HEIGHT: f32 = 38.0;
const KEY_BADGE_HEIGHT: f32 = 24.0;
const KEY_BADGE_RADIUS: f32 = 4.0;
const HEADER_FONT_SIZE: f32 = 16.0;
const LABEL_FONT_SIZE: f32 = 13.0;
const KEY_FONT_SIZE: f32 = 12.0;

// ============================================================================
// Error type
// ============================================================================

/// Errors that can occur during hotkey registration or configuration parsing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HotkeyError {
    /// The hotkey is already bound to another action.
    Conflict {
        hotkey: Hotkey,
        existing: HotkeyAction,
    },
    /// The configuration text contains an invalid line.
    ParseError {
        line_number: usize,
        message: String,
    },
    /// An unrecognized key name was encountered.
    UnknownKey(String),
    /// An unrecognized action name was encountered.
    UnknownAction(String),
}

impl fmt::Display for HotkeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict { hotkey, existing } => {
                write!(
                    f,
                    "hotkey {} is already bound to {:?}",
                    hotkey.display_name(),
                    existing
                )
            }
            Self::ParseError {
                line_number,
                message,
            } => {
                write!(f, "parse error on line {}: {}", line_number, message)
            }
            Self::UnknownKey(name) => write!(f, "unknown key: {}", name),
            Self::UnknownAction(name) => write!(f, "unknown action: {}", name),
        }
    }
}

// ============================================================================
// Hotkey — a key + modifier combination
// ============================================================================

/// A keyboard shortcut: one principal key combined with zero or more modifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Hotkey {
    pub key: Key,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
}

impl Hotkey {
    /// Create a hotkey with the given key and modifier set.
    pub fn new(key: Key, modifiers: Modifiers) -> Self {
        Self {
            key,
            ctrl: modifiers.ctrl,
            alt: modifiers.alt,
            shift: modifiers.shift,
            super_key: modifiers.super_key,
        }
    }

    /// Create a hotkey with no modifiers.
    pub fn bare(key: Key) -> Self {
        Self {
            key,
            ctrl: false,
            alt: false,
            shift: false,
            super_key: false,
        }
    }

    /// Return the modifier state as a `Modifiers` value.
    pub fn modifiers(&self) -> Modifiers {
        Modifiers {
            ctrl: self.ctrl,
            alt: self.alt,
            shift: self.shift,
            super_key: self.super_key,
        }
    }

    /// Test whether a key event matches this hotkey.
    pub fn matches(&self, key: Key, modifiers: &Modifiers) -> bool {
        self.key == key
            && self.ctrl == modifiers.ctrl
            && self.alt == modifiers.alt
            && self.shift == modifiers.shift
            && self.super_key == modifiers.super_key
    }

    /// Human-readable name for display (e.g., "Ctrl+Alt+Delete").
    pub fn display_name(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.super_key {
            parts.push("Super");
        }
        parts.push(key_display_name(self.key));
        parts.join("+")
    }
}

impl PartialOrd for Hotkey {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Hotkey {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        // Sort by modifiers first (ctrl, alt, shift, super), then by key name.
        let self_mods = (self.ctrl, self.alt, self.shift, self.super_key);
        let other_mods = (other.ctrl, other.alt, other.shift, other.super_key);
        self_mods
            .cmp(&other_mods)
            .then_with(|| key_sort_name(self.key).cmp(key_sort_name(other.key)))
    }
}

// ============================================================================
// HotkeyAction — what a hotkey triggers
// ============================================================================

/// Action performed when a hotkey is triggered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HotkeyAction {
    /// Launch an application by name or path.
    LaunchApp(String),
    /// Close the focused window.
    CloseWindow,
    /// Minimize the focused window.
    MinimizeWindow,
    /// Maximize (or restore) the focused window.
    MaximizeWindow,
    /// Switch to a specific virtual desktop (0-indexed).
    SwitchDesktop(u8),
    /// Show the task/process manager.
    ShowTaskManager,
    /// Lock the screen.
    ScreenLock,
    /// Take a full-screen screenshot.
    Screenshot,
    /// Take a region screenshot (interactive selection).
    ScreenshotRegion,
    /// Increase system volume.
    VolumeUp,
    /// Decrease system volume.
    VolumeDown,
    /// Toggle volume mute.
    VolumeMute,
    /// Increase display brightness.
    BrightnessUp,
    /// Decrease display brightness.
    BrightnessDown,
    /// Show the search/launcher overlay.
    ShowSearch,
    /// Show the Run dialog.
    ShowRun,
    /// Show/toggle the desktop (minimize all windows).
    ShowDesktop,
    /// Cycle through windows (Alt+Tab style).
    CycleWindows,
    /// Snap the focused window to the left half.
    MoveWindowLeft,
    /// Snap the focused window to the right half.
    MoveWindowRight,
    /// Open the system settings application.
    SystemSettings,
    /// A user-defined action identified by a string.
    Custom(String),
}

impl HotkeyAction {
    /// Serialize the action to a string for configuration persistence.
    fn to_config_value(&self) -> String {
        match self {
            Self::LaunchApp(app) => format!("launch:{}", app),
            Self::CloseWindow => "close_window".to_string(),
            Self::MinimizeWindow => "minimize_window".to_string(),
            Self::MaximizeWindow => "maximize_window".to_string(),
            Self::SwitchDesktop(n) => format!("switch_desktop:{}", n),
            Self::ShowTaskManager => "show_task_manager".to_string(),
            Self::ScreenLock => "screen_lock".to_string(),
            Self::Screenshot => "screenshot".to_string(),
            Self::ScreenshotRegion => "screenshot_region".to_string(),
            Self::VolumeUp => "volume_up".to_string(),
            Self::VolumeDown => "volume_down".to_string(),
            Self::VolumeMute => "volume_mute".to_string(),
            Self::BrightnessUp => "brightness_up".to_string(),
            Self::BrightnessDown => "brightness_down".to_string(),
            Self::ShowSearch => "show_search".to_string(),
            Self::ShowRun => "show_run".to_string(),
            Self::ShowDesktop => "show_desktop".to_string(),
            Self::CycleWindows => "cycle_windows".to_string(),
            Self::MoveWindowLeft => "move_window_left".to_string(),
            Self::MoveWindowRight => "move_window_right".to_string(),
            Self::SystemSettings => "system_settings".to_string(),
            Self::Custom(name) => format!("custom:{}", name),
        }
    }

    /// Parse an action from a configuration value string.
    fn from_config_value(value: &str) -> Result<Self, HotkeyError> {
        if let Some(app) = value.strip_prefix("launch:") {
            return Ok(Self::LaunchApp(app.to_string()));
        }
        if let Some(n_str) = value.strip_prefix("switch_desktop:") {
            let n = n_str
                .parse::<u8>()
                .map_err(|_| HotkeyError::UnknownAction(value.to_string()))?;
            return Ok(Self::SwitchDesktop(n));
        }
        if let Some(name) = value.strip_prefix("custom:") {
            return Ok(Self::Custom(name.to_string()));
        }
        match value {
            "close_window" => Ok(Self::CloseWindow),
            "minimize_window" => Ok(Self::MinimizeWindow),
            "maximize_window" => Ok(Self::MaximizeWindow),
            "show_task_manager" => Ok(Self::ShowTaskManager),
            "screen_lock" => Ok(Self::ScreenLock),
            "screenshot" => Ok(Self::Screenshot),
            "screenshot_region" => Ok(Self::ScreenshotRegion),
            "volume_up" => Ok(Self::VolumeUp),
            "volume_down" => Ok(Self::VolumeDown),
            "volume_mute" => Ok(Self::VolumeMute),
            "brightness_up" => Ok(Self::BrightnessUp),
            "brightness_down" => Ok(Self::BrightnessDown),
            "show_search" => Ok(Self::ShowSearch),
            "show_run" => Ok(Self::ShowRun),
            "show_desktop" => Ok(Self::ShowDesktop),
            "cycle_windows" => Ok(Self::CycleWindows),
            "move_window_left" => Ok(Self::MoveWindowLeft),
            "move_window_right" => Ok(Self::MoveWindowRight),
            "system_settings" => Ok(Self::SystemSettings),
            _ => Err(HotkeyError::UnknownAction(value.to_string())),
        }
    }

    /// Short human-readable label for display in a settings panel.
    pub fn display_label(&self) -> &str {
        match self {
            Self::LaunchApp(_) => "Launch App",
            Self::CloseWindow => "Close Window",
            Self::MinimizeWindow => "Minimize Window",
            Self::MaximizeWindow => "Maximize Window",
            Self::SwitchDesktop(_) => "Switch Desktop",
            Self::ShowTaskManager => "Task Manager",
            Self::ScreenLock => "Lock Screen",
            Self::Screenshot => "Screenshot",
            Self::ScreenshotRegion => "Screenshot Region",
            Self::VolumeUp => "Volume Up",
            Self::VolumeDown => "Volume Down",
            Self::VolumeMute => "Volume Mute",
            Self::BrightnessUp => "Brightness Up",
            Self::BrightnessDown => "Brightness Down",
            Self::ShowSearch => "Search",
            Self::ShowRun => "Run Dialog",
            Self::ShowDesktop => "Show Desktop",
            Self::CycleWindows => "Cycle Windows",
            Self::MoveWindowLeft => "Snap Left",
            Self::MoveWindowRight => "Snap Right",
            Self::SystemSettings => "Settings",
            Self::Custom(_) => "Custom",
        }
    }
}

// ============================================================================
// HotkeyRegistry — the core binding store
// ============================================================================

/// Registry of keyboard shortcuts mapped to actions.
///
/// Uses a `BTreeMap` for deterministic iteration order, which makes
/// configuration serialization stable across runs.
pub struct HotkeyRegistry {
    bindings: BTreeMap<Hotkey, HotkeyAction>,
}

impl HotkeyRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            bindings: BTreeMap::new(),
        }
    }

    /// Create a registry pre-populated with the standard default bindings.
    pub fn defaults() -> Self {
        let mut reg = Self::new();
        register_defaults(&mut reg);
        reg
    }

    /// Register a hotkey binding. Returns an error if the hotkey is already
    /// bound to a different action.
    pub fn register(&mut self, hotkey: Hotkey, action: HotkeyAction) -> Result<(), HotkeyError> {
        if let Some(existing) = self.bindings.get(&hotkey) {
            // Allow re-registering the same action (idempotent).
            if *existing == action {
                return Ok(());
            }
            return Err(HotkeyError::Conflict {
                hotkey,
                existing: existing.clone(),
            });
        }
        self.bindings.insert(hotkey, action);
        Ok(())
    }

    /// Remove a hotkey binding. Returns `true` if something was removed.
    pub fn unregister(&mut self, hotkey: &Hotkey) -> bool {
        self.bindings.remove(hotkey).is_some()
    }

    /// Look up the action for a given key + modifiers combination.
    pub fn lookup(&self, key: Key, modifiers: &Modifiers) -> Option<&HotkeyAction> {
        // Construct a temporary Hotkey for the lookup.
        let probe = Hotkey::new(key, *modifiers);
        self.bindings.get(&probe)
    }

    /// Iterate over all registered bindings in sorted order.
    pub fn all_bindings(&self) -> impl Iterator<Item = (&Hotkey, &HotkeyAction)> {
        self.bindings.iter()
    }

    /// Return the number of registered bindings.
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// Check whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Check whether a specific hotkey is registered.
    pub fn is_registered(&self, hotkey: &Hotkey) -> bool {
        self.bindings.contains_key(hotkey)
    }

    /// If the hotkey conflicts with an existing binding, return the existing
    /// action. Returns `None` if the hotkey is free.
    pub fn conflicts_with(&self, hotkey: &Hotkey) -> Option<&HotkeyAction> {
        self.bindings.get(hotkey)
    }

    /// Remove all bindings.
    pub fn clear(&mut self) {
        self.bindings.clear();
    }

    /// Remove all bindings and re-populate with the standard defaults.
    pub fn reset_defaults(&mut self) {
        self.bindings.clear();
        register_defaults(self);
    }
}

impl Default for HotkeyRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Default bindings
// ============================================================================

/// Helper: construct a Modifiers with specific flags set.
fn mods(ctrl: bool, alt: bool, shift: bool, super_key: bool) -> Modifiers {
    Modifiers {
        ctrl,
        alt,
        shift,
        super_key,
    }
}

/// Populate a registry with the standard default shortcut bindings.
fn register_defaults(reg: &mut HotkeyRegistry) {
    // The register calls below cannot fail because the registry starts empty
    // and each hotkey is unique. We still use if-let to satisfy the no-unwrap
    // policy.
    let defaults: &[(Hotkey, HotkeyAction)] = &[
        // Window management
        (
            Hotkey::new(Key::F4, Modifiers::alt()),
            HotkeyAction::CloseWindow,
        ),
        (
            Hotkey::new(Key::D, mods(false, false, false, true)),
            HotkeyAction::ShowDesktop,
        ),
        (
            Hotkey::new(Key::Left, mods(false, false, false, true)),
            HotkeyAction::MoveWindowLeft,
        ),
        (
            Hotkey::new(Key::Right, mods(false, false, false, true)),
            HotkeyAction::MoveWindowRight,
        ),
        // Virtual desktops (Ctrl+Super+Arrow)
        (
            Hotkey::new(Key::Left, mods(true, false, false, true)),
            HotkeyAction::SwitchDesktop(0),
        ),
        (
            Hotkey::new(Key::Right, mods(true, false, false, true)),
            HotkeyAction::SwitchDesktop(1),
        ),
        // Window cycling
        (
            Hotkey::new(Key::Tab, Modifiers::alt()),
            HotkeyAction::CycleWindows,
        ),
        // Search, run, settings
        (
            Hotkey::bare(Key::LeftSuper),
            HotkeyAction::ShowSearch,
        ),
        (
            Hotkey::new(Key::R, mods(false, false, false, true)),
            HotkeyAction::ShowRun,
        ),
        (
            Hotkey::new(Key::I, mods(false, false, false, true)),
            HotkeyAction::SystemSettings,
        ),
        (
            Hotkey::new(Key::E, mods(false, false, false, true)),
            HotkeyAction::LaunchApp("explorer".to_string()),
        ),
        // Lock screen
        (
            Hotkey::new(Key::L, mods(false, false, false, true)),
            HotkeyAction::ScreenLock,
        ),
        // Task manager
        (
            Hotkey::new(Key::Delete, mods(true, true, false, false)),
            HotkeyAction::ShowTaskManager,
        ),
        // Screenshots
        (
            Hotkey::bare(Key::PrintScreen),
            HotkeyAction::Screenshot,
        ),
        (
            Hotkey::new(Key::S, mods(false, false, true, true)),
            HotkeyAction::ScreenshotRegion,
        ),
        // Media keys (mapped as bare keys with no modifiers, since hardware
        // media keys generate dedicated key codes).
        (
            Hotkey::bare(Key::Unknown(0xAF)),
            HotkeyAction::VolumeUp,
        ),
        (
            Hotkey::bare(Key::Unknown(0xAE)),
            HotkeyAction::VolumeDown,
        ),
        (
            Hotkey::bare(Key::Unknown(0xAD)),
            HotkeyAction::VolumeMute,
        ),
        (
            Hotkey::bare(Key::Unknown(0xE0)),
            HotkeyAction::BrightnessUp,
        ),
        (
            Hotkey::bare(Key::Unknown(0xE1)),
            HotkeyAction::BrightnessDown,
        ),
    ];

    for (hotkey, action) in defaults {
        // Ignore errors here — all defaults are unique so this cannot fail.
        let _ = reg.register(*hotkey, action.clone());
    }
}

// ============================================================================
// HotkeyConfig — text-based persistence
// ============================================================================

/// Configuration wrapper for loading/saving hotkey bindings as text.
///
/// File format (one binding per line):
/// ```text
/// # Comment lines start with '#'
/// Alt+F4=close_window
/// Super+D=show_desktop
/// Super+E=launch:explorer
/// Ctrl+Alt+Delete=show_task_manager
/// ```
pub struct HotkeyConfig {
    /// Parsed bindings.
    bindings: Vec<(Hotkey, HotkeyAction)>,
}

impl HotkeyConfig {
    /// Build a config snapshot from an existing registry.
    pub fn from_registry(registry: &HotkeyRegistry) -> Self {
        let bindings: Vec<(Hotkey, HotkeyAction)> = registry
            .all_bindings()
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        Self { bindings }
    }

    /// Parse hotkey configuration from text. Lines starting with '#' are
    /// comments. Blank lines are skipped.
    pub fn load(text: &str) -> Result<Self, HotkeyError> {
        let mut bindings = Vec::new();

        for (line_idx, raw_line) in text.lines().enumerate() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let eq_pos = line.find('=').ok_or_else(|| HotkeyError::ParseError {
                line_number: line_idx + 1,
                message: "expected '=' separator".to_string(),
            })?;

            let key_part = line[..eq_pos].trim();
            let value_part = line[eq_pos + 1..].trim();

            let hotkey = parse_hotkey_string(key_part).map_err(|e| HotkeyError::ParseError {
                line_number: line_idx + 1,
                message: format!("{}", e),
            })?;

            let action =
                HotkeyAction::from_config_value(value_part).map_err(|e| HotkeyError::ParseError {
                    line_number: line_idx + 1,
                    message: format!("{}", e),
                })?;

            bindings.push((hotkey, action));
        }

        Ok(Self { bindings })
    }

    /// Serialize the configuration to text.
    pub fn save(&self) -> String {
        let mut output = String::from("# Keyboard shortcut configuration\n");
        output.push_str("# Format: Modifier+Key=action\n\n");

        for (hotkey, action) in &self.bindings {
            output.push_str(&hotkey.display_name());
            output.push('=');
            output.push_str(&action.to_config_value());
            output.push('\n');
        }

        output
    }

    /// Convert this configuration into a populated registry.
    ///
    /// If duplicate hotkeys exist in the config, the last one wins (no error).
    pub fn into_registry(self) -> HotkeyRegistry {
        let mut registry = HotkeyRegistry::new();
        for (hotkey, action) in self.bindings {
            // Overwrite any previous binding for the same hotkey.
            registry.bindings.insert(hotkey, action);
        }
        registry
    }

    /// Create a config with the standard defaults.
    pub fn reset_defaults() -> Self {
        Self::from_registry(&HotkeyRegistry::defaults())
    }

    /// Return the number of bindings.
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// Check whether there are no bindings.
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }
}

// ============================================================================
// Key name parsing and display
// ============================================================================

/// Return a human-readable name for a `Key` variant.
fn key_display_name(key: Key) -> &'static str {
    match key {
        Key::A => "A",
        Key::B => "B",
        Key::C => "C",
        Key::D => "D",
        Key::E => "E",
        Key::F => "F",
        Key::G => "G",
        Key::H => "H",
        Key::I => "I",
        Key::J => "J",
        Key::K => "K",
        Key::L => "L",
        Key::M => "M",
        Key::N => "N",
        Key::O => "O",
        Key::P => "P",
        Key::Q => "Q",
        Key::R => "R",
        Key::S => "S",
        Key::T => "T",
        Key::U => "U",
        Key::V => "V",
        Key::W => "W",
        Key::X => "X",
        Key::Y => "Y",
        Key::Z => "Z",
        Key::Num0 => "0",
        Key::Num1 => "1",
        Key::Num2 => "2",
        Key::Num3 => "3",
        Key::Num4 => "4",
        Key::Num5 => "5",
        Key::Num6 => "6",
        Key::Num7 => "7",
        Key::Num8 => "8",
        Key::Num9 => "9",
        Key::F1 => "F1",
        Key::F2 => "F2",
        Key::F3 => "F3",
        Key::F4 => "F4",
        Key::F5 => "F5",
        Key::F6 => "F6",
        Key::F7 => "F7",
        Key::F8 => "F8",
        Key::F9 => "F9",
        Key::F10 => "F10",
        Key::F11 => "F11",
        Key::F12 => "F12",
        Key::Left => "Left",
        Key::Right => "Right",
        Key::Up => "Up",
        Key::Down => "Down",
        Key::Home => "Home",
        Key::End => "End",
        Key::PageUp => "PageUp",
        Key::PageDown => "PageDown",
        Key::Backspace => "Backspace",
        Key::Delete => "Delete",
        Key::Insert => "Insert",
        Key::Enter => "Enter",
        Key::Tab => "Tab",
        Key::Escape => "Escape",
        Key::Space => "Space",
        Key::LeftShift => "LeftShift",
        Key::RightShift => "RightShift",
        Key::LeftCtrl => "LeftCtrl",
        Key::RightCtrl => "RightCtrl",
        Key::LeftAlt => "LeftAlt",
        Key::RightAlt => "RightAlt",
        Key::LeftSuper => "LeftSuper",
        Key::RightSuper => "RightSuper",
        Key::PrintScreen => "PrintScreen",
        Key::ScrollLock => "ScrollLock",
        Key::Pause => "Pause",
        Key::CapsLock => "CapsLock",
        Key::NumLock => "NumLock",
        Key::Comma => "Comma",
        Key::Period => "Period",
        Key::Semicolon => "Semicolon",
        Key::Colon => "Colon",
        Key::Slash => "Slash",
        Key::Backslash => "Backslash",
        Key::LeftBracket => "LeftBracket",
        Key::RightBracket => "RightBracket",
        Key::Minus => "Minus",
        Key::Equals => "Equals",
        Key::Apostrophe => "Apostrophe",
        Key::Grave => "Grave",
        Key::Unknown(code) => {
            // Media keys use well-known codes; for others we return a generic label.
            match code {
                0xAF => "VolumeUp",
                0xAE => "VolumeDown",
                0xAD => "VolumeMute",
                0xE0 => "BrightnessUp",
                0xE1 => "BrightnessDown",
                _ => "Unknown",
            }
        }
    }
}

/// Stable sort key for hotkeys (used by Ord impl).
fn key_sort_name(key: Key) -> &'static str {
    key_display_name(key)
}

/// Parse a key name string (case-insensitive) into a `Key`.
fn parse_key_name(name: &str) -> Result<Key, HotkeyError> {
    match name.to_ascii_lowercase().as_str() {
        "a" => Ok(Key::A),
        "b" => Ok(Key::B),
        "c" => Ok(Key::C),
        "d" => Ok(Key::D),
        "e" => Ok(Key::E),
        "f" => Ok(Key::F),
        "g" => Ok(Key::G),
        "h" => Ok(Key::H),
        "i" => Ok(Key::I),
        "j" => Ok(Key::J),
        "k" => Ok(Key::K),
        "l" => Ok(Key::L),
        "m" => Ok(Key::M),
        "n" => Ok(Key::N),
        "o" => Ok(Key::O),
        "p" => Ok(Key::P),
        "q" => Ok(Key::Q),
        "r" => Ok(Key::R),
        "s" => Ok(Key::S),
        "t" => Ok(Key::T),
        "u" => Ok(Key::U),
        "v" => Ok(Key::V),
        "w" => Ok(Key::W),
        "x" => Ok(Key::X),
        "y" => Ok(Key::Y),
        "z" => Ok(Key::Z),
        "0" => Ok(Key::Num0),
        "1" => Ok(Key::Num1),
        "2" => Ok(Key::Num2),
        "3" => Ok(Key::Num3),
        "4" => Ok(Key::Num4),
        "5" => Ok(Key::Num5),
        "6" => Ok(Key::Num6),
        "7" => Ok(Key::Num7),
        "8" => Ok(Key::Num8),
        "9" => Ok(Key::Num9),
        "f1" => Ok(Key::F1),
        "f2" => Ok(Key::F2),
        "f3" => Ok(Key::F3),
        "f4" => Ok(Key::F4),
        "f5" => Ok(Key::F5),
        "f6" => Ok(Key::F6),
        "f7" => Ok(Key::F7),
        "f8" => Ok(Key::F8),
        "f9" => Ok(Key::F9),
        "f10" => Ok(Key::F10),
        "f11" => Ok(Key::F11),
        "f12" => Ok(Key::F12),
        "left" => Ok(Key::Left),
        "right" => Ok(Key::Right),
        "up" => Ok(Key::Up),
        "down" => Ok(Key::Down),
        "home" => Ok(Key::Home),
        "end" => Ok(Key::End),
        "pageup" => Ok(Key::PageUp),
        "pagedown" => Ok(Key::PageDown),
        "backspace" => Ok(Key::Backspace),
        "delete" => Ok(Key::Delete),
        "insert" => Ok(Key::Insert),
        "enter" | "return" => Ok(Key::Enter),
        "tab" => Ok(Key::Tab),
        "escape" | "esc" => Ok(Key::Escape),
        "space" => Ok(Key::Space),
        "printscreen" | "print" | "prtsc" => Ok(Key::PrintScreen),
        "scrolllock" => Ok(Key::ScrollLock),
        "pause" | "break" => Ok(Key::Pause),
        "capslock" => Ok(Key::CapsLock),
        "numlock" => Ok(Key::NumLock),
        "comma" => Ok(Key::Comma),
        "period" => Ok(Key::Period),
        "semicolon" => Ok(Key::Semicolon),
        "colon" => Ok(Key::Colon),
        "slash" => Ok(Key::Slash),
        "backslash" => Ok(Key::Backslash),
        "leftbracket" => Ok(Key::LeftBracket),
        "rightbracket" => Ok(Key::RightBracket),
        "minus" => Ok(Key::Minus),
        "equals" => Ok(Key::Equals),
        "apostrophe" => Ok(Key::Apostrophe),
        "grave" | "backtick" => Ok(Key::Grave),
        "super" | "leftsuper" => Ok(Key::LeftSuper),
        "rightsuper" => Ok(Key::RightSuper),
        "leftshift" => Ok(Key::LeftShift),
        "rightshift" => Ok(Key::RightShift),
        "leftctrl" => Ok(Key::LeftCtrl),
        "rightctrl" => Ok(Key::RightCtrl),
        "leftalt" => Ok(Key::LeftAlt),
        "rightalt" => Ok(Key::RightAlt),
        "volumeup" => Ok(Key::Unknown(0xAF)),
        "volumedown" => Ok(Key::Unknown(0xAE)),
        "volumemute" => Ok(Key::Unknown(0xAD)),
        "brightnessup" => Ok(Key::Unknown(0xE0)),
        "brightnessdown" => Ok(Key::Unknown(0xE1)),
        _ => Err(HotkeyError::UnknownKey(name.to_string())),
    }
}

/// Parse a combined hotkey string like "Ctrl+Alt+Delete" into a `Hotkey`.
fn parse_hotkey_string(s: &str) -> Result<Hotkey, HotkeyError> {
    let parts: Vec<&str> = s.split('+').map(str::trim).collect();
    if parts.is_empty() {
        return Err(HotkeyError::UnknownKey(s.to_string()));
    }

    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut super_key = false;
    let mut principal_key: Option<Key> = None;

    for part in &parts {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => ctrl = true,
            "alt" => alt = true,
            "shift" => shift = true,
            "super" | "win" | "meta" | "cmd" => super_key = true,
            _ => {
                // This should be the principal key (last non-modifier token).
                principal_key = Some(parse_key_name(part)?);
            }
        }
    }

    let key = principal_key.ok_or_else(|| HotkeyError::UnknownKey(s.to_string()))?;

    Ok(Hotkey {
        key,
        ctrl,
        alt,
        shift,
        super_key,
    })
}

// ============================================================================
// Settings panel rendering
// ============================================================================

/// Render a hotkey settings panel showing all bindings.
///
/// Produces a self-contained list of `RenderCommand`s that can be composited
/// on top of the desktop. The panel is positioned at `(panel_x, panel_y)`.
///
/// `selected_index` optionally highlights one row (for keyboard navigation).
pub fn render_settings_panel(
    registry: &HotkeyRegistry,
    panel_x: f32,
    panel_y: f32,
    selected_index: Option<usize>,
) -> Vec<RenderCommand> {
    let binding_count = registry.len();
    let content_height = HEADER_HEIGHT + binding_count as f32 * ROW_HEIGHT + PADDING;
    let panel_height = content_height;
    let radii = CornerRadii::all(PANEL_RADIUS);

    let mut cmds: Vec<RenderCommand> = Vec::with_capacity(binding_count * 6 + 8);

    // Shadow.
    cmds.push(RenderCommand::BoxShadow {
        x: panel_x,
        y: panel_y,
        width: PANEL_WIDTH,
        height: panel_height,
        offset_x: 0.0,
        offset_y: 4.0,
        blur: 20.0,
        spread: 6.0,
        color: theme::SHADOW,
        corner_radii: radii,
    });

    // Background.
    cmds.push(RenderCommand::FillRect {
        x: panel_x,
        y: panel_y,
        width: PANEL_WIDTH,
        height: panel_height,
        color: theme::BASE,
        corner_radii: radii,
    });

    // Border.
    cmds.push(RenderCommand::StrokeRect {
        x: panel_x,
        y: panel_y,
        width: PANEL_WIDTH,
        height: panel_height,
        color: theme::SURFACE2,
        line_width: 1.0,
        corner_radii: radii,
    });

    // Clip to panel bounds.
    cmds.push(RenderCommand::PushClip {
        x: panel_x,
        y: panel_y,
        width: PANEL_WIDTH,
        height: panel_height,
    });

    // Header.
    cmds.push(RenderCommand::Text {
        x: panel_x + PADDING,
        y: panel_y + PADDING,
        text: "Keyboard Shortcuts".to_string(),
        color: theme::TEXT,
        font_size: HEADER_FONT_SIZE,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });

    // Separator line below header.
    cmds.push(RenderCommand::Line {
        x1: panel_x + PADDING,
        y1: panel_y + HEADER_HEIGHT,
        x2: panel_x + PANEL_WIDTH - PADDING,
        y2: panel_y + HEADER_HEIGHT,
        color: theme::SURFACE1,
        width: 1.0,
    });

    // Rows.
    let content_width = PANEL_WIDTH - PADDING * 2.0;
    for (i, (hotkey, action)) in registry.all_bindings().enumerate() {
        let row_y = panel_y + HEADER_HEIGHT + i as f32 * ROW_HEIGHT;
        let is_selected = selected_index == Some(i);

        // Selection highlight.
        if is_selected {
            cmds.push(RenderCommand::FillRect {
                x: panel_x + PADDING / 2.0,
                y: row_y + 2.0,
                width: content_width + PADDING,
                height: ROW_HEIGHT - 4.0,
                color: theme::SURFACE0,
                corner_radii: CornerRadii::all(6.0),
            });
        }

        // Action label on the left.
        let label = action.display_label();
        let label_color = if is_selected {
            theme::TEXT
        } else {
            theme::SUBTEXT1
        };
        cmds.push(RenderCommand::Text {
            x: panel_x + PADDING,
            y: row_y + (ROW_HEIGHT - LABEL_FONT_SIZE) / 2.0,
            text: label.to_string(),
            color: label_color,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(content_width * 0.45),
        });

        // For LaunchApp and Custom, show the parameter after the label.
        let extra_text = match action {
            HotkeyAction::LaunchApp(app) => Some(app.as_str()),
            HotkeyAction::Custom(name) => Some(name.as_str()),
            HotkeyAction::SwitchDesktop(n) => {
                // Render inline; handled below instead.
                let _ = n;
                None
            }
            _ => None,
        };
        if let Some(detail) = extra_text {
            let detail_x = panel_x + PADDING + content_width * 0.2;
            cmds.push(RenderCommand::Text {
                x: detail_x,
                y: row_y + (ROW_HEIGHT - KEY_FONT_SIZE) / 2.0 + 1.0,
                text: detail.to_string(),
                color: theme::OVERLAY0,
                font_size: KEY_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(content_width * 0.25),
            });
        }

        // Key badges on the right.
        let display = hotkey.display_name();
        let badge_parts: Vec<&str> = display.split('+').collect();
        let mut badge_x = panel_x + PANEL_WIDTH - PADDING;

        // Render badges right-to-left so they align to the right edge.
        for part in badge_parts.iter().rev() {
            let text_width = part.len() as f32 * KEY_FONT_SIZE * 0.6 + 12.0;
            badge_x -= text_width + 4.0;

            let badge_y = row_y + (ROW_HEIGHT - KEY_BADGE_HEIGHT) / 2.0;

            // Badge background.
            cmds.push(RenderCommand::FillRect {
                x: badge_x,
                y: badge_y,
                width: text_width,
                height: KEY_BADGE_HEIGHT,
                color: theme::MANTLE,
                corner_radii: CornerRadii::all(KEY_BADGE_RADIUS),
            });

            // Badge border.
            cmds.push(RenderCommand::StrokeRect {
                x: badge_x,
                y: badge_y,
                width: text_width,
                height: KEY_BADGE_HEIGHT,
                color: theme::SURFACE1,
                line_width: 1.0,
                corner_radii: CornerRadii::all(KEY_BADGE_RADIUS),
            });

            // Badge text.
            cmds.push(RenderCommand::Text {
                x: badge_x + 6.0,
                y: badge_y + (KEY_BADGE_HEIGHT - KEY_FONT_SIZE) / 2.0,
                text: (*part).to_string(),
                color: theme::SUBTEXT0,
                font_size: KEY_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    cmds.push(RenderCommand::PopClip);

    cmds
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use guitk::event::{Key, Modifiers};

    // ====================================================================
    // Hotkey construction and matching
    // ====================================================================

    #[test]
    fn test_hotkey_new() {
        let hk = Hotkey::new(Key::F4, Modifiers::alt());
        assert_eq!(hk.key, Key::F4);
        assert!(hk.alt);
        assert!(!hk.ctrl);
        assert!(!hk.shift);
        assert!(!hk.super_key);
    }

    #[test]
    fn test_hotkey_bare() {
        let hk = Hotkey::bare(Key::PrintScreen);
        assert_eq!(hk.key, Key::PrintScreen);
        assert!(!hk.alt);
        assert!(!hk.ctrl);
        assert!(!hk.shift);
        assert!(!hk.super_key);
    }

    #[test]
    fn test_hotkey_matches_positive() {
        let hk = Hotkey::new(Key::F4, Modifiers::alt());
        let m = Modifiers {
            alt: true,
            ..Modifiers::NONE
        };
        assert!(hk.matches(Key::F4, &m));
    }

    #[test]
    fn test_hotkey_matches_negative_wrong_key() {
        let hk = Hotkey::new(Key::F4, Modifiers::alt());
        let m = Modifiers {
            alt: true,
            ..Modifiers::NONE
        };
        assert!(!hk.matches(Key::F5, &m));
    }

    #[test]
    fn test_hotkey_matches_negative_wrong_modifier() {
        let hk = Hotkey::new(Key::F4, Modifiers::alt());
        let m = Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        };
        assert!(!hk.matches(Key::F4, &m));
    }

    #[test]
    fn test_hotkey_matches_extra_modifier_fails() {
        let hk = Hotkey::new(Key::F4, Modifiers::alt());
        let m = Modifiers {
            alt: true,
            shift: true,
            ..Modifiers::NONE
        };
        // Extra shift should NOT match — exact modifier match required.
        assert!(!hk.matches(Key::F4, &m));
    }

    #[test]
    fn test_hotkey_display_name_simple() {
        let hk = Hotkey::new(Key::F4, Modifiers::alt());
        assert_eq!(hk.display_name(), "Alt+F4");
    }

    #[test]
    fn test_hotkey_display_name_multi_modifier() {
        let hk = Hotkey::new(Key::Delete, mods(true, true, false, false));
        assert_eq!(hk.display_name(), "Ctrl+Alt+Delete");
    }

    #[test]
    fn test_hotkey_display_name_bare() {
        let hk = Hotkey::bare(Key::PrintScreen);
        assert_eq!(hk.display_name(), "PrintScreen");
    }

    #[test]
    fn test_hotkey_display_name_bare_super() {
        // LeftSuper as a principal key (not modifier) must serialize distinctly.
        let hk = Hotkey::bare(Key::LeftSuper);
        assert_eq!(hk.display_name(), "LeftSuper");
    }

    #[test]
    fn test_hotkey_modifiers_roundtrip() {
        let m = mods(true, false, true, false);
        let hk = Hotkey::new(Key::A, m);
        let m2 = hk.modifiers();
        assert_eq!(m2.ctrl, true);
        assert_eq!(m2.alt, false);
        assert_eq!(m2.shift, true);
        assert_eq!(m2.super_key, false);
    }

    // ====================================================================
    // Registry: register / unregister / lookup
    // ====================================================================

    #[test]
    fn test_registry_empty() {
        let reg = HotkeyRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn test_registry_register_and_lookup() {
        let mut reg = HotkeyRegistry::new();
        let hk = Hotkey::new(Key::F4, Modifiers::alt());
        reg.register(hk, HotkeyAction::CloseWindow).ok();
        assert_eq!(reg.len(), 1);

        let found = reg.lookup(Key::F4, &Modifiers::alt());
        assert_eq!(found, Some(&HotkeyAction::CloseWindow));
    }

    #[test]
    fn test_registry_lookup_miss() {
        let mut reg = HotkeyRegistry::new();
        let hk = Hotkey::new(Key::F4, Modifiers::alt());
        reg.register(hk, HotkeyAction::CloseWindow).ok();

        let found = reg.lookup(Key::F5, &Modifiers::alt());
        assert!(found.is_none());
    }

    #[test]
    fn test_registry_conflict_error() {
        let mut reg = HotkeyRegistry::new();
        let hk = Hotkey::new(Key::F4, Modifiers::alt());
        reg.register(hk, HotkeyAction::CloseWindow).ok();

        let result = reg.register(hk, HotkeyAction::MinimizeWindow);
        assert!(result.is_err());
        match result {
            Err(HotkeyError::Conflict { existing, .. }) => {
                assert_eq!(existing, HotkeyAction::CloseWindow);
            }
            _ => panic!("expected Conflict error"),
        }
    }

    #[test]
    fn test_registry_idempotent_register() {
        let mut reg = HotkeyRegistry::new();
        let hk = Hotkey::new(Key::F4, Modifiers::alt());
        reg.register(hk, HotkeyAction::CloseWindow).ok();
        // Re-registering the same action should succeed.
        let result = reg.register(hk, HotkeyAction::CloseWindow);
        assert!(result.is_ok());
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn test_registry_unregister() {
        let mut reg = HotkeyRegistry::new();
        let hk = Hotkey::new(Key::F4, Modifiers::alt());
        reg.register(hk, HotkeyAction::CloseWindow).ok();
        assert!(reg.unregister(&hk));
        assert!(reg.is_empty());
    }

    #[test]
    fn test_registry_unregister_missing() {
        let mut reg = HotkeyRegistry::new();
        let hk = Hotkey::bare(Key::A);
        assert!(!reg.unregister(&hk));
    }

    #[test]
    fn test_registry_is_registered() {
        let mut reg = HotkeyRegistry::new();
        let hk = Hotkey::bare(Key::PrintScreen);
        assert!(!reg.is_registered(&hk));
        reg.register(hk, HotkeyAction::Screenshot).ok();
        assert!(reg.is_registered(&hk));
    }

    #[test]
    fn test_registry_conflicts_with() {
        let mut reg = HotkeyRegistry::new();
        let hk = Hotkey::new(Key::D, mods(false, false, false, true));
        reg.register(hk, HotkeyAction::ShowDesktop).ok();

        assert_eq!(
            reg.conflicts_with(&hk),
            Some(&HotkeyAction::ShowDesktop)
        );

        let free = Hotkey::bare(Key::A);
        assert!(reg.conflicts_with(&free).is_none());
    }

    #[test]
    fn test_registry_all_bindings_iteration() {
        let mut reg = HotkeyRegistry::new();
        reg.register(Hotkey::bare(Key::A), HotkeyAction::VolumeUp)
            .ok();
        reg.register(Hotkey::bare(Key::B), HotkeyAction::VolumeDown)
            .ok();
        let bindings: Vec<_> = reg.all_bindings().collect();
        assert_eq!(bindings.len(), 2);
    }

    #[test]
    fn test_registry_clear() {
        let mut reg = HotkeyRegistry::defaults();
        assert!(!reg.is_empty());
        reg.clear();
        assert!(reg.is_empty());
    }

    #[test]
    fn test_registry_reset_defaults() {
        let mut reg = HotkeyRegistry::new();
        reg.register(Hotkey::bare(Key::Z), HotkeyAction::Custom("test".into()))
            .ok();
        reg.reset_defaults();
        // Should no longer have the custom binding.
        assert!(reg.lookup(Key::Z, &Modifiers::NONE).is_none());
        // Should have the standard bindings.
        assert!(reg.lookup(Key::F4, &Modifiers::alt()).is_some());
    }

    // ====================================================================
    // Defaults
    // ====================================================================

    #[test]
    fn test_defaults_contains_alt_f4() {
        let reg = HotkeyRegistry::defaults();
        let action = reg.lookup(Key::F4, &Modifiers::alt());
        assert_eq!(action, Some(&HotkeyAction::CloseWindow));
    }

    #[test]
    fn test_defaults_contains_super_d() {
        let reg = HotkeyRegistry::defaults();
        let action = reg.lookup(Key::D, &mods(false, false, false, true));
        assert_eq!(action, Some(&HotkeyAction::ShowDesktop));
    }

    #[test]
    fn test_defaults_contains_ctrl_alt_delete() {
        let reg = HotkeyRegistry::defaults();
        let action = reg.lookup(Key::Delete, &mods(true, true, false, false));
        assert_eq!(action, Some(&HotkeyAction::ShowTaskManager));
    }

    #[test]
    fn test_defaults_contains_printscreen() {
        let reg = HotkeyRegistry::defaults();
        let action = reg.lookup(Key::PrintScreen, &Modifiers::NONE);
        assert_eq!(action, Some(&HotkeyAction::Screenshot));
    }

    #[test]
    fn test_defaults_contains_super_left_right() {
        let reg = HotkeyRegistry::defaults();
        assert_eq!(
            reg.lookup(Key::Left, &mods(false, false, false, true)),
            Some(&HotkeyAction::MoveWindowLeft)
        );
        assert_eq!(
            reg.lookup(Key::Right, &mods(false, false, false, true)),
            Some(&HotkeyAction::MoveWindowRight)
        );
    }

    #[test]
    fn test_defaults_contains_super_l_lock() {
        let reg = HotkeyRegistry::defaults();
        assert_eq!(
            reg.lookup(Key::L, &mods(false, false, false, true)),
            Some(&HotkeyAction::ScreenLock)
        );
    }

    #[test]
    fn test_defaults_contains_alt_tab() {
        let reg = HotkeyRegistry::defaults();
        assert_eq!(
            reg.lookup(Key::Tab, &Modifiers::alt()),
            Some(&HotkeyAction::CycleWindows)
        );
    }

    #[test]
    fn test_defaults_contains_super_shift_s() {
        let reg = HotkeyRegistry::defaults();
        assert_eq!(
            reg.lookup(Key::S, &mods(false, false, true, true)),
            Some(&HotkeyAction::ScreenshotRegion)
        );
    }

    #[test]
    fn test_defaults_contains_super_r() {
        let reg = HotkeyRegistry::defaults();
        assert_eq!(
            reg.lookup(Key::R, &mods(false, false, false, true)),
            Some(&HotkeyAction::ShowRun)
        );
    }

    #[test]
    fn test_defaults_contains_super_i() {
        let reg = HotkeyRegistry::defaults();
        assert_eq!(
            reg.lookup(Key::I, &mods(false, false, false, true)),
            Some(&HotkeyAction::SystemSettings)
        );
    }

    #[test]
    fn test_defaults_contains_super_e() {
        let reg = HotkeyRegistry::defaults();
        assert_eq!(
            reg.lookup(Key::E, &mods(false, false, false, true)),
            Some(&HotkeyAction::LaunchApp("explorer".to_string()))
        );
    }

    // ====================================================================
    // Key parsing
    // ====================================================================

    #[test]
    fn test_parse_key_name_letters() {
        assert_eq!(parse_key_name("A").ok(), Some(Key::A));
        assert_eq!(parse_key_name("z").ok(), Some(Key::Z));
    }

    #[test]
    fn test_parse_key_name_function_keys() {
        assert_eq!(parse_key_name("F1").ok(), Some(Key::F1));
        assert_eq!(parse_key_name("f12").ok(), Some(Key::F12));
    }

    #[test]
    fn test_parse_key_name_navigation() {
        assert_eq!(parse_key_name("Left").ok(), Some(Key::Left));
        assert_eq!(parse_key_name("pageup").ok(), Some(Key::PageUp));
        assert_eq!(parse_key_name("Home").ok(), Some(Key::Home));
    }

    #[test]
    fn test_parse_key_name_special() {
        assert_eq!(parse_key_name("Space").ok(), Some(Key::Space));
        assert_eq!(parse_key_name("Enter").ok(), Some(Key::Enter));
        assert_eq!(parse_key_name("return").ok(), Some(Key::Enter));
        assert_eq!(parse_key_name("Esc").ok(), Some(Key::Escape));
    }

    #[test]
    fn test_parse_key_name_unknown() {
        assert!(parse_key_name("Nosuchkey").is_err());
    }

    #[test]
    fn test_parse_hotkey_string_simple() {
        let hk = parse_hotkey_string("Alt+F4").ok();
        assert!(hk.is_some());
        let hk = hk.unwrap();
        assert_eq!(hk.key, Key::F4);
        assert!(hk.alt);
        assert!(!hk.ctrl);
    }

    #[test]
    fn test_parse_hotkey_string_multi_modifier() {
        let hk = parse_hotkey_string("Ctrl+Alt+Delete").ok();
        assert!(hk.is_some());
        let hk = hk.unwrap();
        assert_eq!(hk.key, Key::Delete);
        assert!(hk.ctrl);
        assert!(hk.alt);
    }

    #[test]
    fn test_parse_hotkey_string_bare() {
        let hk = parse_hotkey_string("PrintScreen").ok();
        assert!(hk.is_some());
        let hk = hk.unwrap();
        assert_eq!(hk.key, Key::PrintScreen);
        assert!(!hk.ctrl);
        assert!(!hk.alt);
    }

    // ====================================================================
    // Config persistence
    // ====================================================================

    #[test]
    fn test_config_save_load_roundtrip() {
        let reg = HotkeyRegistry::defaults();
        let config = HotkeyConfig::from_registry(&reg);
        let text = config.save();

        let loaded = HotkeyConfig::load(&text);
        assert!(loaded.is_ok());
        let restored = loaded.ok().map(|c| c.into_registry());
        assert!(restored.is_some());
        let restored = restored.unwrap();

        // Every original binding should be present in the restored registry.
        for (hk, action) in reg.all_bindings() {
            let found = restored.lookup(hk.key, &hk.modifiers());
            assert_eq!(
                found,
                Some(action),
                "binding {} not restored",
                hk.display_name()
            );
        }
    }

    #[test]
    fn test_config_load_comments_and_blanks() {
        let text = "# comment\n\nAlt+F4=close_window\n# another comment\n";
        let config = HotkeyConfig::load(text);
        assert!(config.is_ok());
        let config = config.unwrap();
        assert_eq!(config.len(), 1);
    }

    #[test]
    fn test_config_load_parse_error_no_equals() {
        let text = "Alt+F4 close_window\n";
        let result = HotkeyConfig::load(text);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_load_unknown_action() {
        let text = "Alt+F4=nonexistent_action\n";
        let result = HotkeyConfig::load(text);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_load_unknown_key() {
        let text = "Alt+Bogus=close_window\n";
        let result = HotkeyConfig::load(text);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_load_launch_action() {
        let text = "Super+E=launch:explorer\n";
        let config = HotkeyConfig::load(text).ok();
        assert!(config.is_some());
        let reg = config.unwrap().into_registry();
        assert_eq!(
            reg.lookup(Key::E, &mods(false, false, false, true)),
            Some(&HotkeyAction::LaunchApp("explorer".to_string()))
        );
    }

    #[test]
    fn test_config_load_switch_desktop() {
        let text = "Ctrl+Super+Left=switch_desktop:0\n";
        let config = HotkeyConfig::load(text).ok();
        assert!(config.is_some());
        let reg = config.unwrap().into_registry();
        assert_eq!(
            reg.lookup(Key::Left, &mods(true, false, false, true)),
            Some(&HotkeyAction::SwitchDesktop(0))
        );
    }

    #[test]
    fn test_config_load_custom_action() {
        let text = "Ctrl+Shift+X=custom:my_action\n";
        let config = HotkeyConfig::load(text).ok();
        assert!(config.is_some());
        let reg = config.unwrap().into_registry();
        assert_eq!(
            reg.lookup(Key::X, &mods(true, false, true, false)),
            Some(&HotkeyAction::Custom("my_action".to_string()))
        );
    }

    #[test]
    fn test_config_reset_defaults() {
        let config = HotkeyConfig::reset_defaults();
        assert!(!config.is_empty());
        let reg = config.into_registry();
        assert_eq!(
            reg.lookup(Key::F4, &Modifiers::alt()),
            Some(&HotkeyAction::CloseWindow)
        );
    }

    // ====================================================================
    // Action serialization
    // ====================================================================

    #[test]
    fn test_action_config_roundtrip() {
        let actions = vec![
            HotkeyAction::CloseWindow,
            HotkeyAction::MinimizeWindow,
            HotkeyAction::MaximizeWindow,
            HotkeyAction::ShowTaskManager,
            HotkeyAction::ScreenLock,
            HotkeyAction::Screenshot,
            HotkeyAction::ScreenshotRegion,
            HotkeyAction::VolumeUp,
            HotkeyAction::VolumeDown,
            HotkeyAction::VolumeMute,
            HotkeyAction::BrightnessUp,
            HotkeyAction::BrightnessDown,
            HotkeyAction::ShowSearch,
            HotkeyAction::ShowRun,
            HotkeyAction::ShowDesktop,
            HotkeyAction::CycleWindows,
            HotkeyAction::MoveWindowLeft,
            HotkeyAction::MoveWindowRight,
            HotkeyAction::SystemSettings,
            HotkeyAction::LaunchApp("my_app".to_string()),
            HotkeyAction::SwitchDesktop(3),
            HotkeyAction::Custom("foo".to_string()),
        ];

        for action in &actions {
            let serialized = action.to_config_value();
            let parsed = HotkeyAction::from_config_value(&serialized);
            assert!(
                parsed.is_ok(),
                "failed to parse '{}' for {:?}",
                serialized,
                action
            );
            assert_eq!(
                parsed.ok().as_ref(),
                Some(action),
                "roundtrip mismatch for {:?}",
                action
            );
        }
    }

    // ====================================================================
    // Display label
    // ====================================================================

    #[test]
    fn test_action_display_labels() {
        assert_eq!(HotkeyAction::CloseWindow.display_label(), "Close Window");
        assert_eq!(HotkeyAction::ShowSearch.display_label(), "Search");
        assert_eq!(
            HotkeyAction::LaunchApp("x".into()).display_label(),
            "Launch App"
        );
        assert_eq!(
            HotkeyAction::Custom("x".into()).display_label(),
            "Custom"
        );
    }

    // ====================================================================
    // Error display
    // ====================================================================

    #[test]
    fn test_error_display_conflict() {
        let err = HotkeyError::Conflict {
            hotkey: Hotkey::new(Key::F4, Modifiers::alt()),
            existing: HotkeyAction::CloseWindow,
        };
        let msg = format!("{}", err);
        assert!(msg.contains("Alt+F4"));
        assert!(msg.contains("CloseWindow"));
    }

    #[test]
    fn test_error_display_parse() {
        let err = HotkeyError::ParseError {
            line_number: 5,
            message: "bad line".to_string(),
        };
        let msg = format!("{}", err);
        assert!(msg.contains("line 5"));
        assert!(msg.contains("bad line"));
    }

    // ====================================================================
    // Hotkey ordering (for BTreeMap determinism)
    // ====================================================================

    #[test]
    fn test_hotkey_ord_stable() {
        let a = Hotkey::bare(Key::A);
        let b = Hotkey::bare(Key::B);
        assert!(a < b);

        let ctrl_a = Hotkey::new(Key::A, Modifiers::ctrl());
        // Modifiers sort before bare keys (ctrl=true > ctrl=false).
        assert!(a < ctrl_a);
    }

    // ====================================================================
    // Settings panel rendering
    // ====================================================================

    #[test]
    fn test_render_settings_panel_nonempty() {
        let reg = HotkeyRegistry::defaults();
        let cmds = render_settings_panel(&reg, 100.0, 100.0, None);
        // Should produce a non-trivial number of render commands:
        // shadow + bg + border + clip + header text + separator + rows.
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_render_settings_panel_empty_registry() {
        let reg = HotkeyRegistry::new();
        let cmds = render_settings_panel(&reg, 0.0, 0.0, None);
        // Should still render header (shadow + bg + border + clip + header + sep + popclip).
        assert!(cmds.len() >= 6);
    }

    #[test]
    fn test_render_settings_panel_with_selection() {
        let reg = HotkeyRegistry::defaults();
        let cmds = render_settings_panel(&reg, 50.0, 50.0, Some(0));
        // Should have at least one extra FillRect for the selection highlight.
        let fill_rects = cmds
            .iter()
            .filter(|c| matches!(c, RenderCommand::FillRect { .. }))
            .count();
        // 1 (shadow is BoxShadow) + 1 bg + 1 selection + N badges.
        assert!(fill_rects >= 3);
    }
}
