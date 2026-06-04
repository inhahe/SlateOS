//! Appearance and personalization settings panel for the desktop shell.
//!
//! Provides configuration for visual themes, accent colors, font settings,
//! transparency effects, animation preferences, icon size, and cursor
//! appearance — all aspects of the desktop's visual presentation.

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);
const PINK: Color = Color::from_hex(0xF5C2E7);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const ROSEWATER: Color = Color::from_hex(0xF5E0DC);
const FLAMINGO: Color = Color::from_hex(0xF2CDCD);
const MAROON: Color = Color::from_hex(0xEBA0AC);
const SKY: Color = Color::from_hex(0x89DCFE);
const SAPPHIRE: Color = Color::from_hex(0x74C7EC);

// ============================================================================
// Theme mode
// ============================================================================

/// Overall theme brightness mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeMode {
    /// Dark theme (Catppuccin Mocha-based).
    Dark,
    /// Light theme.
    Light,
    /// Follow system schedule (auto-switch between light and dark).
    System,
}

impl ThemeMode {
    fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
            Self::System => "System (Auto)",
        }
    }
}

// ============================================================================
// Accent colors
// ============================================================================

/// Named accent color options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccentColor {
    Blue,
    Lavender,
    Teal,
    Green,
    Yellow,
    Peach,
    Pink,
    Mauve,
    Red,
    Rosewater,
    Flamingo,
    Maroon,
    Sky,
    Sapphire,
    Custom,
}

impl AccentColor {
    fn label(self) -> &'static str {
        match self {
            Self::Blue => "Blue",
            Self::Lavender => "Lavender",
            Self::Teal => "Teal",
            Self::Green => "Green",
            Self::Yellow => "Yellow",
            Self::Peach => "Peach",
            Self::Pink => "Pink",
            Self::Mauve => "Mauve",
            Self::Red => "Red",
            Self::Rosewater => "Rosewater",
            Self::Flamingo => "Flamingo",
            Self::Maroon => "Maroon",
            Self::Sky => "Sky",
            Self::Sapphire => "Sapphire",
            Self::Custom => "Custom",
        }
    }

    /// Get the actual Color for this accent.
    pub fn color(self) -> Color {
        match self {
            Self::Blue => BLUE,
            Self::Lavender => LAVENDER,
            Self::Teal => TEAL,
            Self::Green => GREEN,
            Self::Yellow => YELLOW,
            Self::Peach => PEACH,
            Self::Pink => PINK,
            Self::Mauve => MAUVE,
            Self::Red => RED,
            Self::Rosewater => ROSEWATER,
            Self::Flamingo => FLAMINGO,
            Self::Maroon => MAROON,
            Self::Sky => SKY,
            Self::Sapphire => SAPPHIRE,
            Self::Custom => BLUE, // fallback
        }
    }

    /// All preset (non-custom) accent colors.
    pub fn presets() -> &'static [AccentColor] {
        &[
            Self::Blue, Self::Lavender, Self::Teal, Self::Green,
            Self::Yellow, Self::Peach, Self::Pink, Self::Mauve,
            Self::Red, Self::Rosewater, Self::Flamingo, Self::Maroon,
            Self::Sky, Self::Sapphire,
        ]
    }
}

// ============================================================================
// Transparency / blur effects
// ============================================================================

/// Transparency effect level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransparencyLevel {
    /// No transparency effects — fully opaque surfaces.
    Off,
    /// Subtle transparency on overlays and popups only.
    Subtle,
    /// Moderate transparency on taskbar, menus, and overlays.
    Moderate,
    /// Full transparency with blur effects everywhere.
    Full,
}

impl TransparencyLevel {
    fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Subtle => "Subtle",
            Self::Moderate => "Moderate",
            Self::Full => "Full",
        }
    }

    /// Alpha value (0-255) for panels at this level.
    pub fn panel_alpha(self) -> u8 {
        match self {
            Self::Off => 255,
            Self::Subtle => 230,
            Self::Moderate => 200,
            Self::Full => 160,
        }
    }
}

// ============================================================================
// Animation speed
// ============================================================================

/// Animation speed setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimationSpeed {
    /// No animations — instant transitions.
    Off,
    /// Faster than default (75% duration).
    Fast,
    /// Normal animation speed.
    Normal,
    /// Slower than default (150% duration).
    Slow,
}

impl AnimationSpeed {
    fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Fast => "Fast",
            Self::Normal => "Normal",
            Self::Slow => "Slow",
        }
    }

    /// Multiplier applied to animation durations.
    pub fn multiplier(self) -> f32 {
        match self {
            Self::Off => 0.0,
            Self::Fast => 0.75,
            Self::Normal => 1.0,
            Self::Slow => 1.5,
        }
    }
}

// ============================================================================
// Font settings
// ============================================================================

