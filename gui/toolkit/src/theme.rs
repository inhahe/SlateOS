#![allow(dead_code)]
//! Centralized theme system for the GUI toolkit.
//!
//! Provides semantic color roles, built-in themes (Catppuccin Mocha/Latte,
//! high-contrast), a global `ThemeManager` for runtime switching, and color
//! utility functions for computing derived states (hover, pressed, disabled).

use crate::color::Color;

// ---------------------------------------------------------------------------
// Color Utilities
// ---------------------------------------------------------------------------

/// Compute relative luminance of a color (0.0 = black, 1.0 = white).
/// Uses the sRGB linearization approximation (gamma 2.2).
fn luminance(c: Color) -> f32 {
    let r = (c.r as f32 / 255.0).powf(2.2);
    let g = (c.g as f32 / 255.0).powf(2.2);
    let b = (c.b as f32 / 255.0).powf(2.2);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Returns `true` if the color is perceptually dark (luminance < 0.5).
pub fn is_dark(color: Color) -> bool {
    luminance(color) < 0.5
}

/// Returns white or black depending on which has better contrast against
/// the given background color.
pub fn contrast_text(background: Color) -> Color {
    if is_dark(background) {
        Color::WHITE
    } else {
        Color::BLACK
    }
}

/// Lighten a color by `amount` (0.0 = unchanged, 1.0 = white).
pub fn lighten(color: Color, amount: f32) -> Color {
    let amount = amount.clamp(0.0, 1.0);
    Color::rgba(
        (color.r as f32 + (255.0 - color.r as f32) * amount) as u8,
        (color.g as f32 + (255.0 - color.g as f32) * amount) as u8,
        (color.b as f32 + (255.0 - color.b as f32) * amount) as u8,
        color.a,
    )
}

/// Darken a color by `amount` (0.0 = unchanged, 1.0 = black).
pub fn darken(color: Color, amount: f32) -> Color {
    let amount = amount.clamp(0.0, 1.0);
    Color::rgba(
        (color.r as f32 * (1.0 - amount)) as u8,
        (color.g as f32 * (1.0 - amount)) as u8,
        (color.b as f32 * (1.0 - amount)) as u8,
        color.a,
    )
}

/// Return a copy of `color` with the specified alpha value.
pub fn with_alpha(color: Color, alpha: u8) -> Color {
    Color::rgba(color.r, color.g, color.b, alpha)
}

/// Linearly mix two colors. `ratio` 0.0 = all `a`, 1.0 = all `b`.
pub fn mix(a: Color, b: Color, ratio: f32) -> Color {
    let ratio = ratio.clamp(0.0, 1.0);
    let inv = 1.0 - ratio;
    Color::rgba(
        (a.r as f32 * inv + b.r as f32 * ratio) as u8,
        (a.g as f32 * inv + b.g as f32 * ratio) as u8,
        (a.b as f32 * inv + b.b as f32 * ratio) as u8,
        (a.a as f32 * inv + b.a as f32 * ratio) as u8,
    )
}

// ---------------------------------------------------------------------------
// ThemeMode
// ---------------------------------------------------------------------------

/// Identifies which theme is active.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ThemeMode {
    /// Light background, dark text. Default light theme is Catppuccin Latte.
    Light,
    /// Dark background, light text. Default dark theme is Catppuccin Mocha.
    Dark,
    /// A named custom theme registered at runtime.
    Custom(String),
}

impl ThemeMode {
    /// Display name for UI presentation.
    pub fn display_name(&self) -> &str {
        match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
            Self::Custom(name) => name.as_str(),
        }
    }
}

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------

