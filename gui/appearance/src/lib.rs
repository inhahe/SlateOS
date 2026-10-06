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
//!
//! # The two scanners that keep this crate's decisions true
//!
//! Both live beside this file, are tracked, and are the *standing* checks for
//! properties nothing in `cargo test` can see -- because they are properties
//! of the call sites, not of this crate.
//!
//! | script | the question it answers |
//! |---|---|
//! | `convert-fills.py` | does any box still fill a surface role directly, instead of asking [`Palette::surface_paint`]? (§829) |
//! | `ink-text.py` | does any text still name a dual-use role directly, instead of asking [`Palette::ink`]? (§837) |
//!
//! `ink-text.py` has three modes and the difference between them is the point:
//! `--check` counts sites that should be converted, `--verify` finds the
//! opposite error -- a fill or a stroke that asked for a *text* ink -- and
//! `--blind` reports the sites neither can classify, because their colour
//! arrives from a method rather than naming a role.
//!
//! That third mode exists because "`--check` says zero" reads as "the
//! conversion is complete" when it means "complete among the sites I can
//! classify". 108 methods return a themed colour and none of them name a role
//! at the draw site; see `known-issues.md`
//! `TD-C-FORTY-NINE-COLOUR-METHODS-ARE-INVISIBLE-TO-THE-INK-SWEEP`.

/// The sweep that proves a module draws from the palette rather than from
/// colours of its own.
///
/// Behind a feature rather than `#[cfg(test)]`, because `cfg(test)` is set only
/// when *this* crate is under test and never when a dependent is -- the same
/// reason `settingsfile::testing` is a feature. It lived in `gui/desktop` until
/// the blur pass moved to `gui/compositor` and took its tests with it: two
/// crates needed it, and a helper about `Palette` belongs with `Palette`.
#[cfg(feature = "testing")]
pub mod palette_check;

pub mod icons;
// Documented inside the module only: a doc comment here as well made rustdoc
// resolve the module's own links from this scope, where its items are not.
pub mod themes;

pub mod decorations;

pub mod cursors;

pub mod sounds;

pub mod panel;

pub mod themecheck;

/// Where settings files live and how they are replaced.
///
/// This was `appearance::config` before it was a crate of its own, and it is
/// re-exported under the old name because the path is used across the shell,
/// the compositor and the Settings application, and none of those call sites
/// were wrong. See `settingsfile`'s own documentation for why it moved: it is
/// not about appearance, and `inputsettings` needs it without needing colours.
pub use settingsfile as config;

use core::num::NonZeroU32;
use core::time::Duration;
use datetimesettings::Tz;
pub use daywindow::{DailyWindow, TimeOfDay};
use guitk::color::Color;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use yamldoc::Document;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

// ============================================================================
// Configuration-file spellings
// ============================================================================

// The macro lives in `settingsfile` rather than here: `inputsettings` needs the
// same thing, and a macro copied between two settings crates is two file
// formats waiting to disagree about how a name is spelled.
use settingsfile::yaml_enum;

// The palette, its surfaces and the colour ramps now live in `guitk`, and
// design-decisions 838 records why: `appearance` depends on the toolkit, so
// for as long as the type lived here no widget could name it, and eleven
// widget files kept private tables of dark colours instead. Re-exported
// rather than moved-and-renamed, so `use appearance::Palette;` in 153 files
// is untouched and no caller has to learn where the type lives.
// A glob, and deliberately: the list is fifty names and a hand-written one
// is a list that silently loses an entry. The first version of this re-export
// named them individually and left out eleven of the light ramp, which the
// tests found -- but a *caller* would have found it as a missing symbol in
// someone else's crate, which is a worse way to learn.
pub use guitk::palette::*;
pub use guitk::surface::{
    CommandSink, Edge, Surface, SurfacePaint, logical_rect, paint_at, painted_rect,
};

// ---------------------------------------------------------------------------
// The two style enums' file spellings
// ---------------------------------------------------------------------------
//
// Free functions rather than `yaml_enum!`, because the macro writes an
// *inherent* impl and the types now live in `guitk` (838). Keeping them here
// is the point: which values a build understands and how they are spelled in
// `appearance.yaml` is a property of the settings file, not of the toolkit,
// and dragging `settingsfile` into a widget library to regain two method
// names would be a poor trade.

/// `SurfaceStyle`'s spelling in the configuration file.
#[must_use]
pub fn surface_style_yaml_name(style: SurfaceStyle) -> &'static str {
    match style {
        SurfaceStyle::Borders => "borders",
        SurfaceStyle::Cards => "cards",
    }
}

/// Where `appearance.yaml` keeps the filled look's own colours: `theme.cards`,
/// under the look's own spelling (`design-decisions.md` §1421). The outlined
/// look's are `theme.accent` and `theme.custom_accent` themselves, where the
/// one accent was written before each look kept its own.
const FILLED_LOOK_KEY: &str = "cards";

/// The `SurfaceStyle` a configuration file spelling names.
///
/// `None` for a spelling this build does not know, which is how a file
/// written by a newer desktop degrades to the default rather than refusing to
/// load -- the same rule `yaml_enum!` applies to every other setting.
#[must_use]
pub fn surface_style_from_yaml_name(name: &str) -> Option<SurfaceStyle> {
    match name {
        "borders" => Some(SurfaceStyle::Borders),
        "cards" => Some(SurfaceStyle::Cards),
        _ => None,
    }
}

/// `StripStyle`'s spelling in the configuration file.
#[must_use]
pub fn strip_style_yaml_name(style: StripStyle) -> &'static str {
    match style {
        StripStyle::Filled => "filled",
        StripStyle::Separator => "separator",
    }
}

/// The `StripStyle` a configuration file spelling names.
#[must_use]
pub fn strip_style_from_yaml_name(name: &str) -> Option<StripStyle> {
    match name {
        "filled" => Some(StripStyle::Filled),
        "separator" => Some(StripStyle::Separator),
        _ => None,
    }
}

/// How the toolkit's palette reads this desktop's saved preferences.
///
/// The other half of the seam design-decisions 838 describes: `guitk` says
/// what it needs to know and this says it. Everything here is a preference --
/// which accent, which scheme, how transparent -- and none of it is visible
/// to a widget, which sees only the resolved [`Palette`].
impl AccentColor {
    /// This accent's colour in the given mode.
    ///
    /// Was `Palette::hue(accent)` until 838 moved `Palette` into the toolkit,
    /// which cannot name an `AccentColor`. The same answer, asked of the
    /// accent rather than of the palette -- and arguably the better home: an
    /// accent knows its own two values, while a palette only knows the one it
    /// was resolved with.
    ///
    /// For the settings page, which draws all fourteen as swatches and must
    /// draw them in the mode the user is currently in -- otherwise the swatch
    /// chosen is not the colour that appears when it is chosen. Agrees with
    /// the palette's named-hue fields by construction, which
    /// `every_named_hue_agrees_with_the_accent_of_the_same_name` asserts.
    #[must_use]
    pub fn in_mode(self, light: bool) -> Color {
        if light {
            self.color_light()
        } else {
            self.color()
        }
    }
}

impl PaletteSource for AppearanceSettings {
    fn is_light(&self) -> bool {
        AppearanceSettings::is_light(self)
    }

    fn accent(&self) -> Color {
        self.effective_accent()
    }

    fn accent_on(&self, background: Color) -> Color {
        if self.accent_color == AccentColor::Custom {
            // The user named a colour. Choosing a different one for them
            // because it reads better is not this function's call.
            return self.custom_accent;
        }
        let dark_value = self.accent_color.color();
        let light_value = self.accent_color.color_light();
        if contrast_ratio(dark_value, background) >= contrast_ratio(light_value, background) {
            dark_value
        } else {
            light_value
        }
    }

    fn surface_style(&self) -> SurfaceStyle {
        self.surface_style
    }

    fn strip_style(&self) -> StripStyle {
        self.strip_style
    }

    fn panel_alpha(&self) -> u8 {
        self.transparency.panel_alpha()
    }

    fn high_contrast(&self) -> Option<(Color, Color)> {
        self.high_contrast
            .map(|scheme| (scheme.background(), scheme.text()))
    }

    fn theme(&self) -> Option<&ThemeColors> {
        self.color_theme.colors()
    }

    fn widget_style(&self) -> guitk::widget_style::WidgetStyle {
        self.widget_theme.style()
    }

    /// The animation theme's motion at the user's speed: Slow makes the
    /// theme's transitions half again as long, and Off -- like a theme's own
    /// `enabled: false` -- is still.
    fn motion(&self) -> guitk::motion::Motion {
        self.animation_theme
            .motion()
            .at_speed(self.animation_speed.multiplier())
    }

    fn accent_titlebars(&self) -> bool {
        self.accent_titlebars
    }
}

// ============================================================================
// Image fit
// ============================================================================

/// Marks the wallpaper **paths** in a settings file as percent-encoded.
///
/// One marker for the whole group rather than one per key, which is what §426
/// does: it versions the *record*, not each field. The picture and the
/// rotation folder are both paths and are both encoded when this is present.
///
/// design-decisions §426's version marker, in the shape a YAML document can
/// carry: a sibling key rather than a first line. Its absence means the file
/// predates the encoding and its path is raw -- which matters for exactly one
/// kind of path, the kind containing a literal `%`.
const WALLPAPER_ENCODING: &str = "percent";

/// How a wallpaper is scaled and positioned in the space it is drawn in.
///
/// Lives here rather than in `gui/desktop` because two crates need it and only
/// one of them can own it: the shell draws the picture and the Settings app
/// offers the choice. It was in `gui/desktop/src/wallpaper.rs` until
/// 2026-09-14, which made it unreachable from `appearance.yaml` -- the setting
/// could name a picture and not how to place it. design-decisions 852 records
/// why that shipped as a gap rather than as a second copy of this enum.
/// Background style for the login screen.
///
/// Lives here rather than in the greeter that draws it because it is a
/// setting: `appearance.yaml` names it under `login.background`, the Settings
/// app offers it, and `gui/desktop`'s `login_screen` re-exports this type
/// rather than defining a second one to convert to and from.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum LoginBackground {
    /// Whatever the theme's deepest surface is — resolved at render time.
    ///
    /// The default, and the only variant that names no colour. A variant was
    /// needed because [`Default::default`] takes no arguments and so cannot be
    /// handed a [`Palette`]: the previous default was `SolidColor(CRUST)` with
    /// `CRUST` a Mocha literal in this file, which is why the greeter stayed
    /// black for a user who had asked for the light theme. Deferring the
    /// question to `render` is what makes the answer follow the mode.
    #[default]
    Theme,
    /// Solid color, chosen by the user.
    ///
    /// Drawn exactly as given. This is not a theme role and is not re-themed:
    /// a user who picked a colour picked *that* colour.
    SolidColor(Color),
    /// Whatever the desktop is showing behind the session, right now.
    ///
    /// **Carries no path, deliberately.** The obvious shape is to store the
    /// wallpaper's path here when the user ticks "same as my desktop", and it
    /// is wrong: the desktop's picture is not a constant. A rotation folder
    /// changes it every `wallpaper.interval_secs`, and a stored copy would go
    /// stale on the first change -- the greeter would show the picture that
    /// was up when the box was ticked, for ever, while calling itself "same
    /// as desktop". Holding no path is what makes the name true: the session
    /// asks the wallpaper what it is showing at the moment it paints.
    SameAsDesktop,
    /// A picture chosen for the greeter alone, which the desktop does not share.
    ///
    /// A `PathBuf` rather than a `String` because a filename may hold any byte
    /// but `/` and NUL -- the same reason `AppearanceSettings::wallpaper` is
    /// one. A `String` here could not name every file a user may legitimately
    /// pick, and the ones it could not name are exactly the ones a lossy
    /// conversion would silently turn into a *different* path.
    CustomImage(PathBuf),
    /// Gradient between two colors, chosen by the user. Drawn as given.
    Gradient { top: Color, bottom: Color },
}

impl LoginBackground {
    /// The spelling `appearance.yaml` uses under `login.background`.
    ///
    /// Every variant has one. A style the file cannot name is a style the user
    /// can reach from the Settings app and then lose on the next save, which
    /// is worse than not offering it.
    #[must_use]
    pub const fn yaml_name(&self) -> &'static str {
        match self {
            Self::Theme => "theme",
            Self::SolidColor(_) => "color",
            Self::SameAsDesktop => "desktop",
            Self::CustomImage(_) => "image",
            Self::Gradient { .. } => "gradient",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFit {
    /// Scale to cover the entire area, cropping if necessary.
    Fill,
    /// Scale to fit within the area, letterboxing if necessary.
    Fit,
    /// Stretch to exactly match the area (may distort aspect ratio).
    Stretch,
    /// Repeat the image in a tile pattern.
    Tile,
    /// Center the image at native size (no scaling).
    Center,
    /// Span the image across all monitors (multi-monitor setups).
    Span,
}

impl ImageFit {
    /// Every fit, in the order a chooser should offer them.
    ///
    /// Ordered here for the reason `ThemeMode::ALL` states: two front ends that
    /// listed the variants themselves would be free to drift apart. Cropping
    /// first, because it is the default and what most pictures want.
    pub const ALL: &'static [Self] = &[
        Self::Fill,
        Self::Fit,
        Self::Stretch,
        Self::Tile,
        Self::Center,
        Self::Span,
    ];

    /// The name this fit is written as in `appearance.yaml`.
    ///
    /// The same six strings `gui/desktop`'s own wallpaper config already used,
    /// so a file written by the older code still reads.
    #[must_use]
    pub fn yaml_name(self) -> &'static str {
        match self {
            Self::Fill => "fill",
            Self::Fit => "fit",
            Self::Stretch => "stretch",
            Self::Tile => "tile",
            Self::Center => "center",
            Self::Span => "span",
        }
    }

    /// The fit named by `name`, or `None` if it names nothing.
    #[must_use]
    pub fn from_yaml_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|f| f.yaml_name() == name)
    }

    /// What a chooser shows for this fit.
    ///
    /// Says what happens to the picture rather than naming the algorithm: a
    /// user choosing a wallpaper knows whether they mind it being cropped, and
    /// does not know what "fill" means.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Fill => "Fill the screen (may crop)",
            Self::Fit => "Fit the whole picture (may letterbox)",
            Self::Stretch => "Stretch to fit (may distort)",
            Self::Tile => "Tile",
            Self::Center => "Centre at original size",
            Self::Span => "Span all monitors",
        }
    }
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
    /// This is the *setting*. Whether what is drawn is light is
    /// [`AppearanceSettings::is_light`], which also knows about a colour
    /// theme that has only one mode's colours -- ask that for anything that
    /// has to match the palette.
    pub fn is_light(self) -> bool {
        match self {
            Self::Light => true,
            Self::Dark | Self::System => false,
        }
    }
}

// ============================================================================
// Color filter (colour-vision simulation and correction)
// ============================================================================

/// The complement of an 8-bit channel — `255 - value`.
///
/// Written as a bitwise complement because for a `u8` the two are the same
/// value, and unlike the subtraction it cannot underflow for any input.
const fn invert_channel(value: u8) -> u8 {
    !value
}

/// A 3x3 integer matrix that mixes a color's channels: output channel `i` is
/// `(rows[i] . [r, g, b]) / denominator`.
///
/// This type exists so that "recombine the channels with these weights" is
/// written once instead of once per filter. Integer weights over a shared
/// denominator rather than floats because a filter runs once per pixel.
///
/// # Invariant
///
/// **Every row sums to exactly `denominator`.** That single property is what
/// makes a mix well-behaved: it is then a weighted *average* of the inputs, so
/// it maps black to black and white to white, and — since no input channel
/// exceeds 255 — no output channel can either. [`ChannelMix::new`] is the only
/// constructor and it rejects anything else, and because it is a `const fn`
/// every mix in this module is checked when the crate is compiled rather than
/// when a pixel is drawn. That is why [`ChannelMix::apply`] needs no clamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ChannelMix {
    /// One row of weights per output channel, in red-green-blue order.
    rows: [[u32; 3]; 3],
    /// The shared divisor every row sums to.
    denominator: NonZeroU32,
}

/// Whether `row`'s three weights add up to exactly `denominator`.
const fn row_sums_to(row: [u32; 3], denominator: u32) -> bool {
    let [a, b, c] = row;
    match a.checked_add(b) {
        Some(ab) => match ab.checked_add(c) {
            Some(sum) => sum == denominator,
            None => false,
        },
        None => false,
    }
}

/// Unwraps a [`ChannelMix`] that is built in a `const` initializer.
///
/// A `None` here means the weights written a few lines above do not sum to
/// their denominator, which is a typo, not a runtime condition. Panicking in a
/// const context is a *compile* error at the point of use, so this turns the
/// row-sum invariant into something the build enforces at zero runtime cost.
#[allow(
    clippy::panic,
    reason = "evaluated only in a const initializer, where a panic is a build failure"
)]
const fn checked_at_compile_time(mix: Option<ChannelMix>) -> ChannelMix {
    match mix {
        Some(mix) => mix,
        None => panic!("channel mix rows must each sum to the denominator"),
    }
}

impl ChannelMix {
    /// Perceptual luminance weighting: every output channel is the same
    /// weighted average of the input, which is exactly what makes it gray.
    ///
    /// The denominator is 256 rather than 100 so the division is a shift.
    const GRAYSCALE: Self = checked_at_compile_time(Self::new([[77, 150, 29]; 3], 256));
    /// Simplified simulation of red-weak vision.
    const PROTANOPIA: Self =
        checked_at_compile_time(Self::new([[56, 43, 1], [55, 44, 1], [0, 24, 76]], 100));
    /// Simplified simulation of green-weak vision.
    const DEUTERANOPIA: Self =
        checked_at_compile_time(Self::new([[63, 37, 0], [70, 30, 0], [0, 30, 70]], 100));
    /// Simplified simulation of blue-weak vision.
    const TRITANOPIA: Self =
        checked_at_compile_time(Self::new([[95, 5, 0], [0, 43, 57], [0, 47, 53]], 100));

    /// Builds a mix, returning `None` unless the denominator is non-zero and
    /// every row sums to exactly it — see the type's invariant.
    const fn new(rows: [[u32; 3]; 3], denominator: u32) -> Option<Self> {
        let Some(nonzero) = NonZeroU32::new(denominator) else {
            return None;
        };
        let [first, second, third] = rows;
        if row_sums_to(first, denominator)
            && row_sums_to(second, denominator)
            && row_sums_to(third, denominator)
        {
            Some(Self {
                rows,
                denominator: nonzero,
            })
        } else {
            None
        }
    }

    /// Recombines `color`'s channels through this matrix, leaving alpha alone.
    fn apply(self, color: Color) -> Color {
        let input = [u32::from(color.r), u32::from(color.g), u32::from(color.b)];
        let mut out = [0u8; 3];
        for (slot, row) in out.iter_mut().zip(&self.rows) {
            let mut sum = 0u32;
            for (weight, channel) in row.iter().zip(&input) {
                sum = sum.saturating_add(weight.saturating_mul(*channel));
            }
            // The row invariant bounds `sum` by `255 * denominator`, so the
            // quotient always fits in a `u8`. The fallback clamps to white
            // instead of wrapping to black should that ever stop being true.
            *slot = u8::try_from(sum / self.denominator).unwrap_or(u8::MAX);
        }
        let [r, g, b] = out;
        Color::rgba(r, g, b, color.a)
    }
}

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
    /// Every filter, in the order the settings pane offers them.
    ///
    /// Anything that iterates the filters must go through this rather than
    /// write its own list, so that adding a variant cannot leave a stale copy
    /// behind.
    pub const ALL: [Self; 6] = [
        Self::None,
        Self::Protanopia,
        Self::Deuteranopia,
        Self::Tritanopia,
        Self::Grayscale,
        Self::Inverted,
    ];

    /// The channel-mixing matrix this filter applies, or `None` for the two
    /// filters that are not a linear mix of the input channels: [`Self::None`]
    /// changes nothing, and [`Self::Inverted`] is affine (`255 - c`), not
    /// linear.
    const fn channel_mix(&self) -> Option<ChannelMix> {
        match self {
            Self::None | Self::Inverted => None,
            Self::Grayscale => Some(ChannelMix::GRAYSCALE),
            Self::Protanopia => Some(ChannelMix::PROTANOPIA),
            Self::Deuteranopia => Some(ChannelMix::DEUTERANOPIA),
            Self::Tritanopia => Some(ChannelMix::TRITANOPIA),
        }
    }

    /// Apply this filter to one packed ARGB8888 pixel.
    ///
    /// The form a framebuffer needs. It exists beside [`Self::apply`] rather
    /// than at the call site because a second unpack/repack written in the
    /// compositor would be a second definition of what this filter *is*, free
    /// to drift from the first; `the_packed_filter_agrees_with_the_unpacked_one`
    /// holds the two together over every channel value.
    ///
    /// Alpha is carried through untouched. A colour-vision filter answers "what
    /// does this look like", and transparency is not something it can change.
    #[must_use]
    pub fn apply_argb(self, argb: u32) -> u32 {
        // The identity case first and without unpacking: this runs once per
        // pixel of a full frame, and for nearly every user the answer is the
        // pixel it was handed.
        if matches!(self, Self::None) {
            return argb;
        }

        #[allow(clippy::cast_possible_truncation)]
        let (a, r, g, b) = (
            (argb >> 24) as u8,
            (argb >> 16) as u8,
            (argb >> 8) as u8,
            argb as u8,
        );
        let out = self.apply(Color::rgba(r, g, b, a));
        (u32::from(out.a) << 24)
            | (u32::from(out.r) << 16)
            | (u32::from(out.g) << 8)
            | u32::from(out.b)
    }

    /// Apply this filter to a color. Alpha is never touched.
    pub fn apply(&self, color: Color) -> Color {
        match self {
            // Deliberately not routed through the identity matrix, which would
            // give the same answer: this is the overwhelmingly common case and
            // it runs per pixel, so it has to stay a no-op rather than nine
            // multiplies.
            Self::None => color,
            Self::Inverted => Color::rgba(
                invert_channel(color.r),
                invert_channel(color.g),
                invert_channel(color.b),
                color.a,
            ),
            _ => self.channel_mix().map_or(color, |mix| mix.apply(color)),
        }
    }

    /// Label for settings UI.
    /// The name this filter is stored under.
    ///
    /// Separate from [`Self::label`] deliberately: the label is prose shown to
    /// a user and may be reworded, and a stored file must not change meaning
    /// because someone improved a caption.
    #[must_use]
    pub fn yaml_name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Protanopia => "protanopia",
            Self::Deuteranopia => "deuteranopia",
            Self::Tritanopia => "tritanopia",
            Self::Grayscale => "grayscale",
            Self::Inverted => "inverted",
        }
    }

    /// Read a stored name back, or `None` if it names no filter.
    #[must_use]
    pub fn from_yaml_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.yaml_name() == name)
    }

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
// High contrast schemes
// ============================================================================

/// The per-channel gains that warm a finished pixel by `strength`.
///
/// Returned as three multipliers in `0.0..=1.0` rather than applied here, so
/// that a caller filtering a whole frame computes them once and multiplies
/// per pixel. At a 1920x1080 frame that is three floating-point divisions
/// against two million.
///
/// # Where the warm end comes from
///
/// Full strength is 2700 K -- the colour of a domestic warm-white bulb, and
/// the bottom of the range every desktop that has this feature offers. Under
/// Tanner Helland's black-body approximation that is roughly
/// `(255, 169, 87)` against a 6500 K white of `(255, 255, 255)`, which gives
/// the gains below. Red is untouched at every strength: warming a screen means
/// removing blue and some green, not adding red it cannot emit.
///
/// `strength` outside `0.0..=1.0`, or not a number, is clamped to it. A gain
/// is about to multiply every pixel on the display, and a caller that has
/// somehow produced a NaN should get an unchanged screen rather than a black
/// one.
///
/// ```
/// use appearance::night_light_gains;
/// assert_eq!(night_light_gains(0.0), (1.0, 1.0, 1.0), "off changes nothing");
/// let (r, g, b) = night_light_gains(1.0);
/// assert_eq!(r, 1.0, "red is never reduced");
/// assert!(b < g && g < 1.0, "blue is cut hardest");
/// assert_eq!(night_light_gains(f32::NAN), (1.0, 1.0, 1.0));
/// ```
#[must_use]
pub fn night_light_gains(strength: f32) -> (f32, f32, f32) {
    // `clamp` propagates NaN, so the check is explicit rather than implied.
    let s = if strength.is_finite() {
        strength.clamp(0.0, 1.0)
    } else {
        0.0
    };
    const WARM_G: f32 = 169.0 / 255.0;
    const WARM_B: f32 = 87.0 / 255.0;
    (1.0, 1.0 - s * (1.0 - WARM_G), 1.0 - s * (1.0 - WARM_B))
}

/// Warm one packed ARGB pixel by the given gains.
///
/// Alpha is carried through untouched: this runs on a finished frame, where
/// alpha is not a colour but whatever the framebuffer format keeps there, and
/// scaling it would dim the screen rather than warm it.
#[must_use]
pub fn warm_argb(argb: u32, gains: (f32, f32, f32)) -> u32 {
    let (gr, gg, gb) = gains;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "each product is a u8 scaled by a gain in 0..=1, so it lands in 0..=255"
    )]
    let scale = |c: u32, g: f32| -> u32 { (c as f32 * g) as u32 & 0xFF };
    let a = argb & 0xFF00_0000;
    let r = scale((argb >> 16) & 0xFF, gr) << 16;
    let g = scale((argb >> 8) & 0xFF, gg) << 8;
    let b = scale(argb & 0xFF, gb);
    a | r | g | b
}

/// A high-contrast colour scheme: one background, one text colour, no scale
/// between them.
///
/// For users who cannot read an ordinary theme. The whole point is the absence
/// of gradation -- an ordinary palette ranks text by *fading* it, and the
/// faded end is precisely what is unreadable here, so a scheme carries two
/// colours and [`Palette::high_contrast`] paints every neutral role with one
/// or the other.
///
/// The accent is deliberately **not** part of a scheme. It follows the user's
/// Appearance setting, which `design-decisions.md` §816 requires; see
/// [`Palette::high_contrast`] for how its value is chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HighContrastScheme {
    /// Black background, white text. The standard high-contrast look.
    WhiteOnBlack,
    /// White background, black text. The inverse, for glare sensitivity.
    BlackOnWhite,
    /// Yellow on black, which several low-vision guides prefer to white: the
    /// reduced blue component is easier on light sensitivity.
    YellowOnBlack,
    /// Green on black, the terminal look, for minimal eye strain.
    GreenOnBlack,
}

impl HighContrastScheme {
    /// Every scheme, for a settings page that has to offer all of them.
    pub const ALL: [Self; 4] = [
        Self::WhiteOnBlack,
        Self::BlackOnWhite,
        Self::YellowOnBlack,
        Self::GreenOnBlack,
    ];

    /// The colour behind everything.
    #[must_use]
    pub fn background(self) -> Color {
        match self {
            Self::BlackOnWhite => Color::from_hex(0xFFFFFF),
            Self::WhiteOnBlack | Self::YellowOnBlack | Self::GreenOnBlack => {
                Color::from_hex(0x000000)
            }
        }
    }

    /// The colour of every piece of text, at every weight.
    #[must_use]
    pub fn text(self) -> Color {
        match self {
            Self::WhiteOnBlack => Color::from_hex(0xFFFFFF),
            Self::BlackOnWhite => Color::from_hex(0x000000),
            Self::YellowOnBlack => Color::from_hex(0xFFFF00),
            Self::GreenOnBlack => Color::from_hex(0x00FF00),
        }
    }

    /// What to display this scheme as.
    ///
    /// The variant names used to read backwards -- the one now called
    /// `WhiteOnBlack` was `BlackOnWhite`, and drew white text on black. They
    /// were swapped once a second consumer had to map onto them *by meaning*:
    /// `apps/magnifier` has its own `WhiteOnBlack`, which is white-on-black,
    /// so a mapping written by name would have silently picked the opposite
    /// scheme. The stored `yaml_name` strings did not move, so existing
    /// config files still mean what they meant.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::WhiteOnBlack => "White on black",
            Self::BlackOnWhite => "Black on white",
            Self::YellowOnBlack => "Yellow on black",
            Self::GreenOnBlack => "Green on black",
        }
    }

    /// The name this scheme is stored under.
    #[must_use]
    pub fn yaml_name(self) -> &'static str {
        match self {
            Self::WhiteOnBlack => "white_on_black",
            Self::BlackOnWhite => "black_on_white",
            Self::YellowOnBlack => "yellow_on_black",
            Self::GreenOnBlack => "green_on_black",
        }
    }

    /// Read a stored name back, or `None` if it names no scheme.
    #[must_use]
    pub fn from_yaml_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.yaml_name() == name)
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

/// The interface colours a user sets, as one look keeps them.
///
/// `design-decisions.md` §1421, the operator's answer to C-Q15: each look --
/// outlined boxes or filled ones, [`SurfaceStyle`] -- keeps its own, so an
/// accent chosen to suit one is never carried onto the other, where it can
/// read differently (under the filled look the accent is drawn deeper on the
/// grey boxes to keep its text readable). Today that is the accent: which one,
/// and the colour a custom accent names. A struct rather than two loose values
/// so that an interface colour added later is kept per look by being added
/// here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LookColours {
    /// Which accent.
    pub accent_color: AccentColor,
    /// The colour an [`AccentColor::Custom`] accent names.
    pub custom_accent: Color,
}

impl Default for LookColours {
    /// Blue, the accent a machine with no settings file has -- under either
    /// look, so a look nobody has chosen colours for shows the defaults.
    fn default() -> Self {
        Self {
            accent_color: AccentColor::Blue,
            custom_accent: BLUE,
        }
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
// Sound settings
// ============================================================================

/// The volume system sounds play at until the user chooses: a little under
/// full, so a chime does not startle at a volume set for music.
pub const DEFAULT_SOUND_VOLUME: f32 = 0.8;

/// The user's sounds: on or off, how loud, and their own sound for an event
/// -- the `sounds` section of `appearance.yaml`. Which theme the rest come
/// from is [`AppearanceSettings::sound_theme`].
#[derive(Clone, Debug, PartialEq)]
pub struct SoundSettings {
    /// Whether events make sounds at all.
    pub enabled: bool,
    /// How loud they are, 0 to 1.
    pub volume: f32,
    /// The user's own choice for an event, by its sound naming
    /// specification name, over the theme's: `sounds.events.<name>` in the
    /// file, an absolute path or `off`.
    pub events: BTreeMap<String, EventSound>,
}

impl Default for SoundSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            volume: DEFAULT_SOUND_VOLUME,
            events: BTreeMap::new(),
        }
    }
}

/// The user's own choice for one event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventSound {
    /// This file, an absolute path.
    File(PathBuf),
    /// No sound.
    Off,
}

