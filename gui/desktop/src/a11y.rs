//! Accessibility features module for the desktop shell.
//!
//! Provides:
//! - Screen magnifier (zoom lens that follows cursor)
//! - High contrast mode (override theme colors for maximum readability)
//! - Screen reader text generation (alt-text for all UI elements)
//! - Sticky keys (hold modifier state between presses)
//! - Filter keys (ignore brief/repeated keystrokes)
//! - Mouse keys (move cursor via numpad)
//! - Cursor customization (size, color, trail)
//! - Color filter (colorblind modes: protanopia, deuteranopia, tritanopia, grayscale)
//! - Reduced motion (disable animations system-wide)
//! - Focus indicator enhancement (extra-visible keyboard focus ring)

use guitk::color::Color;
use guitk::render::RenderCommand;
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha theme
// ============================================================================

const COL_BASE: Color = Color::from_hex(0x1E1E2E);
const COL_SURFACE0: Color = Color::from_hex(0x313244);
const COL_SURFACE1: Color = Color::from_hex(0x45475A);
const COL_TEXT: Color = Color::from_hex(0xCDD6F4);
const COL_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const COL_BLUE: Color = Color::from_hex(0x89B4FA);
const COL_GREEN: Color = Color::from_hex(0xA6E3A1);
const COL_YELLOW: Color = Color::from_hex(0xF9E2AF);
const COL_RED: Color = Color::from_hex(0xF38BA8);
const COL_PEACH: Color = Color::from_hex(0xFAB387);

// ============================================================================
// High contrast theme
// ============================================================================

/// High contrast color scheme (overrides theme when enabled).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighContrastTheme {
    /// Standard high contrast (black bg, white text, yellow highlights).
    BlackOnWhite,
    /// Inverted (white bg, black text).
    WhiteOnBlack,
    /// Yellow on black (good for low vision).
    YellowOnBlack,
    /// Green on black (terminal style, minimal strain).
    GreenOnBlack,
}

impl HighContrastTheme {
    /// Get the background color for this theme.
    pub fn background(&self) -> Color {
        match self {
            Self::BlackOnWhite => Color::from_hex(0x000000),
            Self::WhiteOnBlack => Color::from_hex(0xFFFFFF),
            Self::YellowOnBlack => Color::from_hex(0x000000),
            Self::GreenOnBlack => Color::from_hex(0x000000),
        }
    }

    /// Get the primary text color.
    pub fn text(&self) -> Color {
        match self {
            Self::BlackOnWhite => Color::from_hex(0xFFFFFF),
            Self::WhiteOnBlack => Color::from_hex(0x000000),
            Self::YellowOnBlack => Color::from_hex(0xFFFF00),
            Self::GreenOnBlack => Color::from_hex(0x00FF00),
        }
    }

    /// Get the accent/highlight color.
    pub fn accent(&self) -> Color {
        match self {
            Self::BlackOnWhite => Color::from_hex(0xFFFF00),
            Self::WhiteOnBlack => Color::from_hex(0x0000FF),
            Self::YellowOnBlack => Color::from_hex(0x00FFFF),
            Self::GreenOnBlack => Color::from_hex(0xFF00FF),
        }
    }

    /// Get the border color.
    pub fn border(&self) -> Color {
        self.text()
    }

    /// Label for display.
    pub fn label(&self) -> &'static str {
        match self {
            Self::BlackOnWhite => "High Contrast (Black bg)",
            Self::WhiteOnBlack => "High Contrast (White bg)",
            Self::YellowOnBlack => "Yellow on Black",
            Self::GreenOnBlack => "Green on Black",
        }
    }
}

// ============================================================================
// Color filter (colorblind simulation/correction)
// ============================================================================

/// Color vision deficiency filter mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorFilter {
    /// No filter (default).
    None,
    /// Red-green deficiency (most common).
    Protanopia,
    /// Green-red deficiency.
    Deuteranopia,
    /// Blue-yellow deficiency.
    Tritanopia,
    /// Full grayscale.
    Grayscale,
    /// Inverted colors.
    Inverted,
}

impl ColorFilter {
    /// Apply this filter to a color.
    pub fn apply(&self, color: Color) -> Color {
        match self {
            Self::None => color,
            Self::Grayscale => {
                // Perceptual luminance weighting.
                let lum = ((color.r as u32 * 77) + (color.g as u32 * 150) + (color.b as u32 * 29)) / 256;
                let l = lum.min(255) as u8;
                Color::rgba(l, l, l, color.a)
            }
            Self::Inverted => {
                Color::rgba(255 - color.r, 255 - color.g, 255 - color.b, color.a)
            }
            Self::Protanopia => {
                // Simplified simulation: reduce red sensitivity.
                let r = ((color.r as u32 * 56) + (color.g as u32 * 43) + color.b as u32) / 100;
                let g = ((color.r as u32 * 55) + (color.g as u32 * 44) + color.b as u32) / 100;
                let b = ((color.g as u32 * 24) + (color.b as u32 * 76)) / 100;
                Color::rgba(r.min(255) as u8, g.min(255) as u8, b.min(255) as u8, color.a)
            }
            Self::Deuteranopia => {
                // Simplified simulation: reduce green sensitivity.
                let r = ((color.r as u32 * 63) + (color.g as u32 * 37)) / 100;
                let g = ((color.r as u32 * 70) + (color.g as u32 * 30)) / 100;
                let b = ((color.g as u32 * 30) + (color.b as u32 * 70)) / 100;
                Color::rgba(r.min(255) as u8, g.min(255) as u8, b.min(255) as u8, color.a)
            }
            Self::Tritanopia => {
                // Simplified simulation: reduce blue sensitivity.
                let r = ((color.r as u32 * 95) + (color.g as u32 * 5)) / 100;
                let g = ((color.g as u32 * 43) + (color.b as u32 * 57)) / 100;
                let b = ((color.g as u32 * 47) + (color.b as u32 * 53)) / 100;
                Color::rgba(r.min(255) as u8, g.min(255) as u8, b.min(255) as u8, color.a)
            }
        }
    }

