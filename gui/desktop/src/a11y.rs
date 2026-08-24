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
//!
//! # Colour: roles, and the two things that are exceptions on purpose
//!
//! Everything this module paints reads its colour from the live
//! [`Palette`](appearance::Palette), for the reason in `known-issues.md`
//! → `TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE`.
//! Two things here do not, and both are exceptions with a reason rather than
//! leftovers:
//!
//! - **[`HighContrastTheme`] is fixed, saturated colour by definition.** Its
//!   four schemes exist precisely to *replace* the theme for someone who
//!   cannot read it; routing them through the theme would delete the feature.
//!   They are pinned by an exact hand-written table
//!   (`the_four_high_contrast_schemes_are_the_ones_the_module_was_written_with`)
//!   rather than merely excused, because module 48 established that an
//!   exemption with nothing behind it is an unchecked region — the sweep stops
//!   looking and no other instrument starts.
//! - **[`ColorFilter`] transforms a colour it is handed.** It holds no colour
//!   of its own — every output is a function of the input — so there is
//!   nothing in it to convert and nothing to exempt.
//!
//! # The lens is opaque, and its crosshairs are ink rather than white
//!
//! The magnifier lens used to be filled with Mocha `base` at alpha 200 and
//! crosshaired in white at alpha 128. Both halves of that only worked because
//! the fill was always dark. Converting the fill to [`Palette::base`] without
//! converting the ink would have put white-on-white crosshairs in light mode:
//! measured, half-alpha white over Latte `base` reaches **1.06:1**, which for
//! a *screen magnifier* — a feature whose entire purpose is legibility for
//! someone who cannot resolve fine detail — is a functional failure, not a
//! cosmetic one. The crosshairs are now `readable_on(p.base)` at full alpha:
//! 14.50:1 dark, 16.58:1 light.
//!
//! The lens fill is opaque for a separate reason. Its contents are magnified
//! screen content; anything showing through composites the *un*magnified
//! desktop under the magnified copy, which is a double image exactly where the
//! user is looking. See `design-decisions.md` §533.

use core::num::NonZeroU32;