/// System font configuration.
#[derive(Clone, Debug)]
pub struct FontSettings {
    /// UI font family name.
    pub ui_font: String,
    /// Monospace font family name.
    pub mono_font: String,
    /// Base UI font size in points.
    pub ui_size: f32,
    /// Monospace font size in points.
    pub mono_size: f32,
    /// Whether to use font hinting.
    pub hinting: bool,
    /// Subpixel rendering mode.
    pub subpixel: SubpixelMode,
    /// Font smoothing (antialiasing).
    pub smoothing: bool,
}

impl Default for FontSettings {
    fn default() -> Self {
        Self {
            ui_font: "Inter".to_string(),
            mono_font: "JetBrains Mono".to_string(),
            ui_size: 13.0,
            mono_size: 12.0,
            hinting: true,
            subpixel: SubpixelMode::Rgb,
            smoothing: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubpixelMode {
    /// No subpixel rendering.
    None,
    /// RGB subpixel order (most common LCD).
    Rgb,
    /// BGR subpixel order.
    Bgr,
    /// Vertical RGB.
    VRgb,
    /// Vertical BGR.
    VBgr,
}

impl SubpixelMode {
    fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Rgb => "RGB",
            Self::Bgr => "BGR",
            Self::VRgb => "V-RGB",
            Self::VBgr => "V-BGR",
        }
    }
}

// ============================================================================
// Icon settings
// ============================================================================

/// Desktop icon size preset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconSize {
    Small,
    Medium,
    Large,
    ExtraLarge,
}

impl IconSize {
    fn label(self) -> &'static str {
        match self {
            Self::Small => "Small (32px)",
            Self::Medium => "Medium (48px)",
            Self::Large => "Large (64px)",
            Self::ExtraLarge => "Extra Large (96px)",
        }
    }

    /// Pixel size for this setting.
    pub fn pixels(self) -> u32 {
        match self {
            Self::Small => 32,
            Self::Medium => 48,
            Self::Large => 64,
            Self::ExtraLarge => 96,
        }
    }
}

// ============================================================================
// Cursor settings
// ============================================================================

/// Cursor size preset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorSize {
    Small,
    Normal,
    Large,
    ExtraLarge,
}

impl CursorSize {
    fn label(self) -> &'static str {
        match self {
            Self::Small => "Small (16px)",
            Self::Normal => "Normal (24px)",
            Self::Large => "Large (32px)",
            Self::ExtraLarge => "Extra Large (48px)",
        }
    }

    pub fn pixels(self) -> u32 {
        match self {
            Self::Small => 16,
            Self::Normal => 24,
            Self::Large => 32,
            Self::ExtraLarge => 48,
        }
    }
}

/// Cursor color scheme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorScheme {
    /// Default system cursor (white with black outline).
    Default,
    /// Inverted cursor (black with white outline).
    Inverted,
    /// Accent-colored cursor.
    AccentColored,
}

impl CursorScheme {
    fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Inverted => "Inverted",
            Self::AccentColored => "Accent Color",
        }
    }
}

// ============================================================================
// Window corner style
// ============================================================================

/// Window corner rounding style.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowCorners {
    /// No rounding — square corners.
    Square,
    /// Subtle rounding (4px radius).
    Subtle,
    /// Standard rounding (8px radius).
    Rounded,
    /// Extra rounding (16px radius).
    ExtraRounded,
}

impl WindowCorners {
    fn label(self) -> &'static str {
        match self {
            Self::Square => "Square",
            Self::Subtle => "Subtle",
            Self::Rounded => "Rounded",
            Self::ExtraRounded => "Extra Rounded",
        }
    }

    /// Corner radius in pixels.
    pub fn radius(self) -> f32 {
        match self {
            Self::Square => 0.0,
            Self::Subtle => 4.0,
            Self::Rounded => 8.0,
            Self::ExtraRounded => 16.0,
        }
    }
}

// ============================================================================
// Taskbar style
// ============================================================================

/// Taskbar visual style.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskbarStyle {
    /// Solid background.
    Solid,
    /// Semi-transparent with blur.
    Translucent,
    /// Fully transparent (floating buttons).
    Transparent,
}

impl TaskbarStyle {
    fn label(self) -> &'static str {
        match self {
            Self::Solid => "Solid",
            Self::Translucent => "Translucent",
            Self::Transparent => "Transparent",
        }
    }
}

// ============================================================================
// Appearance settings aggregate
// ============================================================================