/// Complete color definition for the entire UI.
///
/// Every color role is semantic — widgets reference roles like `primary` or
/// `text_secondary` rather than raw color values, enabling consistent theme
/// switching across the desktop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme {
    /// Human-readable name (e.g. "Catppuccin Mocha").
    pub name: String,
    /// Whether this theme is considered dark or light (affects fallback logic).
    pub mode: ThemeMode,

    // -- Backgrounds --
    /// Main window background.
    pub background: Color,
    /// Elevated surfaces (cards, panels).
    pub surface: Color,
    /// Secondary surface for variety.
    pub surface_variant: Color,
    /// Modal overlay background (should have alpha for transparency).
    pub overlay: Color,

    // -- Text --
    /// Primary/body text.
    pub text_primary: Color,
    /// Dimmed/secondary text (labels, captions).
    pub text_secondary: Color,
    /// Disabled element text.
    pub text_disabled: Color,
    /// Text rendered on a primary-colored background.
    pub text_on_primary: Color,

    // -- Interactive / accent --
    /// Primary action color (buttons, links, accents).
    pub primary: Color,
    /// Primary on hover.
    pub primary_hover: Color,
    /// Primary when pressed/active.
    pub primary_active: Color,
    /// Secondary action color.
    pub secondary: Color,
    /// Secondary on hover.
    pub secondary_hover: Color,

    // -- Status --
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub info: Color,

    // -- Borders & separators --
    pub border: Color,
    /// Border when an element has keyboard/mouse focus.
    pub border_focus: Color,
    /// Thin divider lines between sections.
    pub separator: Color,

    // -- Specific UI elements --
    pub titlebar_bg: Color,
    pub titlebar_text: Color,
    pub titlebar_button_hover: Color,
    pub sidebar_bg: Color,
    pub tooltip_bg: Color,
    pub tooltip_text: Color,
    pub selection_bg: Color,
    pub selection_text: Color,
    pub scrollbar_thumb: Color,
    pub scrollbar_track: Color,

    // -- Elevation --
    /// Shadow color for drop shadows (typically has alpha).
    pub shadow_color: Color,
}

impl Theme {
    // ----- Built-in themes -----

    /// Catppuccin Mocha — the default dark theme.
    pub fn catppuccin_mocha() -> Self {
        Self {
            name: String::from("Catppuccin Mocha"),
            mode: ThemeMode::Dark,

            // Mocha base colors
            background: Color::from_hex(0x1E1E2E),    // Base
            surface: Color::from_hex(0x313244),       // Surface0
            surface_variant: Color::from_hex(0x45475A), // Surface1
            overlay: Color::rgba(0x11, 0x11, 0x1B, 200), // Crust with alpha

            text_primary: Color::from_hex(0xCDD6F4),   // Text
            text_secondary: Color::from_hex(0xA6ADC8), // Subtext0
            text_disabled: Color::from_hex(0x6C7086),  // Overlay0
            text_on_primary: Color::from_hex(0x1E1E2E), // Base (dark on light accent)

            primary: Color::from_hex(0x89B4FA),        // Blue
            primary_hover: Color::from_hex(0xB4D0FC),  // Lighter blue
            primary_active: Color::from_hex(0x74A8F8), // Slightly darker blue
            secondary: Color::from_hex(0xCBA6F7),      // Mauve
            secondary_hover: Color::from_hex(0xDFC2FA),

            success: Color::from_hex(0xA6E3A1),        // Green
            warning: Color::from_hex(0xF9E2AF),        // Yellow
            error: Color::from_hex(0xF38BA8),          // Red
            info: Color::from_hex(0x89DCEB),           // Sky

            border: Color::from_hex(0x45475A),         // Surface1
            border_focus: Color::from_hex(0x89B4FA),   // Blue
            separator: Color::from_hex(0x313244),      // Surface0

            titlebar_bg: Color::from_hex(0x181825),    // Mantle
            titlebar_text: Color::from_hex(0xCDD6F4),  // Text
            titlebar_button_hover: Color::from_hex(0x45475A), // Surface1
            sidebar_bg: Color::from_hex(0x181825),     // Mantle
            tooltip_bg: Color::from_hex(0x313244),     // Surface0
            tooltip_text: Color::from_hex(0xCDD6F4),   // Text
            selection_bg: Color::rgba(0x89, 0xB4, 0xFA, 100), // Blue with alpha
            selection_text: Color::from_hex(0xCDD6F4), // Text
            scrollbar_thumb: Color::from_hex(0x585B70), // Surface2
            scrollbar_track: Color::from_hex(0x313244), // Surface0

            shadow_color: Color::rgba(0x00, 0x00, 0x00, 80),
        }
    }