use appearance::{Palette, readable_on};
use guitk::color::Color;
use guitk::render::RenderCommand;
use guitk::style::CornerRadii;

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
    ///
    /// Returns render commands for the lens background, border and crosshairs.
    /// (The magnified content itself is the compositor's; what is here is the
    /// frame around it and the placeholder the content lands on.)
    ///
    /// `screen_w` is the width of the display the lens is on, and is only read
    /// by [`MagnifierShape::DockedTop`], which spans it. It used to be the
    /// literal `1920.0` with a comment promising it would one day be the real
    /// width — so the docked strip stopped short on a wider screen and ran off
    /// a narrower one, in the one feature whose users are least able to work
    /// out why.
    pub fn render_overlay(&self, p: &Palette, screen_w: f32) -> Vec<RenderCommand> {
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

                // Lens background — the surface the magnified content lands
                // on. Opaque: see the module header and §533.
                cmds.push(RenderCommand::FillRect {
                    x,
                    y,
                    width: d,
                    height: d,
                    color: p.base,
                    corner_radii: radii,
                });

                // Border. The accent is the shell's "this is the thing you are
                // being pointed at" role, which is what a lens rim is.
                cmds.push(RenderCommand::StrokeRect {
                    x,
                    y,
                    width: d,
                    height: d,
                    color: p.accent,
                    line_width: bw,
                    corner_radii: radii,
                });

                // Crosshairs, in ink chosen for the fill they sit on rather
                // than in white, which only read as a crosshair while the fill
                // was guaranteed dark.
                if self.config.show_crosshairs {
                    let cx = self.cursor_x;
                    let cy = self.cursor_y;
                    let ink = readable_on(p.base);
                    cmds.push(RenderCommand::Line {
                        x1: cx - d / 4.0,
                        y1: cy,
                        x2: cx + d / 4.0,
                        y2: cy,
                        color: ink,
                        width: 1.0,
                    });
                    cmds.push(RenderCommand::Line {
                        x1: cx,
                        y1: cy - d / 4.0,
                        x2: cx,
                        y2: cy + d / 4.0,
                        color: ink,
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
                    width: screen_w,
                    height: strip_h,
                    color: p.base,
                    corner_radii: CornerRadii::ZERO,
                });
                cmds.push(RenderCommand::Line {
                    x1: 0.0,
                    y1: strip_h,
                    x2: screen_w,
                    y2: strip_h,
                    color: p.accent,
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

impl Default for Magnifier {
    fn default() -> Self {
        Self::new()
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

impl Default for StickyKeys {
    fn default() -> Self {
        Self::new()
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

impl Default for FilterKeys {
    fn default() -> Self {
        Self::new()
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

impl Default for MouseKeys {
    fn default() -> Self {
        Self::new()
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
    /// Custom cursor colour, or `None` for the compositor's own cursor.
    ///
    /// Deliberately not resolved against the palette. A pointer sits over
    /// whatever happens to be on screen — a photograph, a video, another
    /// application — so there is no fill for the shell to choose ink against,
    /// and `None` here means "the compositor's cursor image", not "some role
    /// this module should substitute".
    pub color: Option<Color>,
    /// Whether to show a cursor trail.
    pub trail_enabled: bool,
    /// Trail length (number of ghost cursors).
    pub trail_length: u8,
    /// Whether to show a locator ring on Ctrl press.
    pub locator_enabled: bool,
    /// Locator ring colour, or `None` to follow the theme's accent.
    ///
    /// `None` rather than a pre-filled colour, for the reason in §528: a
    /// plain field seeded with a literal cannot tell "nobody chose" from
    /// "somebody chose exactly this", and whatever is seeded there stops
    /// following the theme forever. Resolve it with [`Self::locator`].
    pub locator_color: Option<Color>,
}

impl Default for CursorSettings {
    fn default() -> Self {
        Self {
            size_scale: 1.0,
            color: None,
            trail_enabled: false,
            trail_length: 3,
            locator_enabled: true,
            locator_color: None,
        }
    }
}

impl CursorSettings {
    /// Clamp size scale to valid range.
    pub fn set_size(&mut self, scale: f32) {
        self.size_scale = scale.clamp(0.5, 5.0);
    }

    /// The colour the locator ring is drawn in, resolved against `p`.
    ///
    /// An unset ring is a *role read at paint time*, so it moves when the
    /// mode does; a chosen one is the choice, unchanged.
    pub fn locator(&self, p: &Palette) -> Color {
        self.locator_color.unwrap_or(p.accent)
    }

    /// Give the locator ring back to the theme after a colour was chosen.
    ///
    /// Without this the choice is a one-way door: there would be no value to
    /// write into `locator_color` that means "unset" again.
    pub fn follow_accent_locator(&mut self) {
        self.locator_color = None;
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
    /// Focus ring colour, or `None` to follow the theme's accent.
    ///
    /// See [`CursorSettings::locator_color`] for why this is an `Option`
    /// rather than a field pre-filled with a colour. Resolve it with
    /// [`Self::ring`].
    pub color: Option<Color>,
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
            color: None,
            width: 2.0,
            offset: 2.0,
            animate: false,
        }
    }
}

impl FocusIndicator {
    /// The colour the ring is drawn in, resolved against `p`.
    pub fn ring(&self, p: &Palette) -> Color {
        self.color.unwrap_or(p.accent)
    }

    /// Give the ring back to the theme after a colour was chosen.
    pub fn follow_accent(&mut self) {
        self.color = None;
    }

    /// Render the focus ring around an element.
    pub fn render(
        &self,
        p: &Palette,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        frame: u64,
    ) -> Vec<RenderCommand> {
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

        let base = self.ring(p);
        let ring_color = Color::rgba(base.r, base.g, base.b, alpha);
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
            out.push_str(&format!(
                "high_contrast={}\n",
                match hc {
                    HighContrastTheme::BlackOnWhite => "black_on_white",
                    HighContrastTheme::WhiteOnBlack => "white_on_black",
                    HighContrastTheme::YellowOnBlack => "yellow_on_black",
                    HighContrastTheme::GreenOnBlack => "green_on_black",
                }
            ));
        } else {
            out.push_str("high_contrast=off\n");
        }

        out.push_str(&format!(
            "color_filter={}\n",
            match self.color_filter {
                ColorFilter::None => "none",
                ColorFilter::Protanopia => "protanopia",
                ColorFilter::Deuteranopia => "deuteranopia",
                ColorFilter::Tritanopia => "tritanopia",
                ColorFilter::Grayscale => "grayscale",
                ColorFilter::Inverted => "inverted",
            }
        ));

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
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    /// A screen width that is not 1920, so a site that kept the old literal
    /// stands out instead of agreeing with the fixture by accident.
    const SCREEN_W: f32 = 2560.0;

    /// A palette whose accent is off-palette, so "drew the accent" and "drew
    /// some other role that happens to be today's accent" cannot be confused.
    fn accented(light: bool) -> Palette {
        let mut p = Palette::for_mode(light);
        p.accent = Color::from_hex(0xFF00FF);
        assert!(
            !p.roles()
                .iter()
                .any(|(n, r)| *n != "accent" && r.r == 255 && r.g == 0 && r.b == 255),
            "the fixture's accent collided with a role, so accent tests would \
             pass for the wrong reason"
        );
        p
    }

    fn contrast(a: Color, b: Color) -> f64 {
        fn channel(v: u8) -> f64 {
            let v = f64::from(v) / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        }
        fn luminance(c: Color) -> f64 {
            0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
        }
        let (mut hi, mut lo) = (luminance(a), luminance(b));
        if hi < lo {
            std::mem::swap(&mut hi, &mut lo);
        }
        (hi + 0.05) / (lo + 0.05)
    }

    /// A magnifier with the lens fully dressed: border, crosshairs and all.
    fn lens(shape: MagnifierShape) -> Magnifier {
        let mut m = Magnifier::new();
        m.config.enabled = true;
        m.config.shape = shape;
        m.config.show_crosshairs = true;
        m.update_cursor(500.0, 300.0);
        m
    }

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
        assert!(m.render_overlay(&accented(false), SCREEN_W).is_empty());
    }

    #[test]
    fn test_magnifier_render_enabled() {
        let mut m = Magnifier::new();
        m.config.enabled = true;
        m.update_cursor(500.0, 300.0);
        let cmds = m.render_overlay(&accented(false), SCREEN_W);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_magnifier_docked_top() {
        let mut m = Magnifier::new();
        m.config.enabled = true;
        m.config.shape = MagnifierShape::DockedTop;
        let cmds = m.render_overlay(&accented(false), SCREEN_W);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_magnifier_fullscreen_no_overlay() {
        let mut m = Magnifier::new();
        m.config.enabled = true;
        m.config.shape = MagnifierShape::FullScreen;
        let cmds = m.render_overlay(&accented(false), SCREEN_W);
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
        let cmds = fi.render(&accented(false), 10.0, 20.0, 100.0, 50.0, 0);
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_focus_indicator_renders() {
        let fi = FocusIndicator::default();
        let cmds = fi.render(&accented(false), 10.0, 20.0, 100.0, 50.0, 0);
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

    // -- Palette conversion --

    /// Nothing the overlay draws is a colour the palette does not own.
    ///
    /// `readable_on(p.base)` is declared rather than exempt: since
    /// `design-decisions.md` §532 the sweep no longer waves past the two
    /// values `readable_on` can return, because both of them are also roles
    /// (`0xEFF1F5` is Latte `base`, `0x11111B` is Mocha `crust`) and exempting
    /// them un-checked those roles everywhere.
    #[test]
    fn every_colour_the_overlay_draws_comes_from_its_palette() {
        for light in [false, true] {
            let p = accented(light);
            let derived = [readable_on(p.base)];
            for shape in [
                MagnifierShape::Circle,
                MagnifierShape::Rectangle,
                MagnifierShape::DockedTop,
            ] {
                for crosshairs in [false, true] {
                    let mut m = lens(shape);
                    m.config.show_crosshairs = crosshairs;
                    let cmds = m.render_overlay(&p, SCREEN_W);
                    crate::palette_check::assert_drawn_from(
                        &p,
                        &cmds,
                        &derived,
                        &format!("magnifier {shape:?} (crosshairs {crosshairs})"),
                    );
                }
            }
            // The focus ring, animated and not: the animated form rebuilds the
            // colour channel by channel, which is exactly where a role can be
            // dropped without the unanimated form noticing.
            for animate in [false, true] {
                let mut fi = FocusIndicator::default();
                fi.animate = animate;
                for frame in [0_u64, 15, 30, 45] {
                    let cmds = fi.render(&p, 10.0, 20.0, 100.0, 50.0, frame);
                    crate::palette_check::assert_drawn_from(
                        &p,
                        &cmds,
                        &derived,
                        &format!("focus ring (animate {animate}, frame {frame})"),
                    );
                }
            }
        }
    }

    /// The crosshairs are legible on the lens they are drawn on, in both modes.
    ///
    /// This is the property the module used to fail. White at alpha 128 over
    /// the lens reached 1.06:1 in light mode — invisible, in a magnifier. The
    /// threshold is the 7:1 that WCAG calls enhanced rather than the 4.5:1
    /// minimum, because a 1px hairline is the hardest thing on the screen to
    /// resolve and this is the feature for people who cannot resolve it.
    #[test]
    fn the_crosshairs_are_legible_on_the_lens_in_both_modes() {
        for light in [false, true] {
            let p = accented(light);
            let m = lens(MagnifierShape::Circle);
            let cmds = m.render_overlay(&p, SCREEN_W);
            let fill = cmds
                .iter()
                .find_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .expect("the lens draws a background");
            let lines: Vec<Color> = cmds
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Line { color, .. } => Some(*color),
                    _ => None,
                })
                .collect();
            assert_eq!(lines.len(), 2, "the lens draws two crosshairs");
            for ink in lines {
                let r = contrast(fill, ink);
                assert!(
                    r >= 7.0,
                    "crosshair #{:02X}{:02X}{:02X} on lens #{:02X}{:02X}{:02X} in \
                     {} mode is {r:.2}:1",
                    ink.r,
                    ink.g,
                    ink.b,
                    fill.r,
                    fill.g,
                    fill.b,
                    if light { "light" } else { "dark" }
                );
                assert_eq!(ink.a, 255, "a hairline cannot afford to be translucent");
            }
        }
    }

    /// The lens fill is opaque, in both modes and both lens shapes.
    ///
    /// Not a style preference: a translucent lens composites the unmagnified
    /// desktop under the magnified copy of it, which is a double image exactly
    /// where the user is looking. See `design-decisions.md` §533.
    #[test]
    fn the_lens_is_opaque_in_both_modes() {
        for light in [false, true] {
            let p = accented(light);
            for shape in [MagnifierShape::Circle, MagnifierShape::DockedTop] {
                let cmds = lens(shape).render_overlay(&p, SCREEN_W);
                for cmd in &cmds {
                    if let RenderCommand::FillRect { color, .. } = cmd {
                        assert_eq!(
                            color.a,
                            255,
                            "the {shape:?} lens fill is translucent in {} mode",
                            if light { "light" } else { "dark" }
                        );
                    }
                }
            }
        }
    }

    /// The docked strip spans the screen it was handed, whatever that is.
    ///
    /// Two widths, neither 1920, because a single width cannot tell "uses the
    /// argument" from "returns a constant that happens to match".
    #[test]
    fn the_docked_strip_spans_the_screen_it_was_given() {
        let p = accented(false);
        for w in [1280.0_f32, 3840.0] {
            let cmds = lens(MagnifierShape::DockedTop).render_overlay(&p, w);
            let mut saw_fill = false;
            let mut saw_line = false;
            for cmd in &cmds {
                match cmd {
                    RenderCommand::FillRect { width, .. } => {
                        assert!(
                            (width - w).abs() < f32::EPSILON,
                            "strip is {width}, want {w}"
                        );
                        saw_fill = true;
                    }
                    RenderCommand::Line { x2, .. } => {
                        assert!(
                            (x2 - w).abs() < f32::EPSILON,
                            "underline ends at {x2}, want {w}"
                        );
                        saw_line = true;
                    }
                    _ => {}
                }
            }
            assert!(
                saw_fill && saw_line,
                "the docked strip draws a fill and an underline"
            );
        }
    }

    /// The lens rim and the focus ring both wear the accent.
    ///
    /// The fixture accent is off-palette, so this cannot pass by drawing blue
    /// and calling it the accent — which is precisely what the module did
    /// before, since the stock accent *is* blue.
    #[test]
    fn the_lens_rim_and_the_focus_ring_are_the_accent() {
        for light in [false, true] {
            let p = accented(light);
            let rim = lens(MagnifierShape::Circle)
                .render_overlay(&p, SCREEN_W)
                .iter()
                .find_map(|c| match c {
                    RenderCommand::StrokeRect { color, .. } => Some(*color),
                    _ => None,
                })
                .expect("the lens draws a rim");
            assert_eq!(rim, p.accent);

            let ring = FocusIndicator::default().render(&p, 0.0, 0.0, 10.0, 10.0, 0);
            match ring.as_slice() {
                [RenderCommand::StrokeRect { color, .. }] => {
                    assert_eq!(
                        (color.r, color.g, color.b),
                        (p.accent.r, p.accent.g, p.accent.b)
                    );
                }
                other => panic!("expected one StrokeRect, got {other:?}"),
            }
        }
    }

    /// An unset ring follows the theme; a chosen one does not.
    ///
    /// The two states have to stay distinguishable when the mode changes, for
    /// the same reason module 48's wallpaper does: a field pre-filled with a
    /// literal answers both questions with the same value and so cannot tell
    /// them apart.
    #[test]
    fn an_unset_ring_follows_the_accent_and_a_chosen_one_does_not() {
        const CHOSEN: Color = Color::from_hex(0x0012_3456);
        // Two *different* off-palette accents: `accented` deliberately uses one
        // colour for both modes, which is what makes it useful elsewhere and
        // useless here — a ring that stopped following the theme would still
        // report the same value in both modes and pass.
        let mut dark = Palette::for_mode(false);
        dark.accent = Color::from_hex(0x00FF_00FF);
        let mut light = Palette::for_mode(true);
        light.accent = Color::from_hex(0x0000_FF7F);
        assert_ne!(
            dark.accent, light.accent,
            "the fixture must move with the mode"
        );
        assert_ne!(CHOSEN, dark.accent);
        assert_ne!(CHOSEN, light.accent);

        let mut fi = FocusIndicator::default();
        assert_eq!(fi.ring(&dark), dark.accent);
        assert_eq!(fi.ring(&light), light.accent);
        assert_ne!(fi.ring(&dark), fi.ring(&light), "an unset ring must move");

        fi.color = Some(CHOSEN);
        assert_eq!(fi.ring(&dark), CHOSEN);
        assert_eq!(fi.ring(&light), CHOSEN, "a chosen ring must not move");

        let mut cs = CursorSettings::default();
        assert_eq!(cs.locator(&dark), dark.accent);
        assert_eq!(cs.locator(&light), light.accent);
        cs.locator_color = Some(CHOSEN);
        assert_eq!(cs.locator(&dark), CHOSEN);
        assert_eq!(cs.locator(&light), CHOSEN);
    }

    /// Following the theme is reachable again after a colour was chosen.
    ///
    /// Without this the choice is a one-way door and the default is only ever
    /// available to someone who never touched the setting.
    #[test]
    fn following_the_accent_is_reachable_again_after_choosing() {
        let p = accented(false);
        let mut fi = FocusIndicator::default();
        fi.color = Some(Color::from_hex(0x0012_3456));
        fi.follow_accent();
        assert!(fi.color.is_none());
        assert_eq!(fi.ring(&p), p.accent);

        let mut cs = CursorSettings::default();
        cs.locator_color = Some(Color::from_hex(0x0012_3456));
        cs.follow_accent_locator();
        assert!(cs.locator_color.is_none());
        assert_eq!(cs.locator(&p), p.accent);
    }

    // -- The high-contrast exemption, and what stands behind it --

    /// The four high-contrast schemes are the ones the module was written with.
    ///
    /// Hand-written, and deliberately *not* read out of [`HighContrastTheme`]:
    /// an expectation derived from the code under test cannot fail (lesson
    /// 22), and this table is the only instrument that watches these twelve
    /// colours at all — the membership sweep is told to skip them. An
    /// exemption with nothing behind it is an unchecked region, which is the
    /// lesson module 48 paid for.
    #[test]
    fn the_four_high_contrast_schemes_are_the_ones_the_module_was_written_with() {
        // (scheme, background, text, accent)
        let table = [
            (
                HighContrastTheme::BlackOnWhite,
                0x000000,
                0xFFFFFF,
                0xFFFF00,
            ),
            (
                HighContrastTheme::WhiteOnBlack,
                0xFFFFFF,
                0x000000,
                0x0000FF,
            ),
            (
                HighContrastTheme::YellowOnBlack,
                0x000000,
                0xFFFF00,
                0x00FFFF,
            ),
            (
                HighContrastTheme::GreenOnBlack,
                0x000000,
                0x00FF00,
                0xFF00FF,
            ),
        ];
        for (scheme, bg, text, accent) in table {
            assert_eq!(
                scheme.background(),
                Color::from_hex(bg),
                "{scheme:?} background"
            );
            assert_eq!(scheme.text(), Color::from_hex(text), "{scheme:?} text");
            assert_eq!(
                scheme.accent(),
                Color::from_hex(accent),
                "{scheme:?} accent"
            );
            assert_eq!(
                scheme.border(),
                scheme.text(),
                "{scheme:?} border follows text"
            );
        }
    }

    /// No high-contrast colour is a palette role, in either mode.
    ///
    /// This is what makes the exemption an exemption rather than an oversight:
    /// these twelve values are outside the palette by construction, so a sweep
    /// that included them could only ever fail. If one of them ever *became* a
    /// role, that would be the signal to revisit §533's reasoning rather than
    /// to widen the exemption.
    #[test]
    fn no_high_contrast_colour_is_a_palette_role() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for scheme in [
                HighContrastTheme::BlackOnWhite,
                HighContrastTheme::WhiteOnBlack,
                HighContrastTheme::YellowOnBlack,
                HighContrastTheme::GreenOnBlack,
            ] {
                for (what, c) in [
                    ("background", scheme.background()),
                    ("text", scheme.text()),
                    ("accent", scheme.accent()),
                ] {
                    if let Some((role, _)) = p
                        .roles()
                        .iter()
                        .find(|(_, r)| r.r == c.r && r.g == c.g && r.b == c.b)
                    {
                        panic!(
                            "{scheme:?} {what} is the {} palette's `{role}`; the \
                             high-contrast exemption now hides a role",
                            if light { "light" } else { "dark" }
                        );
                    }
                }
            }
        }
    }

    /// Every high-contrast scheme is legible with itself.
    ///
    /// Text against its own background must clear 15:1 — far above WCAG's
    /// 7:1 AAA, because maximum separation is the entire point of the mode.
    /// The accent bar is only 4.5:1 (WCAG AA for text, and half again the 3:1
    /// a non-text UI component needs) and that gap is deliberate rather than
    /// slack: measured, the four accents are 19.56, 8.59, 16.75 and **6.70**,
    /// the last being `GreenOnBlack`'s magenta, which is as bright as magenta
    /// gets. Raising the bar to AAA would mean changing that scheme's accent
    /// hue, which is a user-visible appearance choice and is queued as `C-Q7`
    /// rather than made here. Nothing can drift *toward* that outlier
    /// unnoticed regardless, because
    /// `the_four_high_contrast_schemes_are_the_ones_the_module_was_written_with`
    /// pins all twelve colours exactly; this test states the claim, that one
    /// is the ratchet.
    #[test]
    fn every_high_contrast_scheme_is_legible_with_itself() {
        for scheme in [
            HighContrastTheme::BlackOnWhite,
            HighContrastTheme::WhiteOnBlack,
            HighContrastTheme::YellowOnBlack,
            HighContrastTheme::GreenOnBlack,
        ] {
            let bg = scheme.background();
            let t = contrast(bg, scheme.text());
            assert!(t >= 15.0, "{scheme:?} text on its background is {t:.2}:1");
            let a = contrast(bg, scheme.accent());
            assert!(a >= 4.5, "{scheme:?} accent on its background is {a:.2}:1");
        }
    }

    /// A colour filter is a function of its input, so it holds no palette to
    /// convert — and that claim is checked rather than asserted.
    ///
    /// Every filter maps black to black and white to white (the row-sum
    /// invariant), and `None` is the identity on an arbitrary colour. A filter
    /// that had acquired a constant of its own would break one of these.
    #[test]
    fn a_colour_filter_introduces_no_colour_of_its_own() {
        let probe = Color::rgba(0x12, 0x34, 0x56, 0x78);
        for f in ColorFilter::ALL {
            let black = f.apply(Color::rgba(0, 0, 0, 255));
            let white = f.apply(Color::rgba(255, 255, 255, 255));
            match f {
                ColorFilter::Inverted => {
                    assert_eq!((black.r, black.g, black.b), (255, 255, 255));
                    assert_eq!((white.r, white.g, white.b), (0, 0, 0));
                }
                _ => {
                    assert_eq!((black.r, black.g, black.b), (0, 0, 0), "{f:?} moved black");
                    assert_eq!(
                        (white.r, white.g, white.b),
                        (255, 255, 255),
                        "{f:?} moved white"
                    );
                }
            }
            assert_eq!(f.apply(probe).a, probe.a, "{f:?} touched alpha");
        }
        assert_eq!(ColorFilter::None.apply(probe), probe);
    }
}