    /// Label for settings UI.
    pub fn label(&self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Protanopia => "Protanopia (red-weak)",
            Self::Deuteranopia => "Deuteranopia (green-weak)",
            Self::Tritanopia => "Tritanopia (blue-weak)",
            Self::Grayscale => "Grayscale",
            Self::Inverted => "Inverted",
        }
    }
}

// ============================================================================
// Magnifier
// ============================================================================

/// Screen magnifier configuration.
#[derive(Debug, Clone)]
pub struct MagnifierConfig {
    /// Whether magnifier is active.
    pub enabled: bool,
    /// Zoom factor (1.5x to 10.0x).
    pub zoom: f32,
    /// Lens diameter in pixels.
    pub lens_diameter: f32,
    /// Whether to follow the cursor or be anchored.
    pub follow_cursor: bool,
    /// Lens shape.
    pub shape: MagnifierShape,
    /// Whether to show crosshairs in the lens center.
    pub show_crosshairs: bool,
    /// Border width around the lens.
    pub border_width: f32,
}

/// Magnifier lens shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MagnifierShape {
    /// Circular lens.
    Circle,
    /// Rectangular lens.
    Rectangle,
    /// Full-width strip at top of screen (docked).
    DockedTop,
    /// Full screen zoom (no lens).
    FullScreen,
}

impl Default for MagnifierConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            zoom: 2.0,
            lens_diameter: 200.0,
            follow_cursor: true,
            shape: MagnifierShape::Circle,
            show_crosshairs: false,
            border_width: 2.0,
        }
    }
}

impl MagnifierConfig {
    /// Clamp zoom to valid range.
    pub fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom.clamp(1.5, 10.0);
    }

    /// Increase zoom by 0.5x.
    pub fn zoom_in(&mut self) {
        self.set_zoom(self.zoom + 0.5);
    }

    /// Decrease zoom by 0.5x.
    pub fn zoom_out(&mut self) {
        self.set_zoom(self.zoom - 0.5);
    }
}

/// Magnifier render state (tracks cursor position).
pub struct Magnifier {
    pub config: MagnifierConfig,
    /// Current cursor position.
    cursor_x: f32,
    cursor_y: f32,
}

impl Magnifier {
    /// Create a new magnifier.
    pub fn new() -> Self {
        Self {
            config: MagnifierConfig::default(),
            cursor_x: 0.0,
            cursor_y: 0.0,
        }
    }

    /// Update cursor position.
    pub fn update_cursor(&mut self, x: f32, y: f32) {
        self.cursor_x = x;
        self.cursor_y = y;
    }

    /// Render the magnifier lens overlay.
    /// Returns render commands for the lens border and crosshairs.
    /// (The actual magnified content would be handled by the compositor.)
    pub fn render_overlay(&self) -> Vec<RenderCommand> {
        if !self.config.enabled {
            return Vec::new();
        }

        let mut cmds = Vec::with_capacity(6);
        let d = self.config.lens_diameter;
        let bw = self.config.border_width;

        match self.config.shape {
            MagnifierShape::Circle | MagnifierShape::Rectangle => {
                let x = self.cursor_x - d / 2.0;
                let y = self.cursor_y - d / 2.0;
                let radii = if self.config.shape == MagnifierShape::Circle {
                    CornerRadii::all(d / 2.0)
                } else {
                    CornerRadii::all(4.0)
                };

                // Lens background (would be replaced by magnified content).
                cmds.push(RenderCommand::FillRect {
                    x,
                    y,
                    width: d,
                    height: d,
                    color: Color::rgba(30, 30, 46, 200),
                    corner_radii: radii,
                });

                // Border.
                cmds.push(RenderCommand::StrokeRect {
                    x,
                    y,
                    width: d,
                    height: d,
                    color: COL_BLUE,
                    line_width: bw,
                    corner_radii: radii,
                });

                // Crosshairs.
                if self.config.show_crosshairs {
                    let cx = self.cursor_x;
                    let cy = self.cursor_y;
                    cmds.push(RenderCommand::Line {
                        x1: cx - d / 4.0,
                        y1: cy,
                        x2: cx + d / 4.0,
                        y2: cy,
                        color: Color::rgba(255, 255, 255, 128),
                        width: 1.0,
                    });
                    cmds.push(RenderCommand::Line {
                        x1: cx,
                        y1: cy - d / 4.0,
                        x2: cx,
                        y2: cy + d / 4.0,
                        color: Color::rgba(255, 255, 255, 128),
                        width: 1.0,
                    });
                }
            }
            MagnifierShape::DockedTop => {
                // Full-width strip at top.
                let strip_h = d;
                cmds.push(RenderCommand::FillRect {
                    x: 0.0,
                    y: 0.0,
                    width: 1920.0, // Will be screen width in practice.
                    height: strip_h,
                    color: Color::rgba(30, 30, 46, 220),
                    corner_radii: CornerRadii::ZERO,
                });
                cmds.push(RenderCommand::Line {
                    x1: 0.0,
                    y1: strip_h,
                    x2: 1920.0,
                    y2: strip_h,
                    color: COL_BLUE,
                    width: bw,
                });
            }
            MagnifierShape::FullScreen => {
                // No overlay — compositor handles full-screen zoom.
            }
        }

        cmds
    }
}

// ============================================================================
// Sticky keys
// ============================================================================

