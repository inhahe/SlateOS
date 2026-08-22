//! The desktop's appearance settings: the model, not the panel.
//!
//! One user preference cannot have two owners. The shell paints from these
//! values, the Settings application edits them, and both read and write the
//! same `appearance.yaml` — so the enums, their configuration-file spellings,
//! the file's location and the way it is replaced all have to be one
//! definition rather than a copy in each process. A copy is not a style
//! problem: two crates that disagree about the order of an accent list, or
//! about whether the file is written in place, corrupt the user's settings
//! between them. See known-issues.md TD-THREE-INDEPENDENT-APPEARANCE-MODELS.
//!
//! What deliberately stays out: rendering. This crate knows nothing about
//! widgets, tabs or hit-testing — the panel that edits these values lives in
//! the desktop shell, and a second front end could edit them without pulling
//! any of it in. It also does not decide what the *shell* looks like; that is
//! `DesktopTheme::from_settings`, which derives a palette from these choices.
//!
//! [`config`] is part of the contract for the same reason the schema is: two
//! processes writing one file must agree not only on what the keys mean but
//! on which file it is and how it is replaced.

/// Where settings files live and how they are replaced.
///
/// This was `appearance::config` before it was a crate of its own, and it is
/// re-exported under the old name because the path is used across the shell,
/// the compositor and the Settings application, and none of those call sites
/// were wrong. See `settingsfile`'s own documentation for why it moved: it is
/// not about appearance, and `inputsettings` needs it without needing colours.
pub use settingsfile as config;

use guitk::color::Color;
use guitk::theme::with_alpha;
use yamldoc::Document;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

pub const BASE: Color = Color::from_hex(0x1E1E2E);
pub const MANTLE: Color = Color::from_hex(0x181825);
pub const CRUST: Color = Color::from_hex(0x11111B);
pub const SURFACE0: Color = Color::from_hex(0x313244);
pub const SURFACE1: Color = Color::from_hex(0x45475A);
pub const SURFACE2: Color = Color::from_hex(0x585B70);
pub const TEXT: Color = Color::from_hex(0xCDD6F4);
pub const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
pub const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
pub const BLUE: Color = Color::from_hex(0x89B4FA);
pub const GREEN: Color = Color::from_hex(0xA6E3A1);
pub const RED: Color = Color::from_hex(0xF38BA8);
pub const YELLOW: Color = Color::from_hex(0xF9E2AF);
pub const PEACH: Color = Color::from_hex(0xFAB387);
pub const LAVENDER: Color = Color::from_hex(0xB4BEFE);
pub const OVERLAY0: Color = Color::from_hex(0x6C7086);
pub const TEAL: Color = Color::from_hex(0x94E2D5);
pub const PINK: Color = Color::from_hex(0xF5C2E7);
pub const MAUVE: Color = Color::from_hex(0xCBA6F7);
pub const ROSEWATER: Color = Color::from_hex(0xF5E0DC);
pub const FLAMINGO: Color = Color::from_hex(0xF2CDCD);
pub const MAROON: Color = Color::from_hex(0xEBA0AC);
// `0x89DCEB`, not `0x89DCFE`. This carried a transposed byte pair from the day
// it was written, so `AccentColor::Sky.color()` returned a colour Catppuccin
// does not contain — and, being the crate that owns the answer, it had already
// propagated into `apps/alarmclock` and `apps/emojipicker`. Found by comparing
// every dark constant here against the published Mocha palette; it was the
// only mismatch, which is why a copy that agrees with 2,000 others is not
// evidence of anything. See known-issues.md
// TD-C-EVERY-APPLICATION-CARRIES-ITS-OWN-COPY-OF-THE-PALETTE-TOO.
pub const SKY: Color = Color::from_hex(0x89DCEB);
pub const SAPPHIRE: Color = Color::from_hex(0x74C7EC);

// ============================================================================
// Light-background accents — Catppuccin Latte hues, darkened to read as text
// ============================================================================
//
// An accent is not one colour, it is a *role*: "the hue this desktop is themed
// around". Mocha's accents are pastels tuned to sit on a near-black base, and
// reusing them on a near-white one is not a stylistic compromise but an
// unreadable result — Mocha blue `#89B4FA` on Latte base `#EFF1F5` is a
// contrast ratio of about 1.9:1, against the 4.5:1 that body text needs.
//
// Catppuccin's own Latte accents are the right hues but are still published
// for *decoration*, and measured against the Latte base most of them do not
// carry text either: yellow 2.31:1, pink 2.34:1, rosewater 2.34:1, sky 2.47:1,
// lavender 2.81:1 — only blue, mauve and red clear 4.5:1 unaided. The shell
// draws the accent as text (the start glyph, the start-menu heading), so each
// value below is its Latte hue scaled toward black by the smallest factor that
// reaches 4.6:1 on `#EFF1F5`. Scaling all three channels together holds the
// hue, so these still read as the colours Catppuccin named; blue, mauve and
// red are barely touched because they already passed.
//
// The dark palette needs no such treatment — every Mocha accent is already
// between 7:1 and 13:1 on the Mocha base.

pub const LIGHT_BLUE: Color = Color::from_hex(0x1D62EC);
pub const LIGHT_LAVENDER: Color = Color::from_hex(0x5565BE);
pub const LIGHT_TEAL: Color = Color::from_hex(0x13787E);
pub const LIGHT_GREEN: Color = Color::from_hex(0x317B21);
pub const LIGHT_YELLOW: Color = Color::from_hex(0x976014);
pub const LIGHT_PEACH: Color = Color::from_hex(0xB94908);
pub const LIGHT_PINK: Color = Color::from_hex(0x9F508A);
pub const LIGHT_MAUVE: Color = Color::from_hex(0x8839EF);
pub const LIGHT_RED: Color = Color::from_hex(0xD20F39);
pub const LIGHT_ROSEWATER: Color = Color::from_hex(0x965E52);
pub const LIGHT_FLAMINGO: Color = Color::from_hex(0xA05757);
pub const LIGHT_MAROON: Color = Color::from_hex(0xC33B47);
pub const LIGHT_SKY: Color = Color::from_hex(0x0374A1);
pub const LIGHT_SAPPHIRE: Color = Color::from_hex(0x187788);

// ============================================================================
// Catppuccin Latte surface ladder — the light mode's backgrounds and text
// ============================================================================
//
// Role for role these are the counterparts of the Mocha constants above, and
// they are ordered the same way: `crust` is the layer *behind* the window,
// `base` is the window, `surface0`–`surface2` are things raised off it, and
// `overlay0`, `subtext0`, `subtext1`, `text` are marks drawn on it in
// increasing prominence. Only the direction reverses — in Mocha "raised" means
// lighter, in Latte it means darker — which is precisely why a renderer must
// name the role rather than the colour. Code that says `SURFACE1` keeps
// working when the mode flips; code that says `0x45475A` does not.
//
// These are Catppuccin's published Latte values with one exception, marked
// below. The values are also, individually, already in the tree: they are what
// `DecorationColors::for_mode(true)` and `DesktopTheme::light()` were each
// spelling out in hex. Naming them here is what lets those stop.

pub const LIGHT_CRUST: Color = Color::from_hex(0xDCE0E8);
pub const LIGHT_MANTLE: Color = Color::from_hex(0xE6E9EF);
pub const LIGHT_BASE: Color = Color::from_hex(0xEFF1F5);
pub const LIGHT_SURFACE0: Color = Color::from_hex(0xCCD0DA);
pub const LIGHT_SURFACE1: Color = Color::from_hex(0xBCC0CC);
pub const LIGHT_SURFACE2: Color = Color::from_hex(0xACB0BE);
pub const LIGHT_OVERLAY0: Color = Color::from_hex(0x9CA0B0);

/// Latte `subtext0`, darkened by the same rule as the light accents above.
///
/// Catppuccin's own value is `#6C6F85`, which measures 4.37:1 on the Latte
/// base — under the 4.5:1 that body text needs, and this role *is* body text:
/// it is the secondary line in every list row the shell draws. The Mocha
/// counterpart is 7.37:1, so the shortfall belongs to the light palette alone
/// and fixing it there costs nothing elsewhere.
///
/// Scaled to 96.5% of each channel, which holds the hue and reaches 4.64:1.
/// The difference is not visible side by side; what it buys is that the
/// contrast invariant in this crate's tests can be stated as a flat rule for
/// both modes instead of carrying an exception, and an exception in a
/// legibility floor is how the floor stops being one.
pub const LIGHT_SUBTEXT0: Color = Color::from_hex(0x686B80);

pub const LIGHT_SUBTEXT1: Color = Color::from_hex(0x5C5F77);
pub const LIGHT_TEXT: Color = Color::from_hex(0x4C4F69);

// ============================================================================
// Configuration-file spellings
// ============================================================================