    /// Catppuccin Latte — the default light theme.
    pub fn catppuccin_latte() -> Self {
        Self {
            name: String::from("Catppuccin Latte"),
            mode: ThemeMode::Light,

            background: Color::from_hex(0xEFF1F5),     // Base
            surface: Color::from_hex(0xE6E9EF),       // Surface0
            surface_variant: Color::from_hex(0xDCE0E8), // Surface1 (Crust)
            overlay: Color::rgba(0xDC, 0xE0, 0xE8, 200), // Crust with alpha

            text_primary: Color::from_hex(0x4C4F69),   // Text
            text_secondary: Color::from_hex(0x6C6F85), // Subtext0
            text_disabled: Color::from_hex(0x9CA0B0),  // Overlay0
            text_on_primary: Color::from_hex(0xEFF1F5), // Base (light on dark accent)

            primary: Color::from_hex(0x1E66F5),        // Blue
            primary_hover: Color::from_hex(0x4A83F7),  // Lighter blue
            primary_active: Color::from_hex(0x1558D4), // Darker blue
            secondary: Color::from_hex(0x8839EF),      // Mauve
            secondary_hover: Color::from_hex(0xA05CF2),

            success: Color::from_hex(0x40A02B),        // Green
            warning: Color::from_hex(0xDF8E1D),        // Yellow
            error: Color::from_hex(0xD20F39),          // Red
            info: Color::from_hex(0x04A5E5),           // Sky

            border: Color::from_hex(0xBCC0CC),         // Surface2
            border_focus: Color::from_hex(0x1E66F5),   // Blue
            separator: Color::from_hex(0xCCD0DA),      // Surface1

            titlebar_bg: Color::from_hex(0xE6E9EF),    // Surface0 (Mantle)
            titlebar_text: Color::from_hex(0x4C4F69),  // Text
            titlebar_button_hover: Color::from_hex(0xDCE0E8), // Surface1
            sidebar_bg: Color::from_hex(0xE6E9EF),    // Surface0
            tooltip_bg: Color::from_hex(0x5C5F77),     // Overlay2 (dark tooltip)
            tooltip_text: Color::from_hex(0xEFF1F5),   // Base
            selection_bg: Color::rgba(0x1E, 0x66, 0xF5, 80), // Blue with alpha
            selection_text: Color::from_hex(0x4C4F69), // Text
            scrollbar_thumb: Color::from_hex(0x9CA0B0), // Overlay0
            scrollbar_track: Color::from_hex(0xE6E9EF), // Surface0

            shadow_color: Color::rgba(0x4C, 0x4F, 0x69, 40),
        }
    }

    /// High-contrast dark theme for accessibility.
    pub fn high_contrast_dark() -> Self {
        Self {
            name: String::from("High Contrast Dark"),
            mode: ThemeMode::Dark,

            background: Color::from_hex(0x000000),
            surface: Color::from_hex(0x1A1A1A),
            surface_variant: Color::from_hex(0x2D2D2D),
            overlay: Color::rgba(0x00, 0x00, 0x00, 220),

            text_primary: Color::from_hex(0xFFFFFF),
            text_secondary: Color::from_hex(0xE0E0E0),
            text_disabled: Color::from_hex(0x808080),
            text_on_primary: Color::from_hex(0x000000),

            primary: Color::from_hex(0x00BFFF),        // Bright cyan-blue
            primary_hover: Color::from_hex(0x66D9FF),
            primary_active: Color::from_hex(0x0099CC),
            secondary: Color::from_hex(0xFFD700),      // Gold
            secondary_hover: Color::from_hex(0xFFE44D),

            success: Color::from_hex(0x00FF00),
            warning: Color::from_hex(0xFFFF00),
            error: Color::from_hex(0xFF0000),
            info: Color::from_hex(0x00FFFF),

            border: Color::from_hex(0xFFFFFF),
            border_focus: Color::from_hex(0xFFFF00),   // Yellow focus indicator
            separator: Color::from_hex(0x808080),

            titlebar_bg: Color::from_hex(0x000000),
            titlebar_text: Color::from_hex(0xFFFFFF),
            titlebar_button_hover: Color::from_hex(0x333333),
            sidebar_bg: Color::from_hex(0x0A0A0A),
            tooltip_bg: Color::from_hex(0xFFFFFF),
            tooltip_text: Color::from_hex(0x000000),
            selection_bg: Color::from_hex(0x0066CC),
            selection_text: Color::from_hex(0xFFFFFF),
            scrollbar_thumb: Color::from_hex(0xCCCCCC),
            scrollbar_track: Color::from_hex(0x333333),

            shadow_color: Color::rgba(0xFF, 0xFF, 0xFF, 30),
        }
    }