/// Sticky keys state — modifier keys stay active until next non-modifier key.
#[derive(Debug, Clone)]
pub struct StickyKeys {
    pub enabled: bool,
    /// Whether to play a sound when a sticky key is activated.
    pub play_sound: bool,
    /// Whether double-tap locks the modifier.
    pub double_tap_lock: bool,
    // State tracking for each modifier.
    ctrl: StickyState,
    alt: StickyState,
    shift: StickyState,
    super_key: StickyState,
}

/// State of a single sticky modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StickyState {
    /// Modifier is inactive.
    Off,
    /// Modifier is sticky (will apply to next keypress, then turn off).
    Sticky,
    /// Modifier is locked (stays on until pressed again).
    Locked,
}

/// Which modifier was pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StickyModifier {
    Ctrl,
    Alt,
    Shift,
    Super,
}

impl StickyKeys {
    pub fn new() -> Self {
        Self {
            enabled: false,
            play_sound: true,
            double_tap_lock: true,
            ctrl: StickyState::Off,
            alt: StickyState::Off,
            shift: StickyState::Off,
            super_key: StickyState::Off,
        }
    }

    /// Handle a modifier key press. Returns true if state changed.
    pub fn on_modifier_press(&mut self, modifier: StickyModifier) -> bool {
        if !self.enabled {
            return false;
        }
        let double_lock = self.double_tap_lock;
        let state = self.state_mut(modifier);
        match *state {
            StickyState::Off => {
                *state = StickyState::Sticky;
                true
            }
            StickyState::Sticky if double_lock => {
                *state = StickyState::Locked;
                true
            }
            StickyState::Sticky => {
                *state = StickyState::Off;
                true
            }
            StickyState::Locked => {
                *state = StickyState::Off;
                true
            }
        }
    }

    /// Handle a non-modifier key press. Resets any sticky (not locked) modifiers.
    /// Returns which modifiers were active.
    pub fn on_key_press(&mut self) -> (bool, bool, bool, bool) {
        if !self.enabled {
            return (false, false, false, false);
        }
        let ctrl = self.ctrl != StickyState::Off;
        let alt = self.alt != StickyState::Off;
        let shift = self.shift != StickyState::Off;
        let sup = self.super_key != StickyState::Off;

        // Release sticky (not locked) modifiers.
        if self.ctrl == StickyState::Sticky {
            self.ctrl = StickyState::Off;
        }
        if self.alt == StickyState::Sticky {
            self.alt = StickyState::Off;
        }
        if self.shift == StickyState::Sticky {
            self.shift = StickyState::Off;
        }
        if self.super_key == StickyState::Sticky {
            self.super_key = StickyState::Off;
        }

        (ctrl, alt, shift, sup)
    }

    /// Check if a modifier is currently active (sticky or locked).
    pub fn is_active(&self, modifier: StickyModifier) -> bool {
        *self.state_ref(modifier) != StickyState::Off
    }

    /// Check if a modifier is locked.
    pub fn is_locked(&self, modifier: StickyModifier) -> bool {
        *self.state_ref(modifier) == StickyState::Locked
    }

    /// Reset all modifiers.
    pub fn reset(&mut self) {
        self.ctrl = StickyState::Off;
        self.alt = StickyState::Off;
        self.shift = StickyState::Off;
        self.super_key = StickyState::Off;
    }

    fn state_mut(&mut self, m: StickyModifier) -> &mut StickyState {
        match m {
            StickyModifier::Ctrl => &mut self.ctrl,
            StickyModifier::Alt => &mut self.alt,
            StickyModifier::Shift => &mut self.shift,
            StickyModifier::Super => &mut self.super_key,
        }
    }

    fn state_ref(&self, m: StickyModifier) -> &StickyState {
        match m {
            StickyModifier::Ctrl => &self.ctrl,
            StickyModifier::Alt => &self.alt,
            StickyModifier::Shift => &self.shift,
            StickyModifier::Super => &self.super_key,
        }
    }
}

// ============================================================================
// Filter keys
// ============================================================================

/// Filter keys — ignore brief or repeated keystrokes (for motor impairment).
#[derive(Debug, Clone)]
pub struct FilterKeys {
    pub enabled: bool,
    /// Minimum key hold duration to register (milliseconds).
    pub slow_keys_ms: u32,
    /// Minimum interval between same-key repeats (milliseconds).
    pub bounce_keys_ms: u32,
    /// Whether to play a sound on key acceptance.
    pub play_sound: bool,
    /// Last accepted key timestamp per key code (for bounce detection).
    last_key_time: Vec<(u16, u64)>,
    /// Maximum tracked keys.
    max_tracked: usize,
}

impl FilterKeys {
    pub fn new() -> Self {
        Self {
            enabled: false,
            slow_keys_ms: 300,
            bounce_keys_ms: 500,
            play_sound: true,
            last_key_time: Vec::new(),
            max_tracked: 64,
        }
    }

    /// Check if a key press should be accepted.
    /// `key_code`: key identifier, `hold_ms`: how long the key was held,
    /// `now_ms`: current timestamp in milliseconds.
    pub fn should_accept(&mut self, key_code: u16, hold_ms: u32, now_ms: u64) -> bool {
        if !self.enabled {
            return true;
        }

        // Slow keys: reject if held less than threshold.
        if hold_ms < self.slow_keys_ms {
            return false;
        }

        // Bounce keys: reject if same key pressed too quickly.
        if let Some(entry) = self.last_key_time.iter().find(|(k, _)| *k == key_code) {
            let elapsed = now_ms.saturating_sub(entry.1);
            if elapsed < self.bounce_keys_ms as u64 {
                return false;
            }
        }

        // Accept and record time.
        if let Some(entry) = self.last_key_time.iter_mut().find(|(k, _)| *k == key_code) {
            entry.1 = now_ms;
        } else {
            if self.last_key_time.len() >= self.max_tracked {
                self.last_key_time.remove(0);
            }
            self.last_key_time.push((key_code, now_ms));
        }

        true
    }