/// All appearance/personalization settings.
#[derive(Clone, Debug)]
pub struct AppearanceSettings {
    /// Light/dark/system theme mode.
    pub theme_mode: ThemeMode,
    /// Accent color selection.
    pub accent_color: AccentColor,
    /// Custom accent color (used when accent_color is Custom).
    pub custom_accent: Color,
    /// Transparency/blur effect level.
    pub transparency: TransparencyLevel,
    /// Animation speed.
    pub animation_speed: AnimationSpeed,
    /// Font settings.
    pub fonts: FontSettings,
    /// Desktop icon size.
    pub icon_size: IconSize,
    /// Cursor size.
    pub cursor_size: CursorSize,
    /// Cursor color scheme.
    pub cursor_scheme: CursorScheme,
    /// Window corner style.
    pub window_corners: WindowCorners,
    /// Taskbar visual style.
    pub taskbar_style: TaskbarStyle,
    /// Whether to show accent color on the taskbar.
    pub accent_taskbar: bool,
    /// Whether to show accent color on window title bars.
    pub accent_titlebars: bool,
    /// Whether to show window drop shadows.
    pub drop_shadows: bool,
    /// DPI scaling factor (100 = 100%, 125 = 125%, etc.).
    pub scaling_percent: u16,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme_mode: ThemeMode::Dark,
            accent_color: AccentColor::Blue,
            custom_accent: BLUE,
            transparency: TransparencyLevel::Moderate,
            animation_speed: AnimationSpeed::Normal,
            fonts: FontSettings::default(),
            icon_size: IconSize::Medium,
            cursor_size: CursorSize::Normal,
            cursor_scheme: CursorScheme::Default,
            window_corners: WindowCorners::Rounded,
            taskbar_style: TaskbarStyle::Translucent,
            accent_taskbar: false,
            accent_titlebars: false,
            drop_shadows: true,
            scaling_percent: 100,
        }
    }
}

impl AppearanceSettings {
    /// Get the effective accent Color, resolving Custom if needed.
    pub fn effective_accent(&self) -> Color {
        if self.accent_color == AccentColor::Custom {
            self.custom_accent
        } else {
            self.accent_color.color()
        }
    }

    /// Get DPI scale factor as a float (e.g. 1.0, 1.25, 1.5).
    pub fn scale_factor(&self) -> f32 {
        self.scaling_percent as f32 / 100.0
    }

    /// Whether any animations are enabled.
    pub fn animations_enabled(&self) -> bool {
        self.animation_speed != AnimationSpeed::Off
    }

    /// Whether transparency effects are enabled.
    pub fn transparency_enabled(&self) -> bool {
        self.transparency != TransparencyLevel::Off
    }

    /// Get the effective window corner radius.
    pub fn corner_radius(&self) -> f32 {
        self.window_corners.radius()
    }

    /// Validate and clamp settings to sane ranges.
    pub fn validate(&mut self) {
        // Clamp font sizes
        if self.fonts.ui_size < 8.0 {
            self.fonts.ui_size = 8.0;
        }
        if self.fonts.ui_size > 32.0 {
            self.fonts.ui_size = 32.0;
        }
        if self.fonts.mono_size < 6.0 {
            self.fonts.mono_size = 6.0;
        }
        if self.fonts.mono_size > 32.0 {
            self.fonts.mono_size = 32.0;
        }
        // Clamp scaling
        if self.scaling_percent < 100 {
            self.scaling_percent = 100;
        }
        if self.scaling_percent > 300 {
            self.scaling_percent = 300;
        }
    }
}

// ============================================================================
// UI: Appearance settings panel
// ============================================================================

/// Active tab in the appearance settings UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppearanceTab {
    /// Theme mode, accent color, transparency.
    Theme,
    /// Font settings.
    Fonts,
    /// Effects: animations, shadows, corners, taskbar style.
    Effects,
    /// Cursor and icon settings.
    CursorsIcons,
}

impl AppearanceTab {
    fn label(self) -> &'static str {
        match self {
            Self::Theme => "Theme",
            Self::Fonts => "Fonts",
            Self::Effects => "Effects",
            Self::CursorsIcons => "Cursors & Icons",
        }
    }
}

/// Appearance settings UI state.
pub struct AppearanceSettingsUI {
    /// Active tab.
    pub active_tab: AppearanceTab,
    /// The settings being edited.
    pub settings: AppearanceSettings,
    /// Saved settings for revert/dirty detection.
    saved: AppearanceSettings,
}

impl AppearanceSettingsUI {
    pub fn new() -> Self {
        let settings = AppearanceSettings::default();
        Self {
            active_tab: AppearanceTab::Theme,
            saved: settings.clone(),
            settings,
        }
    }