impl EventSound {
    /// The choice `value` -- `sounds.events.<name>` in the file -- spells:
    /// `off`, or an absolute path. `None` for anything else, a relative path
    /// above all: relative to what would be a guess.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value == "off" {
            return Some(Self::Off);
        }
        let path = pathcodec::decode_path(value);
        path.is_absolute().then_some(Self::File(path))
    }

    /// How the file spells it.
    #[must_use]
    pub fn yaml_value(&self) -> String {
        match self {
            Self::Off => "off".to_string(),
            Self::File(path) => pathcodec::encode_path(path),
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
            // The default theme's typeface -- the Aero reference's
            // `font-family` (design-decisions §815) -- and the first of
            // `guitk::text::DEFAULT_UI_FAMILIES`, so a machine that has it
            // draws in it whether or not anything has been saved.
            ui_font: "Open Sans".to_string(),
            mono_font: "JetBrains Mono".to_string(),
            ui_size: 13.0,
            mono_size: 12.0,
            hinting: true,
            subpixel: SubpixelMode::Rgb,
            smoothing: true,
        }
    }
}

/// What became of one configured font family when it was applied.
///
/// Three states rather than a `bool`, because "the user has chosen nothing"
/// and "the user has chosen a font this machine does not have" are different
/// facts with different things to say about them, and a page that collapsed
/// them would report a missing font as a deliberate default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontOutcome {
    /// No family is configured; whatever was in use stays in use.
    Unset,
    /// The family was found and is now in use.
    Applied,
    /// The family is not installed here. The previous, working font was kept
    /// -- losing text to a bad setting is worse than ignoring the setting.
    Missing,
}

/// What [`FontSettings::apply`] managed to install.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontsApplied {
    /// The UI family.
    pub ui: FontOutcome,
    /// The monospace family.
    pub mono: FontOutcome,
}

impl FontSettings {
    /// How glyphs are rasterized under these settings: smoothing, the subpixel
    /// order and hinting, with `palette` -- which of a colour font's palettes
    /// its emoji are painted with -- taken from the theme in force.
    ///
    /// The one mapping from the settings to a rasterizer's terms, for every
    /// process that rasterizes text: the toolkit's own cache (through
    /// [`apply`](Self::apply)) and the compositor's, which must agree or the
    /// same label looks different depending on who drew it.
    #[must_use]
    pub fn rendering(&self, palette: guitk::text::ColourPalette) -> guitk::text::Rendering {
        use guitk::text::Subpixel;
        guitk::text::Rendering {
            smoothing: self.smoothing,
            subpixel: match self.subpixel {
                SubpixelMode::None => Subpixel::None,
                SubpixelMode::Rgb => Subpixel::Rgb,
                SubpixelMode::Bgr => Subpixel::Bgr,
                SubpixelMode::VRgb => Subpixel::VRgb,
                SubpixelMode::VBgr => Subpixel::VBgr,
            },
            hinting: self.hinting,
            palette,
        }
    }

    /// Draw in these families from now on, in *this* process -- and at
    /// `ui_size`, which the toolkit's controls follow, on this thread.
    ///
    /// `guitk`'s font selection is per-process global state, and its own
    /// documentation is explicit about the consequence: "callers must apply
    /// the same change in every process that draws, or measuring and drawing
    /// will disagree". So this is called wherever appearance is applied --
    /// `gui/window`'s event loop, which covers every application, and
    /// `gui/compositor` separately, because it draws window decorations in
    /// its own process and does not use `oswindow`.
    ///
    /// **Why this did not exist before.** `ui_font` and `mono_font` were
    /// written to `appearance.yaml`, covered by the every-field round-trip
    /// test, and read by nothing except a demo binary -- a setting the user
    /// could change that changed nothing. What hid it is that `ui_size` *was*
    /// wired up, in the compositor's taskbar and the shell's text scaling: the
    /// text did get bigger, so the settings looked as though they worked, and
    /// only the family silently did not. A setting is not connected because
    /// its neighbour is.
    ///
    /// An empty name is reported as [`FontOutcome::Unset`] rather than passed
    /// down, so that "no preference" stays distinguishable from a family this
    /// machine does not have.
    #[must_use]
    pub fn apply(&self) -> FontsApplied {
        // The way glyphs are rasterized, alongside the faces: text this
        // process's toolkit rasterizes itself was drawn unhinted while the
        // compositor's was hinted. The colour-emoji palette is the theme's,
        // which this section does not know, so the one in force is kept.
        guitk::text::set_rendering(self.rendering(guitk::text::rendering().palette));
        // And the size: the toolkit's controls draw their text at it, and lay
        // their rows and boxes out round it (`guitk::text::scaled`). Until
        // this they drew at 13 pixels whatever the user chose, so a larger
        // size reached the desktop's own text and the window titles and left
        // every menu, tooltip, button and field behind. On this thread: the
        // one that lays the program's windows out.
        let _changed = guitk::text::set_base_size(self.ui_size);
        // Asking first, because installing is not free: `set_font_family`
        // reloads the faces and drops every rasterized glyph, so calling it
        // for the family already in use would throw the cache away to arrive
        // back where it started. This is applied from an event loop that runs
        // on every settings notification, so "already correct" is the common
        // case rather than the rare one.
        fn install(
            current: Option<String>,
            family: &str,
            set: impl FnOnce(&str) -> bool,
        ) -> FontOutcome {
            if family.is_empty() {
                FontOutcome::Unset
            } else if current.as_deref() == Some(family) || set(family) {
                FontOutcome::Applied
            } else {
                FontOutcome::Missing
            }
        }
        FontsApplied {
            ui: install(
                guitk::text::font_family(),
                &self.ui_font,
                guitk::text::set_font_family,
            ),
            mono: install(
                guitk::text::mono_family(),
                &self.mono_font,
                guitk::text::set_mono_family,
            ),
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
    /// Every size, smallest first. See [`ThemeMode::ALL`]: the desktop's View
    /// menu and the Settings application both offer these, and each listing
    /// them itself is how the two would come to offer different sizes.
    pub const ALL: &'static [Self] = &[Self::Small, Self::Medium, Self::Large, Self::ExtraLarge];

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

/// How big the mouse pointer is: one of six sizes, in logical pixels (the
/// compositor scales them for the display the pointer is on).
///
/// **The one pointer-size setting on the system** (`design-decisions.md`
/// §872). There were three -- this, `inputsettings`' `mouse.cursor_size`
/// (16-128 px, in `input.yaml`) and the Settings application's own enum,
/// which it never saved -- and nothing read any of them, because nothing drew
/// a pointer. This one survived because the compositor already reads
/// `appearance.yaml` for every other visual setting and reloads it live
/// (`ReloadAppearance`). The two steps past 48 px are where `inputsettings`'
/// larger range went: a pointer someone with low vision can find.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorSize {
    Small,
    Normal,
    Large,
    ExtraLarge,
    /// 64 px.
    Huge,
    /// 96 px -- four times the normal pointer, and the largest.
    Giant,
}

impl CursorSize {
    /// Every size, smallest first. See [`ThemeMode::ALL`]: a front end that
    /// listed the sizes itself is how two lists of them come to disagree.
    pub const ALL: &'static [Self] = &[
        Self::Small,
        Self::Normal,
        Self::Large,
        Self::ExtraLarge,
        Self::Huge,
        Self::Giant,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "Small (16px)",
            Self::Normal => "Normal (24px)",
            Self::Large => "Large (32px)",
            Self::ExtraLarge => "Extra Large (48px)",
            Self::Huge => "Huge (64px)",
            Self::Giant => "Giant (96px)",
        }
    }

    pub fn pixels(self) -> u32 {
        match self {
            Self::Small => 16,
            Self::Normal => 24,
            Self::Large => 32,
            Self::ExtraLarge => 48,
            Self::Huge => 64,
            Self::Giant => 96,
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
    /// Every scheme, the default first. See [`ThemeMode::ALL`].
    pub const ALL: &'static [Self] = &[Self::Default, Self::Inverted, Self::AccentColored];

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
    /// The theme the colours come from, and what reading it gave: the
    /// built-in one unless the user chose another. `theme.colors` in the file,
    /// by the theme's folder name.
    ///
    /// Read when the file is read -- [`read_from`](Self::read_from) loads the
    /// theme along with the rest -- and not when a palette is resolved, which
    /// happens per frame and must not touch a file. One value rather than a
    /// name and a set of colours, so the two cannot disagree; see
    /// [`themes::ColorTheme`].
    pub color_theme: themes::ColorTheme,
    /// The theme the icons come from: the built-in one unless the user chose
    /// another. `theme.icons` in the file, by the theme's folder name -- which
    /// may name a different theme from `theme.colors`, since a theme's icons
    /// and its colours are separate axes (`roadmap-detailed.md` §3.4:
    /// "mix-and-match"). Nothing is read until an icon is drawn; see
    /// [`icons::IconTheme`].
    pub icon_theme: icons::IconTheme,
    /// The theme the pointer's pictures come from: the built-in one -- the
    /// compositor's own pointer -- unless the user chose another. `theme.cursors`
    /// in the file, by the folder name of the theme, which may be another
    /// desktop's cursor theme (Adwaita, Breeze) as well as a SlateOS one.
    /// Nothing is read until a cursor is drawn; see [`cursors::CursorTheme`].
    /// How large the pointer is and its colours for the built-in pictures stay
    /// [`cursor_size`](Self::cursor_size) and
    /// [`cursor_scheme`](Self::cursor_scheme).
    pub cursor_theme: cursors::CursorTheme,
    /// The theme an event's sound comes from -- a message arriving, an
    /// error, the recycle bin emptied: the built-in one, the sounds
    /// `gui/sound` synthesizes, unless the user chose another. `theme.sounds`
    /// in the file, by the folder name of the theme, which may be another
    /// desktop's sound theme (freedesktop's, Yaru) as well as a SlateOS one.
    /// Nothing is read until a sound is asked for; see
    /// [`sounds::SoundTheme`] and [`sound_for`](Self::sound_for).
    pub sound_theme: sounds::SoundTheme,
    /// Whether events make sounds, how loud, and the user's own sound for an
    /// event -- the `sounds` section.
    pub sounds: SoundSettings,
    /// The theme the shapes of the controls come from -- a button's corners,
    /// a field's focus mark, a scrollbar's width: the built-in one unless the
    /// user chose another. `theme.widget_style` in the file, by the theme's
    /// folder name, which may name a third theme again. Read with the file,
    /// for [`color_theme`](Self::color_theme)'s reason; see
    /// [`themes::WidgetTheme`] and `design-decisions.md` §1435.
    pub widget_theme: themes::WidgetTheme,
    /// The theme the desktop's transitions move by -- how long the standard
    /// one takes, its curve, or that nothing moves: the built-in one unless
    /// the user chose another. `theme.animation` in the file, by the theme's
    /// folder name, which may name a fourth theme again. The user's
    /// [`animation_speed`](Self::animation_speed) then scales it; the two
    /// reach everything that animates together, as `Palette::motion`. See
    /// [`themes::AnimationTheme`] and `design-decisions.md` §1446.
    pub animation_theme: themes::AnimationTheme,
    /// The theme every window's frame is shaped by -- its title bar, the
    /// buttons on it, its border and its shadow: the built-in one unless the
    /// user chose another. `theme.decorations` in the file, by the theme's
    /// folder name, which may name yet another theme. The compositor draws
    /// and hit-tests frames from [`decorations`](Self::decorations); see
    /// [`themes::DecorationTheme`] and `design-decisions.md` §1456.
    pub decoration_theme: themes::DecorationTheme,
    /// The theme the taskbar's finish and spacing come from -- how much of
    /// the Aero reference's glass it wears, and the gaps between its tiles:
    /// the built-in one unless the user chose another. `theme.taskbar_panel`
    /// in the file, by the theme's folder name. The desktop draws and lays
    /// out its taskbar from [`panel`](Self::panel); whether the bar is
    /// see-through stays [`taskbar_style`](Self::taskbar_style). See
    /// [`themes::PanelTheme`] and `design-decisions.md` §1460.
    pub panel_theme: themes::PanelTheme,
    /// The theme whose recommended wallpaper the desktop shows -- its picture
    /// for the mode the desktop is drawn in -- or the built-in one, which
    /// recommends none, so that the user's own
    /// [`wallpaper`](Self::wallpaper) is shown. `theme.wallpaper` in the
    /// file, by the theme's folder name. Below a time-of-day schedule and a
    /// rotation folder, each of which *is* the wallpaper when set; above the
    /// fixed picture, which it stands in for. See [`themes::WallpaperTheme`]
    /// and `design-decisions.md` §1471.
    pub wallpaper_theme: themes::WallpaperTheme,
    /// The theme whose recommended fonts are drawn -- for each role the
    /// first of its families this machine has, in place of the user's own
    /// [`fonts`](Self::fonts) -- or the built-in one, which recommends none.
    /// `theme.fonts` in the file, by the theme's folder name. What every
    /// process draws in is [`fonts_in_use`](Self::fonts_in_use). See
    /// [`themes::FontTheme`] and `design-decisions.md` §1472.
    pub font_theme: themes::FontTheme,
    /// The hours `System (Auto)` is light, local time: from the window's start
    /// until its end, and dark the rest of the day. `theme.auto.light_from`
    /// and `theme.auto.dark_from` in the file; 07:00 until 19:00 unless the
    /// user says otherwise.
    pub auto_light_hours: DailyWindow,
    /// Whether `System (Auto)` was in its light hours when these settings
    /// were read: the schedule against the clock and the time zone, resolved
    /// by [`read_from`](Self::read_from) -- never per frame -- and always
    /// `false` in the other two modes, so that comparing two readings does
    /// not change with the time of day unless the time of day is the setting.
    ///
    /// A process learns that it has changed from its watcher
    /// ([`watcher`]), whose fingerprint includes it. See
    /// [`is_light`](Self::is_light) and `design-decisions.md` §876.
    pub auto_is_light: bool,
    /// Whether boxes are outlined or filled. See [`SurfaceStyle`]; defaults to
    /// [`SurfaceStyle::Borders`] (§829), with `Cards` the optional theme.
    ///
    /// Each look keeps its own colours (§1421), so change it with
    /// [`set_surface_style`](Self::set_surface_style), which brings the new
    /// look's back; see [`other_look_colours`](Self::other_look_colours).
    pub surface_style: SurfaceStyle,
    /// Whether a toolbar or status bar is a band or a hairline. See
    /// [`guitk::palette::StripStyle`]; defaults to `Filled` (§835).
    pub strip_style: StripStyle,
    /// The colour-vision filter applied to the whole screen.
    ///
    /// Unlike every other field here this one is not consumed by the palette:
    /// a filter transforms *finished pixels*, so it is applied by the
    /// compositor when it hands a frame to the display. Filtering the
    /// palette's colours instead would shift the window chrome and leave
    /// photographs untouched, which is worse than not filtering at all.
    pub color_filter: ColorFilter,
    /// Whether the screen is warmed to cut blue light.
    ///
    /// Separate from [`night_light_strength`](Self::night_light_strength)
    /// rather than folded into it as "strength zero", because a switch that
    /// forgets how warm you had it is a switch nobody uses twice. Off and on
    /// again gets the same screen back.
    ///
    /// Like [`color_filter`](Self::color_filter), and unlike everything else
    /// here, this does not reach the palette: it warms *finished pixels* on
    /// their way to the display. Warming the palette instead would tint the
    /// window chrome and leave photographs cold, which is worse than not
    /// warming anything.
    pub night_light: bool,
    /// How warm, from 0 (no change) to 1 (fully warm).
    ///
    /// A strength rather than a figure in kelvins, because that is the control
    /// a person can use: "warmer" and "cooler" are the words, and how many
    /// kelvins that produces is a fact about the mapping rather than about
    /// what they asked for. [`night_light_gains`] says where the warm end
    /// comes from.
    ///
    /// Clamped on read, so a hand-edited file cannot ask for a negative
    /// warmth or for more than the mapping defines.
    pub night_light_strength: f32,
    /// The picture to show on the desktop, or `None` for the plain background.
    ///
    /// A path and nothing else. The desktop's `WallpaperManager` can already
    /// crop, letterbox, stretch, tile, centre and span across monitors, and
    /// none of those are here: the fit mode is a second setting, and a setting
    /// whose control does not exist is one a user cannot reach. The manager's
    /// default is used until the Wallpaper page offers the choice, on the same
    /// rule the Mouse settings page states -- each gets its control when it
    /// gets a consumer.
    ///
    /// `None` rather than an empty string, because "no wallpaper" and "a file
    /// called nothing" are different answers and only one of them is a
    /// mistake.
    pub wallpaper: Option<PathBuf>,
    /// How that picture is placed in the screen it is drawn on.
    ///
    /// Meaningless without [`wallpaper`](Self::wallpaper) and harmless with it
    /// unset, so it is a plain value rather than an `Option`: "how would I
    /// place a picture if there were one" always has an answer, and keeping
    /// the choice across a picture being removed and put back is what a user
    /// expects. The same argument `night_light`/`night_light_strength` makes
    /// one field along.
    pub wallpaper_fit: ImageFit,
    /// Which part of the picture shows where it overflows the screen -- and
    /// where it sits where it is smaller than the screen: fractions across
    /// and down, `(0.0, 0.0)` its top-left corner at the screen's, the
    /// default `(0.5, 0.5)` its middle at the screen's middle, `(1.0, 1.0)`
    /// its bottom-right at the screen's. `design.txt`: "let the user scroll
    /// the image up/down or right/left to center it on the desktop how they
    /// want". `wallpaper.position_x` and `position_y` in the file, held to
    /// 0..=1. The login screen, showing the same picture, shows the same part
    /// of it.
    pub wallpaper_position: (f32, f32),

    /// A folder to rotate wallpapers from, instead of one fixed picture.
    ///
    /// `roadmap-detailed.md` §3.4 asks for "random background on boot or daily
    /// rotation". The machinery has existed in `WallpaperManager` for a while
    /// -- `set_slideshow`, `populate_slideshow_paths`, shuffle, a per-image
    /// timer -- and had no key here, which is the whole reason nothing outside
    /// the shell's own tests ever called it.
    ///
    /// Takes precedence over [`wallpaper`](Self::wallpaper) when set, because
    /// a rotation *is* a wallpaper: having both would make the fixed picture a
    /// thing the user can see in the settings file and never on the screen.
    pub wallpaper_folder: Option<PathBuf>,

    /// How long each picture stays up, in seconds.
    ///
    /// Clamped to at least one second on the way into `set_slideshow`: zero
    /// would mean a new picture every tick, which is a slideshow nobody asked
    /// for and a decode storm.
    pub wallpaper_interval_secs: u64,

    /// Whether the rotation is shuffled or goes in directory order.
    pub wallpaper_shuffle: bool,

    /// Pictures that take turns by the time of day: each entry is up from its
    /// `from` time until the next entry's.
    ///
    /// `roadmap-detailed.md` §3.4's "dynamic wallpapers: list of images with
    /// time-of-day triggers (e.g., day image 06:00-18:00, night image
    /// 18:00-06:00)". Kept sorted by time. The day wraps: before the first
    /// entry's time, the last entry is up -- a night picture from 18:00 is
    /// still up at 03:00.
    ///
    /// Takes precedence over [`wallpaper_folder`](Self::wallpaper_folder) and
    /// [`wallpaper`](Self::wallpaper), for the reason the folder takes
    /// precedence over the picture: a schedule *is* the wallpaper, and
    /// honouring two would leave one visible only in the file.
    ///
    /// Written as `wallpaper.schedule`, one `"HH:MM path"` per entry, the path
    /// encoded as the other wallpaper paths are.
    pub wallpaper_schedule: Vec<ScheduledWallpaper>,

    /// Names to leave out of a rotation, as glob patterns.
    ///
    /// `roadmap-detailed.md` §3.4 asks for "exclusion filters" alongside the
    /// rotation. Matched against a picture's **file name**, not its path: the
    /// rotation reads one folder, so a pattern that could cross directories
    /// would have nothing to cross. That is also why this uses the flat
    /// matcher in `gui/globmatch` rather than the path-aware one in
    /// `apps/backup` -- see `known-issues.md`
    /// `TD-C-THE-TWO-GLOB-MATCHERS-ARE-NOT-DUPLICATES`.
    pub wallpaper_exclusions: Vec<String>,
    /// What the login screen draws behind itself.
    ///
    /// `design.txt` line 1247 asks for "login screen background image - easy
    /// way to make the two the same", and [`LoginBackground::SameAsDesktop`]
    /// is that easy way: it names no file, so it follows the desktop wherever
    /// the desktop goes, including through a rotation folder.
    pub login_background: LoginBackground,
    /// The high-contrast scheme in force, or `None` for an ordinary theme.
    ///
    /// When set it *replaces* [`theme_mode`](Self::theme_mode) rather than
    /// modifying it -- a high-contrast palette has no light and dark variant,
    /// it has a background colour. The mode is left untouched so that turning
    /// high contrast off returns the user to the theme they had.
    pub high_contrast: Option<HighContrastScheme>,
    /// The accent, as the look in use keeps it (§1421).
    pub accent_color: AccentColor,
    /// The colour a [`AccentColor::Custom`] accent names, as the look in use
    /// keeps it.
    pub custom_accent: Color,
    /// The colours kept for the look *not* in use: what changing
    /// [`surface_style`](Self::surface_style) brings back
    /// (`design-decisions.md` §1421).
    ///
    /// The look in use keeps its own in [`accent_color`](Self::accent_color)
    /// and [`custom_accent`](Self::custom_accent), where every reader has always
    /// found them, so nothing that draws needs to know there are two. Change
    /// the look with [`set_surface_style`](Self::set_surface_style), which
    /// trades the two over. **Assigning `surface_style` directly does not**:
    /// the accent chosen under the old look is then on the new one -- the thing
    /// §1421 exists to prevent.
    pub other_look_colours: LookColours,
    /// Transparency/blur effect level.
    pub transparency: TransparencyLevel,
    /// Animation speed.
    pub animation_speed: AnimationSpeed,
    /// Font settings.
    pub fonts: FontSettings,
    /// How wide the text caret is drawn, as a multiple of the toolkit's
    /// default.
    ///
    /// An accessibility setting, and a real one: a 2px caret on a 4K display
    /// is a hairline, and a user who cannot see where they are typing cannot
    /// type. It lived in `gui/desktop/src/a11y.rs` until 839, in a struct
    /// nothing constructed, so the setting existed and did nothing.
    ///
    /// Here rather than in `gui/inputsettings` -- which is where the
    /// *keyboard* accessibility settings went -- because a caret is drawn by
    /// the toolkit, and the toolkit's settings arrive through this struct.
    /// See design-decisions.md 839.
    pub caret_width_scale: f32,
    /// How much thicker than normal to draw the keyboard focus ring.
    ///
    /// Here for the same reason `caret_width_scale` is: the ring is drawn by
    /// the toolkit, and the toolkit's settings arrive through this struct. It
    /// is the last of the two fields
    /// `TD-C-STICKY-FILTER-AND-MOUSE-KEYS-ARE-BUILT-TESTED-AND-CONNECTED-TO-NOTHING`
    /// left standing as a record of a wanted feature -- `caret_width` was the
    /// other, and 839 built it.
    pub focus_ring_scale: f32,
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
    /// Whether the taskbar slides out of the way when it is not being used.
    ///
    /// **Off by default, deliberately, and not because the module that
    /// implements it defaults to on.** `AutoHideManager`'s own
    /// `AutoHideConfig::default()` has `enabled: true`, which is a sensible
    /// default for a component asked to auto-hide and a bad one for a desktop:
    /// a taskbar that vanishes is a large, surprising change to how the machine
    /// behaves, and the first thing an unprepared user does is look for the
    /// setting they did not knowingly change. Every desktop this imitates ships
    /// it off. See design-decisions 813.
    pub taskbar_autohide: bool,
    /// Whether a window's taskbar tile shows its title beside its picture:
    /// `design.txt`'s "option to show app name along with app icon in
    /// taskbar". On by default, because the Aero reference -- which the
    /// default theme follows (design-decisions §815) -- labels every running
    /// window. Off, a window's tile is its picture alone, square, as a pinned
    /// program's is; pinned programs are their picture alone either way.
    /// `taskbar.labels` in the file, beside auto-hide: a behaviour of the bar
    /// rather than a visual treatment.
    pub taskbar_labels: bool,
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
            // No picture. A desktop that invented one would be showing a file
            // the user never chose, and there is no stock wallpaper in this
            // tree to choose honestly.
            wallpaper: None,
            // Cropping, because it is what a photograph on a screen of a
            // different shape usually wants, and it is what the shell did
            // unconditionally before this was settable.
            wallpaper_fit: ImageFit::Fill,
            wallpaper_position: (0.5, 0.5),
            wallpaper_folder: None,
            // Ten minutes. Long enough that a picture is a background rather
            // than a distraction, short enough that a user who turns rotation
            // on sees it work without waiting for the next day.
            wallpaper_interval_secs: 600,
            wallpaper_shuffle: true,
            wallpaper_schedule: Vec::new(),
            wallpaper_exclusions: Vec::new(),
            login_background: LoginBackground::Theme,
            theme_mode: ThemeMode::Dark,
            color_theme: themes::ColorTheme::built_in(),
            icon_theme: icons::IconTheme::built_in(),
            cursor_theme: cursors::CursorTheme::built_in(),
            sound_theme: sounds::SoundTheme::built_in(),
            sounds: SoundSettings::default(),
            widget_theme: themes::WidgetTheme::built_in(),
            animation_theme: themes::AnimationTheme::built_in(),
            decoration_theme: themes::DecorationTheme::built_in(),
            panel_theme: themes::PanelTheme::built_in(),
            wallpaper_theme: themes::WallpaperTheme::built_in(),
            font_theme: themes::FontTheme::built_in(),
            auto_light_hours: DEFAULT_AUTO_LIGHT_HOURS,
            auto_is_light: false,
            // Borders, per §829. The `Default` impl is what a machine with no
            // configuration file gets, so this is where "the default theme" is
            // actually decided.
            surface_style: SurfaceStyle::Borders,
            strip_style: StripStyle::Filled,
            color_filter: ColorFilter::None,
            night_light: false,
            // Half way, which is where the control opens the first time
            // somebody switches night light on. Unread while it is off.
            night_light_strength: 0.5,
            high_contrast: None,
            accent_color: LookColours::default().accent_color,
            custom_accent: LookColours::default().custom_accent,
            other_look_colours: LookColours::default(),
            transparency: TransparencyLevel::Moderate,
            animation_speed: AnimationSpeed::Normal,
            fonts: FontSettings::default(),
            icon_size: IconSize::Medium,
            cursor_size: CursorSize::Normal,
            cursor_scheme: CursorScheme::Default,
            window_corners: WindowCorners::Rounded,
            taskbar_style: TaskbarStyle::Translucent,
            accent_taskbar: false,
            taskbar_autohide: false,
            taskbar_labels: true,
            accent_titlebars: false,
            drop_shadows: true,
            scaling_percent: 100,
            caret_width_scale: 1.0,
            focus_ring_scale: 1.0,
        }
    }
}

impl AppearanceSettings {
    /// The colours of the look in use.
    #[must_use]
    pub const fn colours(&self) -> LookColours {
        LookColours {
            accent_color: self.accent_color,
            custom_accent: self.custom_accent,
        }
    }

    /// The colours `look` keeps, whether or not it is the look in use.
    #[must_use]
    pub fn colours_for(&self, look: SurfaceStyle) -> LookColours {
        if look == self.surface_style {
            self.colours()
        } else {
            self.other_look_colours
        }
    }

    /// Set the colours `look` keeps: for the look in use, the accent on
    /// screen; for the other, what changing to it will bring back.
    pub fn set_colours_for(&mut self, look: SurfaceStyle, colours: LookColours) {
        if look == self.surface_style {
            self.accent_color = colours.accent_color;
            self.custom_accent = colours.custom_accent;
        } else {
            self.other_look_colours = colours;
        }
    }

    /// Change the look, bringing back the colours kept for it and keeping the
    /// ones in use for the look being left (`design-decisions.md` §1421).
    ///
    /// Choosing the look already in use changes nothing -- in particular it
    /// does not trade the colours over, which would put the other look's
    /// accent on screen for a click that changed nothing.
    pub fn set_surface_style(&mut self, look: SurfaceStyle) {
        if look == self.surface_style {
            return;
        }
        let leaving = self.colours();
        let arriving = self.other_look_colours;
        self.surface_style = look;
        self.accent_color = arriving.accent_color;
        self.custom_accent = arriving.custom_accent;
        self.other_look_colours = leaving;
    }

    /// The caret width this user asked for, in pixels.
    ///
    /// The one step between the stored setting and something that can be
    /// drawn, and the step that did not exist when `caret_width_scale` was
    /// added. A scale is not a width: every caller that multiplied for itself
    /// would be a caller that could forget to, which is exactly how the dead
    /// `a11y.rs` copy of this setting came to have a passing round-trip test
    /// and no reader. See `design-decisions.md` 839.
    #[must_use]
    pub fn caret_width(&self) -> f32 {
        guitk::textedit::CARET_WIDTH * self.caret_width_scale
    }

    /// The focus-ring width this user asked for, in pixels.
    ///
    /// The same one step between a stored scale and something a widget can
    /// draw that [`caret_width`](Self::caret_width) is, and it exists for the
    /// same reason: a scale is not a width, and a caller that multiplies for
    /// itself is a caller that can forget to.
    #[must_use]
    pub fn focus_ring_width(&self) -> f32 {
        guitk::style::FOCUS_RING_WIDTH * self.focus_ring_scale
    }

    /// Whether what is drawn is light: the one answer the palette, the accent
    /// and anything else with a light and a dark version should all follow.
    ///
    /// Not [`ThemeMode::is_light`], which is the *setting* alone. What is
    /// drawn can differ from it: a colour theme with only one mode's colours
    /// is shown in that mode whichever was chosen
    /// ([`ThemeColors::variant`]), and the accent has to match the grounds it
    /// sits on, not the switch in Settings. Asking the mode for this put the
    /// light-background accent on a dark-only theme's dark grounds.
    #[must_use]
    pub fn is_light(&self) -> bool {
        let asked = match self.theme_mode {
            ThemeMode::Light => true,
            ThemeMode::Dark => false,
            ThemeMode::System => self.auto_is_light,
        };
        self.color_theme
            .colors()
            .map_or(asked, |theme| theme.variant(asked).0)
    }

    /// The picture the chosen wallpaper theme recommends for the mode the
    /// desktop is drawn in ([`is_light`](Self::is_light)) -- so a theme's day
    /// and night pictures follow the automatic mode as its colours do --
    /// or `None` when it recommends none, the built-in theme among them.
    #[must_use]
    pub fn theme_wallpaper(&self) -> Option<&std::path::Path> {
        self.wallpaper_theme.picture(self.is_light())
    }

    /// The fonts every process draws in: the user's own
    /// [`fonts`](Self::fonts) -- families, sizes and rasterizing -- and, once
    /// every process applies through this, a chosen font theme's
    /// recommendations in place of the families where this machine has them
    /// ([`fonts_with_theme`](Self::fonts_with_theme)).
    ///
    /// **The theme's are not applied yet.** A label is measured by the
    /// program that lays it out and drawn by the compositor, so the two must
    /// agree on the face or every centred label sits off centre. The shell
    /// applies through this; the applications' event loop and the
    /// compositor read [`fonts`](Self::fonts) still
    /// (`requests/c-f-apply-the-fonts-in-use.md`), and until both apply
    /// through this it answers as they read: the user's own. Turning the
    /// theme on is then this function's body alone --
    /// `self.fonts_with_theme(guitk::text::family_installed)` -- which every
    /// process takes up together (`design-decisions.md` §1472).
    #[must_use]
    pub fn fonts_in_use(&self) -> FontSettings {
        self.fonts.clone()
    }