    /// Reset all tracked key times.
    pub fn reset(&mut self) {
        self.last_key_time.clear();
    }
}

// ============================================================================
// Mouse keys
// ============================================================================

/// Mouse keys — control cursor via keyboard (numpad).
#[derive(Debug, Clone)]
pub struct MouseKeys {
    pub enabled: bool,
    /// Cursor speed in pixels per key repeat.
    pub speed: f32,
    /// Acceleration factor (speed increases with held duration).
    pub acceleration: f32,
    /// Maximum speed after acceleration.
    pub max_speed: f32,
    /// Current accumulated speed (resets when no movement key is held).
    current_speed: f32,
}

/// Mouse key action (numpad mapping).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseKeyAction {
    /// Move cursor up-left (Numpad 7).
    MoveUpLeft,
    /// Move cursor up (Numpad 8).
    MoveUp,
    /// Move cursor up-right (Numpad 9).
    MoveUpRight,
    /// Move cursor left (Numpad 4).
    MoveLeft,
    /// Left click (Numpad 5).
    Click,
    /// Move cursor right (Numpad 6).
    MoveRight,
    /// Move cursor down-left (Numpad 1).
    MoveDownLeft,
    /// Move cursor down (Numpad 2).
    MoveDown,
    /// Move cursor down-right (Numpad 3).
    MoveDownRight,
    /// Double-click (Numpad +).
    DoubleClick,
    /// Right-click (Numpad 0).
    RightClick,
}

impl MouseKeys {
    pub fn new() -> Self {
        Self {
            enabled: false,
            speed: 5.0,
            acceleration: 1.2,
            max_speed: 30.0,
            current_speed: 0.0,
        }
    }

    /// Calculate cursor delta for a movement action.
    /// Call repeatedly while key is held; speed accelerates.
    pub fn move_delta(&mut self, action: MouseKeyAction) -> (f32, f32) {
        if !self.enabled {
            return (0.0, 0.0);
        }

        // Accelerate.
        self.current_speed = (self.current_speed * self.acceleration).max(self.speed);
        if self.current_speed > self.max_speed {
            self.current_speed = self.max_speed;
        }

        let s = self.current_speed;
        match action {
            MouseKeyAction::MoveUpLeft => (-s, -s),
            MouseKeyAction::MoveUp => (0.0, -s),
            MouseKeyAction::MoveUpRight => (s, -s),
            MouseKeyAction::MoveLeft => (-s, 0.0),
            MouseKeyAction::MoveRight => (s, 0.0),
            MouseKeyAction::MoveDownLeft => (-s, s),
            MouseKeyAction::MoveDown => (0.0, s),
            MouseKeyAction::MoveDownRight => (s, s),
            _ => (0.0, 0.0), // Click actions don't move.
        }
    }

    /// Reset speed (call when no movement key is held).
    pub fn reset_speed(&mut self) {
        self.current_speed = 0.0;
    }
}

// ============================================================================
// Cursor customization
// ============================================================================

/// Custom cursor appearance settings.
#[derive(Debug, Clone)]
pub struct CursorSettings {
    /// Cursor size multiplier (1.0 = default, up to 5.0).
    pub size_scale: f32,
    /// Custom cursor color (None = default).
    pub color: Option<Color>,
    /// Whether to show a cursor trail.
    pub trail_enabled: bool,
    /// Trail length (number of ghost cursors).
    pub trail_length: u8,
    /// Whether to show a locator ring on Ctrl press.
    pub locator_enabled: bool,
    /// Locator ring color.
    pub locator_color: Color,
}

impl Default for CursorSettings {
    fn default() -> Self {
        Self {
            size_scale: 1.0,
            color: None,
            trail_enabled: false,
            trail_length: 3,
            locator_enabled: true,
            locator_color: COL_BLUE,
        }
    }
}

impl CursorSettings {
    /// Clamp size scale to valid range.
    pub fn set_size(&mut self, scale: f32) {
        self.size_scale = scale.clamp(0.5, 5.0);
    }
}

// ============================================================================
// Focus indicator
// ============================================================================

/// Enhanced focus indicator settings (for keyboard navigation).
#[derive(Debug, Clone)]
pub struct FocusIndicator {
    /// Whether to show enhanced focus ring.
    pub enabled: bool,
    /// Focus ring color.
    pub color: Color,
    /// Ring width in pixels.
    pub width: f32,
    /// Ring offset from element edge.
    pub offset: f32,
    /// Whether to animate the ring (pulse).
    pub animate: bool,
}

impl Default for FocusIndicator {
    fn default() -> Self {
        Self {
            enabled: true,
            color: COL_BLUE,
            width: 2.0,
            offset: 2.0,
            animate: false,
        }
    }
}

impl FocusIndicator {
    /// Render the focus ring around an element.
    pub fn render(&self, x: f32, y: f32, w: f32, h: f32, frame: u64) -> Vec<RenderCommand> {
        if !self.enabled {
            return Vec::new();
        }

        let alpha = if self.animate {
            // Pulse between 128 and 255.
            let phase = ((frame % 60) as f32 / 60.0) * std::f32::consts::PI * 2.0;
            (phase.sin() * 64.0 + 191.0) as u8
        } else {
            255
        };

        let ring_color = Color::rgba(self.color.r, self.color.g, self.color.b, alpha);
        let o = self.offset;

        vec![RenderCommand::StrokeRect {
            x: x - o,
            y: y - o,
            width: w + o * 2.0,
            height: h + o * 2.0,
            color: ring_color,
            line_width: self.width,
            corner_radii: CornerRadii::all(4.0),
        }]
    }
}