    /// High-contrast light theme for accessibility.
    pub fn high_contrast_light() -> Self {
        Self {
            name: String::from("High Contrast Light"),
            mode: ThemeMode::Light,

            background: Color::from_hex(0xFFFFFF),
            surface: Color::from_hex(0xF0F0F0),
            surface_variant: Color::from_hex(0xE0E0E0),
            overlay: Color::rgba(0xFF, 0xFF, 0xFF, 220),

            text_primary: Color::from_hex(0x000000),
            text_secondary: Color::from_hex(0x1A1A1A),
            text_disabled: Color::from_hex(0x666666),
            text_on_primary: Color::from_hex(0xFFFFFF),

            primary: Color::from_hex(0x0000CC),        // Deep blue
            primary_hover: Color::from_hex(0x0000FF),
            primary_active: Color::from_hex(0x000099),
            secondary: Color::from_hex(0x6600CC),      // Deep purple
            secondary_hover: Color::from_hex(0x7700EE),

            success: Color::from_hex(0x006600),
            warning: Color::from_hex(0xCC6600),
            error: Color::from_hex(0xCC0000),
            info: Color::from_hex(0x006699),

            border: Color::from_hex(0x000000),
            border_focus: Color::from_hex(0x0000FF),   // Blue focus indicator
            separator: Color::from_hex(0x666666),

            titlebar_bg: Color::from_hex(0xE0E0E0),
            titlebar_text: Color::from_hex(0x000000),
            titlebar_button_hover: Color::from_hex(0xCCCCCC),
            sidebar_bg: Color::from_hex(0xF0F0F0),
            tooltip_bg: Color::from_hex(0x000000),
            tooltip_text: Color::from_hex(0xFFFFFF),
            selection_bg: Color::from_hex(0x0000CC),
            selection_text: Color::from_hex(0xFFFFFF),
            scrollbar_thumb: Color::from_hex(0x333333),
            scrollbar_track: Color::from_hex(0xCCCCCC),

            shadow_color: Color::rgba(0x00, 0x00, 0x00, 100),
        }
    }
}

// ---------------------------------------------------------------------------
// ThemeColors — convenience accessors for widgets
// ---------------------------------------------------------------------------

/// Helper that provides derived color computations from a `Theme`.
///
/// Widgets use this to get computed colors for common states (hover, pressed,
/// disabled) without hard-coding color manipulation logic.
pub struct ThemeColors<'a> {
    theme: &'a Theme,
}

impl<'a> ThemeColors<'a> {
    pub fn new(theme: &'a Theme) -> Self {
        Self { theme }
    }

    /// Standard button background.
    pub fn button_bg(&self) -> Color {
        self.theme.primary
    }

    /// Button background on hover.
    pub fn button_bg_hover(&self) -> Color {
        self.theme.primary_hover
    }

    /// Button background when pressed.
    pub fn button_bg_active(&self) -> Color {
        self.theme.primary_active
    }

    /// Button text color.
    pub fn button_text(&self) -> Color {
        self.theme.text_on_primary
    }

    /// Secondary/ghost button background.
    pub fn button_secondary_bg(&self) -> Color {
        self.theme.surface_variant
    }

    /// Secondary button text.
    pub fn button_secondary_text(&self) -> Color {
        self.theme.text_primary
    }