    /// Whether settings have been changed from the saved state.
    pub fn is_dirty(&self) -> bool {
        // Compare key fields — full eq is tedious, so check the important ones
        self.settings.theme_mode != self.saved.theme_mode
            || self.settings.accent_color != self.saved.accent_color
            || self.settings.transparency != self.saved.transparency
            || self.settings.animation_speed != self.saved.animation_speed
            || self.settings.icon_size != self.saved.icon_size
            || self.settings.cursor_size != self.saved.cursor_size
            || self.settings.window_corners != self.saved.window_corners
            || self.settings.taskbar_style != self.saved.taskbar_style
            || self.settings.accent_taskbar != self.saved.accent_taskbar
            || self.settings.accent_titlebars != self.saved.accent_titlebars
            || self.settings.drop_shadows != self.saved.drop_shadows
            || self.settings.scaling_percent != self.saved.scaling_percent
            || (self.settings.fonts.ui_size - self.saved.fonts.ui_size).abs() > 0.1
            || (self.settings.fonts.mono_size - self.saved.fonts.mono_size).abs() > 0.1
            || self.settings.fonts.ui_font != self.saved.fonts.ui_font
            || self.settings.fonts.hinting != self.saved.fonts.hinting
            || self.settings.cursor_scheme != self.saved.cursor_scheme
    }

    /// Save current settings (marks as clean).
    pub fn save(&mut self) {
        self.saved = self.settings.clone();
    }

    /// Revert to saved settings.
    pub fn revert(&mut self) {
        self.settings = self.saved.clone();
    }

    /// Switch tabs.
    pub fn set_tab(&mut self, tab: AppearanceTab) {
        self.active_tab = tab;
    }