// ============================================================================
// Master accessibility config
// ============================================================================

/// Master accessibility configuration.
#[derive(Debug, Clone)]
pub struct AccessibilityConfig {
    /// High contrast mode.
    pub high_contrast: Option<HighContrastTheme>,
    /// Color filter for colorblind users.
    pub color_filter: ColorFilter,
    /// Reduce motion (disable animations).
    pub reduced_motion: bool,
    /// Screen magnifier settings.
    pub magnifier: MagnifierConfig,
    /// Sticky keys settings.
    pub sticky_keys_enabled: bool,
    pub sticky_keys_sound: bool,
    pub sticky_keys_double_lock: bool,
    /// Filter keys settings.
    pub filter_keys_enabled: bool,
    pub slow_keys_ms: u32,
    pub bounce_keys_ms: u32,
    /// Mouse keys settings.
    pub mouse_keys_enabled: bool,
    pub mouse_keys_speed: f32,
    /// Cursor settings.
    pub cursor: CursorSettings,
    /// Focus indicator settings.
    pub focus_indicator: FocusIndicator,
    /// Screen reader enabled.
    pub screen_reader: bool,
    /// Text scale factor (1.0 = default, up to 3.0).
    pub text_scale: f32,
    /// Caret (text cursor) width multiplier.
    pub caret_width: f32,
    /// Audio visual alerts (flash screen on system sound).
    pub visual_alerts: bool,
}

impl Default for AccessibilityConfig {
    fn default() -> Self {
        Self {
            high_contrast: None,
            color_filter: ColorFilter::None,
            reduced_motion: false,
            magnifier: MagnifierConfig::default(),
            sticky_keys_enabled: false,
            sticky_keys_sound: true,
            sticky_keys_double_lock: true,
            filter_keys_enabled: false,
            slow_keys_ms: 300,
            bounce_keys_ms: 500,
            mouse_keys_enabled: false,
            mouse_keys_speed: 5.0,
            cursor: CursorSettings::default(),
            focus_indicator: FocusIndicator::default(),
            screen_reader: false,
            text_scale: 1.0,
            caret_width: 1.0,
            visual_alerts: false,
        }
    }
}

impl AccessibilityConfig {
    /// Serialize to key=value text.
    pub fn to_config_string(&self) -> String {
        let mut out = String::with_capacity(512);
        out.push_str("# Accessibility Configuration\n");

        if let Some(hc) = &self.high_contrast {
            out.push_str(&format!("high_contrast={}\n", match hc {
                HighContrastTheme::BlackOnWhite => "black_on_white",
                HighContrastTheme::WhiteOnBlack => "white_on_black",
                HighContrastTheme::YellowOnBlack => "yellow_on_black",
                HighContrastTheme::GreenOnBlack => "green_on_black",
            }));
        } else {
            out.push_str("high_contrast=off\n");
        }

        out.push_str(&format!("color_filter={}\n", match self.color_filter {
            ColorFilter::None => "none",
            ColorFilter::Protanopia => "protanopia",
            ColorFilter::Deuteranopia => "deuteranopia",
            ColorFilter::Tritanopia => "tritanopia",
            ColorFilter::Grayscale => "grayscale",
            ColorFilter::Inverted => "inverted",
        }));

        out.push_str(&format!("reduced_motion={}\n", self.reduced_motion));
        out.push_str(&format!("magnifier_enabled={}\n", self.magnifier.enabled));
        out.push_str(&format!("magnifier_zoom={}\n", self.magnifier.zoom));
        out.push_str(&format!("sticky_keys={}\n", self.sticky_keys_enabled));
        out.push_str(&format!("filter_keys={}\n", self.filter_keys_enabled));
        out.push_str(&format!("slow_keys_ms={}\n", self.slow_keys_ms));
        out.push_str(&format!("bounce_keys_ms={}\n", self.bounce_keys_ms));
        out.push_str(&format!("mouse_keys={}\n", self.mouse_keys_enabled));
        out.push_str(&format!("mouse_keys_speed={}\n", self.mouse_keys_speed));
        out.push_str(&format!("screen_reader={}\n", self.screen_reader));
        out.push_str(&format!("text_scale={}\n", self.text_scale));
        out.push_str(&format!("caret_width={}\n", self.caret_width));
        out.push_str(&format!("visual_alerts={}\n", self.visual_alerts));
        out.push_str(&format!("cursor_size={}\n", self.cursor.size_scale));
        out.push_str(&format!("cursor_trail={}\n", self.cursor.trail_enabled));
        out.push_str(&format!("cursor_locator={}\n", self.cursor.locator_enabled));
        out
    }