// The macro lives in `settingsfile` rather than here: `inputsettings` needs the
// same thing, and a macro copied between two settings crates is two file
// formats waiting to disagree about how a name is spelled.
use settingsfile::yaml_enum;

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
    /// Every mode, in the order a chooser should offer them.
    ///
    /// The order is part of the shared model deliberately: the shell's
    /// appearance panel and the Settings application both draw this as a row
    /// of cards, and two front ends that listed the variants themselves would
    /// be free to drift apart — the same preference in two places, offered in
    /// two orders.
    pub const ALL: &'static [Self] = &[Self::Dark, Self::Light, Self::System];

    pub fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
            Self::System => "System (Auto)",
        }
    }

    /// Whether this mode paints a light palette *right now*.
    ///
    /// [`System`](Self::System) is the interesting case: it means "follow the
    /// system's light/dark schedule", and this desktop has no such schedule
    /// yet — nothing computes sunrise, and nothing watches a time-of-day
    /// trigger. Until something does, `System` answers dark, because dark is
    /// what the shell has always painted and what every other default in
    /// [`AppearanceSettings`] is tuned against; answering light would flip the
    /// whole desktop for a user who asked only to be left on automatic.
    ///
    /// When the schedule exists, this is the one place that has to change:
    /// every colour in the shell is derived from the answer.
    pub fn is_light(self) -> bool {
        match self {
            Self::Light => true,
            Self::Dark | Self::System => false,
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
    pub fn label(self) -> &'static str {
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

    /// This accent's value on a dark background.
    ///
    /// [`Custom`](Self::Custom) has no value of its own — the colour the user
    /// picked lives in [`AppearanceSettings::custom_accent`], because it is a
    /// setting rather than a property of the variant. Callers that may hold a
    /// `Custom` want [`AppearanceSettings::effective_accent`] instead of this.
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

    /// This accent's value on a light background.
    ///
    /// Same hue, same name, a darker value — see the light-accent palette
    /// above for why neither the dark-background pastels nor Catppuccin's own
    /// Latte accents can simply be reused.
    pub fn color_light(self) -> Color {
        match self {
            Self::Blue => LIGHT_BLUE,
            Self::Lavender => LIGHT_LAVENDER,
            Self::Teal => LIGHT_TEAL,
            Self::Green => LIGHT_GREEN,
            Self::Yellow => LIGHT_YELLOW,
            Self::Peach => LIGHT_PEACH,
            Self::Pink => LIGHT_PINK,
            Self::Mauve => LIGHT_MAUVE,
            Self::Red => LIGHT_RED,
            Self::Rosewater => LIGHT_ROSEWATER,
            Self::Flamingo => LIGHT_FLAMINGO,
            Self::Maroon => LIGHT_MAROON,
            Self::Sky => LIGHT_SKY,
            Self::Sapphire => LIGHT_SAPPHIRE,
            Self::Custom => LIGHT_BLUE, // fallback
        }
    }

    /// All preset (non-custom) accent colors.
    pub fn presets() -> &'static [AccentColor] {
        &[
            Self::Blue,
            Self::Lavender,
            Self::Teal,
            Self::Green,
            Self::Yellow,
            Self::Peach,
            Self::Pink,
            Self::Mauve,
            Self::Red,
            Self::Rosewater,
            Self::Flamingo,
            Self::Maroon,
            Self::Sky,
            Self::Sapphire,
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
    /// Every level, weakest effect first. See [`ThemeMode::ALL`].
    pub const ALL: &'static [Self] = &[Self::Off, Self::Subtle, Self::Moderate, Self::Full];

    pub fn label(self) -> &'static str {
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
    /// Every speed, slowest-to-fastest with `Off` first. See [`ThemeMode::ALL`].
    pub const ALL: &'static [Self] = &[Self::Off, Self::Slow, Self::Normal, Self::Fast];

    pub fn label(self) -> &'static str {
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
#[derive(Clone, Debug, PartialEq)]
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
    pub fn label(self) -> &'static str {
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
    pub fn label(self) -> &'static str {
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
    pub fn label(self) -> &'static str {
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
    pub fn label(self) -> &'static str {
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
    pub fn label(self) -> &'static str {
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
    pub fn label(self) -> &'static str {
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
#[derive(Clone, Debug, PartialEq)]
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
    /// The accent colour to actually draw with.
    ///
    /// Resolves both things a caller would otherwise have to know: that
    /// `Custom` keeps its value in [`custom_accent`](Self::custom_accent), and
    /// that a preset accent has a different value on a light background than
    /// on a dark one. A custom colour is used exactly as chosen in either mode
    /// — the user picked a specific colour, and quietly darkening it would be
    /// overriding the one choice that was stated in full.
    pub fn effective_accent(&self) -> Color {
        if self.accent_color == AccentColor::Custom {
            self.custom_accent
        } else if self.theme_mode.is_light() {
            self.accent_color.color_light()
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
        // Clamp font sizes. `get_f64` never yields a NaN or an infinity, so
        // `clamp` cannot be handed one from a config file; a NaN written by a
        // future code path would panic here rather than propagate silently.
        self.fonts.ui_size = self.fonts.ui_size.clamp(8.0, 32.0);
        self.fonts.mono_size = self.fonts.mono_size.clamp(6.0, 32.0);
        self.scaling_percent = self.scaling_percent.clamp(100, 300);
    }
}

// ============================================================================
// Window decoration colours
// ============================================================================

/// Black-ish or white-ish, whichever can be read on `bg`.
///
/// The endpoints are the palettes' own extremes rather than pure `#000`/`#fff`
/// so that accented surfaces still look like part of this desktop. Perceived
/// brightness uses the usual luma weights: the eye is far more sensitive to
/// green than to blue, so an average of the channels would call a saturated
/// blue "bright" and put black text on it.
///
/// Deliberately not [`guitk::theme::contrast_text`], which answers the same
/// question with pure black and pure white. That is the right answer for a
/// widget that may be drawn on any background; this is the right answer for a
/// surface that belongs to a specific palette.
#[must_use]
pub fn readable_on(bg: Color) -> Color {
    let luma = 0.299 * f32::from(bg.r) + 0.587 * f32::from(bg.g) + 0.114 * f32::from(bg.b);
    if luma > 140.0 {
        Color::from_hex(0x11111B)
    } else {
        Color::from_hex(0xEFF1F5)
    }
}

/// A visibly different shade of `color`, for the pressed state of a control
/// whose resting state is already `color`.
///
/// Moves away from whichever extreme `color` is nearer, so the emphasis is
/// visible on both a pale and a deep accent instead of vanishing at one end.
#[must_use]
pub fn emphasized(color: Color) -> Color {
    let toward = readable_on(color);
    color.lerp(toward, 0.25)
}

// ============================================================================
// The resolved palette
// ============================================================================

/// Every colour the shell paints with, resolved for one set of choices.
///
/// **What this is for.** The desktop shell had 549 `const … : Color` of its
/// own, spread over 49 modules, and every one of them was a Catppuccin Mocha
/// value written out by hand. `TEXT` was declared 31 separate times;
/// `0x89B4FA` appeared 47 times under four different names. None of them were
/// *wrong* — they all agreed with what the dark theme happens to be — and that
/// is exactly the problem: they agreed by coincidence, so the user's light/dark
/// choice, accent and transparency level reached none of them. See
/// known-issues.md `TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE`.
///
/// **Why a resolved struct rather than a lookup.** By the time a colour is in
/// here the mode, the accent and the transparency level have all been folded
/// in, so a render function does nothing but read a field. A renderer handed
/// [`AppearanceSettings`] instead would re-derive the same colour at every
/// frame and would be free to derive it slightly differently in each of the
/// dozens of places it is drawn — which is the duplication above, relocated
/// rather than removed. This is the same argument [`DecorationColors`] and
/// `DesktopTheme` already make; this type is the one they are both built from.
///
/// **Roles, not colours.** The fields are named for what a colour *does* in a
/// layout, not for what it looks like. The surface ladder runs
/// [`crust`](Self::crust) (behind the window) → [`base`](Self::base) (the
/// window) → [`surface0`](Self::surface0)…[`surface2`](Self::surface2) (things
/// raised off it) → [`overlay0`](Self::overlay0),
/// [`subtext0`](Self::subtext0), [`subtext1`](Self::subtext1),
/// [`text`](Self::text) (marks on it, in increasing prominence). In dark mode
/// "raised" means lighter and in light mode it means darker, so a caller that
/// names the role keeps working across the flip and a caller that names a hex
/// value does not.
///
/// **Why the named hues are still here, next to `accent`.** The shell uses
/// `BLUE` for two unrelated jobs: as "the colour this desktop is themed
/// around" — selection, focus, the start glyph — and as "the blue one" in a
/// fixed set of category colours, next to green and peach in a resource graph
/// legend. Collapsing both onto [`accent`](Self::accent) would give a user who
/// picks Red a graph whose CPU line and temperature line are the same colour,
/// which is not a theme, it is a lost distinction. So the two stay separate:
/// [`accent`](Self::accent) follows the setting, and the named hues are a
/// fixed categorical set that merely changes value between modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    /// Behind the window — the desktop, and the recessed well of a text input.
    pub crust: Color,
    /// One step behind [`base`](Self::base) — a sidebar beside a content pane.
    pub mantle: Color,
    /// The window itself: the default background of any surface the shell owns.
    pub base: Color,
    /// Raised one step off [`base`](Self::base) — a card, a header row.
    pub surface0: Color,
    /// Raised two steps — a button at rest, a selected row.
    pub surface1: Color,
    /// Raised three steps — a hovered button, a scrollbar thumb.
    pub surface2: Color,
    /// The faintest legible mark: separators, disabled text, placeholder text.
    ///
    /// Deliberately *not* required to carry body text — it measures about
    /// 3.4:1 on [`base`](Self::base) in dark mode and 2.3:1 in light. Anything
    /// a user has to read is [`subtext0`](Self::subtext0) or brighter.
    pub overlay0: Color,
    /// Secondary text: the second line of a list row, a caption, a hint.
    pub subtext0: Color,
    /// Text that is secondary but load-bearing — a column heading.
    pub subtext1: Color,
    /// Primary text.
    pub text: Color,
    /// The blue of the categorical set. See the type's note on hues.
    pub blue: Color,
    /// Green — also "this succeeded", "this is allowed", "this is safe".
    pub green: Color,
    /// Red — also "this failed", "this is denied", "this is dangerous".
    pub red: Color,
    /// Yellow — also "this needs attention".
    pub yellow: Color,
    /// Peach — also the step between [`yellow`](Self::yellow) and
    /// [`red`](Self::red) on a severity scale.
    pub peach: Color,
    /// Lavender.
    pub lavender: Color,
    /// Mauve.
    pub mauve: Color,
    /// Sapphire.
    pub sapphire: Color,
    /// Teal.
    ///
    /// Here because the applications need it, not because the shell does — 86
    /// `const TEAL` declarations across `apps/`, against 0 in `gui/desktop`.
    /// Added while the light ladder was being written rather than when
    /// `apps/` is converted, because a hue added later has to be re-checked
    /// against every mode-flip and legibility sweep in this module, and a
    /// sweep that silently skips a field is the failure those sweeps exist to
    /// catch. See known-issues.md
    /// `TD-C-EVERY-APPLICATION-CARRIES-ITS-OWN-COPY-OF-THE-PALETTE-TOO`.
    pub teal: Color,
    /// Sky. Present for the same reason as [`teal`](Self::teal).
    pub sky: Color,
    /// The colour this desktop is themed around, as the user chose it.
    ///
    /// Already resolved for the mode and for a custom colour — this is
    /// [`AppearanceSettings::effective_accent`], not the enum.
    pub accent: Color,
    /// How opaque a floating surface is: [`TransparencyLevel::panel_alpha`].
    ///
    /// Carried rather than applied to every field because most surfaces are
    /// *not* floating. A list row inside a panel must stay opaque no matter
    /// what the panel behind it does, or the desktop shows through the row and
    /// not through its own container.
    pub panel_alpha: u8,
    /// Whether this is the light palette.
    ///
    /// Present so a caller with a genuinely mode-dependent decision — an icon
    /// with a light and a dark artwork, say — can ask, instead of guessing
    /// from the luma of a field it happens to have.
    pub light: bool,
}

/// Fixed alphas for the washes derived from [`Palette::accent`].
///
/// One place, because the shell had six: a selection at 50, a marquee at 30, a
/// snap zone at 50 and its highlight at 90, and two borders at 150 and 160 that
/// meant the same thing and differed by a rounding nobody chose. Alpha is what
/// distinguishes these from one another, so if it is written per module the
/// modules are the definition and drift is silent.
mod wash {
    /// A hint the pointer is currently drawing — a rubber-band marquee.
    pub const HINT: u8 = 30;
    /// A committed selection, or a snap zone at rest.
    pub const FILL: u8 = 50;
    /// The one zone or item the pointer is over.
    pub const HIGHLIGHT: u8 = 90;
    /// The outline of a hint.
    pub const HINT_EDGE: u8 = 120;
    /// The outline of a selection or a highlighted zone.
    pub const EDGE: u8 = 150;
}

impl Palette {
    /// The palette for a mode, before any of the user's other choices apply.
    ///
    /// [`accent`](Self::accent) is the mode's blue, which is what the accent
    /// setting defaults to; [`panel_alpha`](Self::panel_alpha) is opaque.
    /// Callers that have an [`AppearanceSettings`] should use
    /// [`from_settings`](Self::from_settings) instead — this exists for the
    /// two places that legitimately have only a mode: a test asserting a
    /// property of one palette, and a preview swatch.
    #[must_use]
    pub fn for_mode(light: bool) -> Self {
        if light {
            Self {
                crust: LIGHT_CRUST,
                mantle: LIGHT_MANTLE,
                base: LIGHT_BASE,
                surface0: LIGHT_SURFACE0,
                surface1: LIGHT_SURFACE1,
                surface2: LIGHT_SURFACE2,
                overlay0: LIGHT_OVERLAY0,
                subtext0: LIGHT_SUBTEXT0,
                subtext1: LIGHT_SUBTEXT1,
                text: LIGHT_TEXT,
                blue: LIGHT_BLUE,
                green: LIGHT_GREEN,
                red: LIGHT_RED,
                yellow: LIGHT_YELLOW,
                peach: LIGHT_PEACH,
                lavender: LIGHT_LAVENDER,
                mauve: LIGHT_MAUVE,
                sapphire: LIGHT_SAPPHIRE,
                teal: LIGHT_TEAL,
                sky: LIGHT_SKY,
                accent: LIGHT_BLUE,
                panel_alpha: 255,
                light: true,
            }
        } else {
            Self {
                crust: CRUST,
                mantle: MANTLE,
                base: BASE,
                surface0: SURFACE0,
                surface1: SURFACE1,
                surface2: SURFACE2,
                overlay0: OVERLAY0,
                subtext0: SUBTEXT0,
                subtext1: SUBTEXT1,
                text: TEXT,
                blue: BLUE,
                green: GREEN,
                red: RED,
                yellow: YELLOW,
                peach: PEACH,
                lavender: LAVENDER,
                mauve: MAUVE,
                sapphire: SAPPHIRE,
                teal: TEAL,
                sky: SKY,
                accent: BLUE,
                panel_alpha: 255,
                light: false,
            }
        }
    }

    /// Resolve the whole palette from what the user chose.
    #[must_use]
    pub fn from_settings(settings: &AppearanceSettings) -> Self {
        let mut palette = Self::for_mode(settings.theme_mode.is_light());
        palette.accent = settings.effective_accent();
        palette.panel_alpha = settings.transparency.panel_alpha();
        palette
    }

    /// This palette's value for one of the accent presets.
    ///
    /// For the settings page, which draws all fourteen as swatches and must
    /// draw them in the mode the user is currently in — otherwise the chosen
    /// swatch is not the colour that appears when it is chosen. Agrees with
    /// the named-hue fields by construction, which
    /// `every_named_hue_agrees_with_the_accent_of_the_same_name` asserts.
    #[must_use]
    pub fn hue(&self, accent: AccentColor) -> Color {
        if self.light {
            accent.color_light()
        } else {
            accent.color()
        }
    }

    /// Every field of this palette, paired with its name.
    ///
    /// Public rather than test-only because two different sweeps need it and
    /// the alternative is two hand-written lists that must be kept in step —
    /// which is the shape of the bug this whole crate exists to remove. The
    /// second caller is the shell's conversion sweep, which asserts that a
    /// module's render output is drawn from the palette it was handed and
    /// nothing else; a colour constant left behind in a converted module is a
    /// Mocha value, so it is absent from the *light* palette's roles and the
    /// sweep names it. See known-issues.md
    /// `TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE`.
    ///
    /// Written out by hand, and deliberately so: the point of the sweeps that
    /// consume this is that a field added later is *not* silently skipped, and
    /// a macro or a reflection trick would skip it for exactly the same reason
    /// the renderer would. The array's length is part of the signature so that
    /// adding a field without adding it here fails to compile.
    #[must_use]
    pub fn roles(&self) -> [(&'static str, Color); 21] {
        [
            ("crust", self.crust),
            ("mantle", self.mantle),
            ("base", self.base),
            ("surface0", self.surface0),
            ("surface1", self.surface1),
            ("surface2", self.surface2),
            ("overlay0", self.overlay0),
            ("subtext0", self.subtext0),
            ("subtext1", self.subtext1),
            ("text", self.text),
            ("red", self.red),
            ("green", self.green),
            ("yellow", self.yellow),
            ("peach", self.peach),
            ("blue", self.blue),
            ("lavender", self.lavender),
            ("mauve", self.mauve),
            ("sapphire", self.sapphire),
            ("teal", self.teal),
            ("sky", self.sky),
            ("accent", self.accent),
        ]
    }

    /// Text that can be read on [`accent`](Self::accent).
    ///
    /// The accent is the one colour in the palette whose brightness the user
    /// controls, so nothing drawn on it can have a fixed foreground.
    #[must_use]
    pub fn on_accent(&self) -> Color {
        readable_on(self.accent)
    }

    /// A floating surface: a popup, a menu, the launcher.
    ///
    /// [`base`](Self::base) at [`panel_alpha`](Self::panel_alpha) — the only
    /// place transparency is applied, because a panel is the only thing that
    /// floats over something the user might want to keep seeing.
    #[must_use]
    pub fn panel_bg(&self) -> Color {
        with_alpha(self.base, self.panel_alpha)
    }

    /// The hovered row of a floating surface.
    ///
    /// Translucent to the same degree as the panel it sits in. A hover
    /// highlight that stayed opaque inside a translucent menu would read as a
    /// solid tile skating over the wallpaper.
    #[must_use]
    pub fn panel_hover(&self) -> Color {
        with_alpha(self.surface1, self.panel_alpha)
    }

    /// A dim layer over everything behind a modal.
    ///
    /// Black in both modes, for the same reason [`shadow`](Self::shadow) is:
    /// a scrim is an absence of light rather than a colour, and its job is to
    /// push the background back. The light palette makes that argument
    /// necessary rather than merely tidy — the shell used to dim with its own
    /// `base` at alpha, and Latte's base is `#EFF1F5`, so in light mode that
    /// would have *lightened* the desktop and left the dialog with nothing to
    /// stand out against.
    #[must_use]
    pub fn scrim(&self) -> Color {
        Color::rgba(0, 0, 0, 140)
    }

    /// The drop shadow under a floating surface, at its strongest.
    ///
    /// One value, where the shell had three — 100, 120 and 160 in three
    /// modules that all draw the same kind of popup. None of the three was
    /// chosen against the others; they were each chosen alone. A renderer that
    /// fades the shadow outward starts here and falls to nothing.
    ///
    /// Distinct from [`DecorationColors::shadow`], which is the shadow under a
    /// *window* and is weaker: a window sits on the desktop, a popup sits on
    /// top of a window, and the second wants more separation than the first.
    #[must_use]
    pub fn shadow(&self) -> Color {
        Color::rgba(0, 0, 0, 120)
    }

    /// The hard shadow behind text drawn straight onto the wallpaper.
    ///
    /// Much stronger than [`shadow`](Self::shadow) and deliberately not
    /// theme-dependent: a desktop icon's label lands on an arbitrary
    /// photograph, so its legibility cannot come from the palette. This is the
    /// one colour here that is not a design choice but a floor.
    #[must_use]
    pub fn text_shadow(&self) -> Color {
        Color::rgba(0, 0, 0, 180)
    }

    /// The interior of a committed selection, or of a snap zone at rest.
    #[must_use]
    pub fn selection_fill(&self) -> Color {
        with_alpha(self.accent, wash::FILL)
    }

    /// The outline of a selection or of a highlighted snap zone.
    #[must_use]
    pub fn selection_border(&self) -> Color {
        with_alpha(self.accent, wash::EDGE)
    }

    /// The interior of something the pointer is still drawing — a marquee.
    #[must_use]
    pub fn hint_fill(&self) -> Color {
        with_alpha(self.accent, wash::HINT)
    }

    /// The outline of a marquee.
    #[must_use]
    pub fn hint_border(&self) -> Color {
        with_alpha(self.accent, wash::HINT_EDGE)
    }

    /// The one zone or item the pointer is currently over.
    #[must_use]
    pub fn highlight_fill(&self) -> Color {
        with_alpha(self.accent, wash::HIGHLIGHT)
    }

    /// Where a drag would land if it were released now.
    ///
    /// Green rather than the accent, and that is not decoration: a drop target
    /// and a selection are shown at the same moment during a drag, so they
    /// have to differ by hue and not merely by alpha.
    #[must_use]
    pub fn drop_target(&self) -> Color {
        with_alpha(self.green, 60)
    }
}

/// Every colour used to draw a window's frame, and the emptiness behind it.
///
/// This type exists because two processes draw the same frame. The compositor
/// owns the real one — it is the process holding the framebuffer — and the
/// desktop shell has historically drawn its own copy. While that duplicate
/// survives, both must agree, and the only way two renderers agree about a
/// colour is if neither of them decides it. So neither does: they both read
/// this.
///
/// The desktop background is here for the same reason the borders are. It is
/// not part of a frame, but it is the surface a frame is seen against, it is
/// painted by the same process, and it comes from the same palette — putting it
/// anywhere else would mean a caller had to find two answers to assemble one
/// screen.
///
/// The fields are resolved colours, not settings: a renderer that consulted
/// [`AppearanceSettings`] directly would re-derive the accent at every frame,
/// and would be free to derive it slightly differently in each place a frame is
/// drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecorationColors {
    /// Title bar background on the focused window.
    pub title_focused_bg: Color,
    /// Title text on the focused window.
    pub title_focused_fg: Color,
    /// Title bar background on every other window.
    pub title_unfocused_bg: Color,
    /// Title text on every other window.
    ///
    /// Carried separately from [`title_focused_fg`](Self::title_focused_fg)
    /// because with accented title bars the two backgrounds differ by more than
    /// a shade, and one shared text colour would then be unreadable on one of
    /// them.
    pub title_unfocused_fg: Color,
    /// The one-pixel outline around the focused window.
    pub border_focused: Color,
    /// The outline around every other window — dimmer, so that focus is legible
    /// from the frame alone when the title bars are the same colour.
    pub border_unfocused: Color,
    /// The close button.
    pub close_button: Color,
    /// The maximize/restore button.
    pub maximize_button: Color,
    /// The minimize button.
    pub minimize_button: Color,
    /// The drop shadow beneath a floating window, alpha included.
    pub shadow: Color,
    /// The desktop itself, where no window covers it.
    pub desktop_bg: Color,
}

impl DecorationColors {
    /// The palette for a mode, before any of the user's other choices apply.
    ///
    /// Surface for surface the two modes are the same structure — base,
    /// surface0, surface1, surface2, crust — so that a setting applied on top
    /// lands on the same role in either one.
    ///
    /// Which is now said once rather than twice: the two arms below used to be
    /// two hand-written tables of hex, and the sentence above was the only
    /// thing asserting they lined up. Reading both from [`Palette::for_mode`]
    /// makes the claim structural — a frame is `surface0` on `base` in either
    /// mode because that is literally what is written, not because two lists
    /// were kept in step.
    #[must_use]
    pub fn for_mode(light: bool) -> Self {
        Self::from_palette(&Palette::for_mode(light))
    }

    /// Which role of `palette` each part of a frame is.
    ///
    /// The whole body of this type: everything else here either chooses a
    /// palette to hand it or overrides one field afterwards. Taking a
    /// `&Palette` rather than a mode is what lets [`from_settings`] apply the
    /// user's accent to a frame at all — the alternative, and what this was
    /// until the reintroduction proof for defect Q found it, is a frame built
    /// from the *default* palette with the accent painted on afterwards, so
    /// that any frame role which ever came to depend on the accent would
    /// silently get blue.
    ///
    /// [`from_settings`]: Self::from_settings
    #[must_use]
    pub fn from_palette(p: &Palette) -> Self {
        Self {
            title_focused_bg: p.surface0,
            title_focused_fg: p.text,
            title_unfocused_bg: p.base,
            title_unfocused_fg: p.subtext0,
            border_focused: p.surface2,
            border_unfocused: p.surface1,
            // The three buttons are the palette's own red, green and yellow —
            // stop, go, and the middling one — rather than a shape the user
            // has to learn, and they keep those hues when the accent changes
            // so that "close" never becomes whatever colour the desktop is
            // themed around. Which is now a real restraint rather than a
            // description: `p.accent` is in scope here and reading it would
            // compile.
            close_button: p.red,
            maximize_button: p.green,
            minimize_button: p.yellow,
            shadow: SHADOW,
            desktop_bg: p.crust,
        }
    }

    /// Resolve the frame colours from what the user chose.
    #[must_use]
    pub fn from_settings(settings: &AppearanceSettings) -> Self {
        let mut colors = Self::from_palette(&Palette::from_settings(settings));

        if settings.accent_titlebars {
            let accent = settings.effective_accent();
            colors.title_focused_bg = accent;
            colors.title_focused_fg = readable_on(accent);
            // The *unfocused* bar deliberately keeps the base palette: an
            // accent that marks every window marks none of them, and telling
            // the focused window apart is the title bar's first job.
        }

        colors
    }
}

/// The drop shadow, in both modes.
///
/// A shadow is an absence of light rather than a colour of its own, so it is
/// the same black in either palette; what changes between them is the surface
/// it falls on, which is already lighter or darker.
///
/// The alpha is the shadow at its *strongest*, immediately outside the frame.
/// A renderer that fades it outward — which is what makes a hard rectangle look
/// like a shadow — starts here and falls to nothing.
const SHADOW: Color = Color::rgba(0, 0, 0, 40);

// ============================================================================
// Configuration file
// ============================================================================

// The spellings below are the on-disk format for `appearance.yaml`. Adding a
// variant is free; renaming one is a breaking change to every user's file.
yaml_enum!(ThemeMode { Dark => "dark", Light => "light", System => "system" });
yaml_enum!(AccentColor {
    Blue => "blue",
    Lavender => "lavender",
    Teal => "teal",
    Green => "green",
    Yellow => "yellow",
    Peach => "peach",
    Pink => "pink",
    Mauve => "mauve",
    Red => "red",
    Rosewater => "rosewater",
    Flamingo => "flamingo",
    Maroon => "maroon",
    Sky => "sky",
    Sapphire => "sapphire",
    Custom => "custom",
});
yaml_enum!(TransparencyLevel {
    Off => "off",
    Subtle => "subtle",
    Moderate => "moderate",
    Full => "full",
});
yaml_enum!(AnimationSpeed {
    Off => "off",
    Fast => "fast",
    Normal => "normal",
    Slow => "slow",
});
yaml_enum!(SubpixelMode {
    None => "none",
    Rgb => "rgb",
    Bgr => "bgr",
    VRgb => "vrgb",
    VBgr => "vbgr",
});
yaml_enum!(IconSize {
    Small => "small",
    Medium => "medium",
    Large => "large",
    ExtraLarge => "extra-large",
});
yaml_enum!(CursorSize {
    Small => "small",
    Normal => "normal",
    Large => "large",
    ExtraLarge => "extra-large",
});
yaml_enum!(CursorScheme {
    Default => "default",
    Inverted => "inverted",
    AccentColored => "accent",
});
yaml_enum!(WindowCorners {
    Square => "square",
    Subtle => "subtle",
    Rounded => "rounded",
    ExtraRounded => "extra-rounded",
});
yaml_enum!(TaskbarStyle {
    Solid => "solid",
    Translucent => "translucent",
    Transparent => "transparent",
});

/// Spell a colour as CSS-style hex, the notation a user editing the file by
/// hand will already know. The alpha byte appears only when it is not opaque,
/// so the common case stays a familiar six digits.
pub fn color_to_hex(color: Color) -> String {
    if color.a == 255 {
        format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
    } else {
        format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            color.r, color.g, color.b, color.a
        )
    }
}

/// Read a `#rrggbb` or `#rrggbbaa` colour. `None` for anything else, so a
/// mistyped colour falls back to the default rather than to black.
pub fn color_from_hex(text: &str) -> Option<Color> {
    let digits = text.strip_prefix('#')?;
    if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte =
        |i: usize| -> Option<u8> { u8::from_str_radix(digits.get(i..i.checked_add(2)?)?, 16).ok() };
    match digits.len() {
        6 => Some(Color::rgb(byte(0)?, byte(2)?, byte(4)?)),
        8 => Some(Color::rgba(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
        _ => None,
    }
}

/// Read a value if the file has one, otherwise keep what is already there.
///
/// This is the whole reason settings are read into a `Default` rather than
/// built from the file: a key the user has never touched, or one written by a
/// newer version and since removed, leaves the field at its default instead of
/// zeroing it.
macro_rules! read_into {
    ($slot:expr, $value:expr) => {
        if let Some(value) = $value {
            $slot = value;
        }
    };
}

impl AppearanceSettings {
    /// Read settings from a configuration document.
    ///
    /// Every key is optional and every unreadable value is ignored, so a
    /// missing file, a partial file and a file from a different version all
    /// produce a usable result. The outcome is always [`validate`]d, because
    /// the file is user-editable and nothing stops someone typing a font size
    /// of 400.
    ///
    /// [`validate`]: Self::validate
    #[must_use]
    pub fn read_from(doc: &Document) -> Self {
        let mut s = Self::default();

        read_into!(
            s.theme_mode,
            doc.get_str(&["theme", "mode"])
                .and_then(|v| ThemeMode::from_yaml_name(&v))
        );
        read_into!(
            s.accent_color,
            doc.get_str(&["theme", "accent"])
                .and_then(|v| AccentColor::from_yaml_name(&v))
        );
        read_into!(
            s.custom_accent,
            doc.get_str(&["theme", "custom_accent"])
                .and_then(|v| color_from_hex(&v))
        );
        read_into!(
            s.transparency,
            doc.get_str(&["theme", "transparency"])
                .and_then(|v| TransparencyLevel::from_yaml_name(&v))
        );

        read_into!(s.fonts.ui_font, doc.get_str(&["fonts", "ui_font"]));
        read_into!(
            s.fonts.ui_size,
            doc.get_f64(&["fonts", "ui_size"]).map(|v| v as f32)
        );
        read_into!(s.fonts.mono_font, doc.get_str(&["fonts", "mono_font"]));
        read_into!(
            s.fonts.mono_size,
            doc.get_f64(&["fonts", "mono_size"]).map(|v| v as f32)
        );
        read_into!(s.fonts.hinting, doc.get_bool(&["fonts", "hinting"]));
        read_into!(
            s.fonts.subpixel,
            doc.get_str(&["fonts", "subpixel"])
                .and_then(|v| SubpixelMode::from_yaml_name(&v))
        );
        read_into!(s.fonts.smoothing, doc.get_bool(&["fonts", "smoothing"]));

        read_into!(
            s.animation_speed,
            doc.get_str(&["effects", "animation_speed"])
                .and_then(|v| AnimationSpeed::from_yaml_name(&v))
        );
        read_into!(
            s.window_corners,
            doc.get_str(&["effects", "window_corners"])
                .and_then(|v| WindowCorners::from_yaml_name(&v))
        );
        read_into!(
            s.taskbar_style,
            doc.get_str(&["effects", "taskbar_style"])
                .and_then(|v| TaskbarStyle::from_yaml_name(&v))
        );
        read_into!(
            s.accent_taskbar,
            doc.get_bool(&["effects", "accent_taskbar"])
        );
        read_into!(
            s.accent_titlebars,
            doc.get_bool(&["effects", "accent_titlebars"])
        );
        read_into!(s.drop_shadows, doc.get_bool(&["effects", "drop_shadows"]));

        read_into!(
            s.cursor_size,
            doc.get_str(&["cursors", "size"])
                .and_then(|v| CursorSize::from_yaml_name(&v))
        );
        read_into!(
            s.cursor_scheme,
            doc.get_str(&["cursors", "scheme"])
                .and_then(|v| CursorScheme::from_yaml_name(&v))
        );
        read_into!(
            s.icon_size,
            doc.get_str(&["icons", "size"])
                .and_then(|v| IconSize::from_yaml_name(&v))
        );

        // A scaling percentage outside u16 is not a number this UI can mean;
        // `validate` clamps the rest of the range.
        read_into!(
            s.scaling_percent,
            doc.get_i64(&["display", "scaling_percent"])
                .and_then(|v| u16::try_from(v).ok())
        );

        s.validate();
        s
    }

    /// Write these settings into a configuration document, leaving every
    /// comment, blank line and unrelated key in it exactly as it was.
    pub fn write_into(&self, doc: &mut Document) {
        doc.set_str(&["theme", "mode"], self.theme_mode.yaml_name());
        doc.set_str(&["theme", "accent"], self.accent_color.yaml_name());
        doc.set_str(
            &["theme", "custom_accent"],
            &color_to_hex(self.custom_accent),
        );
        doc.set_str(&["theme", "transparency"], self.transparency.yaml_name());

        doc.set_str(&["fonts", "ui_font"], &self.fonts.ui_font);
        doc.set_f64(&["fonts", "ui_size"], f64::from(self.fonts.ui_size));
        doc.set_str(&["fonts", "mono_font"], &self.fonts.mono_font);
        doc.set_f64(&["fonts", "mono_size"], f64::from(self.fonts.mono_size));
        doc.set_bool(&["fonts", "hinting"], self.fonts.hinting);
        doc.set_str(&["fonts", "subpixel"], self.fonts.subpixel.yaml_name());
        doc.set_bool(&["fonts", "smoothing"], self.fonts.smoothing);

        doc.set_str(
            &["effects", "animation_speed"],
            self.animation_speed.yaml_name(),
        );
        doc.set_str(
            &["effects", "window_corners"],
            self.window_corners.yaml_name(),
        );
        doc.set_str(
            &["effects", "taskbar_style"],
            self.taskbar_style.yaml_name(),
        );
        doc.set_bool(&["effects", "accent_taskbar"], self.accent_taskbar);
        doc.set_bool(&["effects", "accent_titlebars"], self.accent_titlebars);
        doc.set_bool(&["effects", "drop_shadows"], self.drop_shadows);

        doc.set_str(&["cursors", "size"], self.cursor_size.yaml_name());
        doc.set_str(&["cursors", "scheme"], self.cursor_scheme.yaml_name());
        doc.set_str(&["icons", "size"], self.icon_size.yaml_name());

        doc.set_i64(
            &["display", "scaling_percent"],
            i64::from(self.scaling_percent),
        );
    }
}

// ============================================================================
// The settings file
// ============================================================================

/// The settings group these preferences live in — `appearance.yaml` in the
/// user's configuration directory.
///
/// The *name* is as much a part of the shared contract as the schema is: two
/// processes that agree on every key but disagree about which file holds them
/// have simply written two files.
pub const CONFIG_NAME: &str = "appearance";

/// The user's appearance settings together with the document they came from.
///
/// The pair is a type rather than two fields because keeping them together is
/// an invariant, not a convenience: a save must splice the changed values back
/// into the document that was read, since that document carries everything
/// this model does not — the user's comments, their blank lines, their key
/// order, and any setting belonging to a different version of the desktop.
/// Rebuilding the file from [`AppearanceSettings`] alone silently deletes all
/// of it, which is exactly the mistake that having one owner is meant to stop.
///
/// Both front ends hold one of these: the shell's appearance panel and the
/// Settings application's Personalization pages.
pub struct AppearanceFile {
    /// The settings being edited. Public because both front ends bind
    /// controls straight to the fields.
    pub settings: AppearanceSettings,
    /// The file as read, kept whole. See the type's documentation.
    doc: Document,
}

impl Default for AppearanceFile {
    fn default() -> Self {
        Self::new()
    }
}

impl AppearanceFile {
    /// The defaults, backed by an empty document.
    ///
    /// Deliberately does *not* read the filesystem: a constructor that
    /// consulted `$HOME` would make every caller's tests depend on the machine
    /// running them. [`load`](Self::load) does the I/O.
    #[must_use]
    pub fn new() -> Self {
        Self {
            settings: AppearanceSettings::default(),
            doc: Document::new(),
        }
    }

    /// Read the user's saved settings from `appearance.yaml`.
    ///
    /// A missing or unreadable file yields the defaults — the ordinary state
    /// on a fresh install, not an error to report to someone who has simply
    /// never changed a setting.
    #[must_use]
    pub fn load() -> Self {
        Self::from_document(config::load(CONFIG_NAME))
    }

    /// Open on an already-read document. Split out from [`load`](Self::load)
    /// so the format can be exercised without a filesystem.
    #[must_use]
    pub fn from_document(doc: Document) -> Self {
        Self {
            settings: AppearanceSettings::read_from(&doc),
            doc,
        }
    }

    /// Fold the current settings into the document without touching the
    /// filesystem, and return it.
    pub fn apply(&mut self) -> &Document {
        self.settings.write_into(&mut self.doc);
        &self.doc
    }

    /// The document as it stands, without folding in pending changes.
    #[must_use]
    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// Write the current settings to `appearance.yaml`, atomically.
    ///
    /// # Errors
    ///
    /// If there is no configuration directory, or the file cannot be written.
    pub fn save(&mut self) -> std::io::Result<()> {
        self.apply();
        config::store(CONFIG_NAME, &self.doc)
    }
}

// Panicking on bad data is the point of a test, and a test that asserts a
// default is `11.0` means exactly 11.0 — the float comparison is the
// assertion, not an approximation mistake.
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp,
    clippy::field_reassign_with_default,
    clippy::bool_assert_comparison
)]
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
    // ---- Configuration file ----

    /// Settings that differ from the defaults in every field, so a
    /// round-trip test cannot pass by accident on a field it forgot.
    fn all_non_default() -> AppearanceSettings {
        AppearanceSettings {
            theme_mode: ThemeMode::Light,
            accent_color: AccentColor::Custom,
            custom_accent: Color::rgba(1, 2, 3, 4),
            transparency: TransparencyLevel::Full,
            animation_speed: AnimationSpeed::Slow,
            fonts: FontSettings {
                ui_font: "Cantarell".to_string(),
                mono_font: "Iosevka".to_string(),
                ui_size: 15.5,
                mono_size: 11.0,
                hinting: false,
                subpixel: SubpixelMode::VBgr,
                smoothing: false,
            },
            icon_size: IconSize::ExtraLarge,
            cursor_size: CursorSize::Large,
            cursor_scheme: CursorScheme::AccentColored,
            window_corners: WindowCorners::Square,
            taskbar_style: TaskbarStyle::Transparent,
            accent_taskbar: true,
            accent_titlebars: true,
            drop_shadows: false,
            scaling_percent: 150,
        }
    }

    #[test]
    fn test_config_round_trips_every_field() {
        let settings = all_non_default();
        assert_ne!(settings, AppearanceSettings::default());
        let mut doc = Document::new();
        settings.write_into(&mut doc);
        let reread = AppearanceSettings::read_from(&Document::parse(&doc.to_text()));
        assert_eq!(reread, settings);
    }

    #[test]
    fn test_config_round_trips_every_enum_variant() {
        // A typo in one `yaml_name` arm would otherwise only show up as one
        // user's setting quietly resetting itself.
        let mut settings = AppearanceSettings::default();
        for accent in AccentColor::presets()
            .iter()
            .copied()
            .chain([AccentColor::Custom])
        {
            settings.accent_color = accent;
            for theme in [ThemeMode::Dark, ThemeMode::Light, ThemeMode::System] {
                settings.theme_mode = theme;
                let mut doc = Document::new();
                settings.write_into(&mut doc);
                let reread = AppearanceSettings::read_from(&Document::parse(&doc.to_text()));
                assert_eq!(reread.accent_color, accent);
                assert_eq!(reread.theme_mode, theme);
            }
        }
        for (subpixel, corners, taskbar, cursor, icon, speed, transparency, scheme) in [
            (
                SubpixelMode::None,
                WindowCorners::Square,
                TaskbarStyle::Solid,
                CursorSize::Small,
                IconSize::Small,
                AnimationSpeed::Off,
                TransparencyLevel::Off,
                CursorScheme::Default,
            ),
            (
                SubpixelMode::Rgb,
                WindowCorners::Subtle,
                TaskbarStyle::Translucent,
                CursorSize::Normal,
                IconSize::Medium,
                AnimationSpeed::Fast,
                TransparencyLevel::Subtle,
                CursorScheme::Inverted,
            ),
            (
                SubpixelMode::Bgr,
                WindowCorners::Rounded,
                TaskbarStyle::Transparent,
                CursorSize::Large,
                IconSize::Large,
                AnimationSpeed::Normal,
                TransparencyLevel::Moderate,
                CursorScheme::AccentColored,
            ),
            (
                SubpixelMode::VRgb,
                WindowCorners::ExtraRounded,
                TaskbarStyle::Solid,
                CursorSize::ExtraLarge,
                IconSize::ExtraLarge,
                AnimationSpeed::Slow,
                TransparencyLevel::Full,
                CursorScheme::Default,
            ),
            (
                SubpixelMode::VBgr,
                WindowCorners::Square,
                TaskbarStyle::Translucent,
                CursorSize::Small,
                IconSize::Small,
                AnimationSpeed::Off,
                TransparencyLevel::Off,
                CursorScheme::Inverted,
            ),
        ] {
            settings.fonts.subpixel = subpixel;
            settings.window_corners = corners;
            settings.taskbar_style = taskbar;
            settings.cursor_size = cursor;
            settings.icon_size = icon;
            settings.animation_speed = speed;
            settings.transparency = transparency;
            settings.cursor_scheme = scheme;
            let mut doc = Document::new();
            settings.write_into(&mut doc);
            let reread = AppearanceSettings::read_from(&Document::parse(&doc.to_text()));
            assert_eq!(reread, settings, "round trip of {settings:?}");
        }
    }

    #[test]
    fn test_config_yaml_names_are_distinct_within_each_enum() {
        // Two variants sharing a spelling would make one of them unreadable.
        let accents: Vec<_> = AccentColor::presets()
            .iter()
            .copied()
            .chain([AccentColor::Custom])
            .map(AccentColor::yaml_name)
            .collect();
        let mut sorted = accents.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), accents.len(), "duplicate accent spelling");
    }

    #[test]
    fn test_config_missing_keys_fall_back_to_defaults() {
        let doc = Document::parse("theme:\n  mode: light\n");
        let settings = AppearanceSettings::read_from(&doc);
        assert_eq!(settings.theme_mode, ThemeMode::Light);
        // Everything the file did not mention is untouched.
        let defaults = AppearanceSettings::default();
        assert_eq!(settings.accent_color, defaults.accent_color);
        assert_eq!(settings.fonts, defaults.fonts);
        assert_eq!(settings.scaling_percent, defaults.scaling_percent);
    }

    #[test]
    fn test_config_unknown_spellings_fall_back_to_defaults() {
        // A file written by a newer desktop, or edited by hand with a typo.
        let doc = Document::parse(
            "theme:\n  mode: solarized\n  accent: chartreuse\n  custom_accent: not-a-color\n\
             fonts:\n  subpixel: quadpixel\n  hinting: maybe\n",
        );
        let settings = AppearanceSettings::read_from(&doc);
        let defaults = AppearanceSettings::default();
        assert_eq!(settings.theme_mode, defaults.theme_mode);
        assert_eq!(settings.accent_color, defaults.accent_color);
        assert_eq!(settings.custom_accent, defaults.custom_accent);
        assert_eq!(settings.fonts.subpixel, defaults.fonts.subpixel);
        assert_eq!(settings.fonts.hinting, defaults.fonts.hinting);
    }

    #[test]
    fn test_config_out_of_range_values_are_clamped_not_rejected() {
        let doc = Document::parse(
            "fonts:\n  ui_size: 400.0\n  mono_size: 1.0\ndisplay:\n  scaling_percent: 9000\n",
        );
        let settings = AppearanceSettings::read_from(&doc);
        assert_eq!(settings.fonts.ui_size, 32.0);
        assert_eq!(settings.fonts.mono_size, 6.0);
        assert_eq!(settings.scaling_percent, 300);
        // A percentage that does not even fit a u16 leaves the default alone.
        let huge = Document::parse("display:\n  scaling_percent: 99999999\n");
        assert_eq!(
            AppearanceSettings::read_from(&huge).scaling_percent,
            AppearanceSettings::default().scaling_percent
        );
    }

    #[test]
    fn test_config_colors_use_css_hex() {
        assert_eq!(color_to_hex(Color::rgb(0x89, 0xB4, 0xFA)), "#89b4fa");
        assert_eq!(color_to_hex(Color::rgba(1, 2, 3, 4)), "#01020304");
        assert_eq!(
            color_from_hex("#89b4fa"),
            Some(Color::rgb(0x89, 0xB4, 0xFA))
        );
        assert_eq!(
            color_from_hex("#89B4FA"),
            Some(Color::rgb(0x89, 0xB4, 0xFA))
        );
        assert_eq!(color_from_hex("#01020304"), Some(Color::rgba(1, 2, 3, 4)));
        for bad in ["89b4fa", "#89b4f", "#gggggg", "#", "", "#89b4fa00ff"] {
            assert_eq!(color_from_hex(bad), None, "{bad} should not parse");
        }
    }

    // ---- DecorationColors ----

    #[test]
    fn the_two_modes_disagree_about_every_colour_a_frame_is_drawn_with() {
        // Not a style opinion — a guard on a mistake with a specific shape. A
        // palette assembled by copying the other one and editing it is easy to
        // leave a line short, and the symptom is a single element that stays
        // dark in light mode: dark title text on a dark bar, or a border that
        // vanishes. Every field genuinely differs between Mocha and Latte
        // except the shadow, which is deliberately the same black.
        let dark = DecorationColors::for_mode(false);
        let light = DecorationColors::for_mode(true);
        let pairs: [(&str, Color, Color); 10] = [
            (
                "title_focused_bg",
                dark.title_focused_bg,
                light.title_focused_bg,
            ),
            (
                "title_focused_fg",
                dark.title_focused_fg,
                light.title_focused_fg,
            ),
            (
                "title_unfocused_bg",
                dark.title_unfocused_bg,
                light.title_unfocused_bg,
            ),
            (
                "title_unfocused_fg",
                dark.title_unfocused_fg,
                light.title_unfocused_fg,
            ),
            ("border_focused", dark.border_focused, light.border_focused),
            (
                "border_unfocused",
                dark.border_unfocused,
                light.border_unfocused,
            ),
            ("close_button", dark.close_button, light.close_button),
            (
                "maximize_button",
                dark.maximize_button,
                light.maximize_button,
            ),
            (
                "minimize_button",
                dark.minimize_button,
                light.minimize_button,
            ),
            ("desktop_bg", dark.desktop_bg, light.desktop_bg),
        ];
        for (field, d, l) in pairs {
            assert_ne!(d, l, "{field} is the same colour in both modes");
        }
        assert_eq!(
            dark.shadow, light.shadow,
            "the shadow is meant to be the same black in both modes"
        );
    }

    #[test]
    fn a_focused_bar_is_legible_against_whatever_accent_it_was_given() {
        // The point of resolving the foreground alongside the background rather
        // than letting a renderer pick one. A yellow accent needs dark title
        // text and a maroon one needs light; a single foreground would make one
        // of the fourteen accents unreadable, and nobody would find out until a
        // user picked it.
        for accent in [
            AccentColor::Yellow,
            AccentColor::Peach,
            AccentColor::Maroon,
            AccentColor::Blue,
            AccentColor::Teal,
        ] {
            let settings = AppearanceSettings {
                accent_titlebars: true,
                accent_color: accent,
                ..AppearanceSettings::default()
            };
            let colors = DecorationColors::from_settings(&settings);
            assert_eq!(
                colors.title_focused_bg,
                settings.effective_accent(),
                "{accent:?} did not reach the focused title bar"
            );
            assert_eq!(
                colors.title_focused_fg,
                readable_on(settings.effective_accent()),
                "{accent:?} got a title colour that was not chosen for it"
            );
        }
    }

    #[test]
    fn accented_title_bars_leave_the_unfocused_windows_in_the_base_palette() {
        // Deliberate: an accent that marks every window marks none of them.
        let settings = AppearanceSettings {
            accent_titlebars: true,
            accent_color: AccentColor::Red,
            ..AppearanceSettings::default()
        };
        let accented = DecorationColors::from_settings(&settings);
        let base = DecorationColors::for_mode(false);

        assert_eq!(accented.title_unfocused_bg, base.title_unfocused_bg);
        assert_eq!(accented.title_unfocused_fg, base.title_unfocused_fg);
        assert_ne!(
            accented.title_focused_bg, base.title_focused_bg,
            "the setting did nothing at all — the assertions above would then \
             hold for the wrong reason"
        );
    }

    /// The WCAG contrast ratio between two opaque colours.
    ///
    /// 4.5 is the AA threshold for body text and 3.0 the one for large text;
    /// a title bar's own label is large and bold enough for the latter.
    fn contrast(a: Color, b: Color) -> f32 {
        fn channel(c: u8) -> f32 {
            let c = f32::from(c) / 255.0;
            if c <= 0.039_28 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        fn luminance(c: Color) -> f32 {
            0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
        }
        let (l1, l2) = (luminance(a), luminance(b));
        let (hi, lo) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// Every title bar the settings can produce is one you can read the title
    /// on.
    ///
    /// Stronger than checking that `readable_on` was *called*: this measures
    /// what it produced. A `readable_on` whose luma threshold drifted would
    /// still be called from the right place and would still return one of the
    /// two palette extremes — it would just return the wrong one, on some
    /// accents and not others, and only a ratio catches that.
    #[test]
    fn a_title_is_readable_on_every_bar_the_settings_can_produce() {
        for &accent in AccentColor::presets() {
            for (mode, base_light) in [(ThemeMode::Dark, false), (ThemeMode::Light, true)] {
                for accent_titlebars in [false, true] {
                    let settings = AppearanceSettings {
                        theme_mode: mode,
                        accent_color: accent,
                        accent_titlebars,
                        ..AppearanceSettings::default()
                    };
                    let colors = DecorationColors::from_settings(&settings);
                    let what = format!("{mode:?} {accent:?} accented={accent_titlebars}");

                    let focused = contrast(colors.title_focused_fg, colors.title_focused_bg);
                    assert!(focused >= 4.5, "{what}: focused title {focused:.2}");
                    let unfocused = contrast(colors.title_unfocused_fg, colors.title_unfocused_bg);
                    assert!(unfocused >= 3.0, "{what}: unfocused title {unfocused:.2}");

                    // An accent that marked every bar would mark none of them,
                    // and the two ratios above would then hold for one bar
                    // twice rather than for both.
                    let base = DecorationColors::for_mode(base_light);
                    assert_eq!(colors.title_unfocused_bg, base.title_unfocused_bg, "{what}");
                    if accent_titlebars {
                        assert_ne!(colors.title_focused_bg, colors.title_unfocused_bg, "{what}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_mode_still_decides_the_palette_when_the_accent_is_off() {
        // `from_settings` with nothing accented must be exactly `for_mode`, or
        // the two ways of asking the same question have started to drift.
        for (mode, light) in [(ThemeMode::Dark, false), (ThemeMode::Light, true)] {
            let settings = AppearanceSettings {
                theme_mode: mode,
                accent_titlebars: false,
                ..AppearanceSettings::default()
            };
            assert_eq!(
                DecorationColors::from_settings(&settings),
                DecorationColors::for_mode(light),
                "{mode:?} did not resolve to its own palette"
            );
        }
    }

    #[test]
    fn readable_on_answers_with_the_palettes_own_extremes() {
        // Not pure black and white: an accented title bar with `#000` text
        // beside a taskbar with `#11111B` text is two different blacks a few
        // pixels apart, which reads as a rendering fault rather than a style.
        assert_eq!(
            readable_on(Color::from_hex(0xF9E2AF)),
            Color::from_hex(0x11111B)
        );
        assert_eq!(
            readable_on(Color::from_hex(0x1E1E2E)),
            Color::from_hex(0xEFF1F5)
        );
        // A saturated blue is dark to the eye however bright its one channel
        // is; averaging the channels instead of weighting them would call this
        // one light and put black text on it.
        assert_eq!(
            readable_on(Color::from_hex(0x0000FF)),
            Color::from_hex(0xEFF1F5)
        );
    }

    #[test]
    fn emphasis_stays_visible_at_both_ends_of_the_range() {
        // A pressed state derived by "darken by 25%" disappears on an accent
        // that is already black. This moves away from whichever extreme the
        // colour is nearer, so both ends move.
        for base in [Color::from_hex(0x000000), Color::from_hex(0xFFFFFF)] {
            assert_ne!(
                emphasized(base),
                base,
                "{base:?} pressed looks exactly like {base:?} at rest"
            );
        }
    }

    // ---- Palette ----

    /// Perceived brightness, for the ordering assertions below.
    fn luma(c: Color) -> f32 {
        0.299 * f32::from(c.r) + 0.587 * f32::from(c.g) + 0.114 * f32::from(c.b)
    }

    #[test]
    fn every_dark_constant_is_the_published_catppuccin_mocha_value() {
        // Transcribed from the Catppuccin Mocha palette, independently of the
        // constants under test — that independence *is* the test. Everything
        // else in this module compares one part of the tree against another,
        // which cannot see an error the whole tree shares.
        //
        // It found one. `SKY` was `0x89DCFE` for the life of the crate, a
        // transposed byte pair, and had already been copied into two
        // applications. Nothing could have caught it: 2,258 duplicate
        // declarations across `apps/` all agree with each other, and the two
        // that agreed with *this* file were the two that were wrong.
        //
        // Only the dark values are pinned. The `LIGHT_*` accents deliberately
        // depart from published Latte — they are darkened to carry text, which
        // is what `every_role_a_user_reads_is_legible_on_the_base_of_its_own_palette`
        // asserts and what §525 records — so pinning them here would assert
        // the opposite of the decision that produced them.
        // `overlay1` and `overlay2` are published but not declared here, and
        // are deliberately absent rather than listed: an entry pairing
        // `Color::from_hex(0x7F849C)` with `0x7F849C` asserts a value against
        // itself, which is the vacuous shape this whole file's tests are
        // written to avoid. A constant this crate does not have is not a
        // constant this test can check.
        let published: [(&str, Color, u32); 24] = [
            ("rosewater", ROSEWATER, 0xF5E0DC),
            ("flamingo", FLAMINGO, 0xF2CDCD),
            ("pink", PINK, 0xF5C2E7),
            ("mauve", MAUVE, 0xCBA6F7),
            ("red", RED, 0xF38BA8),
            ("maroon", MAROON, 0xEBA0AC),
            ("peach", PEACH, 0xFAB387),
            ("yellow", YELLOW, 0xF9E2AF),
            ("green", GREEN, 0xA6E3A1),
            ("teal", TEAL, 0x94E2D5),
            ("sky", SKY, 0x89DCEB),
            ("sapphire", SAPPHIRE, 0x74C7EC),
            ("blue", BLUE, 0x89B4FA),
            ("lavender", LAVENDER, 0xB4BEFE),
            ("text", TEXT, 0xCDD6F4),
            ("subtext1", SUBTEXT1, 0xBAC2DE),
            ("subtext0", SUBTEXT0, 0xA6ADC8),
            ("overlay0", OVERLAY0, 0x6C7086),
            ("surface2", SURFACE2, 0x585B70),
            ("surface1", SURFACE1, 0x45475A),
            ("surface0", SURFACE0, 0x313244),
            ("base", BASE, 0x1E1E2E),
            ("mantle", MANTLE, 0x181825),
            ("crust", CRUST, 0x11111B),
        ];
        for (name, got, want) in published {
            assert_eq!(
                got,
                Color::from_hex(want),
                "{name} is not the published Mocha value 0x{want:06X}"
            );
        }
    }

    /// Every field of the palette, paired with its name, for the sweeps that
    /// have to cover all of them rather than a sample.
    ///
    /// Delegates to [`Palette::roles`] rather than keeping a second list.
    /// It was a second list until the shell's conversion sweep needed the
    /// same enumeration from outside this crate — at which point two
    /// hand-written lists of the same fields would have been exactly the
    /// keep-them-in-step arrangement this crate exists to abolish, and the
    /// one that would have gone stale is this one, because it is the copy
    /// nothing outside the file can see.
    fn roles(p: &Palette) -> [(&'static str, Color); 21] {
        p.roles()
    }

    #[test]
    fn every_role_has_a_different_value_in_the_two_modes() {
        // The failure this guards is specific and was the whole defect: a
        // light palette assembled by copying the dark one and editing it, with
        // one line left unedited. The symptom is a single element that stays
        // dark in light mode — one unreadable label on an otherwise correct
        // page — which is far harder to notice than a page that did not change
        // at all.
        let dark = roles(&Palette::for_mode(false));
        let light = roles(&Palette::for_mode(true));
        for ((name, d), (_, l)) in dark.into_iter().zip(light) {
            assert_ne!(d, l, "{name} is the same colour in both modes");
        }
    }

    #[test]
    fn every_role_a_user_reads_is_legible_on_the_base_of_its_own_palette() {
        // 4.5:1 is the WCAG AA floor for body text. This is the invariant that
        // the whole light palette exists to satisfy and the reason its accents
        // are darkened: Catppuccin's published Latte values are tuned for
        // decoration and most of them measure between 2.3:1 and 2.8:1 here.
        //
        // `overlay0` is deliberately absent. It is the separator/placeholder
        // role and is documented as not carrying text; asserting a floor it is
        // not meant to meet would either fail or force it brighter than the
        // hairlines it draws should be.
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for (name, c) in roles(&p) {
                if matches!(
                    name,
                    "crust" | "mantle" | "base" | "surface0" | "surface1" | "surface2" | "overlay0"
                ) {
                    continue;
                }
                let ratio = contrast(c, p.base);
                assert!(
                    ratio >= 4.5,
                    "{name} on base is {ratio:.2}:1 in {} mode, under the 4.5:1 body-text floor",
                    if light { "light" } else { "dark" }
                );
            }
        }
    }

    #[test]
    fn the_surface_ladder_climbs_away_from_the_base_in_both_modes() {
        // The reason a caller may name a role and forget the mode. "Raised"
        // is lighter in Mocha and darker in Latte, so no assertion about
        // brightness can hold for both — but *distance from the base* rises
        // monotonically in each, and that is what a caller is actually asking
        // for when it reaches for surface1 over surface0.
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let ladder = [
                ("surface0", p.surface0),
                ("surface1", p.surface1),
                ("surface2", p.surface2),
                ("overlay0", p.overlay0),
                ("subtext0", p.subtext0),
                ("subtext1", p.subtext1),
                ("text", p.text),
            ];
            for pair in ladder.windows(2) {
                let [(lo, lo_c), (hi, hi_c)] = pair else {
                    unreachable!("windows(2) yields pairs")
                };
                assert!(
                    contrast(*hi_c, p.base) > contrast(*lo_c, p.base),
                    "{hi} is no further from the base than {lo} in {} mode",
                    if light { "light" } else { "dark" }
                );
            }
        }
    }

    #[test]
    fn the_recessed_layers_are_darker_than_the_base_in_both_modes() {
        // Not symmetrical with the ladder above, and that asymmetry is real:
        // Latte's crust and mantle are *darker* than its base, exactly as
        // Mocha's are. A recess reads as a recess because light falls into it
        // less, which does not flip with the theme the way a raised surface
        // does. A caller drawing the well of a text input can therefore rely
        // on crust being the darker one either way.
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let mode = if light { "light" } else { "dark" };
            assert!(
                luma(p.crust) < luma(p.mantle),
                "crust is not deeper than mantle in {mode} mode"
            );
            assert!(
                luma(p.mantle) < luma(p.base),
                "mantle is not deeper than base in {mode} mode"
            );
        }
    }

    #[test]
    fn every_named_hue_agrees_with_the_accent_of_the_same_name() {
        // The two ways to reach a hue must not become two answers. `hue()`
        // exists for the settings page, which draws all fourteen swatches; the
        // named fields exist for the shell and the applications, which use ten
        // of them as categorical colours. A swatch that is not the colour it
        // selects is
        // the exact bug this crate was created to stop.
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let named = [
                (AccentColor::Blue, p.blue),
                (AccentColor::Green, p.green),
                (AccentColor::Red, p.red),
                (AccentColor::Yellow, p.yellow),
                (AccentColor::Peach, p.peach),
                (AccentColor::Lavender, p.lavender),
                (AccentColor::Mauve, p.mauve),
                (AccentColor::Sapphire, p.sapphire),
                (AccentColor::Teal, p.teal),
                (AccentColor::Sky, p.sky),
            ];
            for (accent, field) in named {
                assert_eq!(
                    p.hue(accent),
                    field,
                    "{accent:?} reads differently through hue() than through its field"
                );
            }
        }
    }

    #[test]
    fn the_accent_setting_moves_the_accent_and_leaves_the_categorical_hues_alone() {
        // The distinction the type's documentation makes, asserted in both
        // directions. If `accent` did not follow the setting the theme would
        // be decorative again; if the named hues *did* follow it, a user who
        // picked Red would get a resource graph whose CPU and temperature
        // lines were the same colour.
        //
        // Swept over *both* modes, and that is not padding. This test read
        // only the dark palette until the reintroduction harness put a
        // hue-collapse behind `if settings.theme_mode.is_light()` and watched
        // it go by (defect U). A light-mode-only defect is the more likely
        // one, too: the light arm of `for_mode` is the newer code and the one
        // a reader checks less.
        //
        // Note what is *not* provable here: writing the chosen accent into the
        // field of the same name is invisible, because the two are equal by
        // construction — that is what
        // `every_named_hue_agrees_with_the_accent_of_the_same_name` asserts.
        // The reachable defect is the accent landing on some *other* hue, so
        // that is what the sweep below covers.
        for light in [false, true] {
            let mode = if light {
                ThemeMode::Light
            } else {
                ThemeMode::Dark
            };
            let default = Palette::for_mode(light);
            for accent in [AccentColor::Red, AccentColor::Green, AccentColor::Teal] {
                let settings = AppearanceSettings {
                    accent_color: accent,
                    theme_mode: mode,
                    ..AppearanceSettings::default()
                };
                let p = Palette::from_settings(&settings);
                assert_eq!(
                    p.accent,
                    settings.effective_accent(),
                    "{accent:?} did not reach the palette in {mode:?} mode"
                );
                for (name, got, want) in [
                    ("blue", p.blue, default.blue),
                    ("green", p.green, default.green),
                    ("red", p.red, default.red),
                    ("yellow", p.yellow, default.yellow),
                    ("peach", p.peach, default.peach),
                    ("lavender", p.lavender, default.lavender),
                    ("mauve", p.mauve, default.mauve),
                    ("sapphire", p.sapphire, default.sapphire),
                    ("teal", p.teal, default.teal),
                    ("sky", p.sky, default.sky),
                ] {
                    assert_eq!(
                        got, want,
                        "{accent:?} moved the {name} category in {mode:?} mode"
                    );
                }
            }
        }
    }

    #[test]
    fn a_custom_accent_reaches_the_palette_exactly_as_chosen() {
        // `effective_accent` darkens a *preset* for light mode and deliberately
        // does not darken a custom colour. The palette must not add a second
        // opinion on top of that one.
        let chosen = Color::from_hex(0x123456);
        for mode in [ThemeMode::Dark, ThemeMode::Light] {
            let settings = AppearanceSettings {
                theme_mode: mode,
                accent_color: AccentColor::Custom,
                custom_accent: chosen,
                ..AppearanceSettings::default()
            };
            assert_eq!(Palette::from_settings(&settings).accent, chosen);
        }
    }

    #[test]
    fn transparency_reaches_panels_and_nothing_behind_them() {
        // Alpha belongs to the surface that floats, not to the palette. A list
        // row inside a translucent menu must stay opaque, or the wallpaper
        // shows through the row and not through its container — which looks
        // like a rendering fault rather than a setting.
        let settings = AppearanceSettings {
            transparency: TransparencyLevel::Full,
            ..AppearanceSettings::default()
        };
        let p = Palette::from_settings(&settings);
        assert_eq!(p.panel_alpha, TransparencyLevel::Full.panel_alpha());
        assert_eq!(p.panel_bg().a, p.panel_alpha);
        assert_eq!(p.panel_hover().a, p.panel_alpha);
        assert!(p.panel_alpha < 255, "the level under test is opaque");

        for (name, c) in roles(&p) {
            assert_eq!(c.a, 255, "{name} became translucent");
        }

        // And the off switch means off.
        let opaque = AppearanceSettings {
            transparency: TransparencyLevel::Off,
            ..AppearanceSettings::default()
        };
        let p = Palette::from_settings(&opaque);
        assert_eq!(p.panel_bg(), p.base);
        assert_eq!(p.panel_hover(), p.surface1);
    }

    #[test]
    fn the_accent_washes_are_the_accent_and_differ_only_in_how_much_shows() {
        // Five overlays that were five hand-picked alphas in five modules.
        // Their hue has to be the accent — a selection rectangle drawn in a
        // fixed blue on a red-themed desktop is the defect this task is about,
        // one layer down — and their strengths have to be ordered, because
        // that ordering is the only thing distinguishing them.
        let settings = AppearanceSettings {
            accent_color: AccentColor::Mauve,
            ..AppearanceSettings::default()
        };
        let p = Palette::from_settings(&settings);
        let washes = [
            ("hint_fill", p.hint_fill()),
            ("selection_fill", p.selection_fill()),
            ("highlight_fill", p.highlight_fill()),
            ("hint_border", p.hint_border()),
            ("selection_border", p.selection_border()),
        ];
        for (name, c) in washes {
            assert_eq!(
                (c.r, c.g, c.b),
                (p.accent.r, p.accent.g, p.accent.b),
                "{name} is not the accent"
            );
            assert!(c.a > 0 && c.a < 255, "{name} is not a wash at all");
        }
        for pair in washes.windows(2) {
            let [(lo, lo_c), (hi, hi_c)] = pair else {
                unreachable!("windows(2) yields pairs")
            };
            assert!(hi_c.a > lo_c.a, "{hi} is no stronger than {lo}");
        }

        // A drop target is shown at the same instant as a selection, so it has
        // to differ by more than strength.
        assert_ne!(
            (p.drop_target().r, p.drop_target().g, p.drop_target().b),
            (p.accent.r, p.accent.g, p.accent.b),
            "a drop target is indistinguishable from a selection"
        );
    }

    #[test]
    fn the_scrim_and_the_shadows_darken_whichever_palette_they_fall_on() {
        // The one place the light palette forced a change rather than a
        // translation. The shell dimmed with its own `base` at alpha, which in
        // Latte is `#EFF1F5` — so a "dim the desktop" layer would have
        // *lightened* it and left the modal with nothing to stand against.
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let mode = if light { "light" } else { "dark" };
            for (name, layer) in [
                ("scrim", p.scrim()),
                ("shadow", p.shadow()),
                ("text_shadow", p.text_shadow()),
            ] {
                assert!(
                    luma(layer.over(p.base)) < luma(p.base),
                    "{name} does not darken the base in {mode} mode"
                );
                assert!(
                    layer.a < 255,
                    "{name} is opaque and would hide what it dims"
                );
            }
            assert!(
                p.text_shadow().a > p.shadow().a,
                "a label's shadow must be stronger than a panel's — it lands on \
                 an arbitrary wallpaper, not on a known surface"
            );
        }
    }

    #[test]
    fn what_is_drawn_on_the_accent_is_chosen_for_the_accent() {
        // The accent is the one colour whose brightness the user controls, so
        // a fixed foreground on it is unreadable for some of the fourteen
        // choices. Yellow needs dark text and maroon needs light.
        for accent in AccentColor::presets() {
            for mode in [ThemeMode::Dark, ThemeMode::Light] {
                let settings = AppearanceSettings {
                    theme_mode: mode,
                    accent_color: *accent,
                    ..AppearanceSettings::default()
                };
                let p = Palette::from_settings(&settings);
                let ratio = contrast(p.on_accent(), p.accent);
                assert!(
                    ratio >= 4.5,
                    "{accent:?} in {mode:?} mode carries text at only {ratio:.2}:1"
                );
            }
        }
    }

    #[test]
    fn a_window_frame_is_built_from_the_palette_of_its_own_mode() {
        // `DecorationColors::for_mode` used to be two hand-written tables of
        // hex whose correspondence was asserted only by a comment. This is
        // that comment made checkable: a frame is surface0-on-base with a
        // surface2 border in *either* mode, because it reads those roles
        // rather than repeating their values.
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let f = DecorationColors::for_mode(light);
            assert_eq!(f.title_focused_bg, p.surface0);
            assert_eq!(f.title_focused_fg, p.text);
            assert_eq!(f.title_unfocused_bg, p.base);
            assert_eq!(f.title_unfocused_fg, p.subtext0);
            assert_eq!(f.border_focused, p.surface2);
            assert_eq!(f.border_unfocused, p.surface1);
            assert_eq!(f.close_button, p.red);
            assert_eq!(f.maximize_button, p.green);
            assert_eq!(f.minimize_button, p.yellow);
            assert_eq!(f.desktop_bg, p.crust);
        }
    }

    #[test]
    fn a_window_button_keeps_its_meaning_when_the_accent_changes() {
        // Stop, go, and the middling one. These three are the palette's fixed
        // hues and not the accent, so that "close" does not become whatever
        // colour the desktop happens to be themed around — which on a red
        // theme would make close, maximize and minimize identical.
        let plain = DecorationColors::for_mode(false);
        for accent in [AccentColor::Red, AccentColor::Green, AccentColor::Yellow] {
            let settings = AppearanceSettings {
                accent_color: accent,
                accent_titlebars: true,
                ..AppearanceSettings::default()
            };
            let f = DecorationColors::from_settings(&settings);
            assert_eq!(f.close_button, plain.close_button, "{accent:?}");
            assert_eq!(f.maximize_button, plain.maximize_button, "{accent:?}");
            assert_eq!(f.minimize_button, plain.minimize_button, "{accent:?}");
        }
    }
}