    /// Render the appearance settings panel.
    pub fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Panel background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: BASE,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: 24.0,
            y: 24.0,
            text: "Appearance".into(),
            font_size: 22.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 48.0),
        });

        // Dirty indicator
        if self.is_dirty() {
            cmds.push(RenderCommand::FillRect {
                x: width - 100.0,
                y: 22.0,
                width: 76.0,
                height: 24.0,
                color: YELLOW,
                corner_radii: CornerRadii::all(12.0),
            });
            cmds.push(RenderCommand::Text {
                x: width - 92.0,
                y: 26.0,
                text: "Unsaved".into(),
                font_size: 12.0,
                color: CRUST,
                font_weight: FontWeightHint::Bold,
                max_width: Some(64.0),
            });
        }

        // Tab bar
        let tabs = [
            AppearanceTab::Theme,
            AppearanceTab::Fonts,
            AppearanceTab::Effects,
            AppearanceTab::CursorsIcons,
        ];
        let tab_y = 60.0;

        let mut tx = 24.0;
        for &tab in &tabs {
            let active = tab == self.active_tab;
            let tab_w = tab.label().len() as f32 * 8.0 + 20.0;

            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: tab_y,
                width: tab_w,
                height: 32.0,
                color: if active { BLUE } else { SURFACE0 },
                corner_radii: CornerRadii::all(6.0),
            });

            cmds.push(RenderCommand::Text {
                x: tx + 10.0,
                y: tab_y + 8.0,
                text: tab.label().into(),
                font_size: 13.0,
                color: if active { CRUST } else { SUBTEXT0 },
                font_weight: if active { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(tab_w - 20.0),
            });

            tx += tab_w + 8.0;
        }

        let content_y = tab_y + 48.0;
        let cw = width - 48.0;

        match self.active_tab {
            AppearanceTab::Theme => self.render_theme_tab(&mut cmds, 24.0, content_y, cw),
            AppearanceTab::Fonts => self.render_fonts_tab(&mut cmds, 24.0, content_y, cw),
            AppearanceTab::Effects => self.render_effects_tab(&mut cmds, 24.0, content_y, cw),
            AppearanceTab::CursorsIcons => self.render_cursors_tab(&mut cmds, 24.0, content_y, cw),
        }

        cmds
    }

    fn render_theme_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut cy = y;

        // Theme mode
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Theme Mode".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 26.0;

        let modes = [ThemeMode::Dark, ThemeMode::Light, ThemeMode::System];
        for mode in &modes {
            let selected = *mode == self.settings.theme_mode;
            cmds.push(RenderCommand::FillRect {
                x,
                y: cy,
                width: 160.0,
                height: 30.0,
                color: if selected { SURFACE1 } else { SURFACE0 },
                corner_radii: CornerRadii::all(6.0),
            });
            if selected {
                cmds.push(RenderCommand::StrokeRect {
                    x,
                    y: cy,
                    width: 160.0,
                    height: 30.0,
                    color: BLUE,
                    corner_radii: CornerRadii::all(6.0),
                    line_width: 2.0,
                });
            }
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: cy + 8.0,
                text: mode.label().into(),
                font_size: 13.0,
                color: if selected { TEXT } else { SUBTEXT0 },
                font_weight: FontWeightHint::Regular,
                max_width: Some(140.0),
            });
            cy += 36.0;
        }

        cy += 8.0;

        // Accent color
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Accent Color".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 26.0;

        // Color swatches in a grid
        let swatch_size = 28.0;
        let swatch_gap = 8.0;
        let cols = 7;
        for (i, accent) in AccentColor::presets().iter().enumerate() {
            let col = i % cols;
            let row = i / cols;
            let sx = x + (col as f32) * (swatch_size + swatch_gap);
            let sy = cy + (row as f32) * (swatch_size + swatch_gap);

            cmds.push(RenderCommand::FillRect {
                x: sx,
                y: sy,
                width: swatch_size,
                height: swatch_size,
                color: accent.color(),
                corner_radii: CornerRadii::all(swatch_size / 2.0),
            });

            if *accent == self.settings.accent_color {
                cmds.push(RenderCommand::StrokeRect {
                    x: sx - 3.0,
                    y: sy - 3.0,
                    width: swatch_size + 6.0,
                    height: swatch_size + 6.0,
                    color: TEXT,
                    corner_radii: CornerRadii::all((swatch_size + 6.0) / 2.0),
                    line_width: 2.0,
                });
            }
        }

        let rows = AccentColor::presets().len().div_ceil(cols);
        cy += (rows as f32) * (swatch_size + swatch_gap) + 16.0;

        // Current accent display
        let accent = self.settings.effective_accent();
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: format!(
                "Current: {} (#{:02X}{:02X}{:02X})",
                self.settings.accent_color.label(), accent.r, accent.g, accent.b,
            ),
            font_size: 12.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width),
        });
        cy += 32.0;

        // Transparency level
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Transparency".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 26.0;

        self.render_label_value(cmds, x, cy, width, "Level", self.settings.transparency.label());
        cy += 28.0;

        // Transparency preview bar
        let alpha = self.settings.transparency.panel_alpha();
        cmds.push(RenderCommand::FillRect {
            x,
            y: cy,
            width,
            height: 40.0,
            color: Color::rgba(SURFACE0.r, SURFACE0.g, SURFACE0.b, alpha),
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: cy + 12.0,
            text: format!("Panel preview (alpha: {})", alpha),
            font_size: 13.0,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 24.0),
        });
        cy += 52.0;

        // Scaling
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Display Scaling".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 26.0;

        self.render_label_value(
            cmds, x, cy, width,
            "Scale",
            &format!("{}%", self.settings.scaling_percent),
        );
        let _ = cy;
    }

    fn render_fonts_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut cy = y;
        let fonts = &self.settings.fonts;

        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "System Font".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 26.0;

        self.render_label_value(cmds, x, cy, width, "Family", &fonts.ui_font);
        cy += 28.0;
        self.render_label_value(cmds, x, cy, width, "Size", &format!("{:.0}pt", fonts.ui_size));
        cy += 36.0;

        // Font preview
        cmds.push(RenderCommand::FillRect {
            x,
            y: cy,
            width,
            height: 48.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: cy + 8.0,
            text: "The quick brown fox jumps over the lazy dog".into(),
            font_size: fonts.ui_size,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 24.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: cy + 28.0,
            text: "ABCDEFGHIJKLM 0123456789".into(),
            font_size: fonts.ui_size,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 24.0),
        });
        cy += 60.0;

        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Monospace Font".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 26.0;

        self.render_label_value(cmds, x, cy, width, "Family", &fonts.mono_font);
        cy += 28.0;
        self.render_label_value(cmds, x, cy, width, "Size", &format!("{:.0}pt", fonts.mono_size));
        cy += 36.0;

        // Mono preview
        cmds.push(RenderCommand::FillRect {
            x,
            y: cy,
            width,
            height: 36.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: cy + 10.0,
            text: "fn main() { println!(\"Hello\"); }".into(),
            font_size: fonts.mono_size,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 24.0),
        });
        cy += 48.0;

        // Rendering settings
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Rendering".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 26.0;

        self.render_toggle_row(cmds, x, cy, width, "Font Hinting", fonts.hinting);
        cy += 32.0;
        self.render_toggle_row(cmds, x, cy, width, "Font Smoothing", fonts.smoothing);
        cy += 32.0;
        self.render_label_value(cmds, x, cy, width, "Subpixel", fonts.subpixel.label());
        let _ = cy;
    }

    fn render_effects_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut cy = y;

        // Animations
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Animations".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 26.0;

        self.render_label_value(cmds, x, cy, width, "Speed", self.settings.animation_speed.label());
        cy += 28.0;
        self.render_label_value(
            cmds, x, cy, width,
            "Multiplier",
            &format!("{:.2}x", self.settings.animation_speed.multiplier()),
        );
        cy += 36.0;

        // Window corners
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Window Corners".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 26.0;

        let corner_styles = [
            WindowCorners::Square,
            WindowCorners::Subtle,
            WindowCorners::Rounded,
            WindowCorners::ExtraRounded,
        ];
        for style in &corner_styles {
            let selected = *style == self.settings.window_corners;
            let preview_w = 50.0;
            cmds.push(RenderCommand::FillRect {
                x,
                y: cy,
                width: preview_w,
                height: 30.0,
                color: if selected { SURFACE1 } else { SURFACE0 },
                corner_radii: CornerRadii::all(style.radius()),
            });
            if selected {
                cmds.push(RenderCommand::StrokeRect {
                    x,
                    y: cy,
                    width: preview_w,
                    height: 30.0,
                    color: BLUE,
                    corner_radii: CornerRadii::all(style.radius()),
                    line_width: 2.0,
                });
            }
            cmds.push(RenderCommand::Text {
                x: x + preview_w + 12.0,
                y: cy + 8.0,
                text: style.label().into(),
                font_size: 13.0,
                color: if selected { TEXT } else { SUBTEXT0 },
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - preview_w - 24.0),
            });
            cy += 38.0;
        }

        cy += 8.0;

        // Taskbar style
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Taskbar Style".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 26.0;

        self.render_label_value(cmds, x, cy, width, "Style", self.settings.taskbar_style.label());
        cy += 36.0;

        // Toggle switches
        self.render_toggle_row(cmds, x, cy, width, "Accent on Taskbar", self.settings.accent_taskbar);
        cy += 32.0;
        self.render_toggle_row(cmds, x, cy, width, "Accent on Title Bars", self.settings.accent_titlebars);
        cy += 32.0;
        self.render_toggle_row(cmds, x, cy, width, "Drop Shadows", self.settings.drop_shadows);
        let _ = cy;
    }

    fn render_cursors_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut cy = y;

        // Cursor settings
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Cursor".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 26.0;

        self.render_label_value(cmds, x, cy, width, "Size", self.settings.cursor_size.label());
        cy += 28.0;
        self.render_label_value(cmds, x, cy, width, "Scheme", self.settings.cursor_scheme.label());
        cy += 36.0;

        // Cursor preview
        let cursor_px = self.settings.cursor_size.pixels() as f32;
        cmds.push(RenderCommand::FillRect {
            x,
            y: cy,
            width: cursor_px + 20.0,
            height: cursor_px + 20.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });
        // Simple arrow cursor approximation
        cmds.push(RenderCommand::FillRect {
            x: x + 10.0,
            y: cy + 10.0,
            width: cursor_px * 0.4,
            height: cursor_px,
            color: TEXT,
            corner_radii: CornerRadii::ZERO,
        });
        cy += cursor_px + 32.0;

        // Icon settings
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Desktop Icons".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 26.0;

        self.render_label_value(cmds, x, cy, width, "Size", self.settings.icon_size.label());
        cy += 28.0;

        // Icon size preview
        let icon_px = self.settings.icon_size.pixels() as f32;
        let preview_items = ["Documents", "Downloads", "Pictures"];
        for (i, name) in preview_items.iter().enumerate() {
            let ix = x + (i as f32) * (icon_px + 24.0);
            cmds.push(RenderCommand::FillRect {
                x: ix,
                y: cy,
                width: icon_px,
                height: icon_px,
                color: SURFACE1,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: ix,
                y: cy + icon_px + 4.0,
                text: (*name).into(),
                font_size: 10.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(icon_px),
            });
        }
        let _ = cy;
    }

    // ---- Render helpers ----

    fn render_toggle_row(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        label: &str,
        enabled: bool,
    ) {
        cmds.push(RenderCommand::Text {
            x,
            y: y + 4.0,
            text: label.into(),
            font_size: 14.0,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 80.0),
        });

        let sw_x = x + width - 44.0;
        cmds.push(RenderCommand::FillRect {
            x: sw_x,
            y: y + 2.0,
            width: 40.0,
            height: 22.0,
            color: if enabled { GREEN } else { SURFACE2 },
            corner_radii: CornerRadii::all(11.0),
        });
        let knob_x = if enabled { sw_x + 20.0 } else { sw_x + 2.0 };
        cmds.push(RenderCommand::FillRect {
            x: knob_x,
            y: y + 4.0,
            width: 18.0,
            height: 18.0,
            color: TEXT,
            corner_radii: CornerRadii::all(9.0),
        });
    }

    fn render_label_value(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        label: &str,
        value: &str,
    ) {
        cmds.push(RenderCommand::Text {
            x,
            y,
            text: label.into(),
            font_size: 13.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.5),
        });
        cmds.push(RenderCommand::Text {
            x: x + width * 0.55,
            y,
            text: value.into(),
            font_size: 13.0,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.45),
        });
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- ThemeMode ----

    #[test]
    fn test_theme_mode_labels() {
        assert_eq!(ThemeMode::Dark.label(), "Dark");
        assert_eq!(ThemeMode::Light.label(), "Light");
        assert_eq!(ThemeMode::System.label(), "System (Auto)");
    }

    // ---- AccentColor ----

    #[test]
    fn test_accent_color_count() {
        assert_eq!(AccentColor::presets().len(), 14);
    }

    #[test]
    fn test_accent_color_labels() {
        assert_eq!(AccentColor::Blue.label(), "Blue");
        assert_eq!(AccentColor::Custom.label(), "Custom");
    }

    #[test]
    fn test_accent_color_values() {
        let c = AccentColor::Blue.color();
        assert_eq!(c.r, BLUE.r);
        assert_eq!(c.g, BLUE.g);
        assert_eq!(c.b, BLUE.b);
    }

    #[test]
    fn test_accent_custom_fallback() {
        let c = AccentColor::Custom.color();
        assert_eq!(c.r, BLUE.r);
    }

    // ---- TransparencyLevel ----

    #[test]
    fn test_transparency_labels() {
        assert_eq!(TransparencyLevel::Off.label(), "Off");
        assert_eq!(TransparencyLevel::Full.label(), "Full");
    }

    #[test]
    fn test_transparency_alpha() {
        assert_eq!(TransparencyLevel::Off.panel_alpha(), 255);
        assert_eq!(TransparencyLevel::Full.panel_alpha(), 160);
        assert!(TransparencyLevel::Moderate.panel_alpha() > TransparencyLevel::Full.panel_alpha());
    }

    // ---- AnimationSpeed ----

    #[test]
    fn test_animation_speed_multipliers() {
        assert_eq!(AnimationSpeed::Off.multiplier(), 0.0);
        assert_eq!(AnimationSpeed::Normal.multiplier(), 1.0);
        assert!(AnimationSpeed::Fast.multiplier() < AnimationSpeed::Normal.multiplier());
        assert!(AnimationSpeed::Slow.multiplier() > AnimationSpeed::Normal.multiplier());
    }

    // ---- FontSettings ----

    #[test]
    fn test_font_settings_default() {
        let f = FontSettings::default();
        assert_eq!(f.ui_font, "Inter");
        assert_eq!(f.mono_font, "JetBrains Mono");
        assert!(f.hinting);
        assert!(f.smoothing);
    }

    // ---- SubpixelMode ----

    #[test]
    fn test_subpixel_labels() {
        assert_eq!(SubpixelMode::Rgb.label(), "RGB");
        assert_eq!(SubpixelMode::None.label(), "None");
    }

    // ---- IconSize ----

    #[test]
    fn test_icon_size_pixels() {
        assert_eq!(IconSize::Small.pixels(), 32);
        assert_eq!(IconSize::Medium.pixels(), 48);
        assert_eq!(IconSize::Large.pixels(), 64);
        assert_eq!(IconSize::ExtraLarge.pixels(), 96);
    }

    // ---- CursorSize ----

    #[test]
    fn test_cursor_size_pixels() {
        assert_eq!(CursorSize::Small.pixels(), 16);
        assert_eq!(CursorSize::Normal.pixels(), 24);
        assert_eq!(CursorSize::Large.pixels(), 32);
    }

    #[test]
    fn test_cursor_scheme_labels() {
        assert_eq!(CursorScheme::Default.label(), "Default");
        assert_eq!(CursorScheme::AccentColored.label(), "Accent Color");
    }

    // ---- WindowCorners ----

    #[test]
    fn test_window_corners_radius() {
        assert_eq!(WindowCorners::Square.radius(), 0.0);
        assert_eq!(WindowCorners::Subtle.radius(), 4.0);
        assert_eq!(WindowCorners::Rounded.radius(), 8.0);
        assert_eq!(WindowCorners::ExtraRounded.radius(), 16.0);
    }

    // ---- TaskbarStyle ----

    #[test]
    fn test_taskbar_style_labels() {
        assert_eq!(TaskbarStyle::Solid.label(), "Solid");
        assert_eq!(TaskbarStyle::Translucent.label(), "Translucent");
        assert_eq!(TaskbarStyle::Transparent.label(), "Transparent");
    }

    // ---- AppearanceSettings ----

    #[test]
    fn test_settings_default() {
        let s = AppearanceSettings::default();
        assert_eq!(s.theme_mode, ThemeMode::Dark);
        assert_eq!(s.accent_color, AccentColor::Blue);
        assert_eq!(s.transparency, TransparencyLevel::Moderate);
        assert_eq!(s.animation_speed, AnimationSpeed::Normal);
        assert_eq!(s.scaling_percent, 100);
        assert!(s.drop_shadows);
    }

    #[test]
    fn test_effective_accent_preset() {
        let s = AppearanceSettings::default();
        let c = s.effective_accent();
        assert_eq!(c.r, BLUE.r);
    }

    #[test]
    fn test_effective_accent_custom() {
        let mut s = AppearanceSettings::default();
        s.accent_color = AccentColor::Custom;
        s.custom_accent = Color::rgb(255, 0, 0);
        let c = s.effective_accent();
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 0);
    }

    #[test]
    fn test_scale_factor() {
        let mut s = AppearanceSettings::default();
        assert!((s.scale_factor() - 1.0).abs() < 0.01);
        s.scaling_percent = 150;
        assert!((s.scale_factor() - 1.5).abs() < 0.01);
    }

    #[test]
    fn test_animations_enabled() {
        let mut s = AppearanceSettings::default();
        assert!(s.animations_enabled());
        s.animation_speed = AnimationSpeed::Off;
        assert!(!s.animations_enabled());
    }

    #[test]
    fn test_transparency_enabled() {
        let mut s = AppearanceSettings::default();
        assert!(s.transparency_enabled());
        s.transparency = TransparencyLevel::Off;
        assert!(!s.transparency_enabled());
    }

    #[test]
    fn test_corner_radius() {
        let s = AppearanceSettings::default();
        assert_eq!(s.corner_radius(), 8.0);
    }

    #[test]
    fn test_validate_clamp_font_sizes() {
        let mut s = AppearanceSettings::default();
        s.fonts.ui_size = 2.0;
        s.fonts.mono_size = 50.0;
        s.scaling_percent = 50;
        s.validate();
        assert_eq!(s.fonts.ui_size, 8.0);
        assert_eq!(s.fonts.mono_size, 32.0);
        assert_eq!(s.scaling_percent, 100);
    }

    #[test]
    fn test_validate_clamp_scaling_high() {
        let mut s = AppearanceSettings::default();
        s.scaling_percent = 500;
        s.validate();
        assert_eq!(s.scaling_percent, 300);
    }

    // ---- AppearanceSettingsUI ----

    #[test]
    fn test_ui_new() {
        let ui = AppearanceSettingsUI::new();
        assert_eq!(ui.active_tab, AppearanceTab::Theme);
        assert!(!ui.is_dirty());
    }

    #[test]
    fn test_ui_dirty_detection() {
        let mut ui = AppearanceSettingsUI::new();
        assert!(!ui.is_dirty());
        ui.settings.theme_mode = ThemeMode::Light;
        assert!(ui.is_dirty());
    }

    #[test]
    fn test_ui_save() {
        let mut ui = AppearanceSettingsUI::new();
        ui.settings.accent_color = AccentColor::Red;
        assert!(ui.is_dirty());
        ui.save();
        assert!(!ui.is_dirty());
    }

    #[test]
    fn test_ui_revert() {
        let mut ui = AppearanceSettingsUI::new();
        ui.settings.theme_mode = ThemeMode::Light;
        ui.settings.accent_color = AccentColor::Green;
        assert!(ui.is_dirty());
        ui.revert();
        assert!(!ui.is_dirty());
        assert_eq!(ui.settings.theme_mode, ThemeMode::Dark);
        assert_eq!(ui.settings.accent_color, AccentColor::Blue);
    }

    #[test]
    fn test_ui_set_tab() {
        let mut ui = AppearanceSettingsUI::new();
        ui.set_tab(AppearanceTab::Fonts);
        assert_eq!(ui.active_tab, AppearanceTab::Fonts);
    }

    #[test]
    fn test_ui_render_theme_tab() {
        let ui = AppearanceSettingsUI::new();
        let cmds = ui.render(600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_fonts_tab() {
        let mut ui = AppearanceSettingsUI::new();
        ui.set_tab(AppearanceTab::Fonts);
        let cmds = ui.render(600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_effects_tab() {
        let mut ui = AppearanceSettingsUI::new();
        ui.set_tab(AppearanceTab::Effects);
        let cmds = ui.render(600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_cursors_tab() {
        let mut ui = AppearanceSettingsUI::new();
        ui.set_tab(AppearanceTab::CursorsIcons);
        let cmds = ui.render(600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_with_dirty() {
        let mut ui = AppearanceSettingsUI::new();
        ui.settings.theme_mode = ThemeMode::Light;
        let cmds = ui.render(600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_dirty_font_change() {
        let mut ui = AppearanceSettingsUI::new();
        ui.settings.fonts.ui_size = 16.0;
        assert!(ui.is_dirty());
    }

    #[test]
    fn test_ui_dirty_cursor_change() {
        let mut ui = AppearanceSettingsUI::new();
        ui.settings.cursor_scheme = CursorScheme::Inverted;
        assert!(ui.is_dirty());
    }

    // ---- Tab labels ----

    #[test]
    fn test_tab_labels() {
        assert_eq!(AppearanceTab::Theme.label(), "Theme");
        assert_eq!(AppearanceTab::Fonts.label(), "Fonts");
        assert_eq!(AppearanceTab::Effects.label(), "Effects");
        assert_eq!(AppearanceTab::CursorsIcons.label(), "Cursors & Icons");
    }
}