    /// Parse from key=value text.
    pub fn from_config_string(text: &str) -> Self {
        let mut cfg = Self::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, val)) = line.split_once('=') {
                let key = key.trim();
                let val = val.trim();
                match key {
                    "high_contrast" => {
                        cfg.high_contrast = match val {
                            "black_on_white" => Some(HighContrastTheme::BlackOnWhite),
                            "white_on_black" => Some(HighContrastTheme::WhiteOnBlack),
                            "yellow_on_black" => Some(HighContrastTheme::YellowOnBlack),
                            "green_on_black" => Some(HighContrastTheme::GreenOnBlack),
                            _ => None,
                        };
                    }
                    "color_filter" => {
                        cfg.color_filter = match val {
                            "protanopia" => ColorFilter::Protanopia,
                            "deuteranopia" => ColorFilter::Deuteranopia,
                            "tritanopia" => ColorFilter::Tritanopia,
                            "grayscale" => ColorFilter::Grayscale,
                            "inverted" => ColorFilter::Inverted,
                            _ => ColorFilter::None,
                        };
                    }
                    "reduced_motion" => cfg.reduced_motion = val == "true",
                    "magnifier_enabled" => cfg.magnifier.enabled = val == "true",
                    "magnifier_zoom" => {
                        if let Ok(z) = val.parse::<f32>() {
                            cfg.magnifier.zoom = z.clamp(1.5, 10.0);
                        }
                    }
                    "sticky_keys" => cfg.sticky_keys_enabled = val == "true",
                    "filter_keys" => cfg.filter_keys_enabled = val == "true",
                    "slow_keys_ms" => {
                        if let Ok(v) = val.parse::<u32>() {
                            cfg.slow_keys_ms = v;
                        }
                    }
                    "bounce_keys_ms" => {
                        if let Ok(v) = val.parse::<u32>() {
                            cfg.bounce_keys_ms = v;
                        }
                    }
                    "mouse_keys" => cfg.mouse_keys_enabled = val == "true",
                    "mouse_keys_speed" => {
                        if let Ok(v) = val.parse::<f32>() {
                            cfg.mouse_keys_speed = v.clamp(1.0, 50.0);
                        }
                    }
                    "screen_reader" => cfg.screen_reader = val == "true",
                    "text_scale" => {
                        if let Ok(v) = val.parse::<f32>() {
                            cfg.text_scale = v.clamp(0.5, 3.0);
                        }
                    }
                    "caret_width" => {
                        if let Ok(v) = val.parse::<f32>() {
                            cfg.caret_width = v.clamp(0.5, 5.0);
                        }
                    }
                    "visual_alerts" => cfg.visual_alerts = val == "true",
                    "cursor_size" => {
                        if let Ok(v) = val.parse::<f32>() {
                            cfg.cursor.size_scale = v.clamp(0.5, 5.0);
                        }
                    }
                    "cursor_trail" => cfg.cursor.trail_enabled = val == "true",
                    "cursor_locator" => cfg.cursor.locator_enabled = val == "true",
                    _ => {}
                }
            }
        }
        cfg
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- High Contrast --

    #[test]
    fn test_high_contrast_colors() {
        let hc = HighContrastTheme::BlackOnWhite;
        assert_eq!(hc.background(), Color::from_hex(0x000000));
        assert_eq!(hc.text(), Color::from_hex(0xFFFFFF));
        assert_eq!(hc.accent(), Color::from_hex(0xFFFF00));
    }

    #[test]
    fn test_high_contrast_labels() {
        assert!(!HighContrastTheme::YellowOnBlack.label().is_empty());
        assert!(!HighContrastTheme::GreenOnBlack.label().is_empty());
    }

    #[test]
    fn test_all_contrast_themes_have_different_text() {
        let themes = [
            HighContrastTheme::BlackOnWhite,
            HighContrastTheme::WhiteOnBlack,
            HighContrastTheme::YellowOnBlack,
            HighContrastTheme::GreenOnBlack,
        ];
        for i in 0..themes.len() {
            for j in (i + 1)..themes.len() {
                // Text colors should differ between themes.
                assert_ne!(themes[i].text(), themes[j].text());
            }
        }
    }

    // -- Color Filter --

    #[test]
    fn test_color_filter_none() {
        let c = Color::rgba(100, 150, 200, 255);
        assert_eq!(ColorFilter::None.apply(c), c);
    }

    #[test]
    fn test_color_filter_grayscale() {
        let c = Color::rgba(255, 0, 0, 255); // Pure red.
        let g = ColorFilter::Grayscale.apply(c);
        assert_eq!(g.r, g.g);
        assert_eq!(g.g, g.b);
        assert_eq!(g.a, 255);
    }

    #[test]
    fn test_color_filter_inverted() {
        let c = Color::rgba(100, 150, 200, 128);
        let inv = ColorFilter::Inverted.apply(c);
        assert_eq!(inv.r, 155);
        assert_eq!(inv.g, 105);
        assert_eq!(inv.b, 55);
        assert_eq!(inv.a, 128); // Alpha preserved.
    }

    #[test]
    fn test_color_filter_protanopia() {
        let c = Color::rgba(200, 100, 50, 255);
        let f = ColorFilter::Protanopia.apply(c);
        // Should shift reds toward yellow/green.
        assert_ne!(f, c);
        assert_eq!(f.a, 255);
    }

    #[test]
    fn test_color_filter_deuteranopia() {
        let c = Color::rgba(100, 200, 50, 255);
        let f = ColorFilter::Deuteranopia.apply(c);
        assert_ne!(f, c);
    }

    #[test]
    fn test_color_filter_tritanopia() {
        let c = Color::rgba(50, 100, 200, 255);
        let f = ColorFilter::Tritanopia.apply(c);
        assert_ne!(f, c);
    }

    #[test]
    fn test_color_filter_labels() {
        for filter in &[
            ColorFilter::None,
            ColorFilter::Protanopia,
            ColorFilter::Deuteranopia,
            ColorFilter::Tritanopia,
            ColorFilter::Grayscale,
            ColorFilter::Inverted,
        ] {
            assert!(!filter.label().is_empty());
        }
    }

    // -- Magnifier --

    #[test]
    fn test_magnifier_default() {
        let cfg = MagnifierConfig::default();
        assert!(!cfg.enabled);
        assert!((cfg.zoom - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_magnifier_zoom_clamp() {
        let mut cfg = MagnifierConfig::default();
        cfg.set_zoom(100.0);
        assert!((cfg.zoom - 10.0).abs() < f32::EPSILON);
        cfg.set_zoom(0.1);
        assert!((cfg.zoom - 1.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_magnifier_zoom_in_out() {
        let mut cfg = MagnifierConfig::default();
        cfg.zoom_in();
        assert!((cfg.zoom - 2.5).abs() < f32::EPSILON);
        cfg.zoom_out();
        assert!((cfg.zoom - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_magnifier_render_disabled() {
        let m = Magnifier::new();
        assert!(m.render_overlay().is_empty());
    }

    #[test]
    fn test_magnifier_render_enabled() {
        let mut m = Magnifier::new();
        m.config.enabled = true;
        m.update_cursor(500.0, 300.0);
        let cmds = m.render_overlay();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_magnifier_docked_top() {
        let mut m = Magnifier::new();
        m.config.enabled = true;
        m.config.shape = MagnifierShape::DockedTop;
        let cmds = m.render_overlay();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_magnifier_fullscreen_no_overlay() {
        let mut m = Magnifier::new();
        m.config.enabled = true;
        m.config.shape = MagnifierShape::FullScreen;
        let cmds = m.render_overlay();
        assert!(cmds.is_empty());
    }

    // -- Sticky Keys --

    #[test]
    fn test_sticky_keys_disabled() {
        let mut sk = StickyKeys::new();
        assert!(!sk.on_modifier_press(StickyModifier::Ctrl));
    }

    #[test]
    fn test_sticky_keys_basic_cycle() {
        let mut sk = StickyKeys::new();
        sk.enabled = true;

        // Press Ctrl → becomes sticky.
        assert!(sk.on_modifier_press(StickyModifier::Ctrl));
        assert!(sk.is_active(StickyModifier::Ctrl));
        assert!(!sk.is_locked(StickyModifier::Ctrl));

        // Press a regular key → Ctrl is consumed and turned off.
        let (ctrl, _, _, _) = sk.on_key_press();
        assert!(ctrl);
        assert!(!sk.is_active(StickyModifier::Ctrl));
    }

    #[test]
    fn test_sticky_keys_double_tap_lock() {
        let mut sk = StickyKeys::new();
        sk.enabled = true;

        sk.on_modifier_press(StickyModifier::Shift);
        sk.on_modifier_press(StickyModifier::Shift); // Double-tap → locked.
        assert!(sk.is_locked(StickyModifier::Shift));

        // Regular key press doesn't clear locked modifier.
        let (_, _, shift, _) = sk.on_key_press();
        assert!(shift);
        assert!(sk.is_active(StickyModifier::Shift)); // Still locked.
    }

    #[test]
    fn test_sticky_keys_unlock() {
        let mut sk = StickyKeys::new();
        sk.enabled = true;

        sk.on_modifier_press(StickyModifier::Alt);
        sk.on_modifier_press(StickyModifier::Alt); // Lock.
        assert!(sk.is_locked(StickyModifier::Alt));

        sk.on_modifier_press(StickyModifier::Alt); // Unlock.
        assert!(!sk.is_active(StickyModifier::Alt));
    }

    #[test]
    fn test_sticky_keys_reset() {
        let mut sk = StickyKeys::new();
        sk.enabled = true;
        sk.on_modifier_press(StickyModifier::Ctrl);
        sk.on_modifier_press(StickyModifier::Alt);
        sk.reset();
        assert!(!sk.is_active(StickyModifier::Ctrl));
        assert!(!sk.is_active(StickyModifier::Alt));
    }

    // -- Filter Keys --

    #[test]
    fn test_filter_keys_disabled() {
        let mut fk = FilterKeys::new();
        assert!(fk.should_accept(42, 10, 1000));
    }

    #[test]
    fn test_filter_keys_slow_reject() {
        let mut fk = FilterKeys::new();
        fk.enabled = true;
        fk.slow_keys_ms = 300;
        assert!(!fk.should_accept(42, 100, 1000)); // Held 100ms < 300ms.
    }

    #[test]
    fn test_filter_keys_slow_accept() {
        let mut fk = FilterKeys::new();
        fk.enabled = true;
        fk.slow_keys_ms = 300;
        assert!(fk.should_accept(42, 400, 1000)); // Held 400ms >= 300ms.
    }

    #[test]
    fn test_filter_keys_bounce_reject() {
        let mut fk = FilterKeys::new();
        fk.enabled = true;
        fk.slow_keys_ms = 0;
        fk.bounce_keys_ms = 500;

        assert!(fk.should_accept(42, 10, 1000)); // First press.
        assert!(!fk.should_accept(42, 10, 1200)); // 200ms later — too fast.
        assert!(fk.should_accept(42, 10, 1600)); // 400ms later (600ms total) — OK.
    }

    #[test]
    fn test_filter_keys_reset() {
        let mut fk = FilterKeys::new();
        fk.enabled = true;
        fk.slow_keys_ms = 0;
        fk.bounce_keys_ms = 500;
        fk.should_accept(42, 10, 1000);
        fk.reset();
        assert!(fk.should_accept(42, 10, 1001)); // Immediate re-press OK after reset.
    }

    // -- Mouse Keys --

    #[test]
    fn test_mouse_keys_disabled() {
        let mut mk = MouseKeys::new();
        let (dx, dy) = mk.move_delta(MouseKeyAction::MoveUp);
        assert!((dx).abs() < f32::EPSILON);
        assert!((dy).abs() < f32::EPSILON);
    }

    #[test]
    fn test_mouse_keys_movement() {
        let mut mk = MouseKeys::new();
        mk.enabled = true;
        let (dx, dy) = mk.move_delta(MouseKeyAction::MoveUp);
        assert!((dx).abs() < f32::EPSILON);
        assert!(dy < 0.0); // Moving up = negative Y.
    }

    #[test]
    fn test_mouse_keys_acceleration() {
        let mut mk = MouseKeys::new();
        mk.enabled = true;
        let (_, dy1) = mk.move_delta(MouseKeyAction::MoveDown);
        let (_, dy2) = mk.move_delta(MouseKeyAction::MoveDown);
        assert!(dy2 >= dy1); // Second move should be at least as fast.
    }

    #[test]
    fn test_mouse_keys_max_speed() {
        let mut mk = MouseKeys::new();
        mk.enabled = true;
        mk.max_speed = 10.0;
        for _ in 0..100 {
            mk.move_delta(MouseKeyAction::MoveRight);
        }
        let (dx, _) = mk.move_delta(MouseKeyAction::MoveRight);
        assert!(dx <= mk.max_speed + f32::EPSILON);
    }

    #[test]
    fn test_mouse_keys_reset_speed() {
        let mut mk = MouseKeys::new();
        mk.enabled = true;
        for _ in 0..10 {
            mk.move_delta(MouseKeyAction::MoveRight);
        }
        mk.reset_speed();
        let (dx, _) = mk.move_delta(MouseKeyAction::MoveRight);
        assert!((dx - mk.speed).abs() < f32::EPSILON);
    }

    #[test]
    fn test_mouse_keys_click_no_movement() {
        let mut mk = MouseKeys::new();
        mk.enabled = true;
        let (dx, dy) = mk.move_delta(MouseKeyAction::Click);
        assert!((dx).abs() < f32::EPSILON);
        assert!((dy).abs() < f32::EPSILON);
    }

    // -- Cursor Settings --

    #[test]
    fn test_cursor_default() {
        let c = CursorSettings::default();
        assert!((c.size_scale - 1.0).abs() < f32::EPSILON);
        assert!(!c.trail_enabled);
        assert!(c.locator_enabled);
    }

    #[test]
    fn test_cursor_size_clamp() {
        let mut c = CursorSettings::default();
        c.set_size(100.0);
        assert!((c.size_scale - 5.0).abs() < f32::EPSILON);
        c.set_size(-1.0);
        assert!((c.size_scale - 0.5).abs() < f32::EPSILON);
    }

    // -- Focus Indicator --

    #[test]
    fn test_focus_indicator_disabled() {
        let mut fi = FocusIndicator::default();
        fi.enabled = false;
        let cmds = fi.render(10.0, 20.0, 100.0, 50.0, 0);
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_focus_indicator_renders() {
        let fi = FocusIndicator::default();
        let cmds = fi.render(10.0, 20.0, 100.0, 50.0, 0);
        assert_eq!(cmds.len(), 1);
    }

    // -- Config Round-Trip --

    #[test]
    fn test_config_default() {
        let cfg = AccessibilityConfig::default();
        assert!(cfg.high_contrast.is_none());
        assert_eq!(cfg.color_filter, ColorFilter::None);
        assert!(!cfg.reduced_motion);
    }

    #[test]
    fn test_config_round_trip() {
        let mut cfg = AccessibilityConfig::default();
        cfg.high_contrast = Some(HighContrastTheme::YellowOnBlack);
        cfg.color_filter = ColorFilter::Deuteranopia;
        cfg.reduced_motion = true;
        cfg.magnifier.enabled = true;
        cfg.magnifier.zoom = 3.5;
        cfg.sticky_keys_enabled = true;
        cfg.filter_keys_enabled = true;
        cfg.slow_keys_ms = 500;
        cfg.mouse_keys_enabled = true;
        cfg.screen_reader = true;
        cfg.text_scale = 1.5;
        cfg.visual_alerts = true;
        cfg.cursor.size_scale = 2.0;
        cfg.cursor.trail_enabled = true;

        let text = cfg.to_config_string();
        let parsed = AccessibilityConfig::from_config_string(&text);

        assert_eq!(parsed.high_contrast, Some(HighContrastTheme::YellowOnBlack));
        assert_eq!(parsed.color_filter, ColorFilter::Deuteranopia);
        assert!(parsed.reduced_motion);
        assert!(parsed.magnifier.enabled);
        assert!((parsed.magnifier.zoom - 3.5).abs() < f32::EPSILON);
        assert!(parsed.sticky_keys_enabled);
        assert!(parsed.filter_keys_enabled);
        assert_eq!(parsed.slow_keys_ms, 500);
        assert!(parsed.mouse_keys_enabled);
        assert!(parsed.screen_reader);
        assert!((parsed.text_scale - 1.5).abs() < f32::EPSILON);
        assert!(parsed.visual_alerts);
        assert!((parsed.cursor.size_scale - 2.0).abs() < f32::EPSILON);
        assert!(parsed.cursor.trail_enabled);
    }

    #[test]
    fn test_config_parse_ignores_comments() {
        let text = "# comment\nhigh_contrast=green_on_black\n# more\n";
        let cfg = AccessibilityConfig::from_config_string(text);
        assert_eq!(cfg.high_contrast, Some(HighContrastTheme::GreenOnBlack));
    }

    #[test]
    fn test_config_parse_ignores_unknown() {
        let text = "unknown=value\nscreen_reader=true\n";
        let cfg = AccessibilityConfig::from_config_string(text);
        assert!(cfg.screen_reader);
    }

    #[test]
    fn test_config_clamp_values() {
        let text = "magnifier_zoom=999\ntext_scale=100\ncursor_size=-5\n";
        let cfg = AccessibilityConfig::from_config_string(text);
        assert!((cfg.magnifier.zoom - 10.0).abs() < f32::EPSILON);
        assert!((cfg.text_scale - 3.0).abs() < f32::EPSILON);
        assert!((cfg.cursor.size_scale - 0.5).abs() < f32::EPSILON);
    }
}