    /// The user's [`fonts`](Self::fonts) with the chosen font theme's
    /// recommendations in place of the families, where `installed` says this
    /// machine has one ([`themes::FontTheme::families_in_use`]): what
    /// [`fonts_in_use`](Self::fonts_in_use) will answer, and what a font page
    /// can show meanwhile.
    #[must_use]
    pub fn fonts_with_theme(&self, installed: impl Fn(&str) -> bool) -> FontSettings {
        self.font_theme.families_in_use(&self.fonts, installed)
    }

    /// How long until the automatic mode next turns light or dark, reading
    /// the time of day at `utc_secs` in `zone`; `None` unless the mode is
    /// automatic, or when its hours start and end at the same time and so
    /// never change.
    ///
    /// What a process with a clock sleeps for -- the shell -- rather than
    /// checking every minute and finding 1 438 times a day that nothing has
    /// changed (`design-decisions.md` 812). Never zero: a timer of no length
    /// would fire before the edge it waits for and re-arm for zero again.
    #[must_use]
    pub fn next_auto_change(&self, utc_secs: u64, zone: Tz) -> Option<Duration> {
        if self.theme_mode != ThemeMode::System {
            return None;
        }
        let now = local_time_of_day(utc_secs, zone);
        let minutes = self.auto_light_hours.minutes_to_next_edge(now)?;
        Some(Duration::from_secs(
            u64::from(minutes)
                .saturating_mul(60)
                .saturating_sub(utc_secs % 60)
                .max(1),
        ))
    }

    /// The scheduled picture that is up at `utc_secs` in `zone`, if there is a
    /// schedule: the entry with the latest time not after now, or -- before
    /// the day's first entry -- the last entry, still up from the evening
    /// before.
    #[must_use]
    pub fn scheduled_wallpaper_at(&self, utc_secs: u64, zone: Tz) -> Option<&Path> {
        let now = local_time_of_day(utc_secs, zone);
        self.wallpaper_schedule
            .iter()
            .rev()
            .find(|entry| entry.from <= now)
            .or_else(|| self.wallpaper_schedule.last())
            .map(|entry| entry.image.as_path())
    }

    /// How long until the scheduled picture next changes, if there is a
    /// schedule with more than one time in it.
    ///
    /// For a process with a clock to sleep until -- the shell -- as
    /// [`next_auto_change`](Self::next_auto_change) is. Never zero.
    #[must_use]
    pub fn next_wallpaper_change(&self, utc_secs: u64, zone: Tz) -> Option<Duration> {
        let first = self.wallpaper_schedule.first()?;
        if self.wallpaper_schedule.iter().all(|e| e.from == first.from) {
            return None;
        }
        let now = local_time_of_day(utc_secs, zone).minutes();
        let next = self
            .wallpaper_schedule
            .iter()
            .map(|e| e.from.minutes())
            .find(|&m| m > now)
            .unwrap_or_else(|| {
                first
                    .from
                    .minutes()
                    .saturating_add(daywindow::MINUTES_PER_DAY)
            });
        let minutes = next.saturating_sub(now);
        Some(Duration::from_secs(
            u64::from(minutes)
                .saturating_mul(60)
                .saturating_sub(utc_secs % 60)
                .max(1),
        ))
    }

    /// Whether the automatic mode's light hours contain `utc_secs` in `zone`
    /// -- what [`auto_is_light`](Self::auto_is_light) will say when the
    /// settings are next read, for a caller holding a clock to compare against
    /// what they say now.
    #[must_use]
    pub fn auto_light_at(&self, utc_secs: u64, zone: Tz) -> bool {
        self.auto_light_hours
            .contains(local_time_of_day(utc_secs, zone))
    }

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
        } else if self.is_light() {
            self.accent_color.color_light()
        } else {
            self.accent_color.color()
        }
    }

    /// Get DPI scale factor as a float (e.g. 1.0, 1.25, 1.5).
    pub fn scale_factor(&self) -> f32 {
        self.scaling_percent as f32 / 100.0
    }

    /// Whether the user has left animation on: their speed is not Off.
    ///
    /// The user's own switch, and only theirs. A theme whose transitions do
    /// not move (`animation.enabled: false`) does not make this false: that is
    /// a look, not the user asking for less motion -- a picture viewer asks
    /// this before playing an animated picture, and a theme's taste in panel
    /// slides is no reason to stop one. The transitions themselves follow
    /// `Palette::motion`, which reads both.
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

    /// The shape of every window's frame: the chosen window-decorations
    /// theme's, or the built-in one where that could not be used
    /// ([`themes::DecorationTheme::problem`] says why). Where a frame's
    /// buttons and title go is
    /// [`DecorationStyle::title_bar`](decorations::DecorationStyle::title_bar).
    #[must_use]
    pub fn decorations(&self) -> decorations::DecorationStyle {
        self.decoration_theme.style()
    }

    /// The taskbar's finish and spacing: the chosen taskbar-panel theme's, or
    /// the built-in one where that could not be used
    /// ([`themes::PanelTheme::problem`] says why).
    #[must_use]
    pub fn panel(&self) -> panel::PanelStyle {
        self.panel_theme.style()
    }

    /// What to play for the event `name` -- a sound naming specification
    /// name (`message-new-instant`, `dialog-error`, `trash-empty`): nothing
    /// with sounds off; else the user's own choice for it, or for its name
    /// cut at a hyphen, so a sound chosen for `dialog-error` is heard for
    /// `dialog-error-serious` too; else the sound theme's
    /// ([`sounds::SoundTheme::sound`]), which ends at the built-in sound.
    /// Play it at [`SoundSettings::volume`].
    #[must_use]
    pub fn sound_for(&self, name: &str) -> sounds::SoundChoice {
        let name = name.trim();
        if !self.sounds.enabled || !sounds::is_valid_name(name) {
            return sounds::SoundChoice::Silent;
        }
        for candidate in sounds::cuts(name) {
            match self.sounds.events.get(candidate) {
                Some(EventSound::File(path)) => return sounds::SoundChoice::File(path.clone()),
                Some(EventSound::Off) => return sounds::SoundChoice::Silent,
                None => {}
            }
        }
        self.sound_theme.sound(name)
    }

    /// Wear the theme `info`, found in `dirs`, on every axis it can be chosen
    /// for -- its colours, controls, motion, window frames, taskbar, wallpaper,
    /// fonts, icons, cursors and sounds -- and leave every other axis as it
    /// is. What a theme browser's "apply" does in one click, and what a
    /// picture of the theme shows (`gui/themepreview`). Answers the axes
    /// taken, by the names `meta.supports` gives them, in that order.
    ///
    /// An axis is taken as [`themes::ThemeInfo`]'s `provides_*` says, so a
    /// theme listed as giving no colours is never worn for them; a folder of
    /// cursors or of sounds (`cursors/`, `stereo/`) beside its file gives
    /// those. Nothing is saved: the caller saves what it means to keep.
    pub fn wear_theme(
        &mut self,
        info: &themes::ThemeInfo,
        dirs: &themes::ThemeDirs,
    ) -> Vec<&'static str> {
        let id = info.id.as_os_str();
        let holds = |folder: &str| {
            info.dir
                .as_ref()
                .is_some_and(|dir| dir.join(folder).is_dir())
        };
        let mut taken = Vec::new();
        if info.provides_colors() {
            self.color_theme = themes::ColorTheme::load_from(dirs, id);
            taken.push(themes::DARK_SECTION);
        }
        if info.provides_widget_style() {
            self.widget_theme = themes::WidgetTheme::load_from(dirs, id);
            taken.push(themes::WIDGET_SECTION);
        }
        if info.provides_animation() {
            self.animation_theme = themes::AnimationTheme::load_from(dirs, id);
            taken.push(themes::ANIMATION_SECTION);
        }
        if info.provides_decorations() {
            self.decoration_theme = themes::DecorationTheme::load_from(dirs, id);
            taken.push(themes::DECORATIONS_SECTION);
        }
        if info.provides_panel() {
            self.panel_theme = themes::PanelTheme::load_from(dirs, id);
            taken.push(themes::PANEL_SECTION);
        }
        if info.provides_wallpapers() {
            self.wallpaper_theme = themes::WallpaperTheme::load_from(dirs, id);
            taken.push(themes::WALLPAPERS_SECTION);
        }
        if info.provides_fonts() {
            self.font_theme = themes::FontTheme::load_from(dirs, id);
            taken.push(themes::FONTS_SECTION);
        }
        if info.provides_icons() {
            self.icon_theme = icons::IconTheme::named(id, dirs.clone());
            taken.push(icons::ICONS_DIR);
        }
        if info.origin == themes::Origin::BuiltIn || holds(cursors::CURSORS_DIR) {
            self.cursor_theme = cursors::CursorTheme::named(id, dirs.clone(), cursors::icon_dirs());
            taken.push(cursors::CURSORS_DIR);
        }
        if info.origin == themes::Origin::BuiltIn || holds(sounds::STEREO_DIR) {
            self.sound_theme = sounds::SoundTheme::named(id, dirs.clone(), sounds::sound_dirs());
            taken.push(sounds::AXIS);
        }
        taken
    }

    /// Validate and clamp settings to sane ranges.
    pub fn validate(&mut self) {
        // A volume is a fraction; a NaN set in code is the default rather
        // than a panic or a silence nobody chose.
        self.sounds.volume = if self.sounds.volume.is_nan() {
            DEFAULT_SOUND_VOLUME
        } else {
            self.sounds.volume.clamp(0.0, 1.0)
        };
        // A name that is no event's would be looked up as nothing anyway;
        // dropped, so a save does not keep writing it back.
        self.sounds
            .events
            .retain(|name, _| sounds::is_valid_name(name));
        // Clamp font sizes. `get_f64` never yields a NaN or an infinity, so
        // `clamp` cannot be handed one from a config file; a NaN written by a
        // future code path would panic here rather than propagate silently.
        self.fonts.ui_size = self.fonts.ui_size.clamp(8.0, 32.0);
        // Fractions of the room a picture leaves; a NaN set in code is the
        // middle rather than a panic or a picture nowhere.
        let place = |v: f32| if v.is_nan() { 0.5 } else { v.clamp(0.0, 1.0) };
        self.wallpaper_position = (
            place(self.wallpaper_position.0),
            place(self.wallpaper_position.1),
        );
        self.fonts.mono_size = self.fonts.mono_size.clamp(6.0, 32.0);
        self.scaling_percent = self.scaling_percent.clamp(100, 300);
        // Up to four times, not the a11y module's five: the caret is drawn
        // inside a line box, and past about 4x it stops being a caret and
        // starts covering the character after it.
        self.caret_width_scale = self.caret_width_scale.clamp(0.5, 4.0);
        // The same range as the caret, and for the same reason: below 0.5 a
        // ring is thinner than the hairline it is meant to replace, and above
        // 4 it starts covering the control it surrounds. A focus indicator
        // that hides the button is not an accessibility win.
        self.focus_ring_scale = self.focus_ring_scale.clamp(0.5, 4.0);
    }
}

// ============================================================================
// Window decoration colours
// ============================================================================

