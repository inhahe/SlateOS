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

pub mod config;

use guitk::color::Color;
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
pub const SKY: Color = Color::from_hex(0x89DCFE);
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
// Configuration-file spellings
// ============================================================================

/// Give an enum a spelling in the configuration file.
///
/// These names are deliberately **not** [`label`](ThemeMode::label). A label is
/// what the user reads on screen — "Extra Large (96px)", "Accent Color" — and
/// it changes when the wording is improved or the size preset is retuned. A
/// config spelling is part of the file format: change it and every existing
/// user's saved choice silently reverts to the default the next time the
/// desktop starts. Keeping them separate means the UI text is free to move.
macro_rules! yaml_enum {
    ($ty:ty { $($variant:ident => $name:literal),+ $(,)? }) => {
        impl $ty {
            /// This value's spelling in the configuration file.
            pub fn yaml_name(self) -> &'static str {
                match self {
                    $(Self::$variant => $name,)+
                }
            }

            /// The value a configuration file spelling names.
            ///
            /// `None` for a spelling this build does not know, which is how a
            /// file written by a newer desktop degrades to the default rather
            /// than refusing to load.
            pub fn from_yaml_name(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

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
        doc.set_str(&["theme", "custom_accent"], &color_to_hex(self.custom_accent));
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
        for accent in AccentColor::presets().iter().copied().chain([AccentColor::Custom]) {
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
        assert_eq!(color_from_hex("#89b4fa"), Some(Color::rgb(0x89, 0xB4, 0xFA)));
        assert_eq!(color_from_hex("#89B4FA"), Some(Color::rgb(0x89, 0xB4, 0xFA)));
        assert_eq!(color_from_hex("#01020304"), Some(Color::rgba(1, 2, 3, 4)));
        for bad in ["89b4fa", "#89b4f", "#gggggg", "#", "", "#89b4fa00ff"] {
            assert_eq!(color_from_hex(bad), None, "{bad} should not parse");
        }
    }
}