    /// Disabled button background.
    pub fn button_disabled_bg(&self) -> Color {
        self.theme.surface_variant
    }

    /// Disabled button text.
    pub fn button_disabled_text(&self) -> Color {
        self.theme.text_disabled
    }

    /// Text input background.
    pub fn input_bg(&self) -> Color {
        self.theme.surface
    }

    /// Text input border.
    pub fn input_border(&self) -> Color {
        self.theme.border
    }

    /// Text input border when focused.
    pub fn input_border_focus(&self) -> Color {
        self.theme.border_focus
    }

    /// Text input placeholder text.
    pub fn input_placeholder(&self) -> Color {
        self.theme.text_disabled
    }

    /// Card/panel background.
    pub fn card_bg(&self) -> Color {
        self.theme.surface
    }

    /// Card border.
    pub fn card_border(&self) -> Color {
        self.theme.border
    }

    /// Compute a hover state for any arbitrary color (lighten in dark mode,
    /// darken in light mode).
    pub fn hover_state(&self, base: Color) -> Color {
        match &self.theme.mode {
            ThemeMode::Dark | ThemeMode::Custom(_) => lighten(base, 0.15),
            ThemeMode::Light => darken(base, 0.08),
        }
    }

    /// Compute a pressed/active state for any arbitrary color.
    pub fn pressed_state(&self, base: Color) -> Color {
        match &self.theme.mode {
            ThemeMode::Dark | ThemeMode::Custom(_) => darken(base, 0.1),
            ThemeMode::Light => darken(base, 0.15),
        }
    }

    /// Compute a disabled state for any color (desaturate + reduce contrast).
    pub fn disabled_state(&self, base: Color) -> Color {
        with_alpha(mix(base, self.theme.background, 0.5), 150)
    }
}

// ---------------------------------------------------------------------------
// ThemeManager
// ---------------------------------------------------------------------------

/// Callback type invoked when the active theme changes.
type ThemeChangedCallback = Box<dyn Fn(&Theme) + Send + Sync>;

/// Manages the active theme and notifies listeners on changes.
///
/// Holds the current theme, a list of built-in themes, any registered custom
/// themes, and change-notification callbacks.
pub struct ThemeManager {
    current: Theme,
    custom_themes: Vec<(String, Theme)>,
    callbacks: Vec<ThemeChangedCallback>,
}

impl ThemeManager {
    /// Create a new `ThemeManager` with the given initial theme.
    pub fn new(initial: Theme) -> Self {
        Self {
            current: initial,
            custom_themes: Vec::new(),
            callbacks: Vec::new(),
        }
    }

    /// Create a `ThemeManager` with the default dark theme (Catppuccin Mocha).
    pub fn default_dark() -> Self {
        Self::new(Theme::catppuccin_mocha())
    }

    /// Create a `ThemeManager` with the default light theme (Catppuccin Latte).
    pub fn default_light() -> Self {
        Self::new(Theme::catppuccin_latte())
    }

    /// Get a reference to the currently active theme.
    pub fn current_theme(&self) -> &Theme {
        &self.current
    }