/// The relative luminance of an opaque colour, and the WCAG 2 contrast ratio
/// between two of them — re-exported from the toolkit, not defined here.
///
/// They belong there because the toolkit is what everything else on this
/// desktop already depends on, whereas this crate is not: a widget asking
/// which ink to use cannot reach an answer that lives in the appearance model,
/// so if the answer lived here the toolkit would grow a second one. It did,
/// for a while, and the copy was wrong for 41.78 % of the colour cube. See
/// `guitk::theme::contrast_text`.
///
/// The re-export is deliberate rather than a wrapper: a wrapper would be a
/// place where the two could drift apart.
pub use guitk::theme::{contrast_ratio, perceptual_difference, relative_luminance};

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
            // The palette's answer, the accent where the user asked for
            // accented title bars: the same one a ribbon's strip, joined to
            // the bar, is drawn from. The *unfocused* bar deliberately keeps
            // the base palette -- an accent that marks every window marks none
            // of them, and telling the focused window apart is the title bar's
            // first job.
            title_focused_bg: p.title_bar(),
            title_focused_fg: p.title_text(),
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
    ///
    /// Resolves a palette to do it. A caller that already holds one -- the
    /// compositor caches one per appearance change -- should call
    /// [`from_settings_with`](Self::from_settings_with) and hand it over,
    /// rather than pay for a second identical resolve.
    #[must_use]
    pub fn from_settings(settings: &AppearanceSettings) -> Self {
        Self::from_settings_with(settings, &Palette::from_settings(settings))
    }

    /// [`from_settings`](Self::from_settings) for a caller that already holds
    /// the palette for these settings.
    ///
    /// **The palette must be the one these settings resolve to.** Passing an
    /// unrelated palette gives window frames from one theme and content from
    /// another, which no assertion here can catch -- the types are the same.
    /// The split exists because the two-resolve version was invisible until a
    /// counter was put on `Palette::from_settings`: `Compositor::set_appearance`
    /// resolved one for its own cache and this resolved a second, identical,
    /// one line later.
    ///
    /// The settings themselves are no longer read: since 2026-09-30 the
    /// palette carries the one thing a frame took from them beyond it --
    /// whether the title bar is in the accent ([`Palette::title_bar`]) -- so
    /// that a ribbon's strip, joined to the bar, is drawn from the same
    /// answer. The parameter stays so that the window manager's call is
    /// unchanged.
    #[must_use]
    pub fn from_settings_with(_settings: &AppearanceSettings, palette: &Palette) -> Self {
        Self::from_palette(palette)
    }

    /// Every colour a frame is drawn with, paired with its field name.
    ///
    /// The counterpart of [`Palette::roles`], and here for the same reason and
    /// held to the struct the same way: the destructure has no `..`, so a
    /// twelfth decoration colour stops this compiling until it is named, which
    /// is the prompt to decide whether the sweeps that read this should cover
    /// it. Without that, adding a field errors only at the struct literals in
    /// [`for_mode`](Self::for_mode) and [`from_palette`](Self::from_palette) —
    /// the author fills those in, the compiler falls silent, and the new colour
    /// is drawn by the window manager and checked by nothing. See
    /// `known-issues.md` lesson 44.
    ///
    /// `#[cfg(test)]` because the only callers are this crate's own sweeps.
    /// [`Palette::roles`] is public because the shell's conversion sweep needs
    /// it from outside; nothing outside needs this one, and a public method
    /// added on the chance that something might is API nobody asked for.
    #[cfg(test)]
    fn roles(&self) -> [(&'static str, Color); 11] {
        let Self {
            title_focused_bg,
            title_focused_fg,
            title_unfocused_bg,
            title_unfocused_fg,
            border_focused,
            border_unfocused,
            close_button,
            maximize_button,
            minimize_button,
            shadow,
            desktop_bg,
        } = *self;
        [
            ("title_focused_bg", title_focused_bg),
            ("title_focused_fg", title_focused_fg),
            ("title_unfocused_bg", title_unfocused_bg),
            ("title_unfocused_fg", title_unfocused_fg),
            ("border_focused", border_focused),
            ("border_unfocused", border_unfocused),
            ("close_button", close_button),
            ("maximize_button", maximize_button),
            ("minimize_button", minimize_button),
            ("shadow", shadow),
            ("desktop_bg", desktop_bg),
        ]
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
    Huge => "huge",
    Giant => "giant",
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

        // An empty string reads as "no wallpaper": a hand-edited file that
        // blanks the value means to turn it off, and treating that as a path
        // would make the desktop report a missing file the user never named.
        read_into!(
            s.wallpaper_fit,
            doc.get_str(&["wallpaper", "fit"])
                .and_then(|v| ImageFit::from_yaml_name(v.trim()))
        );
        read_into!(
            s.wallpaper_position.0,
            doc.get_f64(&["wallpaper", "position_x"])
                .map(|v| (v as f32).clamp(0.0, 1.0))
        );
        read_into!(
            s.wallpaper_position.1,
            doc.get_f64(&["wallpaper", "position_y"])
                .map(|v| (v as f32).clamp(0.0, 1.0))
        );

        if let Some(path) = doc.get_str(&["wallpaper", "image"]) {
            let trimmed = path.trim();
            // A filename may hold any byte but `/` and NUL, so it cannot
            // always be written into YAML as itself. design-decisions §426
            // percent-encodes it; the marker beside it is what tells an
            // encoded file from one written before this existed, because a
            // path containing a literal `%` would otherwise decode to a
            // different path. Absence of the marker means version 1: raw.
            let encoded = doc
                .get_str(&["wallpaper", "image_encoding"])
                .is_some_and(|v| v.trim() == WALLPAPER_ENCODING);
            s.wallpaper = if trimmed.is_empty() {
                None
            } else if encoded {
                Some(pathcodec::decode_path(trimmed))
            } else {
                Some(PathBuf::from(trimmed))
            };
        }

        if let Some(folder) = doc.get_str(&["wallpaper", "folder"]) {
            let trimmed = folder.trim();
            let encoded = doc
                .get_str(&["wallpaper", "image_encoding"])
                .is_some_and(|v| v.trim() == WALLPAPER_ENCODING);
            s.wallpaper_folder = if trimmed.is_empty() {
                None
            } else if encoded {
                Some(pathcodec::decode_path(trimmed))
            } else {
                Some(PathBuf::from(trimmed))
            };
        }
        if let Some(secs) = doc.get_i64(&["wallpaper", "interval_secs"]) {
            // Clamped on read as well as on write: this file is meant to be
            // hand-editable, and `interval_secs: 0` typed into it should give
            // a slow rotation rather than a new decode every frame.
            s.wallpaper_interval_secs = u64::try_from(secs).unwrap_or(600).max(1);
        }
        if let Some(shuffle) = doc.get_i64(&["wallpaper", "shuffle"]) {
            s.wallpaper_shuffle = shuffle != 0;
        }
        if let Some(entries) = doc.get_seq(&["wallpaper", "schedule"]) {
            let encoded = doc
                .get_str(&["wallpaper", "image_encoding"])
                .is_some_and(|v| v.trim() == WALLPAPER_ENCODING);
            s.wallpaper_schedule = read_wallpaper_schedule(&entries, encoded);
        }
        if let Some(patterns) = doc.get_seq(&["wallpaper", "exclude"]) {
            // Empty lines dropped: a YAML list a person edited by hand grows
            // blank entries, and an empty pattern matches nothing useful but
            // would sit in the list looking like it does something.
            s.wallpaper_exclusions = patterns
                .into_iter()
                .filter(|p| !p.trim().is_empty())
                .collect();
        }

        // The colour theme, by the name of its folder, percent-encoded as a
        // filename is (design-decisions 426): a folder's name need not be
        // text. Loaded here, file and all -- see `color_theme` for why here.
        // An empty value is the built-in theme, as a blanked wallpaper is no
        // wallpaper.
        if let Some(name) = color_theme_name(doc) {
            s.color_theme = themes::ColorTheme::load(&name);
        }
        // The icon theme, spelled as the colour theme is. Nothing to load yet:
        // an icon is looked up when it is drawn.
        if let Some(name) = doc
            .get_str(&["theme", "icons"])
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
        {
            s.icon_theme = icons::IconTheme::load(&pathcodec::decode_path(&name).into_os_string());
        }
        // The cursor theme, spelled as the icon theme is, and for its reason
        // read no further: a cursor is looked up when the pointer is drawn.
        if let Some(name) = doc
            .get_str(&["theme", "cursors"])
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
        {
            s.cursor_theme =
                cursors::CursorTheme::load(&pathcodec::decode_path(&name).into_os_string());
        }
        // The sound theme, spelled as the cursor theme is, and for its
        // reason read no further: a sound is looked up when it is played.
        if let Some(name) = doc
            .get_str(&["theme", "sounds"])
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
        {
            s.sound_theme =
                sounds::SoundTheme::load(&pathcodec::decode_path(&name).into_os_string());
        }
        if let Some(on) = doc.get_bool(&["sounds", "enabled"]) {
            s.sounds.enabled = on;
        }
        if let Some(volume) = doc.get_f64(&["sounds", "volume"]) {
            // Clamped by `validate`; the narrowing is of a fraction.
            #[allow(clippy::cast_possible_truncation, reason = "a volume, 0 to 1")]
            let volume = volume as f32;
            s.sounds.volume = volume;
        }
        // The user's own sounds: an event's name and `off` or a path. A value
        // that is neither is passed over rather than costing the rest; a
        // name that is no event's is dropped by `validate`, below, which
        // holds that rule for settings made in code as well as read.
        for name in doc.keys(&["sounds", "events"]) {
            if let Some(choice) = doc
                .get_str(&["sounds", "events", &name])
                .and_then(|value| EventSound::parse(&value))
            {
                s.sounds.events.insert(name, choice);
            }
        }
        // The widget style, spelled and loaded as the colour theme is -- file
        // and all, since the palette carries it and a palette is resolved per
        // frame.
        if let Some(name) = widget_theme_name(doc) {
            s.widget_theme = themes::WidgetTheme::load(&name);
        }
        // The motion, for the same reason: the palette carries it.
        if let Some(name) = animation_theme_name(doc) {
            s.animation_theme = themes::AnimationTheme::load(&name);
        }
        // The window frames, the same way.
        if let Some(name) = decoration_theme_name(doc) {
            s.decoration_theme = themes::DecorationTheme::load(&name);
        }
        // The taskbar's panel, the same way.
        if let Some(name) = panel_theme_name(doc) {
            s.panel_theme = themes::PanelTheme::load(&name);
        }
        // And the wallpaper a theme recommends, the same way: the pictures
        // are found when the settings are read, never when one is drawn.
        if let Some(name) = wallpaper_theme_name(doc) {
            s.wallpaper_theme = themes::WallpaperTheme::load(&name);
        }
        // And the fonts a theme recommends, as it names them. Which of them
        // this machine has is asked where they are drawn (`fonts_in_use`):
        // a fact about the machine, not about the file.
        if let Some(name) = font_theme_name(doc) {
            s.font_theme = themes::FontTheme::load(&name);
        }
        read_into!(
            s.theme_mode,
            doc.get_str(&["theme", "mode"])
                .and_then(|v| ThemeMode::from_yaml_name(&v))
        );
        if let Some(hours) = auto_light_hours_in(doc) {
            s.auto_light_hours = hours;
        }
        // Resolved here, against the clock, for the reason the colour theme is
        // loaded here: every reader of the settings gets it, and none works it
        // out per frame.
        s.auto_is_light = s.theme_mode == ThemeMode::System && auto_light_now(s.auto_light_hours);
        read_into!(
            s.surface_style,
            doc.get_str(&["theme", "surface_style"])
                .and_then(|v| surface_style_from_yaml_name(&v))
        );
        read_into!(
            s.strip_style,
            doc.get_str(&["theme", "strip_style"])
                .and_then(|v| strip_style_from_yaml_name(&v))
        );
        // Absent and "off" both mean no high contrast, and an unrecognised
        // name does too. A scheme this build does not know is not a reason to
        // refuse to draw, and falling back to the ordinary theme is the safe
        // direction: it is legible to everyone, which a half-applied
        // high-contrast palette would not be.
        s.high_contrast = doc
            .get_str(&["theme", "high_contrast"])
            .and_then(|v| HighContrastScheme::from_yaml_name(&v));

        read_into!(
            s.color_filter,
            doc.get_str(&["theme", "color_filter"])
                .and_then(|v| ColorFilter::from_yaml_name(&v))
        );

        // The accent, kept per look (§1421). `theme.accent` and
        // `theme.custom_accent` are the outlined look's -- where the one accent
        // was always written -- and `theme.cards` holds the filled look's.
        // Whatever the filled look does not say it takes from the outlined
        // one, which is how a file written before looks kept colours of their
        // own reads: as that one accent for both, so nobody's choice is lost.
        // After `surface_style`, which decides which of the two is on screen.
        let mut outlined = LookColours::default();
        read_into!(
            outlined.accent_color,
            doc.get_str(&["theme", "accent"])
                .and_then(|v| AccentColor::from_yaml_name(&v))
        );
        read_into!(
            outlined.custom_accent,
            doc.get_str(&["theme", "custom_accent"])
                .and_then(|v| Color::from_hex_text(&v))
        );
        let mut filled = outlined;
        read_into!(
            filled.accent_color,
            doc.get_str(&["theme", FILLED_LOOK_KEY, "accent"])
                .and_then(|v| AccentColor::from_yaml_name(&v))
        );
        read_into!(
            filled.custom_accent,
            doc.get_str(&["theme", FILLED_LOOK_KEY, "custom_accent"])
                .and_then(|v| Color::from_hex_text(&v))
        );
        s.set_colours_for(SurfaceStyle::Borders, outlined);
        s.set_colours_for(SurfaceStyle::Cards, filled);
        read_into!(
            s.transparency,
            doc.get_str(&["theme", "transparency"])
                .and_then(|v| TransparencyLevel::from_yaml_name(&v))
        );

        read_into!(s.night_light, doc.get_bool(&["theme", "night_light"]));
        read_into!(
            s.night_light_strength,
            doc.get_f64(&["theme", "night_light_strength"])
                .map(|v| (v as f32).clamp(0.0, 1.0))
        );

        read_into!(
            s.caret_width_scale,
            doc.get_f64(&["accessibility", "caret_width_scale"])
                .map(|v| v as f32)
        );
        read_into!(
            s.focus_ring_scale,
            doc.get_f64(&["accessibility", "focus_ring_scale"])
                .map(|v| v as f32)
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
        read_into!(s.taskbar_autohide, doc.get_bool(&["taskbar", "autohide"]));
        read_into!(s.taskbar_labels, doc.get_bool(&["taskbar", "labels"]));
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

        // The greeter's background. The mode decides which other keys are
        // consulted, so a file that names a colour *and* a picture is not
        // ambiguous: whichever the mode says wins, and the other is left alone
        // for when the user switches back to it.
        if let Some(mode) = doc.get_str(&["login", "background"]) {
            s.login_background = match mode.trim() {
                "desktop" => LoginBackground::SameAsDesktop,
                "color" => doc
                    .get_str(&["login", "color"])
                    .and_then(|v| Color::from_hex_text(&v))
                    .map_or(LoginBackground::Theme, LoginBackground::SolidColor),
                "gradient" => {
                    let top = doc
                        .get_str(&["login", "gradient_top"])
                        .and_then(|v| Color::from_hex_text(&v));
                    let bottom = doc
                        .get_str(&["login", "gradient_bottom"])
                        .and_then(|v| Color::from_hex_text(&v));
                    match (top, bottom) {
                        (Some(top), Some(bottom)) => LoginBackground::Gradient { top, bottom },
                        // Half a gradient is not a gradient. The theme is the
                        // honest answer, and is what an unreadable value means
                        // everywhere else in this file.
                        _ => LoginBackground::Theme,
                    }
                }
                "image" => doc
                    .get_str(&["login", "image"])
                    .map(|raw| {
                        let trimmed = raw.trim();
                        let encoded = doc
                            .get_str(&["login", "image_encoding"])
                            .is_some_and(|v| v.trim() == WALLPAPER_ENCODING);
                        if encoded {
                            pathcodec::decode_path(trimmed)
                        } else {
                            PathBuf::from(trimmed)
                        }
                    })
                    .filter(|p| !p.as_os_str().is_empty())
                    .map_or(LoginBackground::Theme, LoginBackground::CustomImage),
                // An unknown mode is left at whatever the default is rather
                // than guessed at, the same rule `PreviewSide::from_yaml_name`
                // follows: a typo should not move the user's background.
                _ => s.login_background,
            };
        }

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
        // Written even when unset, as the empty string, so the key is in the
        // file with a comment beside it rather than absent. A key you can see
        // is a key you can edit; an absent one has to be guessed at.
        doc.set_str(
            &["wallpaper", "image"],
            &self
                .wallpaper
                .as_deref()
                .map(pathcodec::encode_path)
                .unwrap_or_default(),
        );
        // Written whenever the file is written, so a file this version has
        // touched is always self-describing. See the read side for why its
        // absence has to mean "raw" rather than "assume encoded".
        doc.set_str(&["wallpaper", "image_encoding"], WALLPAPER_ENCODING);
        doc.set_str(
            &["wallpaper", "folder"],
            &self
                .wallpaper_folder
                .as_deref()
                .map(pathcodec::encode_path)
                .unwrap_or_default(),
        );
        // `as` after a value this crate chose: an interval is seconds and
        // cannot exceed what an `i64` holds.
        doc.set_i64(
            &["wallpaper", "interval_secs"],
            i64::try_from(self.wallpaper_interval_secs).unwrap_or(600),
        );
        doc.set_i64(&["wallpaper", "shuffle"], i64::from(self.wallpaper_shuffle));
        let excludes: Vec<&str> = self
            .wallpaper_exclusions
            .iter()
            .map(String::as_str)
            .collect();
        doc.set_seq(&["wallpaper", "exclude"], &excludes);
        let schedule: Vec<String> = self
            .wallpaper_schedule
            .iter()
            .map(|entry| format!("{} {}", entry.from, pathcodec::encode_path(&entry.image)))
            .collect();
        let schedule: Vec<&str> = schedule.iter().map(String::as_str).collect();
        doc.set_seq(&["wallpaper", "schedule"], &schedule);
        doc.set_str(&["wallpaper", "fit"], self.wallpaper_fit.yaml_name());
        doc.set_f64(
            &["wallpaper", "position_x"],
            f64::from(self.wallpaper_position.0),
        );
        doc.set_f64(
            &["wallpaper", "position_y"],
            f64::from(self.wallpaper_position.1),
        );
        doc.set_str(&["login", "background"], self.login_background.yaml_name());
        match &self.login_background {
            LoginBackground::SolidColor(color) => {
                doc.set_str(&["login", "color"], &Color::hex_text(*color));
            }
            LoginBackground::Gradient { top, bottom } => {
                doc.set_str(&["login", "gradient_top"], &Color::hex_text(*top));
                doc.set_str(&["login", "gradient_bottom"], &Color::hex_text(*bottom));
            }
            LoginBackground::CustomImage(path) => {
                // Percent-encoded under the same marker as the wallpaper, and
                // for the same reason: a filename may hold any byte but `/`
                // and NUL, and YAML scalars are text. design-decisions 426.
                doc.set_str(&["login", "image"], &pathcodec::encode_path(path));
                doc.set_str(&["login", "image_encoding"], WALLPAPER_ENCODING);
            }
            // Nothing else to write down: these two name no value of their own.
            LoginBackground::Theme | LoginBackground::SameAsDesktop => {}
        }
        doc.set_str(&["theme", "mode"], self.theme_mode.yaml_name());
        doc.set_str(
            &["theme", "auto", "light_from"],
            &self.auto_light_hours.start().to_string(),
        );
        doc.set_str(
            &["theme", "auto", "dark_from"],
            &self.auto_light_hours.end().to_string(),
        );
        doc.set_str(
            &["theme", "colors"],
            &pathcodec::encode_path(std::path::Path::new(self.color_theme.id())),
        );
        doc.set_str(
            &["theme", "icons"],
            &pathcodec::encode_path(std::path::Path::new(self.icon_theme.id())),
        );
        doc.set_str(
            &["theme", "cursors"],
            &pathcodec::encode_path(std::path::Path::new(self.cursor_theme.id())),
        );
        doc.set_str(
            &["theme", "sounds"],
            &pathcodec::encode_path(std::path::Path::new(self.sound_theme.id())),
        );
        doc.set_bool(&["sounds", "enabled"], self.sounds.enabled);
        doc.set_f64(&["sounds", "volume"], f64::from(self.sounds.volume));
        // An event no longer given a sound of its own is taken out, so the
        // theme's is heard again; the rest are written as they are.
        for stale in doc.keys(&["sounds", "events"]) {
            if !self.sounds.events.contains_key(&stale) {
                let _removed = doc.remove(&["sounds", "events", &stale]);
            }
        }
        for (name, choice) in &self.sounds.events {
            doc.set_str(&["sounds", "events", name], &choice.yaml_value());
        }
        doc.set_str(
            &["theme", "widget_style"],
            &pathcodec::encode_path(std::path::Path::new(self.widget_theme.id())),
        );
        doc.set_str(
            &["theme", "animation"],
            &pathcodec::encode_path(std::path::Path::new(self.animation_theme.id())),
        );
        doc.set_str(
            &["theme", "decorations"],
            &pathcodec::encode_path(std::path::Path::new(self.decoration_theme.id())),
        );
        doc.set_str(
            &["theme", "taskbar_panel"],
            &pathcodec::encode_path(std::path::Path::new(self.panel_theme.id())),
        );
        doc.set_str(
            &["theme", "wallpaper"],
            &pathcodec::encode_path(std::path::Path::new(self.wallpaper_theme.id())),
        );
        doc.set_str(
            &["theme", "fonts"],
            &pathcodec::encode_path(std::path::Path::new(self.font_theme.id())),
        );
        doc.set_str(
            &["theme", "surface_style"],
            surface_style_yaml_name(self.surface_style),
        );
        doc.set_str(
            &["theme", "strip_style"],
            strip_style_yaml_name(self.strip_style),
        );
        doc.set_str(&["theme", "color_filter"], self.color_filter.yaml_name());
        doc.set_bool(&["theme", "night_light"], self.night_light);
        doc.set_f64(
            &["theme", "night_light_strength"],
            f64::from(self.night_light_strength),
        );
        doc.set_str(
            &["theme", "high_contrast"],
            self.high_contrast
                .map_or("off", HighContrastScheme::yaml_name),
        );
        // Per look (§1421), and both whichever is in use, so the file
        // says what each look will show: the outlined look's where the one
        // accent has always been, the filled look's under `theme.cards`.
        let outlined = self.colours_for(SurfaceStyle::Borders);
        let filled = self.colours_for(SurfaceStyle::Cards);
        doc.set_str(&["theme", "accent"], outlined.accent_color.yaml_name());
        doc.set_str(
            &["theme", "custom_accent"],
            &Color::hex_text(outlined.custom_accent),
        );
        doc.set_str(
            &["theme", FILLED_LOOK_KEY, "accent"],
            filled.accent_color.yaml_name(),
        );
        doc.set_str(
            &["theme", FILLED_LOOK_KEY, "custom_accent"],
            &Color::hex_text(filled.custom_accent),
        );
        doc.set_str(&["theme", "transparency"], self.transparency.yaml_name());

        doc.set_str(&["fonts", "ui_font"], &self.fonts.ui_font);
        doc.set_f64(
            &["accessibility", "caret_width_scale"],
            f64::from(self.caret_width_scale),
        );
        doc.set_f64(
            &["accessibility", "focus_ring_scale"],
            f64::from(self.focus_ring_scale),
        );
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
        // Under `taskbar` rather than `effects`, because it is a behaviour and
        // not an appearance: the group a key sits in is the only clue a person
        // hand-editing this file gets about what else to look for nearby.
        doc.set_bool(&["taskbar", "autohide"], self.taskbar_autohide);
        doc.set_bool(&["taskbar", "labels"], self.taskbar_labels);
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

/// The watcher for `appearance.yaml`: one that also notices the chosen colour
/// theme's own file changing -- which changes every colour read from the
/// settings without changing a byte of `appearance.yaml`.
///
/// Use this rather than `config::Watcher::new(CONFIG_NAME)`, which reports an
/// in-place edit of the theme as nothing at all (`known-issues-resolved/`
/// `TD-C-AN-EDITED-THEME-FILE-IS-NOT-NOTICED-UNTIL-THE-SETTINGS-CHANGE`). It
/// still only looks when asked: a process learns to look from a
/// `SettingsChanged` announcement, as for any other change -- which the
/// desktop's settings watcher makes for a theme's folder changing too
/// ([`dependency_paths`]).
#[must_use]
pub fn watcher() -> config::Watcher {
    config::Watcher::with_dependencies(CONFIG_NAME, dependencies)
}

/// What can change what the appearance settings mean without changing
/// `appearance.yaml`: each themes directory -- a theme installed, removed,
/// or standing in for another changes which file a chosen name reads -- the
/// folder of every theme the settings choose for an axis read with them,
/// with the folders of the wallpaper theme's recommended pictures; and the
/// user's own pictures the settings show -- the wallpaper, each of a
/// time-of-day schedule's, the login screen's -- each as itself, since the
/// folder it is in may hold a great deal else.
///
/// What the desktop's settings watcher follows for this group
/// (`settingswatch::Dependents`, which follows a folder whole and anything
/// else by its name): a change there is announced as a change to
/// `appearance`, and each process's [`watcher`] then compares what it reads
/// -- and the desktop compares its pictures' stamps -- so a theme edited in
/// place, by the `theme` program, a theme editor or a text editor, and a
/// picture saved over under its own name, reach every window as a change of
/// setting does. The list is asked again each time the group is announced,
/// which is when it can change.
#[must_use]
pub fn dependency_paths() -> Vec<PathBuf> {
    dependency_paths_in(&config::load(CONFIG_NAME), &themes::ThemeDirs::standard())
}

/// [`dependency_paths`] for the settings in `doc`, the themes in `dirs`.
fn dependency_paths_in(doc: &Document, dirs: &themes::ThemeDirs) -> Vec<PathBuf> {
    let mut paths = themes::dependency_folders(doc, dirs);
    // The settings as every reader reads them: one decoding of a picture's
    // path, the one the desktop shows it by.
    let s = AppearanceSettings::read_from(doc);
    let login = match &s.login_background {
        LoginBackground::CustomImage(path) => Some(path.clone()),
        _ => None,
    };
    let pictures = s
        .wallpaper
        .iter()
        .cloned()
        .chain(s.wallpaper_schedule.iter().map(|entry| entry.image.clone()))
        .chain(login);
    for picture in pictures {
        if !paths.contains(&picture) {
            paths.push(picture);
        }
    }
    paths
}

/// What the settings read from `doc` depend on besides the document: the
/// chosen theme's file, and -- in the automatic mode -- whether it is light
/// now. The file does not change at 19:00; what it means does.
fn dependencies(doc: &Document) -> Vec<u8> {
    let mut out = themes::fingerprint(doc);
    let automatic = doc
        .get_str(&["theme", "mode"])
        .and_then(|v| ThemeMode::from_yaml_name(&v))
        == Some(ThemeMode::System);
    if automatic {
        let hours = auto_light_hours_in(doc).unwrap_or(DEFAULT_AUTO_LIGHT_HOURS);
        out.push(if auto_light_now(hours) { b'L' } else { b'D' });
    }
    out
}

/// 07:00 until 19:00: the hours the automatic mode is light unless the user
/// says otherwise -- roughly the working day, and the hours a room is most
/// likely lit by daylight across the year in the latitudes most people live
/// in. See `design-decisions.md` §876 for why fixed hours and not sunrise.
pub const DEFAULT_AUTO_LIGHT_HOURS: DailyWindow = match (
    TimeOfDay::from_minutes(7 * 60),
    TimeOfDay::from_minutes(19 * 60),
) {
    (Some(start), Some(end)) => DailyWindow::new(start, end),
    // Unreachable -- both are under a day -- and a `const` cannot panic
    // politely. An empty window (dark all day) is the harmless failure.
    _ => DailyWindow::new(TimeOfDay::MIDNIGHT, TimeOfDay::MIDNIGHT),
};

/// The automatic mode's hours as `doc` states them: both ends or neither, as
/// quiet hours read theirs -- a start paired with a default end is a window
/// nobody chose.
fn auto_light_hours_in(doc: &Document) -> Option<DailyWindow> {
    let start = doc
        .get_str(&["theme", "auto", "light_from"])
        .and_then(|v| TimeOfDay::parse(&v))?;
    let end = doc
        .get_str(&["theme", "auto", "dark_from"])
        .and_then(|v| TimeOfDay::parse(&v))?;
    Some(DailyWindow::new(start, end))
}

/// Whether `hours` contain the time of day now, in the zone the clock is in
/// -- the user's choice in `datetime.yaml`, or the machine's.
///
/// Reads a file and perhaps the machine's zone, so it is asked when the
/// settings are read and when a watcher looks, never per frame.
fn auto_light_now(hours: DailyWindow) -> bool {
    let zone = datetimesettings::DateTimeFile::load()
        .settings
        .rule(datetimesettings::system_zone());
    hours.contains(local_time_of_day(
        datetimesettings::clock::now_utc_secs(),
        zone,
    ))
}

/// The local time of day at `utc_secs` in `zone`.
#[must_use]
pub fn local_time_of_day(utc_secs: u64, zone: Tz) -> TimeOfDay {
    let t = i64::try_from(utc_secs).unwrap_or(i64::MAX);
    let local = t.saturating_add(i64::from(zone.lookup(t).gmtoff));
    // `rem_euclid` of a day, over sixty: 0..1440, so both steps are exact.
    let minutes = u16::try_from(local.rem_euclid(86_400) / 60).unwrap_or(0);
    TimeOfDay::from_minutes(minutes).unwrap_or(TimeOfDay::MIDNIGHT)
}

/// One picture in a time-of-day wallpaper schedule: up from `from` until the
/// next entry's time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduledWallpaper {
    /// When it goes up, in the local time of day.
    pub from: TimeOfDay,
    /// The picture.
    pub image: PathBuf,
}

/// Read `wallpaper.schedule`'s entries: `"HH:MM path"` each, the path encoded
/// when `encoded` says the file encodes its paths.
///
/// An entry that does not start with a time, or names no picture, is left
/// out -- the file is hand-editable, and one line typed wrong should cost
/// that line rather than the schedule. Two entries at the same time: the later
/// line wins, as a later setting does everywhere else in the file. The result
/// is sorted by time.
fn read_wallpaper_schedule(entries: &[String], encoded: bool) -> Vec<ScheduledWallpaper> {
    let mut out: Vec<ScheduledWallpaper> = Vec::new();
    for entry in entries {
        let entry = entry.trim();
        let Some((time, path)) = entry.split_once(char::is_whitespace) else {
            continue;
        };
        let (Some(from), path) = (TimeOfDay::parse(time), path.trim()) else {
            continue;
        };
        if path.is_empty() {
            continue;
        }
        let image = if encoded {
            pathcodec::decode_path(path)
        } else {
            PathBuf::from(path)
        };
        out.retain(|e| e.from != from);
        out.push(ScheduledWallpaper { from, image });
    }
    out.sort_by_key(|e| e.from);
    out
}

/// The colour theme a settings document names, decoded; `None` when it names
/// none -- the key is absent or blank, which is the built-in theme.
///
/// One decoding, shared by the reader and the watcher's fingerprint, so the
/// two cannot disagree about which theme a file means.
pub(crate) fn color_theme_name(doc: &Document) -> Option<std::ffi::OsString> {
    theme_name_at(doc, "colors")
}

/// The widget-style theme a settings document names, decoded; `None` for the
/// built-in one. Shared by the reader and the watcher, as
/// [`color_theme_name`] is.
pub(crate) fn widget_theme_name(doc: &Document) -> Option<std::ffi::OsString> {
    theme_name_at(doc, "widget_style")
}

/// The animation theme a settings document names, decoded; `None` for the
/// built-in one. Shared by the reader and the watcher, as
/// [`color_theme_name`] is.
pub(crate) fn animation_theme_name(doc: &Document) -> Option<std::ffi::OsString> {
    theme_name_at(doc, "animation")
}

/// The window-decorations theme a settings document names, decoded; `None`
/// for the built-in one. Shared by the reader and the watcher, as
/// [`color_theme_name`] is.
pub(crate) fn decoration_theme_name(doc: &Document) -> Option<std::ffi::OsString> {
    theme_name_at(doc, "decorations")
}

/// The taskbar-panel theme a settings document names, decoded; `None` for
/// the built-in one. Shared by the reader and the watcher, as
/// [`color_theme_name`] is.
pub(crate) fn panel_theme_name(doc: &Document) -> Option<std::ffi::OsString> {
    theme_name_at(doc, "taskbar_panel")
}

/// The wallpaper theme a settings document names, decoded; `None` for the
/// built-in one. Shared by the reader and the watcher, as
/// [`color_theme_name`] is.
pub(crate) fn wallpaper_theme_name(doc: &Document) -> Option<std::ffi::OsString> {
    theme_name_at(doc, "wallpaper")
}

/// The font theme a settings document names, decoded; `None` for the
/// built-in one. Shared by the reader and the watcher, as
/// [`color_theme_name`] is.
pub(crate) fn font_theme_name(doc: &Document) -> Option<std::ffi::OsString> {
    theme_name_at(doc, "fonts")
}

/// The theme `theme.<axis>` names, decoded; `None` when the key is absent or
/// blank.
fn theme_name_at(doc: &Document, axis: &str) -> Option<std::ffi::OsString> {
    let name = doc.get_str(&["theme", axis])?;
    let name = name.trim();
    (!name.is_empty()).then(|| pathcodec::decode_path(name).into_os_string())
}

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
        assert_eq!(f.ui_font, "Open Sans");
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

    /// `IconSize::ALL` is every size, smallest first. The match stops
    /// compiling when a size is added, which is the moment `ALL` needs it.
    #[test]
    fn every_icon_size_is_in_all_smallest_first() {
        for size in IconSize::ALL {
            match size {
                IconSize::Small | IconSize::Medium | IconSize::Large | IconSize::ExtraLarge => {}
            }
        }
        assert_eq!(IconSize::ALL.len(), 4);
        assert!(
            IconSize::ALL
                .windows(2)
                .all(|pair| pair[0].pixels() < pair[1].pixels()),
            "not smallest first"
        );
    }

    // ---- CursorSize ----

    #[test]
    fn test_cursor_size_pixels() {
        assert_eq!(CursorSize::Small.pixels(), 16);
        assert_eq!(CursorSize::Normal.pixels(), 24);
        assert_eq!(CursorSize::Large.pixels(), 32);
        assert_eq!(CursorSize::ExtraLarge.pixels(), 48);
        assert_eq!(CursorSize::Huge.pixels(), 64);
        assert_eq!(CursorSize::Giant.pixels(), 96);
    }

    /// `CursorSize::ALL` is every size, smallest first -- the match fails to
    /// compile when a size is added, which is the prompt to add it here too.
    #[test]
    fn every_cursor_size_is_in_all_smallest_first() {
        for size in CursorSize::ALL {
            match size {
                CursorSize::Small
                | CursorSize::Normal
                | CursorSize::Large
                | CursorSize::ExtraLarge
                | CursorSize::Huge
                | CursorSize::Giant => {}
            }
        }
        assert_eq!(CursorSize::ALL.len(), 6);
        assert!(
            CursorSize::ALL
                .windows(2)
                .all(|pair| pair[0].pixels() < pair[1].pixels()),
            "not smallest first"
        );
        // The largest step keeps what `inputsettings`' range was for: a
        // pointer four times the normal one.
        assert_eq!(CursorSize::Giant.pixels(), CursorSize::Normal.pixels() * 4);
    }

    /// Every size and every scheme survives `appearance.yaml`.
    #[test]
    fn every_cursor_size_and_scheme_round_trips() {
        for size in CursorSize::ALL {
            for scheme in CursorScheme::ALL {
                let settings = AppearanceSettings {
                    cursor_size: *size,
                    cursor_scheme: *scheme,
                    ..AppearanceSettings::default()
                };
                let mut doc = Document::new();
                settings.write_into(&mut doc);
                let reread = AppearanceSettings::read_from(&Document::parse(&doc.to_text()));
                assert_eq!(reread.cursor_size, *size);
                assert_eq!(reread.cursor_scheme, *scheme);
            }
        }
        assert_eq!(CursorScheme::ALL.len(), 3);
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
    /// A family this machine does not have leaves the working font alone.
    ///
    /// Only the two non-mutating outcomes are asserted here, deliberately.
    /// `guitk`'s font selection is per-process global state, so a test that
    /// successfully installed a family would change the face every *other*
    /// test in this binary measures text with, and the damage would surface as
    /// failures in unrelated assertions about layout. What can be checked
    /// safely is that a failed lookup changes nothing, which is the property
    /// the callers depend on: they discard the outcome precisely because a
    /// miss is harmless.
    #[test]
    fn a_missing_family_is_reported_and_changes_nothing() {
        let fonts = FontSettings {
            ui_font: "NoSuchFamily-8f3a2c".to_string(),
            mono_font: "NoSuchMonoFamily-8f3a2c".to_string(),
            ..FontSettings::default()
        };
        let before_ui = guitk::text::font_family();
        let before_mono = guitk::text::mono_family();
        assert_eq!(
            fonts.apply(),
            FontsApplied {
                ui: FontOutcome::Missing,
                mono: FontOutcome::Missing
            }
        );
        assert_eq!(
            guitk::text::font_family(),
            before_ui,
            "a failed lookup changed the UI font"
        );
        assert_eq!(
            guitk::text::mono_family(),
            before_mono,
            "a failed lookup changed the monospace font"
        );
    }

    /// The settings' rasterizing choices reach the toolkit's own cache.
    ///
    /// Text the toolkit rasterizes itself was drawn unhinted while the
    /// compositor's was hinted. The cache starts unhinted, and every caller of
    /// `apply` in this binary applies the defaults, whose hinting is on --
    /// so this holds whatever runs beside it.
    #[test]
    fn applying_the_fonts_sets_how_this_process_rasterizes() {
        let _ = FontSettings::default().apply();
        let r = guitk::text::rendering();
        assert!(r.hinting, "hinting did not reach the toolkit's cache");
        assert!(r.smoothing);
        assert_eq!(r.subpixel, guitk::text::Subpixel::Rgb);
    }

    /// **Applying the fonts sets the toolkit's text size**, on the thread
    /// that applies them -- the one that lays the program's windows out -- so
    /// the toolkit's controls follow the user's size. The families stay the
    /// defaults here, as every caller of `apply` in this binary leaves them.
    #[test]
    fn applying_the_fonts_sets_the_toolkits_text_size() {
        let fonts = FontSettings {
            ui_size: 20.0,
            ..FontSettings::default()
        };
        let _ = fonts.apply();
        assert!((guitk::text::base_size() - 20.0).abs() < f32::EPSILON);
        let _ = FontSettings::default().apply();
        assert!((guitk::text::base_size() - guitk::text::DEFAULT_SIZE).abs() < f32::EPSILON);
    }

    /// One mapping from the settings to a rasterizer's terms, field by field.
    #[test]
    fn the_rendering_is_the_settings_field_for_field() {
        use guitk::text::{ColourPalette, Subpixel};
        let plain = FontSettings {
            hinting: false,
            smoothing: false,
            subpixel: SubpixelMode::None,
            ..FontSettings::default()
        };
        let r = plain.rendering(ColourPalette::Dark);
        assert!(!r.hinting && !r.smoothing);
        assert_eq!(r.subpixel, Subpixel::None);
        assert_eq!(
            r.palette,
            ColourPalette::Dark,
            "the palette is the caller's"
        );
        for (mode, want) in [
            (SubpixelMode::Rgb, Subpixel::Rgb),
            (SubpixelMode::Bgr, Subpixel::Bgr),
            (SubpixelMode::VRgb, Subpixel::VRgb),
            (SubpixelMode::VBgr, Subpixel::VBgr),
        ] {
            let s = FontSettings {
                subpixel: mode,
                ..FontSettings::default()
            };
            assert_eq!(s.rendering(ColourPalette::Light).subpixel, want, "{mode:?}");
            assert!(s.rendering(ColourPalette::Light).hinting);
        }
    }

    /// Choosing nothing is not the same as choosing something absent.
    ///
    /// The distinction is the reason this returns an enum rather than a bool:
    /// a settings page that showed "not installed" for a user who has simply
    /// expressed no preference would be inventing a problem.
    #[test]
    fn an_empty_family_is_unset_rather_than_missing() {
        let fonts = FontSettings {
            ui_font: String::new(),
            mono_font: String::new(),
            ..FontSettings::default()
        };
        assert_eq!(
            fonts.apply(),
            FontsApplied {
                ui: FontOutcome::Unset,
                mono: FontOutcome::Unset
            }
        );
    }

    // ---- Configuration file ----

    /// The colour theme [`all_non_default`] chooses: its folder's name, with a
    /// space, a `%` and a letter outside ASCII in it for the encoding to get
    /// wrong, and its file.
    const ROUND_TRIP_THEME: &str = "nord 100% ça";
    const ROUND_TRIP_THEME_FILE: &str =
        "colors:\n  base: \"#102030\"\ncolors-light:\n  text: \"#0a0b0c\"\n";
    /// The round trip's widget-style theme: a third theme, so a writer that
    /// crossed one axis's name into another's key is caught.
    const ROUND_TRIP_WIDGETS: &str = "round été";
    const ROUND_TRIP_WIDGETS_FILE: &str =
        "widget-style:\n  button:\n    radius: 11\n    gloss: false\n  toggle: checkbox\n";
    /// The round trip's animation theme: a fourth, for the same reason.
    const ROUND_TRIP_ANIMATION: &str = "ressort ü";
    const ROUND_TRIP_ANIMATION_FILE: &str = "animation:\n  duration-ms: 320\n  easing: spring\n";
    /// The round trip's window-frames theme: a fifth.
    const ROUND_TRIP_DECORATIONS: &str = "cadres é";
    const ROUND_TRIP_DECORATIONS_FILE: &str =
        "window-decorations:\n  title-bar:\n    height: 36\n  buttons:\n    side: left\n";
    /// The round trip's taskbar-panel theme: a sixth.
    const ROUND_TRIP_PANEL: &str = "barre ö";
    const ROUND_TRIP_PANEL_FILE: &str = "taskbar-panel:\n  gloss: 0.25\n  spacing:\n    tiles: 4\n";
    /// The round trip's wallpaper theme: a seventh, recommending one picture
    /// for dark mode, which the round trip puts in its folder.
    const ROUND_TRIP_WALLPAPERS: &str = "fonds ï";
    const ROUND_TRIP_WALLPAPERS_FILE: &str = "wallpapers:\n  dark: wallpapers/night.png\n";
    /// The round trip's font theme: an eighth, recommending two families for
    /// the interface's text and one for code.
    const ROUND_TRIP_FONTS: &str = "polices ë";
    const ROUND_TRIP_FONTS_FILE: &str = "fonts:\n  ui: [Inter, Cantarell]\n  mono: Fira Code\n";

    /// Install a theme in the scratch user's data directory under `root`,
    /// where `AppearanceSettings::read_from` will look for it.
    fn install_theme(root: &std::path::Path, name: &str, text: &str) {
        let dir = config::testing::scratch_data_dir(root)
            .join("slateos")
            .join("themes")
            .join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(themes::FILE_NAME), text).unwrap();
    }

    /// Settings that differ from the defaults in every field, so a
    /// round-trip test cannot pass by accident on a field it forgot.
    fn all_non_default() -> AppearanceSettings {
        AppearanceSettings {
            // Read back from a file the round trip installs, so the colours
            // here are the file's.
            color_theme: themes::ColorTheme::from_colors(
                ROUND_TRIP_THEME,
                themes::parse(ROUND_TRIP_THEME_FILE).colors,
            ),
            // A theme of its own, not the colour theme's, so a round trip that
            // wrote one axis into the other would be caught.
            icon_theme: icons::IconTheme::load(std::ffi::OsStr::new("line-icons")),
            // Another again, and another desktop's: a cursor theme need not
            // be a SlateOS theme at all.
            cursor_theme: cursors::CursorTheme::load(std::ffi::OsStr::new("Adwaita")),
            // Another desktop's sound theme, and none of the others': the
            // sound axis is its own.
            sound_theme: sounds::SoundTheme::load(std::ffi::OsStr::new("Yaru")),
            // Off, an uncommon volume, and an event of each kind: a file
            // whose name holds a space, a non-ASCII letter and a `%`, and a
            // silence.
            sounds: SoundSettings {
                enabled: false,
                volume: 0.35,
                events: BTreeMap::from([
                    ("dialog-error".to_string(), EventSound::Off),
                    (
                        "message-new-instant".to_string(),
                        EventSound::File(PathBuf::from(host_absolute(
                            "home/u/Sounds/d\u{ed}ng 100%.oga",
                        ))),
                    ),
                ]),
            },
            // A third, read back from the file the round trip installs.
            widget_theme: themes::WidgetTheme::from_style(
                ROUND_TRIP_WIDGETS,
                themes::parse(ROUND_TRIP_WIDGETS_FILE)
                    .widget_style
                    .expect("the fixture sets a widget style"),
            ),
            // A fourth, read back from the file the round trip installs.
            animation_theme: themes::AnimationTheme::from_motion(
                ROUND_TRIP_ANIMATION,
                themes::parse(ROUND_TRIP_ANIMATION_FILE)
                    .motion
                    .expect("the fixture sets a motion"),
            ),
            // A fifth, for the window frames.
            decoration_theme: themes::DecorationTheme::from_style(
                ROUND_TRIP_DECORATIONS,
                themes::parse(ROUND_TRIP_DECORATIONS_FILE)
                    .decorations
                    .expect("the fixture sets window frames"),
            ),
            // A sixth, for the taskbar's panel.
            panel_theme: themes::PanelTheme::from_style(
                ROUND_TRIP_PANEL,
                themes::parse(ROUND_TRIP_PANEL_FILE)
                    .panel
                    .expect("the fixture sets a taskbar panel"),
            ),
            // A seventh, for the wallpaper: only its name is written; what it
            // reads back as is a path in the scratch folder, which the round
            // trip compares inside it.
            wallpaper_theme: themes::WallpaperTheme::from_pictures(
                ROUND_TRIP_WALLPAPERS,
                None,
                None,
            ),
            // An eighth, for the fonts: read back as its file names them.
            font_theme: themes::FontTheme::from_families(
                ROUND_TRIP_FONTS,
                vec!["Inter".to_string(), "Cantarell".to_string()],
                vec!["Fira Code".to_string()],
            ),
            // Every one of these differs from the default, which is what the
            // fixture is for: the defaults are `None`, 600 and `true`.
            wallpaper_folder: Some(PathBuf::from("/home/u/Pictures/rotation")),
            // Non-default, like every other field here: the default is empty.
            wallpaper_exclusions: vec!["*.gif".to_string(), "draft-*".to_string()],
            // Two pictures, one with a space, a non-ASCII letter and a `%` in
            // its name, so a round trip that lost the path codec shows.
            wallpaper_schedule: vec![
                ScheduledWallpaper {
                    from: TimeOfDay::new(6, 0).unwrap(),
                    image: PathBuf::from("/home/u/Pictures/d\u{ed}a 100%.png"),
                },
                ScheduledWallpaper {
                    from: TimeOfDay::new(18, 30).unwrap(),
                    image: PathBuf::from("/home/u/Pictures/night.png"),
                },
            ],
            // Not `SameAsDesktop`: that one carries no value, so a round trip
            // could lose the path and still compare equal. The variant with
            // something to lose is the one worth round-tripping.
            login_background: LoginBackground::CustomImage(PathBuf::from(
                "/home/u/greeter/100% sure.png",
            )),
            wallpaper_interval_secs: 45,
            wallpaper_shuffle: false,
            // A path with a space and a non-ASCII character in it, because a
            // wallpaper is the one appearance setting whose value comes from a
            // filesystem the user named, and a tidy ASCII fixture would pass
            // through a codec that mangled either.
            wallpaper_fit: ImageFit::Tile,
            wallpaper_position: (0.25, 0.75),
            wallpaper: Some(PathBuf::from("/home/u/Pictures/maíz del alba.png")),
            theme_mode: ThemeMode::Light,
            // Not 07:00-19:00. Read back whatever the mode, since the hours are
            // a setting even while the mode does not use them.
            auto_light_hours: DailyWindow::from_hm(6, 30, 20, 15).unwrap(),
            // Derived, and `false` outside the automatic mode -- which this
            // fixture is not in -- so the default is the only value it can
            // round-trip as.
            auto_is_light: false,
            caret_width_scale: 2.5,
            focus_ring_scale: 3.0,
            night_light: true,
            // Not 0.5, which is the default, and not 1.0 either: a value in
            // the middle of the range catches a writer that clamped when it
            // should not have.
            night_light_strength: 0.8,
            // Non-default, which is this helper's whole contract: the
            // round-trip test must not be able to pass on a field it forgot.
            surface_style: SurfaceStyle::Cards,
            strip_style: StripStyle::Separator,
            color_filter: ColorFilter::Tritanopia,
            high_contrast: Some(HighContrastScheme::YellowOnBlack),
            accent_color: AccentColor::Custom,
            custom_accent: Color::rgba(1, 2, 3, 4),
            // The outlined look's, since the filled one is in use: a
            // different accent *and* a different custom colour, so a writer
            // that crossed the two looks over, or wrote one look twice, is
            // caught on either value.
            other_look_colours: LookColours {
                accent_color: AccentColor::Mauve,
                custom_accent: Color::rgba(5, 6, 7, 8),
            },
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
            taskbar_autohide: true,
            taskbar_labels: false,
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
        // The colour theme is read from its own file, so that file has to be
        // where the reader looks: a scratch user's data directory.
        let (reread, wallpapers) = config::testing::with_scratch_config("round-trip", |root| {
            install_theme(root, ROUND_TRIP_THEME, ROUND_TRIP_THEME_FILE);
            install_theme(root, ROUND_TRIP_WIDGETS, ROUND_TRIP_WIDGETS_FILE);
            install_theme(root, ROUND_TRIP_ANIMATION, ROUND_TRIP_ANIMATION_FILE);
            install_theme(root, ROUND_TRIP_DECORATIONS, ROUND_TRIP_DECORATIONS_FILE);
            install_theme(root, ROUND_TRIP_PANEL, ROUND_TRIP_PANEL_FILE);
            install_theme(root, ROUND_TRIP_WALLPAPERS, ROUND_TRIP_WALLPAPERS_FILE);
            install_theme(root, ROUND_TRIP_FONTS, ROUND_TRIP_FONTS_FILE);
            let folder = config::testing::scratch_data_dir(root)
                .join("slateos")
                .join("themes")
                .join(ROUND_TRIP_WALLPAPERS);
            let night = folder.join(themes::WALLPAPERS_DIR).join("night.png");
            std::fs::create_dir_all(night.parent().unwrap()).unwrap();
            std::fs::write(&night, b"a picture").unwrap();
            let wallpapers =
                themes::WallpaperTheme::from_pictures(ROUND_TRIP_WALLPAPERS, Some(night), None);
            (
                AppearanceSettings::read_from(&Document::parse(&doc.to_text())),
                wallpapers,
            )
        });
        let mut settings = settings;
        settings.wallpaper_theme = wallpapers;
        assert_eq!(reread, settings);
    }

    /// The file names the colour theme by its folder, encoded as a filename
    /// is -- so the name survives whatever bytes are in it -- and the name is
    /// written even for the built-in theme, so the key is there to edit.
    #[test]
    fn the_colour_theme_is_written_by_name() {
        let mut doc = Document::new();
        AppearanceSettings::default().write_into(&mut doc);
        assert_eq!(doc.get_str(&["theme", "colors"]).as_deref(), Some("aero"));

        all_non_default().write_into(&mut doc);
        assert_eq!(
            doc.get_str(&["theme", "colors"]).as_deref(),
            Some("nord 100%25 %C3%A7a")
        );
    }

    /// **Windows' titles are on the taskbar unless the file says otherwise**:
    /// the Aero reference labels every running window, and `taskbar.labels`
    /// is what says otherwise.
    #[test]
    fn the_taskbar_shows_titles_unless_the_file_says_not() {
        assert!(AppearanceSettings::default().taskbar_labels);
        let off = AppearanceSettings::read_from(&Document::parse("taskbar:\n  labels: false\n"));
        assert!(!off.taskbar_labels);
        let unrelated =
            AppearanceSettings::read_from(&Document::parse("taskbar:\n  autohide: true\n"));
        assert!(
            unrelated.taskbar_labels,
            "a file that does not say keeps the default"
        );
    }

    /// A theme's colours reach the palette from nothing but the settings file
    /// and the theme's own: the path every reader of the settings takes.
    #[test]
    fn a_chosen_themes_colours_reach_the_palette() {
        config::testing::with_scratch_config("theme-palette", |root| {
            install_theme(root, "nord", "colors:\n  base: \"#2e3440\"\n");
            let s = AppearanceSettings::read_from(&Document::parse("theme:\n  colors: nord\n"));
            assert_eq!(s.color_theme.problem(), None);
            assert_eq!(Palette::from_settings(&s).base, Color::from_hex(0x2E3440));
        });
    }

    /// A theme that cannot be used shows the built-in colours and says why --
    /// and keeps its name, so that saving an unrelated setting does not
    /// quietly put the user back on the built-in theme for good.
    #[test]
    fn a_theme_that_is_not_installed_keeps_its_name_through_a_save() {
        config::testing::with_scratch_config("theme-missing", |_| {
            let doc = Document::parse("theme:\n  colors: gone\n");
            let s = AppearanceSettings::read_from(&doc);
            assert_eq!(s.color_theme.id(), "gone");
            assert_eq!(s.color_theme.colors(), None);
            assert!(
                s.color_theme
                    .problem()
                    .is_some_and(|why| why.contains("\"gone\" is not installed")),
                "{:?}",
                s.color_theme.problem()
            );
            assert_eq!(
                Palette::from_settings(&s),
                Palette::from_settings(&AppearanceSettings::default())
            );

            let mut saved = doc.clone();
            s.write_into(&mut saved);
            assert_eq!(saved.get_str(&["theme", "colors"]).as_deref(), Some("gone"));
        });
    }

    /// The icon theme is its own setting: read from `theme.icons`, written
    /// back there, independent of the colour theme, and the built-in one when
    /// the file names none or a blank. A folder name that is not text comes
    /// back byte for byte.
    #[test]
    fn the_icon_theme_is_its_own_setting_and_survives_a_save() {
        config::testing::with_scratch_config("icon-theme", |_| {
            let s = AppearanceSettings::read_from(&Document::parse(""));
            assert_eq!(s.icon_theme, icons::IconTheme::built_in());

            let doc = Document::parse("theme:\n  colors: nord\n  icons: papirus\n");
            let s = AppearanceSettings::read_from(&doc);
            assert_eq!(s.icon_theme.id(), "papirus");
            assert_eq!(s.color_theme.id(), "nord");
            let mut saved = Document::parse("");
            s.write_into(&mut saved);
            assert_eq!(
                saved.get_str(&["theme", "icons"]).as_deref(),
                Some("papirus")
            );

            let blank = AppearanceSettings::read_from(&Document::parse("theme:\n  icons: \" \"\n"));
            assert_eq!(blank.icon_theme, icons::IconTheme::built_in());

            let odd = std::path::Path::new(&pathcodec::decode_path("caf%E9"))
                .as_os_str()
                .to_os_string();
            let mut settings = AppearanceSettings::default();
            settings.icon_theme = icons::IconTheme::load(&odd);
            let mut written = Document::parse("");
            settings.write_into(&mut written);
            let back = AppearanceSettings::read_from(&written);
            assert_eq!(back.icon_theme.id(), odd.as_os_str());
        });
    }

    /// The cursor theme is its own setting too: `theme.cursors`, apart from
    /// the icon theme even where an icon theme and a cursor theme share a
    /// folder (as Adwaita's do), the built-in one -- the compositor's own
    /// pointer -- when the file names none or a blank, and a folder name that
    /// is not text kept byte for byte.
    #[test]
    fn the_cursor_theme_is_its_own_setting_and_survives_a_save() {
        config::testing::with_scratch_config("cursor-theme", |_| {
            let s = AppearanceSettings::read_from(&Document::parse(""));
            assert!(s.cursor_theme.is_built_in());

            let doc = Document::parse("theme:\n  icons: papirus\n  cursors: Adwaita\n");
            let s = AppearanceSettings::read_from(&doc);
            assert_eq!(s.cursor_theme.id(), "Adwaita");
            assert_eq!(s.icon_theme.id(), "papirus");
            let mut saved = Document::parse("");
            s.write_into(&mut saved);
            assert_eq!(
                saved.get_str(&["theme", "cursors"]).as_deref(),
                Some("Adwaita")
            );
            assert_eq!(
                saved.get_str(&["theme", "icons"]).as_deref(),
                Some("papirus")
            );

            let blank =
                AppearanceSettings::read_from(&Document::parse("theme:\n  cursors: \" \"\n"));
            assert!(blank.cursor_theme.is_built_in());

            let odd = std::path::Path::new(&pathcodec::decode_path("caf%E9"))
                .as_os_str()
                .to_os_string();
            let mut settings = AppearanceSettings::default();
            settings.cursor_theme = cursors::CursorTheme::load(&odd);
            let mut written = Document::parse("");
            settings.write_into(&mut written);
            let back = AppearanceSettings::read_from(&written);
            assert_eq!(back.cursor_theme.id(), odd.as_os_str());
        });
    }

    /// `tail` made absolute on the host the tests run on. A SlateOS path is
    /// absolute from `/`, but `Path::is_absolute` on a Windows host also
    /// wants a drive -- and with one, the path is still spelled with `/`
    /// alone, so it reads the same in a file as anywhere else.
    fn host_absolute(tail: &str) -> String {
        if cfg!(windows) {
            format!("C:/{tail}")
        } else {
            format!("/{tail}")
        }
    }

    /// Settings whose sound theme is `id` under a scratch directory's roots,
    /// with one file there: `t/stereo/bell.oga`.
    fn with_sound_theme(scratch: &scratchdir::ScratchDir) -> (AppearanceSettings, PathBuf) {
        let share = scratch.dir().join("share");
        let bell = share.join("t").join("stereo").join("bell.oga");
        std::fs::create_dir_all(bell.parent().unwrap()).unwrap();
        std::fs::write(&bell, b"a sound").unwrap();
        let mut s = AppearanceSettings::default();
        s.sound_theme = sounds::SoundTheme::named(
            std::ffi::OsStr::new("t"),
            themes::ThemeDirs {
                user: None,
                system: scratch.dir().join("system"),
            },
            vec![share],
        );
        (s, bell)
    }

    /// **An event's sound is the user's own, then the theme's, then the
    /// built-in one** -- the user's for an event's shorter name too, so a
    /// sound chosen for `dialog-error` is heard for `dialog-error-serious`.
    #[test]
    fn an_events_sound_is_the_users_then_the_themes_then_the_built_in() {
        let scratch = scratchdir::ScratchDir::new("appearance-sound-for");
        let (mut s, bell) = with_sound_theme(&scratch);
        assert_eq!(s.sound_for("bell"), sounds::SoundChoice::File(bell.clone()));
        assert_eq!(
            s.sound_for("trash-empty"),
            sounds::SoundChoice::BuiltIn("trash-empty".to_string())
        );
        let mine = PathBuf::from("/home/u/my bell.wav");
        s.sounds
            .events
            .insert("bell".to_string(), EventSound::File(mine.clone()));
        s.sounds
            .events
            .insert("dialog-error".to_string(), EventSound::Off);
        assert_eq!(s.sound_for("bell"), sounds::SoundChoice::File(mine));
        assert_eq!(
            s.sound_for("dialog-error-serious"),
            sounds::SoundChoice::Silent
        );
        assert_eq!(s.sound_for(" bell "), s.sound_for("bell"), "trimmed");
        assert_eq!(s.sound_for("Not An Event"), sounds::SoundChoice::Silent);
    }

    /// **With sounds off, no event makes one** -- not the user's own, not the
    /// theme's, not the built-in.
    #[test]
    fn with_sounds_off_nothing_sounds() {
        let scratch = scratchdir::ScratchDir::new("appearance-sounds-off");
        let (mut s, _) = with_sound_theme(&scratch);
        s.sounds.enabled = false;
        s.sounds.events.insert(
            "complete".to_string(),
            EventSound::File(PathBuf::from("/x.oga")),
        );
        for event in ["bell", "complete", "trash-empty"] {
            assert_eq!(s.sound_for(event), sounds::SoundChoice::Silent, "{event}");
        }
    }

    /// **The file spells a choice `off` or an absolute path**; a relative
    /// path, a blank and a name that is no event's are passed over, and the
    /// rest are kept.
    #[test]
    fn the_file_spells_a_choice_off_or_an_absolute_path() {
        let done = host_absolute("home/u/done.oga");
        let doc = Document::parse(&format!(
            "theme:\n  sounds: Yaru\nsounds:\n  enabled: false\n  volume: 0.25\n  events:\n    bell: relative/x.oga\n    Bad: \"off\"\n    trash-empty: \"off\"\n    complete: {done}\n    message: \" \"\n",
        ));
        let s = AppearanceSettings::read_from(&doc);
        assert_eq!(s.sound_theme.id(), "Yaru");
        assert!(!s.sounds.enabled);
        assert!((s.sounds.volume - 0.25).abs() < 1e-6);
        assert_eq!(
            s.sounds.events,
            BTreeMap::from([
                ("trash-empty".to_string(), EventSound::Off),
                (
                    "complete".to_string(),
                    EventSound::File(PathBuf::from(&done))
                ),
            ])
        );
        assert_eq!(EventSound::parse("off"), Some(EventSound::Off));
        assert_eq!(EventSound::parse(" off "), Some(EventSound::Off));
        assert_eq!(EventSound::parse("relative.oga"), None);
        assert_eq!(EventSound::parse(""), None);
        // Spelled as the file spells any path: a `%` is `%25`.
        assert_eq!(
            EventSound::parse(&host_absolute("100%25.oga")),
            Some(EventSound::File(PathBuf::from(host_absolute("100%.oga"))))
        );
    }

    /// **An event given back to the theme leaves the file**, and the
    /// defaults -- sounds on, the built-in theme -- are written as such.
    #[test]
    fn an_event_given_back_to_the_theme_leaves_the_file() {
        let mut s = AppearanceSettings::default();
        let mut doc = Document::parse("");
        s.write_into(&mut doc);
        assert_eq!(doc.get_bool(&["sounds", "enabled"]), Some(true));
        assert_eq!(
            doc.get_str(&["theme", "sounds"]).as_deref(),
            Some(themes::BUILT_IN)
        );
        s.sounds.events.insert("bell".to_string(), EventSound::Off);
        s.sounds.events.insert(
            "complete".to_string(),
            EventSound::File(PathBuf::from(host_absolute("a.oga"))),
        );
        s.write_into(&mut doc);
        assert_eq!(doc.keys(&["sounds", "events"]), ["bell", "complete"]);
        s.sounds.events.remove("bell");
        s.write_into(&mut doc);
        assert_eq!(doc.keys(&["sounds", "events"]), ["complete"]);
        assert_eq!(AppearanceSettings::read_from(&doc).sounds, s.sounds);
    }

    /// **A volume is held to 0 to 1, a NaN is the default, and a name that is
    /// no event's is dropped.**
    #[test]
    fn the_sound_settings_are_validated() {
        let mut s = AppearanceSettings::default();
        s.sounds.volume = 3.0;
        s.validate();
        assert_eq!(s.sounds.volume, 1.0);
        s.sounds.volume = -1.0;
        s.validate();
        assert_eq!(s.sounds.volume, 0.0);
        s.sounds.volume = f32::NAN;
        s.validate();
        assert_eq!(s.sounds.volume, DEFAULT_SOUND_VOLUME);
        s.sounds
            .events
            .insert("../escape".to_string(), EventSound::Off);
        s.sounds.events.insert("bell".to_string(), EventSound::Off);
        s.validate();
        assert_eq!(s.sounds.events.keys().collect::<Vec<_>>(), ["bell"]);
    }

    /// A blank name is the built-in theme, as a blanked wallpaper is none.
    #[test]
    fn a_blank_colour_theme_is_the_built_in_one() {
        let s = AppearanceSettings::read_from(&Document::parse("theme:\n  colors: \"  \"\n"));
        assert_eq!(s.color_theme, themes::ColorTheme::built_in());
    }

    /// `watcher()` sees the chosen theme's own file edited in place -- which
    /// changes the colours without changing a byte of `appearance.yaml`, and
    /// which a watcher of that file alone reported as nothing.
    #[test]
    fn the_appearance_watcher_sees_the_chosen_theme_edited_in_place() {
        config::testing::with_scratch_config("watch-theme", |root| {
            install_theme(root, "nord", "colors:\n  base: \"#2e3440\"\n");
            let mut file = AppearanceFile::load();
            file.settings.color_theme = themes::ColorTheme::load(std::ffi::OsStr::new("nord"));
            file.save().unwrap();

            let mut plain = config::Watcher::new(CONFIG_NAME);
            let mut w = watcher();
            assert!(
                plain.poll().is_some() && w.poll().is_some(),
                "the first look"
            );
            assert!(w.poll().is_none(), "nothing has changed");

            install_theme(root, "nord", "colors:\n  base: \"#000000\"\n");
            assert!(
                plain.poll().is_none(),
                "appearance.yaml itself did not change"
            );
            let doc = w.poll().expect("the theme changed, so the colours did");
            let s = AppearanceSettings::read_from(&doc);
            assert_eq!(Palette::from_settings(&s).base, Color::from_hex(0x000000));
            assert!(w.poll().is_none(), "reported once");

            // Uninstalled is a change too: the colours fall back.
            std::fs::remove_dir_all(
                config::testing::scratch_data_dir(root).join("slateos/themes/nord"),
            )
            .unwrap();
            let doc = w.poll().expect("the theme went away");
            assert!(
                AppearanceSettings::read_from(&doc)
                    .color_theme
                    .problem()
                    .is_some()
            );
        });
    }

    /// What the desktop's settings watcher follows for this group is the
    /// chosen theme's folder -- where an edit in place happens -- and the
    /// themes directory it is in; after the theme is chosen away, no longer
    /// its folder.
    #[test]
    fn the_folders_followed_are_the_chosen_themes() {
        config::testing::with_scratch_config("watch-folders", |root| {
            install_theme(root, "nord", "colors:\n  base: \"#2e3440\"\n");
            let themes_dir = config::testing::scratch_data_dir(root).join("slateos/themes");
            let nord = themes_dir.join("nord");
            assert!(!dependency_paths().contains(&nord), "not chosen yet");

            let mut file = AppearanceFile::load();
            file.settings.color_theme = themes::ColorTheme::load(std::ffi::OsStr::new("nord"));
            file.save().unwrap();
            let followed = dependency_paths();
            assert!(followed.contains(&nord), "{followed:?}");
            assert!(followed.contains(&themes_dir), "{followed:?}");

            file.settings.color_theme = themes::ColorTheme::built_in();
            file.save().unwrap();
            let followed = dependency_paths();
            assert!(!followed.contains(&nord), "{followed:?}");
            assert!(followed.contains(&themes_dir), "{followed:?}");
        });
    }

    /// The user's own pictures the settings show are followed each as
    /// itself -- the wallpaper, a schedule's, the login screen's -- whether
    /// or not the picture is there yet: its folder is what is watched.
    #[test]
    fn the_pictures_shown_are_followed_by_their_paths() {
        let scratch = scratchdir::ScratchDir::new("appearance-followed-pictures");
        let system = scratch.dir().join("themes");
        let dirs = themes::ThemeDirs {
            user: None,
            system: system.clone(),
        };
        let mut s = AppearanceSettings {
            wallpaper: Some(PathBuf::from("/home/u/Pictures/a.png")),
            login_background: LoginBackground::CustomImage(PathBuf::from("/home/u/login.jpg")),
            ..AppearanceSettings::default()
        };
        s.wallpaper_schedule = vec![
            ScheduledWallpaper {
                from: TimeOfDay::from_minutes(7 * 60).unwrap(),
                image: PathBuf::from("/home/u/Pictures/day.png"),
            },
            ScheduledWallpaper {
                from: TimeOfDay::from_minutes(19 * 60).unwrap(),
                // The fixed wallpaper again: listed once.
                image: PathBuf::from("/home/u/Pictures/a.png"),
            },
        ];
        let mut doc = Document::new();
        s.write_into(&mut doc);
        let followed = dependency_paths_in(&doc, &dirs);
        assert_eq!(
            followed,
            [
                // The themes directory, not made yet, by its name.
                system.clone(),
                PathBuf::from("/home/u/Pictures/a.png"),
                PathBuf::from("/home/u/Pictures/day.png"),
                PathBuf::from("/home/u/login.jpg"),
            ]
        );
        // A login screen in a colour, and no wallpaper: nothing of the user's.
        let plain = AppearanceSettings::default();
        let mut doc = Document::new();
        plain.write_into(&mut doc);
        assert_eq!(dependency_paths_in(&doc, &dirs), [system]);
    }

    /// **A theme is worn on every axis it covers, and on no other**: a full
    /// theme on all ten, a colours-only theme on its colours alone with the
    /// rest left as they were, the built-in theme on everything compiled in.
    #[test]
    fn a_theme_is_worn_on_every_axis_it_covers() {
        let scratch = scratchdir::ScratchDir::new("appearance-wear-theme");
        let dirs = themes::ThemeDirs {
            user: Some(scratch.dir().join("user")),
            system: scratch.dir().join("system"),
        };
        let full = scratch.dir().join("user").join("full");
        for folder in ["icons", "cursors", "stereo", "wallpapers"] {
            std::fs::create_dir_all(full.join(folder)).unwrap();
        }
        std::fs::write(full.join("wallpapers/night.png"), b"not decoded here").unwrap();
        std::fs::write(
            full.join("theme.yaml"),
            "colors:\n  base: \"#101010\"\n\
             widget-style:\n  button:\n    radius: 9\n\
             animation:\n  duration-ms: 300\n\
             window-decorations:\n  border: 2\n\
             taskbar-panel:\n  gloss: 0\n\
             wallpapers:\n  dark: wallpapers/night.png\n\
             fonts:\n  ui: [Inter]\n",
        )
        .unwrap();
        let only = scratch.dir().join("user").join("only");
        std::fs::create_dir_all(&only).unwrap();
        std::fs::write(only.join("theme.yaml"), "colors:\n  base: \"#202020\"\n").unwrap();
        let listed = themes::available_in(&dirs);
        let info = |id: &str| listed.iter().find(|i| i.id == id).unwrap().clone();

        let mut s = AppearanceSettings::default();
        let taken = s.wear_theme(&info("full"), &dirs);
        assert_eq!(
            taken,
            [
                "colors",
                "widget-style",
                "animation",
                "window-decorations",
                "taskbar-panel",
                "wallpapers",
                "fonts",
                "icons",
                "cursors",
                "sounds"
            ]
        );
        let full_id = std::ffi::OsStr::new("full");
        assert_eq!(s.color_theme.id(), full_id);
        assert_eq!(s.widget_theme.id(), full_id);
        assert_eq!(s.animation_theme.id(), full_id);
        assert_eq!(s.decoration_theme.id(), full_id);
        assert_eq!(s.panel_theme.id(), full_id);
        assert_eq!(s.wallpaper_theme.id(), full_id);
        assert_eq!(s.font_theme.id(), full_id);
        assert_eq!(s.icon_theme.id(), full_id);
        assert_eq!(s.cursor_theme.id(), full_id);
        assert_eq!(s.sound_theme.id(), full_id);
        assert_eq!(Palette::from_settings(&s).base, Color::from_hex(0x10_1010));

        // Colours only: the rest stays the full theme's -- worn before, and
        // nothing about this theme speaks for them.
        let taken = s.wear_theme(&info("only"), &dirs);
        assert_eq!(taken, ["colors"]);
        assert_eq!(s.color_theme.id(), std::ffi::OsStr::new("only"));
        assert_eq!(s.widget_theme.id(), full_id);
        assert_eq!(s.icon_theme.id(), full_id);
        assert_eq!(Palette::from_settings(&s).base, Color::from_hex(0x20_2020));

        // The built-in theme: everything it has compiled in, which is all
        // but a wallpaper and fonts -- choosing it for those is choosing
        // your own.
        let built_in = listed
            .iter()
            .find(|i| i.origin == themes::Origin::BuiltIn)
            .unwrap();
        let taken = s.wear_theme(built_in, &dirs);
        assert!(
            !taken.contains(&"wallpapers") && !taken.contains(&"fonts"),
            "{taken:?}"
        );
        assert!(
            taken.contains(&"colors") && taken.contains(&"icons"),
            "{taken:?}"
        );
        assert!(s.color_theme.is_built_in());
        assert_eq!(
            s.wallpaper_theme.id(),
            full_id,
            "a wallpaper it does not give"
        );
    }

    // ---- the widget style ----

    /// A theme's controls reach the palette from the settings file and the
    /// theme's own, as its colours do -- and they are a separate choice: the
    /// colours stay the built-in ones.
    #[test]
    fn a_chosen_widget_style_reaches_the_palette() {
        config::testing::with_scratch_config("widget-palette", |root| {
            install_theme(
                root,
                "soft",
                "widget-style:\n  button:\n    radius: 10\n  scrollbar:\n    width: thin\n",
            );
            let s =
                AppearanceSettings::read_from(&Document::parse("theme:\n  widget_style: soft\n"));
            assert_eq!(s.widget_theme.problem(), None);
            let p = Palette::from_settings(&s);
            assert_eq!(p.widget_style.button.radius, 10);
            assert_eq!(
                p.widget_style.scrollbar.width,
                guitk::widget_style::ScrollbarWidth::Thin
            );
            // What the theme left out is the built-in theme's.
            assert_eq!(
                p.widget_style.field,
                guitk::widget_style::WidgetStyle::AERO.field
            );
            // And the colours are not the theme's business here.
            assert_eq!(
                p.roles(),
                Palette::from_settings(&AppearanceSettings::default()).roles()
            );
        });
    }

    /// The widget style is its own setting: read from `theme.widget_style`,
    /// written back there, the built-in one when the file names none or a
    /// blank -- and a theme that cannot be used keeps its name through a save
    /// and says why, while the built-in controls are drawn.
    #[test]
    fn the_widget_style_is_its_own_setting_and_survives_a_save() {
        config::testing::with_scratch_config("widget-setting", |root| {
            let none = AppearanceSettings::read_from(&Document::parse(""));
            assert_eq!(none.widget_theme, themes::WidgetTheme::built_in());
            let blank =
                AppearanceSettings::read_from(&Document::parse("theme:\n  widget_style: \" \"\n"));
            assert_eq!(blank.widget_theme, themes::WidgetTheme::built_in());

            let mut written = Document::new();
            AppearanceSettings::default().write_into(&mut written);
            assert_eq!(
                written.get_str(&["theme", "widget_style"]).as_deref(),
                Some("aero"),
                "the key is there to edit"
            );

            // A colours-only theme cannot give the controls.
            install_theme(root, "nord", "colors:\n  base: \"#2e3440\"\n");
            let doc = Document::parse("theme:\n  colors: nord\n  widget_style: nord\n");
            let s = AppearanceSettings::read_from(&doc);
            assert_eq!(s.color_theme.problem(), None, "the colours are usable");
            assert_eq!(s.widget_theme.id(), "nord");
            assert_eq!(
                s.widget_theme.style(),
                guitk::widget_style::WidgetStyle::AERO
            );
            assert!(
                s.widget_theme
                    .problem()
                    .is_some_and(|why| why.contains("\"nord\" sets no widget style")),
                "{:?}",
                s.widget_theme.problem()
            );
            let mut saved = doc.clone();
            s.write_into(&mut saved);
            assert_eq!(
                saved.get_str(&["theme", "widget_style"]).as_deref(),
                Some("nord")
            );
        });
    }

    /// **High contrast keeps the chosen controls, less what hides**: the
    /// corners and the pill stay, the gloss and the overlaid scrollbar go.
    #[test]
    fn high_contrast_keeps_the_widget_style_less_what_hides() {
        use guitk::widget_style::{ScrollbarVisibility, WidgetStyle};
        let mut chosen = WidgetStyle::AERO;
        chosen.button.radius = 12;
        chosen.scrollbar.visibility = ScrollbarVisibility::Overlay;
        let s = AppearanceSettings {
            widget_theme: themes::WidgetTheme::from_style("soft", chosen),
            high_contrast: Some(HighContrastScheme::WhiteOnBlack),
            ..AppearanceSettings::default()
        };
        let p = Palette::from_settings(&s);
        assert_eq!(p.widget_style, chosen.for_high_contrast());
        assert_eq!(p.widget_style.button.radius, 12);
        assert!(!p.widget_style.button.gloss);
        assert_eq!(
            p.widget_style.scrollbar.visibility,
            ScrollbarVisibility::Always
        );
        // A palette built for high contrast with no settings at all is the
        // built-in shapes, adjusted the same way.
        let bare = Palette::high_contrast(
            Color::rgb(0, 0, 0),
            Color::rgb(255, 255, 255),
            Color::rgb(0, 128, 255),
        );
        assert_eq!(bare.widget_style, WidgetStyle::AERO.for_high_contrast());
    }

    /// `watcher()` sees the chosen widget-style theme's file edited in place,
    /// as it sees a colour theme's -- the controls change without a byte of
    /// `appearance.yaml` changing.
    #[test]
    fn the_appearance_watcher_sees_the_chosen_widget_style_edited_in_place() {
        config::testing::with_scratch_config("watch-widgets", |root| {
            install_theme(root, "soft", "widget-style:\n  button:\n    radius: 10\n");
            let mut file = AppearanceFile::load();
            file.settings.widget_theme = themes::WidgetTheme::load(std::ffi::OsStr::new("soft"));
            file.save().unwrap();

            let mut w = watcher();
            assert!(w.poll().is_some(), "the first look");
            assert!(w.poll().is_none(), "nothing has changed");

            install_theme(root, "soft", "widget-style:\n  button:\n    radius: 2\n");
            let doc = w.poll().expect("the theme changed, so the controls did");
            let s = AppearanceSettings::read_from(&doc);
            assert_eq!(Palette::from_settings(&s).widget_style.button.radius, 2);
            assert!(w.poll().is_none(), "reported once");
        });
    }

    /// A theme chosen for both axes is one file, and depends on it once: the
    /// fingerprint is the colours-only one, not that file twice.
    #[test]
    fn a_theme_chosen_for_both_axes_is_one_dependency() {
        config::testing::with_scratch_config("both-axes", |root| {
            install_theme(
                root,
                "nord",
                "colors:\n  base: \"#2e3440\"\nwidget-style:\n  toggle: checkbox\n",
            );
            install_theme(root, "soft", "widget-style:\n  button:\n    radius: 10\n");
            let colours = Document::parse("theme:\n  colors: nord\n");
            let both = Document::parse("theme:\n  colors: nord\n  widget_style: nord\n");
            let two = Document::parse("theme:\n  colors: nord\n  widget_style: soft\n");
            assert!(!themes::fingerprint(&colours).is_empty());
            assert_eq!(themes::fingerprint(&both), themes::fingerprint(&colours));
            assert_ne!(themes::fingerprint(&two), themes::fingerprint(&colours));
            // The widget theme alone depends on its file too.
            let widgets = Document::parse("theme:\n  widget_style: soft\n");
            assert!(!themes::fingerprint(&widgets).is_empty());
        });
    }

    // ---- the animation ----

    /// A theme's motion reaches the palette from the settings file and the
    /// theme's own, at the user's speed -- and it is a separate choice: the
    /// colours and the controls stay the built-in ones.
    #[test]
    fn a_chosen_animation_reaches_the_palette_at_the_users_speed() {
        use guitk::motion::{Curve, Motion};
        config::testing::with_scratch_config("animation-palette", |root| {
            install_theme(
                root,
                "springy",
                "animation:\n  duration-ms: 300\n  easing: spring\n",
            );
            for (speed, standard_ms) in [("normal", 300), ("slow", 450), ("fast", 225)] {
                let s = AppearanceSettings::read_from(&Document::parse(&format!(
                    "theme:\n  animation: springy\neffects:\n  animation_speed: {speed}\n"
                )));
                assert_eq!(s.animation_theme.problem(), None);
                let p = Palette::from_settings(&s);
                assert_eq!(p.motion, Motion::new(standard_ms, Curve::Spring), "{speed}");
                assert_eq!(
                    p.roles(),
                    Palette::from_settings(&AppearanceSettings::default()).roles()
                );
                assert_eq!(p.widget_style, guitk::widget_style::WidgetStyle::AERO);
            }
            let off = AppearanceSettings::read_from(&Document::parse(
                "theme:\n  animation: springy\neffects:\n  animation_speed: off\n",
            ));
            assert!(Palette::from_settings(&off).motion.is_still());
        });
        // With nothing chosen, the built-in motion at the normal speed.
        assert_eq!(
            Palette::from_settings(&AppearanceSettings::default()).motion,
            Motion::STANDARD
        );
    }

    /// **A still theme is still at every speed** -- and it is not the user
    /// turning animation off: `animations_enabled` is the user's switch, and
    /// stays on.
    #[test]
    fn a_still_theme_is_still_at_every_speed() {
        for speed in [
            AnimationSpeed::Fast,
            AnimationSpeed::Normal,
            AnimationSpeed::Slow,
        ] {
            let s = AppearanceSettings {
                animation_theme: themes::AnimationTheme::from_motion(
                    "calm",
                    guitk::motion::Motion::STILL,
                ),
                animation_speed: speed,
                ..AppearanceSettings::default()
            };
            assert!(Palette::from_settings(&s).motion.is_still(), "{speed:?}");
            assert!(s.animations_enabled(), "{speed:?}");
        }
    }

    /// The animation is its own setting: read from `theme.animation`, written
    /// back there, the built-in one when the file names none or a blank --
    /// and a theme that cannot be used keeps its name through a save and says
    /// why, while the built-in motion is used.
    #[test]
    fn the_animation_is_its_own_setting_and_survives_a_save() {
        config::testing::with_scratch_config("animation-setting", |root| {
            let none = AppearanceSettings::read_from(&Document::parse(""));
            assert_eq!(none.animation_theme, themes::AnimationTheme::built_in());
            let blank =
                AppearanceSettings::read_from(&Document::parse("theme:\n  animation: \" \"\n"));
            assert_eq!(blank.animation_theme, themes::AnimationTheme::built_in());

            let mut written = Document::new();
            AppearanceSettings::default().write_into(&mut written);
            assert_eq!(
                written.get_str(&["theme", "animation"]).as_deref(),
                Some("aero"),
                "the key is there to edit"
            );

            // A colours-only theme cannot give the motion.
            install_theme(root, "nord", "colors:\n  base: \"#2e3440\"\n");
            let doc = Document::parse("theme:\n  colors: nord\n  animation: nord\n");
            let s = AppearanceSettings::read_from(&doc);
            assert_eq!(s.color_theme.problem(), None, "the colours are usable");
            assert_eq!(s.animation_theme.id(), "nord");
            assert_eq!(
                Palette::from_settings(&s).motion,
                guitk::motion::Motion::STANDARD
            );
            assert!(
                s.animation_theme
                    .problem()
                    .is_some_and(|why| why.contains("\"nord\" sets no animation")),
                "{:?}",
                s.animation_theme.problem()
            );
            let mut saved = doc.clone();
            s.write_into(&mut saved);
            assert_eq!(
                saved.get_str(&["theme", "animation"]).as_deref(),
                Some("nord")
            );
        });
    }

    /// **The wallpaper's position is read, held to 0..=1 and written back**;
    /// absent, it is the middle, and a NaN set in code is the middle too.
    #[test]
    fn the_wallpaper_position_is_read_held_and_written() {
        let none = AppearanceSettings::read_from(&Document::parse(""));
        assert_eq!(none.wallpaper_position, (0.5, 0.5));

        let s = AppearanceSettings::read_from(&Document::parse(
            "wallpaper:\n  position_x: 0.25\n  position_y: 0.9\n",
        ));
        assert_eq!(s.wallpaper_position, (0.25, 0.9));

        let held = AppearanceSettings::read_from(&Document::parse(
            "wallpaper:\n  position_x: 1.7\n  position_y: -0.5\n",
        ));
        assert_eq!(held.wallpaper_position, (1.0, 0.0));

        let mut written = Document::new();
        s.write_into(&mut written);
        assert_eq!(written.get_f64(&["wallpaper", "position_x"]), Some(0.25));
        let back = AppearanceSettings::read_from(&written);
        assert_eq!(back.wallpaper_position, (0.25, 0.9));

        let mut nan = AppearanceSettings {
            wallpaper_position: (f32::NAN, 2.0),
            ..AppearanceSettings::default()
        };
        nan.validate();
        assert_eq!(nan.wallpaper_position, (0.5, 1.0));
    }

    /// **The window frames are their own setting**, `theme.decorations`:
    /// read, carried to `decorations()`, written back -- the built-in frame
    /// where nothing or a blank is chosen, and where the chosen theme cannot
    /// give one (which keeps its name and says why).
    #[test]
    fn the_window_frames_are_their_own_setting_and_survive_a_save() {
        config::testing::with_scratch_config("decorations-setting", |root| {
            let none = AppearanceSettings::read_from(&Document::parse(""));
            assert_eq!(none.decoration_theme, themes::DecorationTheme::built_in());
            assert_eq!(none.decorations(), decorations::DecorationStyle::AERO);
            let blank =
                AppearanceSettings::read_from(&Document::parse("theme:\n  decorations: \" \"\n"));
            assert_eq!(blank.decoration_theme, themes::DecorationTheme::built_in());

            let mut written = Document::new();
            AppearanceSettings::default().write_into(&mut written);
            assert_eq!(
                written.get_str(&["theme", "decorations"]).as_deref(),
                Some("aero"),
                "the key is there to edit"
            );

            install_theme(
                root,
                "roomy",
                "window-decorations:\n  title-bar:\n    height: 40\n  buttons:\n    side: left\n",
            );
            let doc = Document::parse("theme:\n  decorations: roomy\n");
            let s = AppearanceSettings::read_from(&doc);
            assert_eq!(s.decoration_theme.problem(), None);
            assert_eq!(s.decorations().title_height, 40);
            assert_eq!(s.decorations().button_side, decorations::ButtonSide::Left);
            let mut saved = doc.clone();
            s.write_into(&mut saved);
            assert_eq!(
                saved.get_str(&["theme", "decorations"]).as_deref(),
                Some("roomy")
            );

            // A colours-only theme cannot give the frames.
            install_theme(root, "nord", "colors:\n  base: \"#2e3440\"\n");
            let s =
                AppearanceSettings::read_from(&Document::parse("theme:\n  decorations: nord\n"));
            assert_eq!(s.decoration_theme.id(), "nord");
            assert_eq!(s.decorations(), decorations::DecorationStyle::AERO);
            assert!(
                s.decoration_theme
                    .problem()
                    .is_some_and(|why| why.contains("\"nord\" sets no window frames")),
                "{:?}",
                s.decoration_theme.problem()
            );
        });
    }

    /// **The taskbar panel is its own setting**, `theme.taskbar_panel`:
    /// read, carried to `panel()`, written back -- the built-in panel where
    /// nothing or a blank is chosen, and where the chosen theme cannot give
    /// one (which keeps its name and says why).
    #[test]
    fn the_taskbar_panel_is_its_own_setting_and_survives_a_save() {
        config::testing::with_scratch_config("panel-setting", |root| {
            let none = AppearanceSettings::read_from(&Document::parse(""));
            assert_eq!(none.panel_theme, themes::PanelTheme::built_in());
            assert_eq!(none.panel(), panel::PanelStyle::AERO);
            let blank =
                AppearanceSettings::read_from(&Document::parse("theme:\n  taskbar_panel: \" \"\n"));
            assert_eq!(blank.panel_theme, themes::PanelTheme::built_in());

            let mut written = Document::new();
            AppearanceSettings::default().write_into(&mut written);
            assert_eq!(
                written.get_str(&["theme", "taskbar_panel"]).as_deref(),
                Some("aero"),
                "the key is there to edit"
            );

            install_theme(
                root,
                "flat",
                "taskbar-panel:\n  gloss: 0\n  spacing:\n    tiles: 5\n",
            );
            let doc = Document::parse("theme:\n  taskbar_panel: flat\n");
            let s = AppearanceSettings::read_from(&doc);
            assert_eq!(s.panel_theme.problem(), None);
            assert_eq!(s.panel().gloss, 0);
            assert_eq!(s.panel().tile_gap, 5);
            let mut saved = doc.clone();
            s.write_into(&mut saved);
            assert_eq!(
                saved.get_str(&["theme", "taskbar_panel"]).as_deref(),
                Some("flat")
            );

            // A colours-only theme cannot give the panel.
            install_theme(root, "nord", "colors:\n  base: \"#2e3440\"\n");
            let s =
                AppearanceSettings::read_from(&Document::parse("theme:\n  taskbar_panel: nord\n"));
            assert_eq!(s.panel_theme.id(), "nord");
            assert_eq!(s.panel(), panel::PanelStyle::AERO);
            assert!(
                s.panel_theme
                    .problem()
                    .is_some_and(|why| why.contains("\"nord\" sets no taskbar panel")),
                "{:?}",
                s.panel_theme.problem()
            );
        });
    }

    /// **High contrast keeps the motion, whole**: it is about telling things
    /// apart, and how fast they move does not change that.
    #[test]
    fn high_contrast_keeps_the_motion() {
        use guitk::motion::{Curve, Motion};
        let chosen = Motion::new(400, Curve::Linear);
        let s = AppearanceSettings {
            animation_theme: themes::AnimationTheme::from_motion("slowish", chosen),
            high_contrast: Some(HighContrastScheme::WhiteOnBlack),
            ..AppearanceSettings::default()
        };
        assert_eq!(Palette::from_settings(&s).motion, chosen);
        let bare = Palette::high_contrast(
            Color::rgb(0, 0, 0),
            Color::rgb(255, 255, 255),
            Color::rgb(0, 128, 255),
        );
        assert_eq!(bare.motion, Motion::STANDARD);
    }

    /// `watcher()` sees the chosen animation theme's file edited in place, as
    /// it sees a colour theme's -- the motion changes without a byte of
    /// `appearance.yaml` changing.
    #[test]
    fn the_appearance_watcher_sees_the_chosen_animation_edited_in_place() {
        config::testing::with_scratch_config("watch-animation", |root| {
            install_theme(root, "springy", "animation:\n  easing: spring\n");
            let mut file = AppearanceFile::load();
            file.settings.animation_theme =
                themes::AnimationTheme::load(std::ffi::OsStr::new("springy"));
            file.save().unwrap();

            let mut w = watcher();
            assert!(w.poll().is_some(), "the first look");
            assert!(w.poll().is_none(), "nothing has changed");

            install_theme(root, "springy", "animation:\n  enabled: false\n");
            let doc = w.poll().expect("the theme changed, so the motion did");
            let s = AppearanceSettings::read_from(&doc);
            assert!(Palette::from_settings(&s).motion.is_still());
            assert!(w.poll().is_none(), "reported once");
        });
    }

    /// A theme chosen for every axis is one file, and depends on it once; the
    /// animation theme alone depends on its file too.
    #[test]
    fn a_theme_chosen_for_every_axis_is_one_dependency() {
        config::testing::with_scratch_config("every-axis", |root| {
            install_theme(
                root,
                "nord",
                "colors:\n  base: \"#2e3440\"\nwidget-style:\n  toggle: checkbox\n\
                 animation:\n  easing: linear\n",
            );
            install_theme(root, "calm", "animation:\n  enabled: false\n");
            let colours = Document::parse("theme:\n  colors: nord\n");
            let all = Document::parse(
                "theme:\n  colors: nord\n  widget_style: nord\n  animation: nord\n",
            );
            let other = Document::parse("theme:\n  colors: nord\n  animation: calm\n");
            assert_eq!(themes::fingerprint(&all), themes::fingerprint(&colours));
            assert_ne!(themes::fingerprint(&other), themes::fingerprint(&colours));
            let alone = Document::parse("theme:\n  animation: calm\n");
            assert!(!themes::fingerprint(&alone).is_empty());
        });
    }

    /// The wallpaper theme is a dependency as the other axes' are, and so are
    /// the pictures it recommends: `watcher()` sees one arrive after the
    /// theme's file -- a theme being copied in -- and the desktop is told the
    /// picture it can now show. They count even when the theme's file was
    /// counted for another axis.
    #[test]
    fn a_wallpaper_theme_and_its_pictures_are_a_dependency() {
        config::testing::with_scratch_config("wallpaper-axis", |root| {
            let text = "colors:\n  base: \"#2e3440\"\nwallpapers:\n  dark: night.png\n";
            install_theme(root, "aurora", text);
            let dir = config::testing::scratch_data_dir(root)
                .join("slateos")
                .join("themes")
                .join("aurora");
            let mut file = AppearanceFile::load();
            file.settings.wallpaper_theme =
                themes::WallpaperTheme::load(std::ffi::OsStr::new("aurora"));
            assert!(file.settings.theme_wallpaper().is_none(), "no picture yet");
            file.save().unwrap();

            let mut w = watcher();
            assert!(w.poll().is_some(), "the first look");
            assert!(w.poll().is_none(), "nothing has changed");

            std::fs::write(dir.join("night.png"), b"a picture").unwrap();
            let doc = w
                .poll()
                .expect("the picture arrived, so the wallpaper changed");
            let s = AppearanceSettings::read_from(&doc);
            assert_eq!(s.theme_wallpaper(), Some(dir.join("night.png").as_path()));
            assert!(w.poll().is_none(), "reported once");

            // An edit that leaves the same pictures found: the file itself
            // is a dependency, not only what it recommends.
            install_theme(root, "aurora", &text.replace("#2e3440", "#3b4252"));
            assert!(w.poll().is_some(), "the theme's file edited");

            let both = Document::parse("theme:\n  colors: aurora\n  wallpaper: aurora\n");
            let with_picture = themes::fingerprint(&both);
            std::fs::remove_file(dir.join("night.png")).unwrap();
            assert_ne!(
                themes::fingerprint(&both),
                with_picture,
                "the pictures count when the file was counted for the colours"
            );
        });
    }

    /// A font theme is chosen by name, as the other axes are, written back
    /// by name, and its file is a dependency. What it gives is its first
    /// installed family for each role, the user's own where it has none
    /// installed ([`AppearanceSettings::fonts_with_theme`]) -- and until every
    /// process applies through [`AppearanceSettings::fonts_in_use`], that
    /// answers with the user's own, so no two processes draw in different
    /// faces.
    #[test]
    fn a_font_theme_is_chosen_and_its_families_resolved() {
        config::testing::with_scratch_config("font-axis", |root| {
            install_theme(
                root,
                "nord",
                "fonts:\n  ui: [Inter, Noto Sans]\n  mono: Fira Code\n",
            );
            let doc = Document::parse("theme:\n  fonts: nord\n");
            let s = AppearanceSettings::read_from(&doc);
            assert_eq!(s.font_theme.id(), "nord");
            assert_eq!(s.font_theme.problem(), None);

            let fonts = s.fonts_with_theme(|family| family == "Noto Sans");
            assert_eq!(fonts.ui_font, "Noto Sans", "the first of its installed");
            assert_eq!(
                fonts.mono_font, s.fonts.mono_font,
                "none of its installed: the user's own"
            );
            assert_eq!(
                s.fonts_in_use(),
                s.fonts,
                "not drawn until every process applies through fonts_in_use"
            );

            let mut out = Document::new();
            s.write_into(&mut out);
            assert_eq!(out.get_str(&["theme", "fonts"]).as_deref(), Some("nord"));

            let before = themes::fingerprint(&doc);
            assert!(!before.is_empty(), "the font theme's file is a dependency");
            install_theme(root, "nord", "fonts:\n  ui: Cantarell\n");
            assert_ne!(themes::fingerprint(&doc), before);
        });
    }

    /// The built-in theme depends on no file, so its fingerprint is empty and
    /// nothing about a theme directory can make its watcher report.
    #[test]
    fn the_built_in_theme_depends_on_nothing() {
        let mut doc = Document::new();
        AppearanceSettings::default().write_into(&mut doc);
        assert!(themes::fingerprint(&doc).is_empty());
        assert!(themes::fingerprint(&Document::new()).is_empty());
    }

    // ---- the automatic mode ----

    /// 2026-09-25 at 12:00 UTC, and at 23:00 UTC.
    const NOON: u64 = 1_790_337_600;
    const NIGHT: u64 = NOON + 11 * 3600;

    /// A scratch user whose clock is in UTC, so the time of day in these
    /// tests is the same on every machine that runs them.
    fn in_utc<T>(tag: &str, body: impl FnOnce(&std::path::Path) -> T) -> T {
        config::testing::with_scratch_config(tag, |root| {
            let mut clock = datetimesettings::DateTimeFile::load();
            assert!(clock.settings.set_zone(Some("UTC")));
            clock.save().unwrap();
            body(root)
        })
    }

    fn automatic() -> Document {
        Document::parse("theme:\n  mode: system\n")
    }

    /// `System (Auto)` is light in its hours and dark outside them -- and says
    /// so to everything drawn from it, the palette and the accent included.
    #[test]
    fn the_automatic_mode_follows_its_hours() {
        in_utc("auto-hours", |_| {
            let day = datetimesettings::clock::with_time(NOON, || {
                AppearanceSettings::read_from(&automatic())
            });
            assert!(day.auto_is_light && day.is_light());
            assert!(Palette::from_settings(&day).light);
            assert_eq!(day.effective_accent(), day.accent_color.color_light());

            let night = datetimesettings::clock::with_time(NIGHT, || {
                AppearanceSettings::read_from(&automatic())
            });
            assert!(!night.auto_is_light && !night.is_light());
            assert!(!Palette::from_settings(&night).light);
        });
    }

    /// Only the automatic mode reads the clock: in the other two a reading at
    /// noon and one at midnight are the same settings.
    #[test]
    fn the_other_modes_do_not_change_with_the_time_of_day() {
        in_utc("auto-fixed-modes", |_| {
            for mode in ["light", "dark"] {
                let doc = Document::parse(&format!("theme:\n  mode: {mode}\n"));
                let at = |t| {
                    datetimesettings::clock::with_time(t, || AppearanceSettings::read_from(&doc))
                };
                assert_eq!(at(NOON), at(NIGHT), "{mode}");
            }
        });
    }

    /// The hours are the user's to set, both ends or neither, and are
    /// written back in the spelling they were read in.
    #[test]
    fn the_automatic_modes_hours_round_trip() {
        let doc = Document::parse(
            "theme:\n  mode: system\n  auto:\n    light_from: \"06:30\"\n    dark_from: \"20:15\"\n",
        );
        let s = AppearanceSettings::read_from(&doc);
        assert_eq!(
            s.auto_light_hours,
            DailyWindow::from_hm(6, 30, 20, 15).unwrap()
        );
        let mut out = Document::new();
        s.write_into(&mut out);
        assert_eq!(
            out.get_str(&["theme", "auto", "light_from"]).as_deref(),
            Some("06:30")
        );
        assert_eq!(
            out.get_str(&["theme", "auto", "dark_from"]).as_deref(),
            Some("20:15")
        );

        let one_end = Document::parse("theme:\n  auto:\n    light_from: \"05:00\"\n");
        assert_eq!(
            AppearanceSettings::read_from(&one_end).auto_light_hours,
            DEFAULT_AUTO_LIGHT_HOURS,
            "a start without an end is a window nobody chose"
        );
    }

    /// A schedule of a day picture and a night picture.
    fn day_and_night() -> AppearanceSettings {
        AppearanceSettings {
            wallpaper_schedule: read_wallpaper_schedule(
                &[
                    "18:00 /pics/night.jpg".to_string(),
                    "06:00 /pics/day.jpg".to_string(),
                ],
                false,
            ),
            ..AppearanceSettings::default()
        }
    }

    /// The picture up at a time is the latest entry not after it, and before
    /// the first entry of the day it is the last, still up from the evening.
    #[test]
    fn the_scheduled_picture_is_the_latest_one_started() {
        let utc = datetimesettings::Tz::utc();
        let s = day_and_night();
        let at = |h: u64, m: u64| NOON - 12 * 3600 + h * 3600 + m * 60;
        let pic = |t| s.scheduled_wallpaper_at(t, utc).map(Path::to_path_buf);
        assert_eq!(
            pic(at(3, 0)),
            Some(PathBuf::from("/pics/night.jpg")),
            "03:00 is still night"
        );
        assert_eq!(
            pic(at(6, 0)),
            Some(PathBuf::from("/pics/day.jpg")),
            "06:00 on the dot"
        );
        assert_eq!(pic(at(12, 0)), Some(PathBuf::from("/pics/day.jpg")));
        assert_eq!(pic(at(17, 59)), Some(PathBuf::from("/pics/day.jpg")));
        assert_eq!(pic(at(18, 0)), Some(PathBuf::from("/pics/night.jpg")));
        assert_eq!(pic(at(23, 59)), Some(PathBuf::from("/pics/night.jpg")));
        assert_eq!(
            AppearanceSettings::default().scheduled_wallpaper_at(NOON, utc),
            None
        );
    }

    /// The next change is the next entry's time, round the end of the day;
    /// a schedule that never changes has none; the seconds already gone in
    /// this minute come off, and it is never zero.
    #[test]
    fn the_schedule_says_when_it_next_changes() {
        let utc = datetimesettings::Tz::utc();
        let s = day_and_night();
        assert_eq!(
            s.next_wallpaper_change(NOON, utc),
            Some(Duration::from_hours(6))
        );
        let eight_pm = NOON + 8 * 3600;
        assert_eq!(
            s.next_wallpaper_change(eight_pm, utc),
            Some(Duration::from_hours(10))
        );
        assert_eq!(
            s.next_wallpaper_change(NOON + 20, utc),
            Some(Duration::from_secs(6 * 3600 - 20))
        );
        let one = AppearanceSettings {
            wallpaper_schedule: read_wallpaper_schedule(&["09:00 /a.png".to_string()], false),
            ..AppearanceSettings::default()
        };
        assert_eq!(
            one.next_wallpaper_change(NOON, utc),
            None,
            "one picture never changes"
        );
        assert_eq!(
            one.scheduled_wallpaper_at(NOON, utc),
            Some(Path::new("/a.png")),
            "and is up all day"
        );
        assert_eq!(
            AppearanceSettings::default().next_wallpaper_change(NOON, utc),
            None
        );
        // In a zone ahead of UTC the edge comes that much sooner: 12:00 UTC is
        // 21:00 in Tokyo, night, and day again at 06:00 -- nine hours on.
        let tokyo = datetimesettings::zone("Asia/Tokyo").unwrap().rule;
        assert_eq!(
            s.next_wallpaper_change(NOON, tokyo),
            Some(Duration::from_hours(9))
        );
        assert_eq!(
            s.scheduled_wallpaper_at(NOON, tokyo),
            Some(Path::new("/pics/night.jpg"))
        );
    }

    /// A line typed wrong costs that line; a time given twice keeps the later
    /// line; the result is in time order.
    #[test]
    fn a_schedule_line_typed_wrong_costs_only_that_line() {
        let lines: Vec<String> = [
            "",
            "nonsense",
            "25:00 /late.png",
            "07:00",
            "07:00    ",
            "12:00 /noon-first.png",
            "08:15 /morning.png",
            "12:00 /noon-second.png",
        ]
        .map(String::from)
        .to_vec();
        let read = read_wallpaper_schedule(&lines, false);
        let got: Vec<(String, PathBuf)> = read
            .iter()
            .map(|e| (e.from.to_string(), e.image.clone()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("08:15".to_string(), PathBuf::from("/morning.png")),
                ("12:00".to_string(), PathBuf::from("/noon-second.png")),
            ]
        );
    }

    /// The next edge, for the shell to sleep until: the evening one just
    /// before 19:00, the morning one just after, none outside the automatic
    /// mode, and never a timer of no length.
    #[test]
    fn the_next_change_is_the_next_edge_of_the_hours() {
        let utc = datetimesettings::Tz::utc();
        let s = AppearanceSettings {
            theme_mode: ThemeMode::System,
            ..AppearanceSettings::default()
        };
        let evening = NOON + 7 * 3600; // 19:00:00
        assert_eq!(
            s.next_auto_change(evening - 30, utc),
            Some(Duration::from_secs(30))
        );
        assert_eq!(
            s.next_auto_change(evening, utc),
            Some(Duration::from_hours(12))
        );
        assert_eq!(
            s.next_auto_change(evening + 59, utc),
            Some(Duration::from_secs(12 * 3600 - 59))
        );
        let dark = AppearanceSettings::default();
        assert_eq!(dark.next_auto_change(NOON, utc), None);
        // In a zone ahead of UTC the edge comes that much sooner.
        let tokyo = datetimesettings::zone("Asia/Tokyo").unwrap().rule;
        // 12:00 UTC is 21:00 in Tokyo: dark, and light again at 07:00, ten
        // hours on.
        assert_eq!(
            s.next_auto_change(NOON, tokyo),
            Some(Duration::from_hours(10))
        );
        assert!(!s.auto_light_at(NOON, tokyo));
    }

    /// The watcher reports the edge though `appearance.yaml` did not change:
    /// its fingerprint includes the phase.
    #[test]
    fn the_appearance_watcher_sees_the_automatic_modes_edge() {
        in_utc("auto-watch", |_| {
            let mut file = AppearanceFile::load();
            file.settings.theme_mode = ThemeMode::System;
            file.save().unwrap();
            let mut w = watcher();
            let before = NOON + 7 * 3600 - 60; // 18:59
            datetimesettings::clock::with_time(before, || {
                let doc = w.poll().expect("the first look");
                assert!(AppearanceSettings::read_from(&doc).is_light());
                assert!(w.poll().is_none(), "nothing has changed");
            });
            datetimesettings::clock::with_time(before + 120, || {
                let doc = w.poll().expect("the edge passed");
                assert!(!AppearanceSettings::read_from(&doc).is_light());
                assert!(w.poll().is_none(), "reported once");
            });
        });
    }

    #[test]
    fn test_config_round_trips_every_enum_variant() {
        // A typo in one `yaml_name` arm would otherwise only show up as one
        // user's setting quietly resetting itself.
        //
        // In UTC at a fixed hour, because the first loop leaves the mode on
        // `System (Auto)`, whose `auto_is_light` is not stored: it is worked
        // out from the clock as the file is read. Unpinned, this failed every
        // day from 07:00 to 19:00 in the host's zone -- it was written at
        // night.
        in_utc("round-trip-enums", |_| {
            datetimesettings::clock::with_time(NIGHT, || {
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
                        let reread =
                            AppearanceSettings::read_from(&Document::parse(&doc.to_text()));
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
            });
        });
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
        assert_eq!(Color::hex_text(Color::rgb(0x89, 0xB4, 0xFA)), "#89b4fa");
        assert_eq!(Color::hex_text(Color::rgba(1, 2, 3, 4)), "#01020304");
        assert_eq!(
            Color::from_hex_text("#89b4fa"),
            Some(Color::rgb(0x89, 0xB4, 0xFA))
        );
        assert_eq!(
            Color::from_hex_text("#89B4FA"),
            Some(Color::rgb(0x89, 0xB4, 0xFA))
        );
        assert_eq!(
            Color::from_hex_text("#01020304"),
            Some(Color::rgba(1, 2, 3, 4))
        );
        for bad in ["89b4fa", "#89b4f", "#gggggg", "#", "", "#89b4fa00ff"] {
            assert_eq!(Color::from_hex_text(bad), None, "{bad} should not parse");
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
        //
        // The field list is `DecorationColors::roles` rather than a copy of it
        // written out here. It was written out here, in an array whose declared
        // length held it to nothing: a twelfth decoration colour would have left
        // this test asserting about ten of twelve while still being named
        // "every colour a frame is drawn with". `roles` destructures the struct
        // exhaustively, so the same addition now fails to compile there.
        let dark = DecorationColors::for_mode(false);
        let light = DecorationColors::for_mode(true);
        let mut checked_shadow = false;
        for ((field, d), (_, l)) in dark.roles().into_iter().zip(light.roles()) {
            if field == "shadow" {
                checked_shadow = true;
                assert_eq!(
                    d, l,
                    "the shadow is meant to be the same black in both modes"
                );
            } else {
                assert_ne!(d, l, "{field} is the same colour in both modes");
            }
        }
        // The exception is matched by name, and the two ways that can rot are
        // not symmetrical. A *renamed* shadow becomes a field asserted to
        // differ, and fails loudly. A shadow dropped from `roles` altogether
        // just stops being checked, and the test stays green having quietly
        // given up its one assertion about a colour that must not change. So
        // say that the branch ran -- lesson 41.
        assert!(checked_shadow, "`roles` no longer yields the shadow");
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

    /// **The window's title bar and a ribbon's strip joined to it are one
    /// colour, with one ink,** for every accent, mode and setting: both are
    /// the palette's answer (`Palette::title_bar`), so they cannot part.
    #[test]
    fn the_title_bar_and_a_ribbons_strip_are_one_colour() {
        for &accent in AccentColor::presets() {
            for mode in [ThemeMode::Dark, ThemeMode::Light] {
                for accent_titlebars in [false, true] {
                    let settings = AppearanceSettings {
                        theme_mode: mode,
                        accent_color: accent,
                        accent_titlebars,
                        ..AppearanceSettings::default()
                    };
                    let palette = Palette::from_settings(&settings);
                    let frame = DecorationColors::from_settings(&settings);
                    let what = format!("{mode:?} {accent:?} accented={accent_titlebars}");
                    assert_eq!(frame.title_focused_bg, palette.title_bar(), "{what}");
                    assert_eq!(frame.title_focused_fg, palette.title_text(), "{what}");
                    assert_eq!(palette.accent_titlebars, accent_titlebars, "{what}");
                }
            }
        }
    }

    /// The WCAG contrast ratio between two opaque colours.
    ///
    /// 4.5 is the AA threshold for body text and 3.0 the one for large text;
    /// a title bar's own label is large and bold enough for the latter.
    ///
    /// **Deliberately a second implementation of [`contrast_ratio`], not a
    /// call to it.** Everything below states a floor as a ratio, and
    /// `readable_on` now *chooses* by that same ratio — so measuring the
    /// choice with the function that made it would assert the production code
    /// against itself and pass however wrong it was. Transcribed from the
    /// WCAG 2 definition instead, the same way the Catppuccin values below are
    /// transcribed from the published palette and for the same reason.
    /// [`the_published_ratios_are_what_this_crate_computes`] is what keeps the
    /// two honest about being the same function.
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
        // The answers are the palettes' own extremes one step further out --
        // Mocha's crust and Latte's base, each channel moved by one -- the
        // same colours to the eye, and values no role has, so a palette
        // check can tell them from a leftover page colour.
        assert_eq!(
            readable_on(Color::from_hex(0xF9E2AF)),
            Color::from_hex(0x10101A)
        );
        assert_eq!(
            readable_on(Color::from_hex(0x1E1E2E)),
            Color::from_hex(0xF0F2F6)
        );
        // A saturated blue is dark to the eye however bright its one channel
        // is: near-white on `#0000FF` is 7.60:1 and near-black only 2.18:1.
        // Any rule that read the largest channel, or averaged the three
        // unweighted, would call this one light and put black text on it.
        assert_eq!(
            readable_on(Color::from_hex(0x0000FF)),
            Color::from_hex(0xF0F2F6)
        );
        // No role of either built-in palette is either answer.
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for (name, role) in p.roles() {
                assert_ne!(role, DARK_EXTREME, "{name} (light {light})");
                assert_ne!(role, LIGHT_EXTREME, "{name} (light {light})");
            }
        }
    }

    /// [`contrast_ratio`] is the WCAG ratio and not merely something shaped
    /// like it.
    ///
    /// The three anchors are definitional rather than measured, so this can be
    /// checked by hand against the standard: a colour against itself is 1:1,
    /// black against white is 21:1 exactly, and the ratio does not care which
    /// way round its arguments are given.
    #[test]
    fn the_published_ratios_are_what_this_crate_computes() {
        let (black, white) = (Color::from_hex(0x000000), Color::from_hex(0xFFFFFF));
        assert!((contrast_ratio(black, black) - 1.0).abs() < 0.001);
        assert!((contrast_ratio(white, white) - 1.0).abs() < 0.001);
        assert!(
            (contrast_ratio(black, white) - 21.0).abs() < 0.01,
            "black on white came out {:.4}:1, and it is 21:1 by definition",
            contrast_ratio(black, white)
        );
        assert!((contrast_ratio(white, black) - contrast_ratio(black, white)).abs() < 0.001);
        // And it agrees with the independent transcription the floors below
        // are measured with, or one of the two is not the WCAG ratio.
        for c in [
            Color::from_hex(0x1E1E2E),
            Color::from_hex(0xF9E2AF),
            Color::from_hex(0x00EE02),
            Color::from_hex(0x7F849C),
        ] {
            let (mine, theirs) = (contrast_ratio(c, LIGHT_EXTREME), contrast(c, LIGHT_EXTREME));
            assert!(
                (mine - theirs).abs() < 0.001,
                "{c:?}: the crate says {mine:.4}:1, the standard says {theirs:.4}:1"
            );
        }
    }

    /// The ratio this crate publishes is the toolkit's, not a copy of it.
    ///
    /// Structural rather than behavioural: two independent implementations
    /// agreeing on every colour tested is exactly what the tree had before,
    /// and it is not the same thing as there being one implementation. If
    /// someone gives `appearance` its own luminance again, the two items stop
    /// being the same item and this fails, even though every ratio still
    /// matches.
    #[test]
    fn the_ratio_is_the_toolkits_own_and_not_a_second_copy_of_it() {
        let here: fn(Color, Color) -> f32 = contrast_ratio;
        let toolkit: fn(Color, Color) -> f32 = guitk::theme::contrast_ratio;
        assert!(
            core::ptr::fn_addr_eq(here, toolkit),
            "appearance::contrast_ratio is no longer guitk::theme::contrast_ratio"
        );
        let here: fn(Color) -> f32 = relative_luminance;
        let toolkit: fn(Color) -> f32 = guitk::theme::relative_luminance;
        assert!(
            core::ptr::fn_addr_eq(here, toolkit),
            "appearance::relative_luminance is no longer guitk::theme::relative_luminance"
        );
    }

    /// A dense walk of the colour cube, because the accent is not a list.
    ///
    /// Every other legibility test in the shell walks
    /// [`AccentColor::presets`] — the hues the appearance page offers. But
    /// [`AccentColor::Custom`] lets a user name *any* of the sixteen million,
    /// and `readable_on` is what inks the switch knobs, title bars and slider
    /// labels drawn on it. A test over a palette can only ever say that the
    /// palette is fine today; this says the *rule* is fine for anything.
    ///
    /// A step of 15 gives 18 levels per channel and so 5,832 colours — every
    /// corner and every face of the cube, including the saturated primaries
    /// and secondaries where a brightness estimate and a contrast ratio part
    /// company.
    #[test]
    fn the_chosen_ink_is_the_more_legible_of_the_two_for_any_colour_at_all() {
        let mut worst: Option<(Color, f32)> = None;
        let mut step = 0u16;
        while step < 18 * 18 * 18 {
            let at = |shift: u32| {
                let i = u32::from(step) / 18u32.pow(shift) % 18;
                u8::try_from((i * 255 / 17).min(255)).expect("a byte")
            };
            let bg = Color::rgba(at(0), at(1), at(2), 255);
            step += 1;

            let ink = readable_on(bg);
            let other = if ink == DARK_EXTREME {
                LIGHT_EXTREME
            } else {
                DARK_EXTREME
            };
            let (chosen, rejected) = (contrast(bg, ink), contrast(bg, other));
            assert!(
                chosen >= rejected,
                "on {bg:?} the ink chosen is worth {chosen:.2}:1 and the one \
                 rejected was worth {rejected:.2}:1"
            );
            if worst.as_ref().is_none_or(|&(_, w)| chosen < w) {
                worst = Some((bg, chosen));
            }
        }

        // The floor the function guarantees, asserted on the minimum so the
        // message names the hardest colour in the cube rather than whichever
        // one the walk reached first.
        //
        // 4.07:1 is not a target anyone chose — it is what falls out of the
        // two extremes, the ratio at the colour equidistant from both. It is
        // above the 3:1 that SC 1.4.11 asks of a control's outline and below
        // the 4.5:1 that SC 1.4.3 asks of body text, which is the honest
        // statement of what a custom accent can promise. Raising it would mean
        // moving the extremes apart, not editing this number.
        let (where_, c) = worst.expect("the walk visits colours");
        assert!(
            c >= 4.0,
            "the hardest background in the cube is {where_:?}, at {c:.2}:1"
        );
    }

    /// The defect that removing the luma threshold repaired, kept as a case.
    ///
    /// `#00EE02` has a luma of 139.93 — a hair under the 140 the old rule
    /// thresholded at — so it was called dark and inked `#EFF1F5`, which on it
    /// is 1.40:1. It is not a contrived colour: it is what a user gets by
    /// dragging a custom-accent picker to a bright green, and the knob of
    /// every switch in the shell is drawn on it.
    #[test]
    fn a_bright_green_custom_accent_does_not_get_pale_ink() {
        let green = Color::from_hex(0x00EE02);
        assert_eq!(
            readable_on(green),
            DARK_EXTREME,
            "a luma sum calls this dark; it is the brightest thing in the shell"
        );
        let c = contrast(green, readable_on(green));
        assert!(c >= 4.0, "the knob on a green custom accent is {c:.2}:1");

        // And it reaches the shell through the settings the picker writes, not
        // only through the function — an accent that never became `p.accent`
        // would make the paragraph above true and irrelevant.
        let settings = AppearanceSettings {
            accent_color: AccentColor::Custom,
            custom_accent: green,
            ..AppearanceSettings::default()
        };
        assert_eq!(settings.effective_accent(), green);
        let ratio = contrast(green, readable_on(settings.effective_accent()));
        assert!(ratio >= 4.0, "the ink on the chosen accent is {ratio:.2}:1");
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
        //
        // The other direction, since the name says *every* dark constant: this
        // file declares 26 non-`LIGHT_` colours and 24 are pinned below. The
        // two that are not are `DARK_EXTREME` and `SHADOW`, and they are absent
        // because Catppuccin does not publish them — `DARK_EXTREME` is the
        // high-contrast accessibility black and `SHADOW` is a black with an
        // alpha, so neither has a Mocha value to disagree with. Every constant
        // that *is* a Mocha colour is here. Note that nothing enforces that
        // arithmetic: a twenty-seventh dark constant would leave this array at
        // 24 and compile, because a module's constants cannot be destructured
        // the way `Palette`'s fields can. See known-issues.md lesson 44.
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
    fn roles(p: &Palette) -> [(&'static str, Color); 27] {
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
        let mut ties: Vec<String> = Vec::new();
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
                // `>=`, not `>`, and only because §831 has subtext0 and
                // subtext1 deliberately holding one value while staying two
                // roles. Everywhere else a tie would mean the ladder had
                // collapsed, so the exact permitted tie is pinned immediately
                // below rather than this being relaxed for everyone.
                assert!(
                    contrast(*hi_c, p.base) >= contrast(*lo_c, p.base),
                    "{hi} is closer to the base than {lo} in {} mode",
                    if light { "light" } else { "dark" }
                );
                if (contrast(*hi_c, p.base) - contrast(*lo_c, p.base)).abs() < f32::EPSILON {
                    ties.push(format!(
                        "{lo}/{hi} in {} mode",
                        if light { "light" } else { "dark" }
                    ));
                }
            }
        }
        // The permitted ties, exactly. Relaxing the rung comparison to `>=`
        // above would otherwise let the whole ladder quietly collapse to one
        // colour and still pass, so every tie it now allows is named here.
        // Light mode has one, by §831: subtext0 and subtext1 hold a single
        // value while staying two roles. Dark mode has none.
        assert_eq!(
            ties,
            ["subtext0/subtext1 in light mode"],
            "the set of ladder rungs sharing a value changed"
        );
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
                    accent.in_mode(p.light),
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

    /// A picture whose filename is not text survives being saved and reloaded.
    ///
    /// The defect this encoding exists for: the path used to be stored with
    /// `to_string_lossy`, so choosing such a file saved a setting naming a
    /// *different* one. The wallpaper then silently did not appear, and the
    /// settings page showed a path nobody had picked.
    #[cfg(windows)]
    #[test]
    fn a_wallpaper_whose_name_is_not_text_round_trips() {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;

        // `/pictures/z<D800>.png` -- a real filename with no text form.
        let odd = PathBuf::from(OsString::from_wide(&[
            u16::from(b'/'),
            u16::from(b'z'),
            0xD800,
            u16::from(b'.'),
            u16::from(b'p'),
            u16::from(b'n'),
            u16::from(b'g'),
        ]));

        let mut s = AppearanceSettings::default();
        s.wallpaper = Some(odd.clone());

        let mut doc = Document::new();
        s.write_into(&mut doc);

        // Asserted on BYTES, not by round-tripping through `PathBuf`, and the
        // difference matters on this host. A Windows `OsString` is WTF-8 and
        // cannot hold an unpaired surrogate as bytes, so reading the setting
        // back into a `PathBuf` here would test the host's limitation rather
        // than the format -- and would fail against code that is correct on
        // the target. `gui/pathcodec` says so in as many words, and the test
        // `apps/backup` used to carry said it too.
        //
        // The bytes are the level the settings file is written at, and they
        // are exact: that is the property the fix has to have.
        let stored = doc
            .get_str(&["wallpaper", "image"])
            .expect("the key is always written");
        assert_eq!(
            pathcodec::decode_bytes(&stored),
            odd.as_os_str().as_encoded_bytes(),
            "the saved setting names a different file than the one chosen"
        );
    }

    /// The stored form is printable text, because the file is YAML.
    #[cfg(windows)]
    #[test]
    fn the_stored_wallpaper_is_printable_ascii() {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;

        let odd = PathBuf::from(OsString::from_wide(&[u16::from(b'/'), 0xD800]));
        let mut s = AppearanceSettings::default();
        s.wallpaper = Some(odd);

        let mut doc = Document::new();
        s.write_into(&mut doc);

        let stored = doc
            .get_str(&["wallpaper", "image"])
            .expect("the key is always written");
        assert!(
            stored.is_ascii(),
            "a YAML scalar has to be text: {stored:?}"
        );
        assert_eq!(
            doc.get_str(&["wallpaper", "image_encoding"]).as_deref(),
            Some("percent"),
            "a file written by this version must say how it is encoded"
        );
    }

    /// An ordinary path is stored unchanged, which is the point of §426's
    /// choice over base64: only the rare path pays.
    #[test]
    fn an_ordinary_wallpaper_path_is_stored_as_itself() {
        let mut s = AppearanceSettings::default();
        s.wallpaper = Some(PathBuf::from("/home/u/Pictures/sunset.png"));

        let mut doc = Document::new();
        s.write_into(&mut doc);

        assert_eq!(
            doc.get_str(&["wallpaper", "image"]).as_deref(),
            Some("/home/u/Pictures/sunset.png")
        );
    }

    /// A file written before the encoding existed is still read correctly.
    ///
    /// The whole reason for the marker. Without it, a path from an older file
    /// containing a literal `%` would be decoded as an escape and name a
    /// different picture -- a fix that breaks the case it was meant to protect.
    #[test]
    fn a_file_without_the_marker_is_read_raw() {
        let mut doc = Document::new();
        doc.set_str(&["wallpaper", "image"], "/home/u/100%25 done.png");
        // No `image_encoding` key: this is what version 1 looks like.

        let s = AppearanceSettings::read_from(&doc);
        assert_eq!(
            s.wallpaper,
            Some(PathBuf::from("/home/u/100%25 done.png")),
            "an old file's path was decoded as though it were escaped"
        );
    }

    /// And the same text, once marked, decodes.
    #[test]
    fn the_same_text_with_the_marker_decodes() {
        let mut doc = Document::new();
        doc.set_str(&["wallpaper", "image"], "/home/u/100%25 done.png");
        doc.set_str(&["wallpaper", "image_encoding"], "percent");

        let s = AppearanceSettings::read_from(&doc);
        assert_eq!(
            s.wallpaper,
            Some(PathBuf::from("/home/u/100% done.png")),
            "a marked file should decode its escapes"
        );
    }

    /// "Same as my desktop" survives a save, and stores no path while doing it.
    ///
    /// The second half is the point. A background that remembered *which*
    /// picture the desktop was showing when the box was ticked would be wrong
    /// the moment a rotation folder advanced, and would go on calling itself
    /// "same as desktop" while showing something else.
    #[test]
    fn following_the_desktop_round_trips_and_names_no_file() {
        let mut s = AppearanceSettings::default();
        s.login_background = LoginBackground::SameAsDesktop;

        let mut doc = Document::new();
        s.write_into(&mut doc);

        assert_eq!(
            doc.get_str(&["login", "background"]).as_deref(),
            Some("desktop")
        );
        assert_eq!(
            doc.get_str(&["login", "image"]),
            None,
            "following the desktop wrote a filename down, which is the one \
             thing it must never do"
        );
        assert_eq!(
            AppearanceSettings::read_from(&doc).login_background,
            LoginBackground::SameAsDesktop
        );
    }

    /// A greeter picture whose filename is not text survives a save.
    ///
    /// Same defect, same escape and same marker as the wallpaper's: see
    /// [`a_wallpaper_whose_name_is_not_text_round_trips`]. Asserted separately
    /// because it is a separate key, and a key that forgot to encode would be
    /// invisible until somebody picked such a file.
    #[cfg(windows)]
    #[test]
    fn a_greeter_picture_whose_name_is_not_text_round_trips() {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;

        let odd = PathBuf::from(OsString::from_wide(&[
            u16::from(b'/'),
            u16::from(b'z'),
            0xD800,
            u16::from(b'.'),
            u16::from(b'p'),
            u16::from(b'n'),
            u16::from(b'g'),
        ]));

        let mut s = AppearanceSettings::default();
        s.login_background = LoginBackground::CustomImage(odd.clone());

        let mut doc = Document::new();
        s.write_into(&mut doc);

        // Asserted on BYTES rather than by reading the setting back into a
        // `PathBuf`, for the reason spelled out in
        // `a_wallpaper_whose_name_is_not_text_round_trips` just above: a
        // Windows `OsString` is WTF-8 and cannot hold an unpaired surrogate,
        // so the round trip through `PathBuf` measures this host and not the
        // format, and fails against code that is right on the target. Written
        // the other way first, this test duly failed with `/z<3x U+FFFD>.png`
        // -- which is the host substituting, not the setting losing anything.
        let stored = doc
            .get_str(&["login", "image"])
            .expect("the key is always written for a picture");
        assert_eq!(
            pathcodec::decode_bytes(&stored),
            odd.as_os_str().as_encoded_bytes(),
            "the greeter's picture did not survive the settings file exactly"
        );
        assert_eq!(
            doc.get_str(&["login", "image_encoding"]).as_deref(),
            Some(WALLPAPER_ENCODING),
            "without the marker a later read cannot tell an escape from a \
             literal percent"
        );
    }

    /// A percent in a greeter picture's name is not mistaken for an escape.
    #[test]
    fn a_percent_in_the_greeters_picture_survives() {
        let path = PathBuf::from("/home/u/100% sure.png");
        let mut s = AppearanceSettings::default();
        s.login_background = LoginBackground::CustomImage(path.clone());

        let mut doc = Document::new();
        s.write_into(&mut doc);
        assert_eq!(
            AppearanceSettings::read_from(&doc).login_background,
            LoginBackground::CustomImage(path)
        );
    }

    /// A mode this version does not know leaves the background alone.
    ///
    /// The same rule `PreviewSide::from_yaml_name` follows: a typo, or a file
    /// written by a newer build, must not move the user's background to
    /// something they never chose.
    #[test]
    fn an_unknown_greeter_mode_changes_nothing() {
        let mut doc = Document::new();
        doc.set_str(&["login", "background"], "aquarium");
        assert_eq!(
            AppearanceSettings::read_from(&doc).login_background,
            AppearanceSettings::default().login_background
        );
    }

    /// Half a gradient is not a gradient.
    ///
    /// A hand-edited file that names a top colour and no bottom one has not
    /// described anything drawable, and inventing the missing half would put a
    /// colour on screen that nobody chose.
    #[test]
    fn a_gradient_missing_a_colour_falls_back_to_the_theme() {
        let mut doc = Document::new();
        doc.set_str(&["login", "background"], "gradient");
        doc.set_str(&["login", "gradient_top"], "#112233");
        assert_eq!(
            AppearanceSettings::read_from(&doc).login_background,
            LoginBackground::Theme
        );
    }

    /// A colour the user picked comes back as that colour.
    #[test]
    fn a_greeter_colour_round_trips() {
        let mut s = AppearanceSettings::default();
        s.login_background = LoginBackground::SolidColor(Color::from_hex(0xAB12CD));

        let mut doc = Document::new();
        s.write_into(&mut doc);
        assert_eq!(
            AppearanceSettings::read_from(&doc).login_background,
            LoginBackground::SolidColor(Color::from_hex(0xAB12CD))
        );
    }

    /// A wallpaper label stays pale in Latte, and is legible over its own
    /// shadow in both modes.
    ///
    /// The trap this exists to stop is the obvious one: someone converting a
    /// module off its constants sees a label colour, reaches for `p.text`, and
    /// a Light desktop gets dark labels under a black shadow — which is
    /// illegible against every wallpaper rather than merely some of them. The
    /// first assertion is what makes `on_wallpaper` different from `text`; the
    /// second is why it has to be.
    #[test]
    fn a_label_on_the_wallpaper_does_not_follow_the_mode() {
        let dark = Palette::for_mode(false);
        let light = Palette::for_mode(true);
        assert_eq!(
            (light.on_wallpaper(), light.on_wallpaper_dim()),
            (dark.on_wallpaper(), dark.on_wallpaper_dim()),
            "a wallpaper label flipped with the mode; the wallpaper did not"
        );
        // Whereas `text` — the role it must not be confused with — does flip.
        assert_ne!(
            light.text, dark.text,
            "if `text` stopped flipping, `on_wallpaper` is no longer distinct \
             and this test proves nothing"
        );
        for p in [&dark, &light] {
            for (name, c) in [
                ("on_wallpaper", p.on_wallpaper()),
                ("on_wallpaper_dim", p.on_wallpaper_dim()),
            ] {
                // Legible against the shadow that is drawn under it, which is
                // the only background either colour is guaranteed to meet.
                let backdrop = p.text_shadow().over(Color::from_hex(0x808080));
                assert!(
                    contrast(c.over(backdrop), backdrop) > 3.0,
                    "{name} is not legible over its own shadow"
                );
            }
        }
        // The dim one is dimmer, or the selected/unselected cue is gone.
        assert!(dark.on_wallpaper_dim().a < dark.on_wallpaper().a);
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

    // ------------------------------------------------------------------
    // High contrast
    //
    // The scheme's own two colours contrasting is necessary and not
    // sufficient: what a user reads is the *palette*, and a palette that kept
    // one graded grey surface would put text on it at whatever ratio that
    // grey happens to give. So these assert the built palette, every text
    // role against every background role.
    // ------------------------------------------------------------------

    /// Text roles, which must all be legible on any background role.
    fn hc_text_roles(p: &Palette) -> [(&'static str, Color); 4] {
        [
            ("text", p.text),
            ("subtext1", p.subtext1),
            ("subtext0", p.subtext0),
            ("overlay0", p.overlay0),
        ]
    }

    /// Background roles, which in this mode must all be the same colour.
    fn hc_bg_roles(p: &Palette) -> [(&'static str, Color); 6] {
        [
            ("crust", p.crust),
            ("mantle", p.mantle),
            ("base", p.base),
            ("surface0", p.surface0),
            ("surface1", p.surface1),
            ("surface2", p.surface2),
        ]
    }

    fn hc_settings(scheme: HighContrastScheme) -> AppearanceSettings {
        AppearanceSettings {
            high_contrast: Some(scheme),
            ..AppearanceSettings::default()
        }
    }

    #[test]
    fn every_text_role_clears_seven_to_one_on_every_surface() {
        for scheme in HighContrastScheme::ALL {
            let p = Palette::from_settings(&hc_settings(scheme));
            for (tname, text) in hc_text_roles(&p) {
                for (bname, bg) in hc_bg_roles(&p) {
                    let ratio = contrast_ratio(text, bg);
                    assert!(
                        ratio >= 7.0,
                        "{}: {tname} on {bname} is {ratio:.2}:1, below the 7:1 \
                         this mode exists to provide",
                        scheme.label()
                    );
                }
            }
        }
    }

    /// `overlay0` is the one this mode is really about.
    ///
    /// It is documented as "the faintest legible mark" and measures about
    /// 3.4:1 in ordinary dark mode -- a role defined by being hard to see. In
    /// high contrast it must not still be the faintest thing on screen.
    #[test]
    fn the_faintest_role_is_no_longer_faint() {
        let ordinary = Palette::for_mode(false);
        assert!(
            contrast_ratio(ordinary.overlay0, ordinary.base) < 4.5,
            "the premise: in an ordinary palette this role is below body-text \
             contrast, which is why it needs replacing here"
        );

        for scheme in HighContrastScheme::ALL {
            let p = Palette::from_settings(&hc_settings(scheme));
            assert_eq!(
                p.overlay0,
                p.text,
                "{}: the faint role must become the text colour",
                scheme.label()
            );
        }
    }

    /// The surfaces collapse, deliberately. A raised surface is a gradient,
    /// and the gradient is the thing that cannot be seen.
    #[test]
    fn every_surface_is_the_same_colour_in_high_contrast() {
        for scheme in HighContrastScheme::ALL {
            let p = Palette::from_settings(&hc_settings(scheme));
            for (name, bg) in hc_bg_roles(&p) {
                assert_eq!(bg, scheme.background(), "{}: {name}", scheme.label());
            }
        }
    }

    /// "Red means this failed" is information and must survive the mode.
    #[test]
    fn the_categorical_hues_do_not_collapse_into_the_text_colour() {
        for scheme in HighContrastScheme::ALL {
            let p = Palette::from_settings(&hc_settings(scheme));
            let hues = [p.red, p.green, p.yellow, p.blue];
            for hue in hues {
                assert_ne!(
                    hue,
                    p.text,
                    "{}: a categorical hue was flattened away",
                    scheme.label()
                );
            }
            assert_ne!(p.red, p.green, "{}: red and green", scheme.label());
        }
    }

    /// The accent follows the user, which §816 requires.
    #[test]
    fn the_accent_follows_the_users_setting_not_the_scheme() {
        let mut a = hc_settings(HighContrastScheme::GreenOnBlack);
        a.accent_color = AccentColor::Red;
        let mut b = hc_settings(HighContrastScheme::GreenOnBlack);
        b.accent_color = AccentColor::Blue;

        assert_ne!(
            Palette::from_settings(&a).accent,
            Palette::from_settings(&b).accent,
            "two different accent settings must give two different highlights"
        );
    }

    /// Letting the accent follow the user must not let a dim highlight into
    /// the mode where contrast matters most.
    ///
    /// This is the guarantee that replaced a per-scheme one. When the accent
    /// was a property of a scheme there were four values to check and a fixed
    /// bar; now there are fourteen presets against four backgrounds, and what
    /// holds the line is picking the better-contrasting of each hue's two
    /// values (see [`Palette::high_contrast`]).
    ///
    /// The bar is 4.5:1, WCAG AA for body text -- the same bar the old
    /// per-scheme check used, so this is not a relaxation. It is deliberately
    /// lower than the 7:1 the *text* roles must clear: the accent marks
    /// things, it does not have paragraphs set in it.
    #[test]
    fn the_worst_accent_on_the_worst_scheme_is_still_legible() {
        let mut worst = f32::INFINITY;
        let mut worst_case = String::new();

        for scheme in HighContrastScheme::ALL {
            for accent in AccentColor::presets().iter().copied() {
                let mut s = hc_settings(scheme);
                s.accent_color = accent;
                let p = Palette::from_settings(&s);
                let ratio = contrast_ratio(p.accent, p.base);
                if ratio < worst {
                    worst = ratio;
                    worst_case = format!("{} on {}", accent.label(), scheme.label());
                }
            }
        }

        assert!(
            worst >= 4.5,
            "the worst accent/scheme pairing is {worst_case} at {worst:.2}:1"
        );
    }

    /// And the variant choice is what does it -- the same sweep against the
    /// dark value alone finds a pairing that fails.
    ///
    /// Without this, the test above passes and says nothing about *why*: a
    /// build that ignored the light/dark choice entirely might still clear
    /// 4.5 by luck, and this is what distinguishes luck from the mechanism.
    #[test]
    fn choosing_the_better_variant_is_what_keeps_the_accent_legible() {
        let mut worst_fixed = f32::INFINITY;
        for scheme in HighContrastScheme::ALL {
            for accent in AccentColor::presets().iter().copied() {
                worst_fixed = worst_fixed.min(contrast_ratio(accent.color(), scheme.background()));
            }
        }
        assert!(
            worst_fixed < 4.5,
            "if the fixed dark value already cleared the bar at {worst_fixed:.2}:1, \
             the variant choice would be decoration rather than the mechanism"
        );
    }

    /// A custom accent is used exactly as given: an exact colour is an exact
    /// request, and there is no second value of it to choose between.
    #[test]
    fn a_custom_accent_is_used_verbatim() {
        let mut s = hc_settings(HighContrastScheme::WhiteOnBlack);
        s.accent_color = AccentColor::Custom;
        s.custom_accent = Color::from_hex(0xAB12CD);

        assert_eq!(Palette::from_settings(&s).accent, Color::from_hex(0xAB12CD));
    }

    /// Transparency is dropped, because blending lowers contrast by
    /// construction.
    #[test]
    fn high_contrast_is_never_translucent() {
        let mut s = hc_settings(HighContrastScheme::WhiteOnBlack);
        s.transparency = TransparencyLevel::Full;
        assert_eq!(Palette::from_settings(&s).panel_alpha, 255);
    }

    /// The scheme replaces the light/dark choice rather than modifying it, so
    /// a user in the light theme who turns on a dark scheme gets the dark
    /// scheme.
    #[test]
    fn the_scheme_overrides_the_theme_mode() {
        let mut s = hc_settings(HighContrastScheme::WhiteOnBlack);
        s.theme_mode = ThemeMode::Light;
        let p = Palette::from_settings(&s);

        assert_eq!(p.base, Color::from_hex(0x000000));
        assert!(!p.light, "and the palette must say which it is");
    }

    #[test]
    fn high_contrast_is_off_unless_asked_for() {
        assert_eq!(AppearanceSettings::default().high_contrast, None);
        let p = Palette::from_settings(&AppearanceSettings::default());
        assert_ne!(p.overlay0, p.text, "an ordinary palette still grades");
    }

    #[test]
    fn a_scheme_survives_being_written_and_read_back() {
        for scheme in HighContrastScheme::ALL {
            let mut doc = Document::new();
            hc_settings(scheme).write_into(&mut doc);
            assert_eq!(
                AppearanceSettings::read_from(&doc).high_contrast,
                Some(scheme),
                "{}",
                scheme.label()
            );
        }
    }

    #[test]
    fn off_survives_the_round_trip_too() {
        let mut doc = Document::new();
        AppearanceSettings::default().write_into(&mut doc);
        assert_eq!(AppearanceSettings::read_from(&doc).high_contrast, None);
    }

    /// An unknown scheme name reads as off rather than refusing to load.
    #[test]
    fn an_unrecognised_scheme_falls_back_to_the_ordinary_theme() {
        let mut doc = Document::new();
        AppearanceSettings::default().write_into(&mut doc);
        doc.set_str(&["theme", "high_contrast"], "chartreuse_on_beige");
        assert_eq!(AppearanceSettings::read_from(&doc).high_contrast, None);
    }

    // ------------------------------------------------------------------
    // Colour filters
    //
    // Moved here with the code they cover. They were the only tests any of
    // the four `ColorFilter` copies had, and they sat beside the only copy
    // that could actually transform a colour.
    // ------------------------------------------------------------------

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
        for filter in &ColorFilter::ALL {
            assert!(!filter.label().is_empty());
        }
    }

    #[test]
    fn every_filter_appears_in_all_exactly_once() {
        // An exhaustive match, so a new variant fails to compile here rather
        // than silently going missing from every loop that uses `ALL`.
        let position = |filter: ColorFilter| match filter {
            ColorFilter::None => 0,
            ColorFilter::Protanopia => 1,
            ColorFilter::Deuteranopia => 2,
            ColorFilter::Tritanopia => 3,
            ColorFilter::Grayscale => 4,
            ColorFilter::Inverted => 5,
        };
        for (index, filter) in ColorFilter::ALL.into_iter().enumerate() {
            assert_eq!(position(filter), index, "{filter:?} is out of place");
        }
    }

    #[test]
    fn no_two_filters_share_a_label() {
        for (i, a) in ColorFilter::ALL.into_iter().enumerate() {
            for b in ColorFilter::ALL.into_iter().skip(i + 1) {
                assert_ne!(a.label(), b.label(), "{a:?} and {b:?} both say this");
            }
        }
    }

    #[test]
    fn every_channel_mixing_filter_has_well_formed_weights() {
        // `ChannelMix::new` is the thing that enforces "every row sums to the
        // denominator", and it runs at compile time, so what is left to check
        // here is that the filters that ought to mix actually do.
        for filter in ColorFilter::ALL {
            let mixes = filter.channel_mix().is_some();
            let should_mix = match filter {
                ColorFilter::None | ColorFilter::Inverted => false,
                ColorFilter::Protanopia
                | ColorFilter::Deuteranopia
                | ColorFilter::Tritanopia
                | ColorFilter::Grayscale => true,
            };
            assert_eq!(mixes, should_mix, "{filter:?}");
        }
    }

    #[test]
    fn rows_that_do_not_sum_to_the_denominator_are_rejected() {
        // Too dark, too bright, and a zero denominator.
        assert!(ChannelMix::new([[50, 40, 9]; 3], 100).is_none());
        assert!(ChannelMix::new([[50, 40, 11]; 3], 100).is_none());
        assert!(ChannelMix::new([[1, 0, 0]; 3], 0).is_none());
        // One bad row among three good ones is still rejected.
        assert!(ChannelMix::new([[100, 0, 0], [0, 100, 0], [0, 0, 99]], 100).is_none());
        assert!(ChannelMix::new([[100, 0, 0], [0, 100, 0], [0, 0, 100]], 100).is_some());
    }

    #[test]
    fn black_and_white_survive_every_filter() {
        // The point of the row-sum invariant: a weighted average of equal
        // inputs is that input, so the extremes are fixed points of every mix.
        // Only inversion moves them, and it swaps them.
        let black = Color::rgba(0, 0, 0, 255);
        let white = Color::rgba(255, 255, 255, 255);
        for filter in ColorFilter::ALL {
            let (want_black, want_white) = match filter {
                ColorFilter::Inverted => (white, black),
                ColorFilter::None
                | ColorFilter::Protanopia
                | ColorFilter::Deuteranopia
                | ColorFilter::Tritanopia
                | ColorFilter::Grayscale => (black, white),
            };
            assert_eq!(filter.apply(black), want_black, "{filter:?} on black");
            assert_eq!(filter.apply(white), want_white, "{filter:?} on white");
        }
    }

    #[test]
    fn no_filter_touches_alpha() {
        for filter in ColorFilter::ALL {
            for alpha in [0u8, 1, 128, 254, 255] {
                let out = filter.apply(Color::rgba(203, 17, 96, alpha));
                assert_eq!(out.a, alpha, "{filter:?} at alpha {alpha}");
            }
        }
    }

    #[test]
    fn every_filter_accepts_every_channel_value() {
        // Sweeps each channel across its whole range with the other two pinned
        // at both extremes; the old hand-written sums were the kind of code
        // where an out-of-range intermediate would only show up at one end.
        for filter in ColorFilter::ALL {
            for other in [0u8, 255] {
                for value in 0..=255u8 {
                    let _ = filter.apply(Color::rgba(value, other, other, 255));
                    let _ = filter.apply(Color::rgba(other, value, other, 255));
                    let _ = filter.apply(Color::rgba(other, other, value, 255));
                }
            }
        }
    }

    #[test]
    fn inverting_twice_returns_the_original_color() {
        for value in 0..=255u8 {
            let c = Color::rgba(value, 255 - value, value / 2, 77);
            assert_eq!(
                ColorFilter::Inverted.apply(ColorFilter::Inverted.apply(c)),
                c
            );
        }
    }

    #[test]
    fn the_filters_still_produce_the_weights_they_were_written_with() {
        // Pins the numbers the matrices replaced, so the rewrite is provably
        // the same filter and not merely a plausible one.
        assert_eq!(
            ColorFilter::Grayscale.apply(Color::rgba(255, 0, 0, 255)),
            Color::rgba(76, 76, 76, 255)
        );
        assert_eq!(
            ColorFilter::Protanopia.apply(Color::rgba(200, 100, 50, 255)),
            Color::rgba(155, 154, 62, 255)
        );
        assert_eq!(
            ColorFilter::Deuteranopia.apply(Color::rgba(100, 200, 50, 255)),
            Color::rgba(137, 130, 95, 255)
        );
        assert_eq!(
            ColorFilter::Tritanopia.apply(Color::rgba(50, 100, 200, 255)),
            Color::rgba(52, 157, 153, 255)
        );
        assert_eq!(
            ColorFilter::Inverted.apply(Color::rgba(100, 150, 200, 128)),
            Color::rgba(155, 105, 55, 128)
        );
    }

    // -- Magnifier --

    /// The packed form and the unpacked one are the same law, so they must
    /// give the same answer -- for every filter, over every channel value.
    ///
    /// This is what makes `apply_argb` an encoding of `apply` rather than a
    /// second implementation free to drift from it.
    #[test]
    fn the_packed_filter_agrees_with_the_unpacked_one() {
        for filter in ColorFilter::ALL {
            for v in 0u16..=255 {
                #[allow(clippy::cast_possible_truncation)]
                let c = v as u8;
                // A value in each channel position, and an alpha that is not
                // opaque, so a filter that dropped alpha would be caught.
                let color = Color::rgba(c, c.wrapping_add(85), c.wrapping_add(170), 0x7F);
                let packed = (0x7F_u32 << 24)
                    | (u32::from(color.r) << 16)
                    | (u32::from(color.g) << 8)
                    | u32::from(color.b);

                let via_color = filter.apply(color);
                let expected = (u32::from(via_color.a) << 24)
                    | (u32::from(via_color.r) << 16)
                    | (u32::from(via_color.g) << 8)
                    | u32::from(via_color.b);

                assert_eq!(
                    filter.apply_argb(packed),
                    expected,
                    "{} disagrees at {packed:#010X}",
                    filter.label()
                );
            }
        }
    }

    #[test]
    fn no_filter_changes_a_pixel_it_is_handed() {
        for argb in [0x0000_0000, 0xFFFF_FFFF, 0x8012_3456, 0xFF00_FF00] {
            assert_eq!(ColorFilter::None.apply_argb(argb), argb);
        }
    }

    #[test]
    fn every_filter_leaves_alpha_alone_when_packed() {
        for filter in ColorFilter::ALL {
            for alpha in [0x00_u32, 0x7F, 0xFF] {
                let argb = (alpha << 24) | 0x0012_3456;
                assert_eq!(
                    filter.apply_argb(argb) >> 24,
                    alpha,
                    "{} touched alpha",
                    filter.label()
                );
            }
        }
    }

    #[test]
    fn a_filter_survives_being_written_and_read_back() {
        for filter in ColorFilter::ALL {
            let mut doc = Document::new();
            AppearanceSettings {
                color_filter: filter,
                ..AppearanceSettings::default()
            }
            .write_into(&mut doc);
            assert_eq!(
                AppearanceSettings::read_from(&doc).color_filter,
                filter,
                "{}",
                filter.label()
            );
        }
    }

    /// A stored name is not a caption. Rewording a label must not silently
    /// change what an existing config file means.
    #[test]
    fn stored_names_are_distinct_and_not_the_labels() {
        let mut seen = Vec::new();
        for filter in ColorFilter::ALL {
            let name = filter.yaml_name();
            assert!(!seen.contains(&name), "{name} is stored twice");
            seen.push(name);
            assert_eq!(ColorFilter::from_yaml_name(name), Some(filter));
        }
        assert_eq!(ColorFilter::from_yaml_name("sepia"), None);
    }

    /// A newline, so the assertion messages below need no escape in a
    /// heredoc-hostile position.
    const NEWLINE: &str = "
";

    /// Every ink clears 4.5:1 on every ground its own theme puts it on --
    /// both modes, both surface styles, both strip styles, all fourteen
    /// accents, and a hostile custom one.
    ///
    /// This is the test `TD-C-THIRTEEN-LIGHT-ACCENTS-STILL-FAIL-ON-CARDS` said
    /// would exist "the day this is decided". It asserts the *property*, not a
    /// table of values, so it stays true if a colour is retuned and it fails
    /// if a new ink or a new ground is added without being thought about.
    ///
    /// Note what the loop varies. The old guard fixed the theme and varied the
    /// ink; the defect it missed was that the set of grounds *depends on the
    /// theme*. Under the bordered theme a card is an outline and nothing is
    /// drawn on `surface0` at all; under the card theme a selected row is a
    /// `surface1` fill with a label on it. A test written against one of those
    /// says nothing about the other.
    #[test]
    fn every_ink_clears_the_floor_on_every_ground_it_lands_on() {
        let mut failures = Vec::new();
        for light in [false, true] {
            for surface_style in [SurfaceStyle::Borders, SurfaceStyle::Cards] {
                for strip_style in [StripStyle::Filled, StripStyle::Separator] {
                    // The fourteen presets, plus two a user could actually
                    // choose and that no table would contain: one very pale
                    // and one mid-grey, which is the luminance band where
                    // neither black nor a palette extreme is obviously right.
                    let mut accents: Vec<Color> = AccentColor::presets()
                        .iter()
                        .map(|a| if light { a.color_light() } else { a.color() })
                        .collect();
                    accents.push(Color::from_hex(0xFFFFE0));
                    accents.push(Color::from_hex(0x808080));

                    for accent in accents {
                        let mut p = Palette::for_mode(light);
                        p.accent = accent;
                        p.set_surface_style(surface_style);
                        p.set_strip_style(strip_style);
                        // An exhaustive pattern, on the same terms as
                        // `roles`: a new colour cannot reach the palette
                        // without someone deciding here whether it is an ink
                        // that has to be readable.
                        let Palette {
                            // Grounds, not inks.
                            crust: _,
                            mantle: _,
                            base: _,
                            surface0: _,
                            surface1: _,
                            surface2: _,
                            // The muted ink -- the placeholder in an empty
                            // field, the label of a disabled control. WCAG
                            // 1.4.3 exempts inactive controls, and making this
                            // legible would make a disabled control look
                            // enabled, which is the worse failure.
                            overlay0: _,
                            // Not text: SC 1.4.11 floors a component outline
                            // at 3.0, which
                            // `the_border_is_visible_against_every_surface`
                            // covers.
                            border: _,
                            subtext0,
                            subtext1,
                            text,
                            link,
                            red,
                            green,
                            yellow,
                            peach,
                            blue,
                            lavender,
                            mauve,
                            sapphire,
                            teal,
                            sky,
                            accent,
                            // `..`, not the four remaining fields spelled
                            // out. Two of them are private to `guitk` since
                            // 838 and cannot be named from here -- and the
                            // exhaustiveness this pattern used to guarantee
                            // is held by `Palette::roles`, which destructures
                            // with no `..` at all and stops compiling the
                            // moment the struct grows a field.
                            ..
                        } = p;

                        let grounds = p.text_grounds();
                        // Each role is checked the way a draw site *actually*
                        // reads it, which is the property that matters. The
                        // three text-only roles are already floored in the
                        // palette, so a site says `p.subtext0`; the dual-use
                        // ones are not, so a site drawing text says
                        // `p.ink(p.accent)`. Checking both through `ink` would
                        // make this pass while `p.subtext0` was unreadable.
                        for (name, ink, floored_in_place) in [
                            ("text", text, true),
                            ("subtext0", subtext0, true),
                            ("subtext1", subtext1, true),
                            ("link", link, true),
                            ("accent", p.ink(accent), false),
                            ("red", p.ink(red), false),
                            ("green", p.ink(green), false),
                            ("yellow", p.ink(yellow), false),
                            ("peach", p.ink(peach), false),
                            ("blue", p.ink(blue), false),
                            ("lavender", p.ink(lavender), false),
                            ("mauve", p.ink(mauve), false),
                            ("sapphire", p.ink(sapphire), false),
                            ("teal", p.ink(teal), false),
                            ("sky", p.ink(sky), false),
                        ] {
                            let _ = floored_in_place;
                            for ground in grounds.iter().flatten() {
                                let ratio = contrast_ratio(ink, *ground);
                                if ratio < TEXT_CONTRAST_FLOOR {
                                    failures.push(format!(
                                        "light={light} {surface_style:?}/{strip_style:?} accent=#{:06X}: {name} on #{:06X} is {ratio:.2}",
                                        (u32::from(accent.r) << 16)
                                            | (u32::from(accent.g) << 8)
                                            | u32::from(accent.b),
                                        (u32::from(ground.r) << 16)
                                            | (u32::from(ground.g) << 8)
                                            | u32::from(ground.b),
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(
            failures.is_empty(),
            "{} ink/ground pairs below the floor, first 10:{}{}",
            failures.len(),
            NEWLINE,
            failures
                .iter()
                .take(10)
                .cloned()
                .collect::<Vec<_>>()
                .join(NEWLINE)
        );
    }

    /// `text` needs no adjustment, which is why its ~5,000 draw sites were
    /// left saying `p.text`.
    ///
    /// A reachable check for an argument that would otherwise be a comment. If
    /// the ladder is ever retuned so that the main ink no longer clears the
    /// floor on its own, this fails and says so -- rather than the guard above
    /// passing on `p.ink(p.text)` while every real site draws `p.text`.
    #[test]
    fn the_main_ink_clears_the_floor_without_help() {
        for light in [false, true] {
            for surface_style in [SurfaceStyle::Borders, SurfaceStyle::Cards] {
                for strip_style in [StripStyle::Filled, StripStyle::Separator] {
                    let mut p = Palette::for_mode(light);
                    p.set_surface_style(surface_style);
                    p.set_strip_style(strip_style);
                    assert_eq!(
                        p.ink(p.text),
                        p.text,
                        "p.text needs adjusting under {surface_style:?}/{strip_style:?} (light={light}), so its draw sites can no longer say `p.text`"
                    );
                }
            }
        }
    }

    /// `ink` is idempotent: adjusting for one ground does not push a colour
    /// below the floor on another.
    ///
    /// The property `ink`'s loop relies on and does not prove. If it failed,
    /// a second call would move the colour again, and a palette re-resolved
    /// on every frame would drift.
    #[test]
    fn ink_converges_rather_than_trading_one_ground_for_another() {
        for light in [false, true] {
            let mut p = Palette::for_mode(light);
            p.set_surface_style(SurfaceStyle::Cards);
            p.set_strip_style(StripStyle::Filled);
            for (_, role) in p.roles() {
                let once = p.ink(role);
                assert_eq!(
                    p.ink(once),
                    once,
                    "inking twice moved it again, so the first pass had not finished (light={light})"
                );
            }
        }
    }

    /// The palette's own fields are left exactly as chosen.
    ///
    /// The first version of this work adjusted them in place, and three
    /// existing tests said no -- `a_custom_accent_reaches_the_palette_exactly_
    /// as_chosen` most directly. They were right. Legibility is a property of
    /// an ink *and a ground*, so it belongs to the pair, not to the field: an
    /// accent is also a fill, a badge and a progress bar, and those do not
    /// want the text adjustment.
    #[test]
    fn resolving_a_palette_does_not_alter_the_colours_it_was_given() {
        let mut settings = AppearanceSettings::default();
        settings.accent_color = AccentColor::Custom;
        settings.custom_accent = Color::from_hex(0x123456);
        settings.surface_style = SurfaceStyle::Cards;
        let p = Palette::from_settings(&settings);
        assert_eq!(p.accent, Color::from_hex(0x123456));
        assert_ne!(
            p.ink(p.accent),
            p.accent,
            "this fixture is only meaningful if the ink *would* have moved"
        );
    }

    /// "Too close to see one on the other" (`design-decisions.md` §1424)
    /// needs both the light and the colour between them to be too small. A
    /// shade off the accent is; a different hue of the same lightness is not,
    /// and neither is anything far apart in lightness.
    #[test]
    fn hard_to_tell_apart_needs_both_the_light_and_the_colour_to_be_close() {
        let teal = Color::rgb(0x00, 0x68, 0x8B);
        let near_teal = Color::rgb(0x00, 0x72, 0x96);
        assert!(hard_to_tell_apart(teal, near_teal));
        assert!(
            hard_to_tell_apart(near_teal, teal),
            "the answer depends on the order"
        );
        assert!(hard_to_tell_apart(teal, teal));

        let red = Color::rgb(0xD2, 0x0F, 0x39);
        let blue = Color::rgb(0x1E, 0x66, 0xF5);
        assert!(
            contrast_ratio(red, blue) < NON_TEXT_CONTRAST_FLOOR,
            "fixture: the lightness alone must not separate these two"
        );
        assert!(
            !hard_to_tell_apart(red, blue),
            "a red dot on a blue disc is seen by its hue"
        );

        let dark = Color::rgb(0x00, 0x2A, 0x38);
        let pale = Color::rgb(0x9C, 0xE4, 0xFF);
        assert!(
            !hard_to_tell_apart(dark, pale),
            "one hue, far apart in light"
        );
        assert!(!hard_to_tell_apart(
            Color::rgb(0, 0, 0),
            Color::rgb(255, 255, 255)
        ));

        // The light half deciding on its own: two greys just past 3:1 are
        // seen by their lightness, though as colours they are close.
        let (grey, lighter) = (Color::rgb(64, 64, 64), Color::rgb(140, 140, 140));
        assert!(contrast_ratio(grey, lighter) >= NON_TEXT_CONTRAST_FLOOR);
        assert!(perceptual_difference(grey, lighter) < DISTINCT_COLOUR_DIFFERENCE);
        assert!(
            !hard_to_tell_apart(grey, lighter),
            "3:1 apart in light is seen"
        );

        // Neighbouring hues of nearly one lightness: a lavender dot on a blue
        // disc is the calendar's case in another colour.
        let lavender = Color::rgb(0x72, 0x87, 0xFD);
        assert!(hard_to_tell_apart(blue, lavender));
    }

    /// The colour difference is the CIE 1976 one: nothing between a colour and
    /// itself, the same both ways, and black to white the whole lightness
    /// scale, 100.
    #[test]
    fn the_perceptual_difference_is_the_cielab_distance() {
        let (black, white) = (Color::rgb(0, 0, 0), Color::rgb(255, 255, 255));
        assert!(perceptual_difference(white, white) < 1e-3);
        let across = perceptual_difference(black, white);
        assert!((across - 100.0).abs() < 0.5, "black to white is {across}");
        let (x, y) = (Color::rgb(200, 30, 90), Color::rgb(20, 180, 60));
        assert!((perceptual_difference(x, y) - perceptual_difference(y, x)).abs() < 1e-4);
    }

    /// An ink that already clears the floor is returned untouched.
    ///
    /// Otherwise every palette would drift by a rounding step on every
    /// resolve, and a preview drawn from a re-resolved palette would slowly
    /// diverge from the desktop.
    #[test]
    fn legible_on_is_a_no_op_for_a_pair_that_already_passes() {
        assert_eq!(legible_on(LIGHT_TEXT, LIGHT_BASE), LIGHT_TEXT);
        assert_eq!(legible_on(TEXT, BASE), TEXT);
        // And it does move one that does not.
        let moved = legible_on(LIGHT_MAROON, LIGHT_SURFACE1);
        assert_ne!(moved, LIGHT_MAROON);
        assert!(contrast_ratio(moved, LIGHT_SURFACE1) >= TEXT_CONTRAST_FLOOR);
        // Hue survives: still recognisably red rather than a neutral.
        assert!(
            moved.r > moved.g && moved.r > moved.b,
            "maroon stopped being red: {moved:?}"
        );
    }

    /// Every light-theme text ink clears 4.5:1 on every surface it can be
    /// drawn on.
    ///
    /// The defect this exists to prevent is not hypothetical and was not
    /// caught for months: each of these inks was chosen against the *page*,
    /// measured once, and never checked against a card. Secondary text
    /// measured 2.42:1 on the greyest card, in roughly 850 places, while a
    /// test asserting it cleared the floor *on the page* passed the whole
    /// time. A per-role check is not enough; the pairing is the thing.
    ///
    /// Deliberately does **not** cover the fourteen accents. Thirteen of them
    /// still fail here, between 2.33:1 and 3.52:1, and asserting that would
    /// mean either a red build or a muted test. The gap is recorded instead —
    /// `known-issues.md` `TD-C-THIRTEEN-LIGHT-ACCENTS-STILL-FAIL-ON-CARDS` —
    /// and this test grows to cover them when they are fixed.
    #[test]
    fn light_inks_clear_the_contrast_floor_on_every_surface() {
        const FLOOR: f32 = 4.5;
        let inks: [(&str, Color); 4] = [
            ("text", LIGHT_TEXT),
            ("subtext0", LIGHT_SUBTEXT0),
            ("subtext1", LIGHT_SUBTEXT1),
            ("link", LIGHT_LINK),
        ];
        // The surfaces the **default** theme puts text on. Since §829 a box is
        // told apart by its outline rather than by being filled, so a reader is
        // looking at text on the page, on a menu or dialog, or on a sidebar --
        // and never on a card, because in this theme there are none.
        //
        // `border` is deliberately not an ink here: nothing is drawn on top of
        // a border, so its bar is the 3:1 of a UI component, not 4.5. It is
        // checked separately below.
        let surfaces: [(&str, Color); 3] = [
            ("base", LIGHT_BASE),
            ("mantle", LIGHT_MANTLE),
            ("crust", LIGHT_CRUST),
        ];

        let mut failures = Vec::new();
        for (ink_name, ink) in inks {
            for (surface_name, surface) in surfaces {
                let ratio = contrast_ratio(ink, surface);
                if ratio < FLOOR {
                    failures.push(format!("{ink_name} on {surface_name}: {ratio:.2}"));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "light-theme text below the {FLOOR}:1 floor:
  {}",
            failures.join(
                "
  "
            )
        );

        // The optional card theme, pinned rather than asserted clean.
        //
        // Under `SurfaceStyle::Cards` text does land on the three card shades,
        // and the cerulean secondary does not clear the floor on any of them.
        // That is a known, open gap the operator scoped deliberately when they
        // chose borders as the default -- "I guess we still have to figure out
        // how to color them" -- and it is tracked as
        // `TD-C-THIRTEEN-LIGHT-ACCENTS-STILL-FAIL-ON-CARDS`.
        //
        // The exact set is asserted, not the empty set, for the reason §828
        // gives: an assertion that cannot pass gets muted or deleted, and then
        // stops reporting the *next* failure too. A fourth entry appearing here
        // -- main text or the link slipping below the floor, say -- still fails
        // this test.
        let cards: [(&str, Color); 3] = [
            ("surface0", LIGHT_SURFACE0),
            ("surface1", LIGHT_SURFACE1),
            ("surface2", LIGHT_SURFACE2),
        ];
        let mut card_failures = Vec::new();
        for (ink_name, ink) in inks {
            for (surface_name, surface) in cards {
                if contrast_ratio(ink, surface) < FLOOR {
                    card_failures.push(format!("{ink_name} on {surface_name}"));
                }
            }
        }
        card_failures.sort_unstable();
        assert_eq!(
            card_failures,
            [
                "subtext0 on surface0",
                "subtext0 on surface1",
                "subtext0 on surface2",
                "subtext1 on surface0",
                "subtext1 on surface1",
                "subtext1 on surface2",
            ],
            "the card theme's known-failing set changed; if something new fell below the floor it needs fixing, and if something was fixed this list should shrink to match"
        );
    }

    /// The inks stay distinguishable by *weight*, not only by hue.
    ///
    /// The candidate fix this crate's notes recommended before the operator
    /// supplied values would have darkened the greys until they cleared the
    /// greyest card, which put all three inks at identical luminance — body
    /// text, captions and links weighing exactly the same. Hue is the one
    /// channel colour-blind vision cannot rely on, so that traded one
    /// accessibility defect for another. This asserts the trade was not made.
    #[test]
    fn the_light_inks_are_not_all_the_same_weight() {
        let separation = contrast_ratio(LIGHT_TEXT, LIGHT_SUBTEXT0);
        assert!(
            separation > 1.3,
            "main and secondary text are within {separation:.2} of each other, so the hierarchy reads as flat"
        );
    }

    /// `FOCUS_RING_WIDTH * s.focus_ring_scale`, which is a test of `*` in the
    /// same sense `the_caret_width_scale_reaches_a_width_in_pixels` is: the
    /// point is not the arithmetic, it is that the step exists at all and has
    /// exactly one home. A scale multiplied at each call site is a scale that
    /// some call site will fail to multiply.
    #[test]
    fn the_focus_ring_scale_reaches_a_width_in_pixels() {
        let mut s = AppearanceSettings::default();
        assert!(
            (s.focus_ring_width() - guitk::style::FOCUS_RING_WIDTH).abs() < 0.001,
            "an unscaled setting should be the toolkit's own width"
        );
        s.focus_ring_scale = 2.0;
        assert!(
            (s.focus_ring_width() - guitk::style::FOCUS_RING_WIDTH * 2.0).abs() < 0.001,
            "doubling the scale should double the width"
        );

        // The clamp, at both ends. A ring thinner than half the hairline is
        // not an indicator, and one four times thicker already covers the
        // control it surrounds.
        s.focus_ring_scale = 99.0;
        s.validate();
        assert!((s.focus_ring_scale - 4.0).abs() < 0.001);
        s.focus_ring_scale = 0.01;
        s.validate();
        assert!((s.focus_ring_scale - 0.5).abs() < 0.001);
    }

    /// The caret width is a live setting, not a stored one.
    ///
    /// The test that matters for 839 is not that the value round-trips -- the
    /// dead module in `a11y.rs` had a passing round-trip test for its own copy
    /// of this setting, which is precisely why nobody noticed it was wired to
    /// nothing. It is that a caller can get from the settings to a width in
    /// pixels, which is the step that did not exist.
    ///
    /// **And for one day this test did not check that either.** It asserted
    /// `CARET_WIDTH * s.caret_width_scale`, which is a test of `*`: it
    /// performed the multiplication a caller would have to perform, and would
    /// have passed with no caller and no helper anywhere in the tree. It calls
    /// [`AppearanceSettings::caret_width`] now, so it fails if that step stops
    /// existing. The trap named in the paragraph above is the one it fell into.
    #[test]
    fn the_caret_width_scale_reaches_a_width_in_pixels() {
        let mut s = AppearanceSettings::default();
        assert_eq!(
            s.caret_width(),
            guitk::textedit::CARET_WIDTH,
            "the default scale must leave the toolkit's width alone"
        );

        s.caret_width_scale = 2.0;
        assert_eq!(s.caret_width(), guitk::textedit::CARET_WIDTH * 2.0);

        // Clamped, and at four rather than the dead module's five: past about
        // 4x a caret stops being a caret and starts covering the character
        // after it.
        s.caret_width_scale = 99.0;
        s.validate();
        assert_eq!(s.caret_width_scale, 4.0);
        s.caret_width_scale = 0.01;
        s.validate();
        assert_eq!(s.caret_width_scale, 0.5);
    }

    // ---- each look keeps its own colours (§1421) ----

    fn colours(accent_color: AccentColor, custom_accent: Color) -> LookColours {
        LookColours {
            accent_color,
            custom_accent,
        }
    }

    /// **Changing the look brings back the colours kept for it**, and keeps
    /// the ones being left for when that look comes back: an accent chosen
    /// under one look never lands on the other.
    #[test]
    fn each_look_keeps_its_own_accent() {
        let mut s = AppearanceSettings::default();
        assert_eq!(s.surface_style, SurfaceStyle::Borders);
        s.accent_color = AccentColor::Red;

        s.set_surface_style(SurfaceStyle::Cards);
        assert_eq!(
            s.accent_color,
            AccentColor::Blue,
            "the outlined look's accent was carried onto the filled one"
        );
        s.accent_color = AccentColor::Teal;

        s.set_surface_style(SurfaceStyle::Borders);
        assert_eq!(
            s.accent_color,
            AccentColor::Red,
            "the outlined look lost its accent"
        );
        s.set_surface_style(SurfaceStyle::Cards);
        assert_eq!(
            s.accent_color,
            AccentColor::Teal,
            "the filled look lost its accent"
        );
    }

    /// A custom accent travels with its look, colour and all.
    #[test]
    fn a_custom_accent_is_kept_with_its_look() {
        let chosen = Color::rgb(0x12, 0x34, 0x56);
        let mut s = AppearanceSettings::default();
        s.set_surface_style(SurfaceStyle::Cards);
        s.accent_color = AccentColor::Custom;
        s.custom_accent = chosen;
        s.set_surface_style(SurfaceStyle::Borders);
        assert_eq!(s.colours(), LookColours::default());
        s.set_surface_style(SurfaceStyle::Cards);
        assert_eq!(s.colours(), colours(AccentColor::Custom, chosen));
    }

    /// Choosing the look in use again is not a change, so nothing is traded
    /// over: a second click on the selected look must not put the other look's
    /// accent on screen.
    #[test]
    fn choosing_the_look_in_use_again_changes_no_colour() {
        let mut s = AppearanceSettings::default();
        s.accent_color = AccentColor::Red;
        s.other_look_colours.accent_color = AccentColor::Teal;
        s.set_surface_style(SurfaceStyle::Borders);
        assert_eq!(s.accent_color, AccentColor::Red);
        assert_eq!(s.other_look_colours.accent_color, AccentColor::Teal);
    }

    /// Either look's colours can be read and set without changing the look --
    /// what a colour page that edits the look not in use needs -- and setting
    /// the other look's leaves the screen alone.
    #[test]
    fn either_looks_colours_can_be_set_without_changing_the_look() {
        let mut s = AppearanceSettings::default();
        s.set_colours_for(SurfaceStyle::Cards, colours(AccentColor::Green, BLUE));
        assert_eq!(s.surface_style, SurfaceStyle::Borders);
        assert_eq!(s.accent_color, AccentColor::Blue, "the look in use changed");
        assert_eq!(
            s.colours_for(SurfaceStyle::Cards).accent_color,
            AccentColor::Green
        );

        s.set_colours_for(SurfaceStyle::Borders, colours(AccentColor::Mauve, BLUE));
        assert_eq!(s.accent_color, AccentColor::Mauve);
        assert_eq!(s.colours_for(SurfaceStyle::Borders), s.colours());
        assert_eq!(
            s.colours_for(SurfaceStyle::Cards).accent_color,
            AccentColor::Green
        );
    }

    /// The palette draws the accent of the look in use -- the same palette a
    /// settings file naming that accent directly would give.
    #[test]
    fn the_palette_draws_the_accent_of_the_look_in_use() {
        let mut s = AppearanceSettings::default();
        s.accent_color = AccentColor::Red;
        s.set_colours_for(SurfaceStyle::Cards, colours(AccentColor::Green, BLUE));
        let outlined = Palette::from_settings(&s).accent;
        s.set_surface_style(SurfaceStyle::Cards);
        let filled = Palette::from_settings(&s).accent;

        let direct = |surface_style, accent_color| {
            Palette::from_settings(&AppearanceSettings {
                surface_style,
                accent_color,
                ..AppearanceSettings::default()
            })
            .accent
        };
        assert_eq!(outlined, direct(SurfaceStyle::Borders, AccentColor::Red));
        assert_eq!(filled, direct(SurfaceStyle::Cards, AccentColor::Green));
        assert_ne!(outlined, filled);
    }

    /// **Both looks' colours survive the file, whichever look is in use.**
    #[test]
    fn both_looks_colours_survive_the_file_whichever_is_in_use() {
        let custom = Color::rgb(0x12, 0x34, 0x56);
        for look in [SurfaceStyle::Borders, SurfaceStyle::Cards] {
            let mut s = AppearanceSettings::default();
            s.set_colours_for(SurfaceStyle::Borders, colours(AccentColor::Custom, custom));
            s.set_colours_for(SurfaceStyle::Cards, colours(AccentColor::Teal, BLUE));
            s.set_surface_style(look);

            let mut doc = Document::new();
            s.write_into(&mut doc);
            let back = AppearanceSettings::read_from(&doc);
            assert_eq!(back.surface_style, look);
            for each in [SurfaceStyle::Borders, SurfaceStyle::Cards] {
                assert_eq!(
                    back.colours_for(each),
                    s.colours_for(each),
                    "{each:?}'s colours did not survive with {look:?} in use"
                );
            }
        }
    }

    /// The file keeps the colours by look, not by which look is in use: the
    /// outlined look's accent is where the one accent always was, so a file
    /// read by a desktop from before this change still shows the default
    /// look's colours.
    #[test]
    fn the_outlined_looks_accent_is_where_the_one_accent_always_was() {
        let mut s = AppearanceSettings::default();
        s.accent_color = AccentColor::Red;
        s.set_surface_style(SurfaceStyle::Cards);
        s.accent_color = AccentColor::Teal;

        let mut doc = Document::new();
        s.write_into(&mut doc);
        assert_eq!(
            doc.get_str(&["theme", "accent"]).as_deref(),
            Some(AccentColor::Red.yaml_name())
        );
        assert_eq!(
            doc.get_str(&["theme", "cards", "accent"]).as_deref(),
            Some(AccentColor::Teal.yaml_name())
        );
    }

    /// **A file written before each look kept its own colours** reads as its
    /// one accent for both looks, so nobody's choice is lost -- whichever look
    /// the file has in use.
    #[test]
    fn a_file_with_one_accent_gives_it_to_both_looks() {
        for look in ["borders", "cards"] {
            let doc = Document::parse(&format!(
                "theme:\n  surface_style: {look}\n  accent: custom\n  custom_accent: '#123456'\n"
            ));
            let s = AppearanceSettings::read_from(&doc);
            let one = colours(AccentColor::Custom, Color::rgb(0x12, 0x34, 0x56));
            assert_eq!(s.colours(), one, "{look}: the accent in use");
            assert_eq!(
                s.colours_for(SurfaceStyle::Borders),
                one,
                "{look}: outlined"
            );
            assert_eq!(s.colours_for(SurfaceStyle::Cards), one, "{look}: filled");
        }
    }

    /// What the filled look does not say it takes from the outlined one -- the
    /// same rule as a file with no filled section at all, applied per value.
    #[test]
    fn what_the_filled_look_does_not_say_it_takes_from_the_outlined_one() {
        let doc = Document::parse(
            "theme:\n  accent: custom\n  custom_accent: '#123456'\n  cards:\n    accent: teal\n",
        );
        let s = AppearanceSettings::read_from(&doc);
        assert_eq!(
            s.colours_for(SurfaceStyle::Cards),
            colours(AccentColor::Teal, Color::rgb(0x12, 0x34, 0x56))
        );
        assert_eq!(
            s.colours_for(SurfaceStyle::Borders).accent_color,
            AccentColor::Custom
        );
    }

    /// The filled look's section is named by the look's own spelling, so the
    /// two cannot come apart.
    #[test]
    fn the_filled_looks_section_is_the_looks_own_spelling() {
        assert_eq!(
            FILLED_LOOK_KEY,
            surface_style_yaml_name(SurfaceStyle::Cards)
        );
    }
}