    /// Switch to a theme identified by mode.
    ///
    /// For `ThemeMode::Light` uses Catppuccin Latte, for `ThemeMode::Dark` uses
    /// Catppuccin Mocha, for `ThemeMode::Custom(name)` looks up a registered
    /// custom theme.
    ///
    /// Returns `true` if the theme was found and applied, `false` if the
    /// requested custom theme does not exist.
    pub fn set_theme(&mut self, mode: ThemeMode) -> bool {
        let new_theme = match &mode {
            ThemeMode::Light => Some(Theme::catppuccin_latte()),
            ThemeMode::Dark => Some(Theme::catppuccin_mocha()),
            ThemeMode::Custom(name) => self
                .custom_themes
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, t)| t.clone()),
        };

        if let Some(theme) = new_theme {
            self.current = theme;
            self.notify_change();
            true
        } else {
            false
        }
    }

    /// Register a custom theme under the given name.
    ///
    /// If a custom theme with the same name already exists, it is replaced.
    pub fn set_custom_theme(&mut self, name: &str, theme: Theme) {
        if let Some(pos) = self.custom_themes.iter().position(|(n, _)| n == name) {
            self.custom_themes[pos] = (name.to_string(), theme);
        } else {
            self.custom_themes.push((name.to_string(), theme));
        }
    }

    /// List all available theme modes (built-in + registered custom).
    pub fn available_themes(&self) -> Vec<ThemeMode> {
        let mut modes = vec![ThemeMode::Light, ThemeMode::Dark];
        for (name, _) in &self.custom_themes {
            modes.push(ThemeMode::Custom(name.clone()));
        }
        modes
    }

    /// Register a callback to be invoked whenever the theme changes.
    pub fn on_theme_changed<F>(&mut self, callback: F)
    where
        F: Fn(&Theme) + Send + Sync + 'static,
    {
        self.callbacks.push(Box::new(callback));
    }

    /// Get a `ThemeColors` helper for the current theme.
    pub fn colors(&self) -> ThemeColors<'_> {
        ThemeColors::new(&self.current)
    }

    /// Internal: invoke all registered change callbacks.
    fn notify_change(&self) {
        for cb in &self.callbacks {
            cb(&self.current);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lighten_zero_is_identity() {
        let c = Color::rgb(100, 150, 200);
        let result = lighten(c, 0.0);
        assert_eq!(result, c);
    }

    #[test]
    fn test_lighten_one_is_white() {
        let c = Color::rgb(100, 150, 200);
        let result = lighten(c, 1.0);
        assert_eq!(result.r, 255);
        assert_eq!(result.g, 255);
        assert_eq!(result.b, 255);
    }

    #[test]
    fn test_darken_zero_is_identity() {
        let c = Color::rgb(100, 150, 200);
        let result = darken(c, 0.0);
        assert_eq!(result, c);
    }

    #[test]
    fn test_darken_one_is_black() {
        let c = Color::rgb(100, 150, 200);
        let result = darken(c, 1.0);
        assert_eq!(result.r, 0);
        assert_eq!(result.g, 0);
        assert_eq!(result.b, 0);
    }

    #[test]
    fn test_darken_preserves_alpha() {
        let c = Color::rgba(100, 150, 200, 128);
        let result = darken(c, 0.5);
        assert_eq!(result.a, 128);
    }

    #[test]
    fn test_lighten_preserves_alpha() {
        let c = Color::rgba(100, 150, 200, 128);
        let result = lighten(c, 0.5);
        assert_eq!(result.a, 128);
    }

    #[test]
    fn test_mix_zero_is_first() {
        let a = Color::rgb(255, 0, 0);
        let b = Color::rgb(0, 0, 255);
        let result = mix(a, b, 0.0);
        assert_eq!(result, a);
    }

    #[test]
    fn test_mix_one_is_second() {
        let a = Color::rgb(255, 0, 0);
        let b = Color::rgb(0, 0, 255);
        let result = mix(a, b, 1.0);
        assert_eq!(result, b);
    }

    #[test]
    fn test_mix_half() {
        let a = Color::rgb(0, 0, 0);
        let b = Color::rgb(200, 200, 200);
        let result = mix(a, b, 0.5);
        assert_eq!(result.r, 100);
        assert_eq!(result.g, 100);
        assert_eq!(result.b, 100);
    }

    #[test]
    fn test_with_alpha() {
        let c = Color::rgb(100, 150, 200);
        let result = with_alpha(c, 128);
        assert_eq!(result.r, 100);
        assert_eq!(result.g, 150);
        assert_eq!(result.b, 200);
        assert_eq!(result.a, 128);
    }

    #[test]
    fn test_contrast_text_on_dark_bg() {
        let dark = Color::rgb(20, 20, 30);
        assert_eq!(contrast_text(dark), Color::WHITE);
    }

    #[test]
    fn test_contrast_text_on_light_bg() {
        let light = Color::rgb(240, 240, 240);
        assert_eq!(contrast_text(light), Color::BLACK);
    }

    #[test]
    fn test_is_dark_black() {
        assert!(is_dark(Color::BLACK));
    }

    #[test]
    fn test_is_dark_white() {
        assert!(!is_dark(Color::WHITE));
    }

    #[test]
    fn test_catppuccin_mocha_is_dark_mode() {
        let theme = Theme::catppuccin_mocha();
        assert_eq!(theme.mode, ThemeMode::Dark);
        assert!(is_dark(theme.background));
    }

    #[test]
    fn test_catppuccin_latte_is_light_mode() {
        let theme = Theme::catppuccin_latte();
        assert_eq!(theme.mode, ThemeMode::Light);
        assert!(!is_dark(theme.background));
    }

    #[test]
    fn test_high_contrast_dark_has_black_bg() {
        let theme = Theme::high_contrast_dark();
        assert_eq!(theme.background, Color::from_hex(0x000000));
        assert_eq!(theme.text_primary, Color::from_hex(0xFFFFFF));
    }

    #[test]
    fn test_high_contrast_light_has_white_bg() {
        let theme = Theme::high_contrast_light();
        assert_eq!(theme.background, Color::from_hex(0xFFFFFF));
        assert_eq!(theme.text_primary, Color::from_hex(0x000000));
    }

    #[test]
    fn test_mocha_colors_correct() {
        let theme = Theme::catppuccin_mocha();
        // Verify key Mocha palette values
        assert_eq!(theme.background, Color::from_hex(0x1E1E2E)); // Base
        assert_eq!(theme.text_primary, Color::from_hex(0xCDD6F4)); // Text
        assert_eq!(theme.primary, Color::from_hex(0x89B4FA)); // Blue
        assert_eq!(theme.error, Color::from_hex(0xF38BA8)); // Red
        assert_eq!(theme.success, Color::from_hex(0xA6E3A1)); // Green
    }

    #[test]
    fn test_latte_colors_correct() {
        let theme = Theme::catppuccin_latte();
        assert_eq!(theme.background, Color::from_hex(0xEFF1F5)); // Base
        assert_eq!(theme.text_primary, Color::from_hex(0x4C4F69)); // Text
        assert_eq!(theme.primary, Color::from_hex(0x1E66F5)); // Blue
        assert_eq!(theme.error, Color::from_hex(0xD20F39)); // Red
        assert_eq!(theme.success, Color::from_hex(0x40A02B)); // Green
    }

    #[test]
    fn test_theme_manager_new() {
        let mgr = ThemeManager::default_dark();
        assert_eq!(mgr.current_theme().name, "Catppuccin Mocha");
    }

    #[test]
    fn test_theme_manager_set_theme_light() {
        let mut mgr = ThemeManager::default_dark();
        assert!(mgr.set_theme(ThemeMode::Light));
        assert_eq!(mgr.current_theme().name, "Catppuccin Latte");
    }

    #[test]
    fn test_theme_manager_set_theme_dark() {
        let mut mgr = ThemeManager::default_light();
        assert!(mgr.set_theme(ThemeMode::Dark));
        assert_eq!(mgr.current_theme().name, "Catppuccin Mocha");
    }

    #[test]
    fn test_theme_manager_custom_theme() {
        let mut mgr = ThemeManager::default_dark();
        let mut custom = Theme::catppuccin_mocha();
        custom.name = String::from("My Theme");
        custom.background = Color::rgb(30, 30, 50);

        mgr.set_custom_theme("my-theme", custom.clone());
        assert!(mgr.set_theme(ThemeMode::Custom("my-theme".to_string())));
        assert_eq!(mgr.current_theme().name, "My Theme");
        assert_eq!(mgr.current_theme().background, Color::rgb(30, 30, 50));
    }

    #[test]
    fn test_theme_manager_missing_custom_returns_false() {
        let mut mgr = ThemeManager::default_dark();
        assert!(!mgr.set_theme(ThemeMode::Custom("nonexistent".to_string())));
        // Theme unchanged
        assert_eq!(mgr.current_theme().name, "Catppuccin Mocha");
    }

    #[test]
    fn test_theme_manager_available_themes() {
        let mut mgr = ThemeManager::default_dark();
        mgr.set_custom_theme("Nord", Theme::catppuccin_mocha());
        mgr.set_custom_theme("Dracula", Theme::catppuccin_mocha());

        let available = mgr.available_themes();
        assert_eq!(available.len(), 4); // Light, Dark, Nord, Dracula
        assert!(available.contains(&ThemeMode::Light));
        assert!(available.contains(&ThemeMode::Dark));
        assert!(available.contains(&ThemeMode::Custom("Nord".to_string())));
        assert!(available.contains(&ThemeMode::Custom("Dracula".to_string())));
    }

    #[test]
    fn test_theme_manager_replace_custom_theme() {
        let mut mgr = ThemeManager::default_dark();
        let mut v1 = Theme::catppuccin_mocha();
        v1.name = String::from("V1");
        let mut v2 = Theme::catppuccin_mocha();
        v2.name = String::from("V2");

        mgr.set_custom_theme("test", v1);
        mgr.set_custom_theme("test", v2);

        // Should still only have one entry
        assert_eq!(mgr.available_themes().len(), 3); // Light, Dark, test
        assert!(mgr.set_theme(ThemeMode::Custom("test".to_string())));
        assert_eq!(mgr.current_theme().name, "V2");
    }

    #[test]
    fn test_theme_manager_on_theme_changed() {
        use core::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;

        let mut mgr = ThemeManager::default_dark();
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        mgr.on_theme_changed(move |_theme| {
            counter_clone.fetch_add(1, Ordering::Relaxed);
        });

        mgr.set_theme(ThemeMode::Light);
        assert_eq!(counter.load(Ordering::Relaxed), 1);

        mgr.set_theme(ThemeMode::Dark);
        assert_eq!(counter.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_theme_colors_button() {
        let theme = Theme::catppuccin_mocha();
        let colors = ThemeColors::new(&theme);
        assert_eq!(colors.button_bg(), theme.primary);
        assert_eq!(colors.button_text(), theme.text_on_primary);
        assert_eq!(colors.button_bg_hover(), theme.primary_hover);
    }

    #[test]
    fn test_theme_colors_hover_state_dark() {
        let theme = Theme::catppuccin_mocha();
        let colors = ThemeColors::new(&theme);
        let base = Color::rgb(100, 100, 100);
        let hovered = colors.hover_state(base);
        // Dark mode lightens on hover
        assert!(hovered.r > base.r);
        assert!(hovered.g > base.g);
        assert!(hovered.b > base.b);
    }

    #[test]
    fn test_theme_colors_hover_state_light() {
        let theme = Theme::catppuccin_latte();
        let colors = ThemeColors::new(&theme);
        let base = Color::rgb(100, 100, 100);
        let hovered = colors.hover_state(base);
        // Light mode darkens on hover
        assert!(hovered.r < base.r);
        assert!(hovered.g < base.g);
        assert!(hovered.b < base.b);
    }

    #[test]
    fn test_theme_mode_display_name() {
        assert_eq!(ThemeMode::Light.display_name(), "Light");
        assert_eq!(ThemeMode::Dark.display_name(), "Dark");
        assert_eq!(
            ThemeMode::Custom("Nord".to_string()).display_name(),
            "Nord"
        );
    }

    #[test]
    fn test_lighten_clamps_above_one() {
        let c = Color::rgb(100, 100, 100);
        let result = lighten(c, 2.0); // Should clamp to 1.0
        assert_eq!(result.r, 255);
    }

    #[test]
    fn test_darken_clamps_below_zero() {
        let c = Color::rgb(100, 100, 100);
        let result = darken(c, -1.0); // Should clamp to 0.0
        assert_eq!(result, c);
    }

    #[test]
    fn test_mix_clamps_ratio() {
        let a = Color::rgb(0, 0, 0);
        let b = Color::rgb(200, 200, 200);
        // ratio > 1.0 should clamp to 1.0
        let result = mix(a, b, 5.0);
        assert_eq!(result, b);
    }
}
