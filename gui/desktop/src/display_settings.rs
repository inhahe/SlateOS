//! Display settings and color calibration module.
//!
//! Provides:
//! - Night light / blue light filter (adjustable color temperature)
//! - Display brightness control
//! - Color temperature profiles (warm, neutral, cool, custom)
//! - Gamma calibration (per-channel RGB curves)
//! - Scheduled night light (sunset/sunrise or manual times)
//! - Multi-monitor per-display settings
//! - Color profile management (sRGB, DCI-P3, custom ICC)
//! - Test patterns for calibration (grayscale, color bars, gradient)
//!
//! # Colour
//!
//! Chrome is read out of the caller's [`Palette`]; part 2 of
//! `TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE` deleted
//! the nine Mocha constants this module used to carry. Three judgements are
//! worth stating, because "read it from the palette" does not by itself say
//! *which* role — and here it is twice the wrong instruction entirely.
//!
//! **Three things in this file are instruments, not decoration, and the
//! palette must not reach them.** A settings page that themes its own
//! measuring equipment is lying to the user:
//!
//! - [`TestPattern::render`] emits a sixteen-step grey ramp, eight SMPTE bars,
//!   a twenty-four-step hue sweep, a black-and-white checkerboard and
//!   `rgb(128, 128, 128)`. These are calibration targets. Their whole purpose
//!   is that they are *exact* and identical on every machine and in every
//!   theme, so the function takes no `&Palette` and cannot be themed even by
//!   accident.
//! - The night-light swatch shows what the screen will physically look like at
//!   the chosen colour temperature. It is computed by
//!   [`ColorTemperature::preview_color`] and follows the temperature, not the
//!   theme.
//! - The Red, Green and Blue gamma rows are coloured because they *are* the
//!   red, green and blue channels. That is a label, not a style. They take
//!   `p.red` / `p.green` / `p.blue`, which shift between modes for legibility
//!   but never follow the accent — the same reasoning as the recorder's
//!   transport buttons in `screen_capture`.
//!
//! **The deleted `MOCHA_BLUE` was two different things, and the conversion is
//! where they separate.** It coloured the active tab's label, the filled part
//! of a slider and the selected pattern chip — all of which mean "this one is
//! chosen" and become `p.accent` — *and* the Blue Gamma row, which means "the
//! blue channel" and becomes `p.blue`. Under the shipped theme the accent
//! **is** blue, so the two were indistinguishable; under any other accent they
//! are not, and the file previously had no way to say which it meant.
//!
//! **The selected chip's lettering was near-black and had to stop being.**
//! `MOCHA_MANTLE` on Mocha blue is legible; on Latte's blue (`#1D62EC`, luma
//! 93) it is not, so the ink is [`Palette::on_accent`] — computed from the
//! fill rather than named beside it.

use appearance::Palette;
use guitk::color::Color;
use guitk::daywindow::DailyWindow;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::step;
use guitk::style::CornerRadii;

// ============================================================================
// Color temperature
// ============================================================================

/// Color temperature in Kelvin. Lower = warmer (redder), higher = cooler (bluer).
/// Standard daylight is ~6500K.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorTemperature(pub u32);

impl ColorTemperature {
    /// Warm (candlelight-like). ~3000K.
    pub const WARM: Self = Self(3000);
    /// Neutral (standard daylight). ~6500K.
    pub const NEUTRAL: Self = Self(6500);
    /// Cool (blue-white). ~9000K.
    pub const COOL: Self = Self(9000);

    /// Clamp temperature to valid range [1000, 15000].
    pub fn clamped(self) -> Self {
        Self(self.0.clamp(1000, 15000))
    }

    /// Convert color temperature to an approximate RGB tint using
    /// Tanner Helland's algorithm (simplified).
    /// Returns (r, g, b) as f32 multipliers in [0.0, 1.0].
    pub fn to_rgb_multiplier(self) -> (f32, f32, f32) {
        let temp = self.clamped().0 as f32 / 100.0;

        // Red channel
        let r = if temp <= 66.0 {
            1.0
        } else {
            let r_raw = 329.698_73 * (temp - 60.0).powf(-0.133_204_76);
            (r_raw / 255.0).clamp(0.0, 1.0)
        };

        // Green channel
        let g = if temp <= 66.0 {
            let g_raw = 99.470_8 * temp.ln() - 161.119_57;
            (g_raw / 255.0).clamp(0.0, 1.0)
        } else {
            let g_raw = 288.122_16 * (temp - 60.0).powf(-0.075_514_846);
            (g_raw / 255.0).clamp(0.0, 1.0)
        };

        // Blue channel
        let b = if temp >= 66.0 {
            1.0
        } else if temp <= 19.0 {
            0.0
        } else {
            let b_raw = 138.517_73 * (temp - 10.0).ln() - 305.044_8;
            (b_raw / 255.0).clamp(0.0, 1.0)
        };

        (r, g, b)
    }

    /// Produce a preview color at this temperature.
    pub fn preview_color(self) -> Color {
        let (r, g, b) = self.to_rgb_multiplier();
        Color::rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
    }
}

// ============================================================================
// Night light
// ============================================================================

/// Schedule mode for night light.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NightLightSchedule {
    /// Always off.
    Off,
    /// Always on at the configured strength.
    AlwaysOn,
    /// On between sunset and sunrise (auto-detected from timezone/location).
    SunsetToSunrise,
    /// On between custom start and end times.
    ///
    /// A [`DailyWindow`] rather than four `u8`s: the four fields were public
    /// and validated nowhere, so a start hour of 25 became a minute count past
    /// the end of the day, which compared as an overnight window and then
    /// never opened. Night light would simply stop happening. The same four
    /// fields shipped that bug in the notification daemon; the window type is
    /// the fix, made once.
    Custom(DailyWindow),
}

impl NightLightSchedule {
    /// Check if night light should be active at the given time.
    /// `hour` is 0-23, `minute` is 0-59.
    pub fn is_active(&self, hour: u8, minute: u8) -> bool {
        match self {
            Self::Off => false,
            Self::AlwaysOn => true,
            Self::SunsetToSunrise => {
                // Approximate: 7 PM to 7 AM
                !(7..19).contains(&hour)
            }
            Self::Custom(window) => window.contains_hm(hour, minute),
        }
    }

    /// Display name for this schedule mode.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::AlwaysOn => "Always On",
            Self::SunsetToSunrise => "Sunset to Sunrise",
            Self::Custom { .. } => "Custom Schedule",
        }
    }
}

/// Night light configuration.
#[derive(Clone, Debug)]
pub struct NightLightConfig {
    /// Schedule mode.
    pub schedule: NightLightSchedule,
    /// Color temperature when active (lower = warmer).
    pub temperature: ColorTemperature,
    /// Transition duration in minutes (gradual warm-up/cool-down).
    pub transition_minutes: u32,
}

impl Default for NightLightConfig {
    fn default() -> Self {
        Self {
            schedule: NightLightSchedule::Off,
            temperature: ColorTemperature(3400),
            transition_minutes: 30,
        }
    }
}

// ============================================================================
// Gamma curves
// ============================================================================

/// Per-channel gamma adjustment. 1.0 = no change.
/// Values < 1.0 brighten midtones, > 1.0 darken midtones.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GammaSettings {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
}

impl Default for GammaSettings {
    fn default() -> Self {
        Self {
            red: 1.0,
            green: 1.0,
            blue: 1.0,
        }
    }
}

impl GammaSettings {
    /// How far a gamma may sit from 1.0 and still count as "no correction".
    ///
    /// Not an arbitrary epsilon: gamma reaches this type from a UI slider, and
    /// a deviation this small moves an 8-bit channel by well under one level,
    /// so it is a difference nothing downstream can display.
    const IDENTITY_TOLERANCE: f32 = 0.001;

    /// Whether a single channel's gamma is close enough to 1.0 to be a no-op.
    ///
    /// Both the per-channel fast path and [`Self::is_default`] ask this
    /// question, and they must not answer it differently — a gamma that
    /// `is_default` calls untouched but `apply_channel` decides to correct
    /// would be a setting the UI reports as off while it is still running.
    pub fn is_identity(gamma: f32) -> bool {
        (gamma - 1.0).abs() < Self::IDENTITY_TOLERANCE
    }

    /// Apply gamma correction to a color value (0-255).
    pub fn apply_channel(value: u8, gamma: f32) -> u8 {
        if Self::is_identity(gamma) {
            return value;
        }
        let normalized = value as f32 / 255.0;
        let corrected = normalized.powf(1.0 / gamma);
        (corrected * 255.0).clamp(0.0, 255.0) as u8
    }

    /// Apply gamma correction to a full color.
    pub fn apply(&self, color: Color) -> Color {
        Color::rgba(
            Self::apply_channel(color.r, self.red),
            Self::apply_channel(color.g, self.green),
            Self::apply_channel(color.b, self.blue),
            color.a,
        )
    }

    /// Whether all channels are at default (1.0).
    pub fn is_default(&self) -> bool {
        Self::is_identity(self.red) && Self::is_identity(self.green) && Self::is_identity(self.blue)
    }
}

// ============================================================================
// Color profiles
// ============================================================================

/// Named color profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ColorProfile {
    /// Standard RGB (most common).
    SRGB,
    /// DCI-P3 wide color gamut.
    DciP3,
    /// Adobe RGB.
    AdobeRGB,
    /// Display native (no correction).
    Native,
    /// Custom ICC profile loaded from file.
    Custom(String),
}

impl ColorProfile {
    /// Display name.
    pub fn display_name(&self) -> &str {
        match self {
            Self::SRGB => "sRGB",
            Self::DciP3 => "DCI-P3",
            Self::AdobeRGB => "Adobe RGB",
            Self::Native => "Native (no correction)",
            Self::Custom(name) => name.as_str(),
        }
    }

    /// Short identifier for serialization.
    pub fn id(&self) -> String {
        match self {
            Self::SRGB => "srgb".to_string(),
            Self::DciP3 => "dci-p3".to_string(),
            Self::AdobeRGB => "adobe-rgb".to_string(),
            Self::Native => "native".to_string(),
            Self::Custom(name) => format!("custom:{}", name),
        }
    }

    /// Parse from serialized id.
    pub fn from_id(id: &str) -> Self {
        match id {
            "srgb" => Self::SRGB,
            "dci-p3" => Self::DciP3,
            "adobe-rgb" => Self::AdobeRGB,
            "native" => Self::Native,
            other => {
                if let Some(name) = other.strip_prefix("custom:") {
                    Self::Custom(name.to_string())
                } else {
                    Self::SRGB
                }
            }
        }
    }
}

// ============================================================================
// Test patterns
// ============================================================================

/// Calibration test pattern type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestPattern {
    /// Grayscale gradient from black to white.
    Grayscale,
    /// Color bars (red, green, blue, cyan, magenta, yellow, white, black).
    ColorBars,
    /// Smooth gradient across the full hue spectrum.
    HueGradient,
    /// Checkerboard pattern for sharpness/alignment.
    Checkerboard,
    /// Solid gray for uniformity check.
    SolidGray,
}

impl TestPattern {
    /// All available patterns.
    pub const ALL: &'static [Self] = &[
        Self::Grayscale,
        Self::ColorBars,
        Self::HueGradient,
        Self::Checkerboard,
        Self::SolidGray,
    ];

    /// Display name.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Grayscale => "Grayscale Gradient",
            Self::ColorBars => "Color Bars",
            Self::HueGradient => "Hue Gradient",
            Self::Checkerboard => "Checkerboard",
            Self::SolidGray => "Solid Gray",
        }
    }

    /// Render this test pattern into a given rectangle.
    pub fn render(self, x: f32, y: f32, width: f32, height: f32) -> Vec<RenderCommand> {
        match self {
            Self::Grayscale => Self::render_grayscale(x, y, width, height),
            Self::ColorBars => Self::render_color_bars(x, y, width, height),
            Self::HueGradient => Self::render_hue_gradient(x, y, width, height),
            Self::Checkerboard => Self::render_checkerboard(x, y, width, height),
            Self::SolidGray => Self::render_solid_gray(x, y, width, height),
        }
    }

    fn render_grayscale(x: f32, y: f32, width: f32, height: f32) -> Vec<RenderCommand> {
        let steps = 16u32;
        let step_width = width / steps as f32;
        let mut cmds = Vec::with_capacity(steps as usize);
        // The ramp spans the ends inclusively, so it is divided by the number
        // of *gaps* between steps rather than the number of steps. `steps` is
        // a literal 16 here; the saturating form keeps that from being the
        // reason this is correct.
        let gaps = steps.saturating_sub(1).max(1);
        for i in 0..steps {
            let gray = i.saturating_mul(255).checked_div(gaps).unwrap_or(0) as u8;
            cmds.push(RenderCommand::FillRect {
                x: x + i as f32 * step_width,
                y,
                width: step_width + 1.0, // +1 to avoid gaps
                height,
                color: Color::rgb(gray, gray, gray),
                corner_radii: CornerRadii::ZERO,
            });
        }
        cmds
    }

    fn render_color_bars(x: f32, y: f32, width: f32, height: f32) -> Vec<RenderCommand> {
        let colors = [
            Color::rgb(255, 255, 255), // White
            Color::rgb(255, 255, 0),   // Yellow
            Color::rgb(0, 255, 255),   // Cyan
            Color::rgb(0, 255, 0),     // Green
            Color::rgb(255, 0, 255),   // Magenta
            Color::rgb(255, 0, 0),     // Red
            Color::rgb(0, 0, 255),     // Blue
            Color::rgb(0, 0, 0),       // Black
        ];
        let bar_width = width / colors.len() as f32;
        let mut cmds = Vec::with_capacity(colors.len());
        for (i, &color) in colors.iter().enumerate() {
            cmds.push(RenderCommand::FillRect {
                x: x + i as f32 * bar_width,
                y,
                width: bar_width + 1.0,
                height,
                color,
                corner_radii: CornerRadii::ZERO,
            });
        }
        cmds
    }

    fn render_hue_gradient(x: f32, y: f32, width: f32, height: f32) -> Vec<RenderCommand> {
        let steps = 24u32;
        let step_width = width / steps as f32;
        let mut cmds = Vec::with_capacity(steps as usize);
        for i in 0..steps {
            let hue = i as f32 / steps as f32;
            let color = hue_to_rgb(hue);
            cmds.push(RenderCommand::FillRect {
                x: x + i as f32 * step_width,
                y,
                width: step_width + 1.0,
                height,
                color,
                corner_radii: CornerRadii::ZERO,
            });
        }
        cmds
    }

    fn render_checkerboard(x: f32, y: f32, width: f32, height: f32) -> Vec<RenderCommand> {
        let cell_size = 20.0_f32;
        let cols = (width / cell_size).ceil() as u32;
        let rows = (height / cell_size).ceil() as u32;
        let mut cmds = Vec::with_capacity(cols.saturating_mul(rows) as usize);
        for row in 0..rows {
            for col in 0..cols {
                // Parity of the sum, so a wrap would invert the whole board
                // from that point on rather than merely miscolour one cell.
                let is_white = row.saturating_add(col) % 2 == 0;
                let color = if is_white {
                    Color::rgb(255, 255, 255)
                } else {
                    Color::rgb(0, 0, 0)
                };
                let cell_w = cell_size.min(width - col as f32 * cell_size);
                let cell_h = cell_size.min(height - row as f32 * cell_size);
                if cell_w > 0.0 && cell_h > 0.0 {
                    cmds.push(RenderCommand::FillRect {
                        x: x + col as f32 * cell_size,
                        y: y + row as f32 * cell_size,
                        width: cell_w,
                        height: cell_h,
                        color,
                        corner_radii: CornerRadii::ZERO,
                    });
                }
            }
        }
        cmds
    }

    fn render_solid_gray(x: f32, y: f32, width: f32, height: f32) -> Vec<RenderCommand> {
        vec![RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: Color::rgb(128, 128, 128),
            corner_radii: CornerRadii::ZERO,
        }]
    }
}

/// Convert HSV hue (0.0-1.0) at full saturation and value to RGB.
fn hue_to_rgb(h: f32) -> Color {
    let h = h * 6.0;
    let sector = h as u32;
    let frac = h - sector as f32;
    let q = (1.0 - frac) * 255.0;
    let t = frac * 255.0;

    match sector % 6 {
        0 => Color::rgb(255, t as u8, 0),
        1 => Color::rgb(q as u8, 255, 0),
        2 => Color::rgb(0, 255, t as u8),
        3 => Color::rgb(0, q as u8, 255),
        4 => Color::rgb(t as u8, 0, 255),
        _ => Color::rgb(255, 0, q as u8),
    }
}

// ============================================================================
// Per-display settings
// ============================================================================

/// Settings for a single display/monitor.
#[derive(Clone, Debug)]
pub struct DisplayConfig {
    /// Display identifier.
    pub display_id: u32,
    /// Display name (e.g., "DELL U2720Q").
    pub name: String,
    /// Resolution width.
    pub resolution_width: u32,
    /// Resolution height.
    pub resolution_height: u32,
    /// Refresh rate in Hz.
    pub refresh_rate: u32,
    /// DPI scaling factor (1.0 = 100%, 1.5 = 150%, 2.0 = 200%).
    pub scale_factor: f32,
    /// Brightness (0-100).
    pub brightness: u32,
    /// Contrast (0-100).
    pub contrast: u32,
    /// Color temperature override (None = follow global night light).
    pub color_temperature: Option<ColorTemperature>,
    /// Gamma correction.
    pub gamma: GammaSettings,
    /// Color profile.
    pub color_profile: ColorProfile,
    /// Rotation in degrees (0, 90, 180, 270).
    pub rotation: u32,
    /// Whether this is the primary display.
    pub is_primary: bool,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            display_id: 0,
            name: "Display 1".to_string(),
            resolution_width: 1920,
            resolution_height: 1080,
            refresh_rate: 60,
            scale_factor: 1.0,
            brightness: 100,
            contrast: 50,
            color_temperature: None,
            gamma: GammaSettings::default(),
            color_profile: ColorProfile::SRGB,
            rotation: 0,
            is_primary: true,
        }
    }
}

impl DisplayConfig {
    /// Resolution as "WxH @ HzHz" string.
    pub fn resolution_string(&self) -> String {
        format!(
            "{}x{} @ {}Hz",
            self.resolution_width, self.resolution_height, self.refresh_rate
        )
    }

    /// Scale factor as percentage string.
    pub fn scale_string(&self) -> String {
        format!("{}%", (self.scale_factor * 100.0) as u32)
    }
}

// ============================================================================
// Display settings manager
// ============================================================================

/// Manages display settings for all monitors.
#[derive(Clone, Debug)]
pub struct DisplaySettingsManager {
    /// Per-display configurations.
    pub displays: Vec<DisplayConfig>,
    /// Global night light configuration.
    pub night_light: NightLightConfig,
    /// Currently selected display index for editing.
    pub selected_display: usize,
    /// Whether a test pattern is being shown.
    pub active_test_pattern: Option<TestPattern>,
    /// Currently selected settings tab.
    pub active_tab: DisplaySettingsTab,
}

/// Tabs in the display settings panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplaySettingsTab {
    General,
    NightLight,
    ColorCalibration,
    TestPatterns,
}

impl DisplaySettingsTab {
    /// All tabs.
    pub const ALL: &'static [Self] = &[
        Self::General,
        Self::NightLight,
        Self::ColorCalibration,
        Self::TestPatterns,
    ];

    /// Display name.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::NightLight => "Night Light",
            Self::ColorCalibration => "Color Calibration",
            Self::TestPatterns => "Test Patterns",
        }
    }
}

impl Default for DisplaySettingsManager {
    fn default() -> Self {
        Self {
            displays: vec![DisplayConfig::default()],
            night_light: NightLightConfig::default(),
            selected_display: 0,
            active_test_pattern: None,
            active_tab: DisplaySettingsTab::General,
        }
    }
}

impl DisplaySettingsManager {
    /// Create with a list of displays.
    pub fn new(displays: Vec<DisplayConfig>) -> Self {
        Self {
            displays,
            ..Self::default()
        }
    }

    /// Get the currently selected display config, if valid.
    pub fn selected_config(&self) -> Option<&DisplayConfig> {
        self.displays.get(self.selected_display)
    }

    /// Get mutable reference to the selected display.
    pub fn selected_config_mut(&mut self) -> Option<&mut DisplayConfig> {
        self.displays.get_mut(self.selected_display)
    }

    /// Select the next display (wrapping).
    pub fn select_next_display(&mut self) {
        self.selected_display = step::wrapping_after(self.displays.len(), self.selected_display);
    }

    /// Set brightness for the selected display.
    pub fn set_brightness(&mut self, brightness: u32) {
        if let Some(d) = self.selected_config_mut() {
            d.brightness = brightness.min(100);
        }
    }

    /// Set contrast for the selected display.
    pub fn set_contrast(&mut self, contrast: u32) {
        if let Some(d) = self.selected_config_mut() {
            d.contrast = contrast.min(100);
        }
    }

    /// Set scale factor for the selected display.
    pub fn set_scale(&mut self, scale: f32) {
        if let Some(d) = self.selected_config_mut() {
            d.scale_factor = scale.clamp(0.5, 4.0);
        }
    }

    /// Set color profile for the selected display.
    pub fn set_color_profile(&mut self, profile: ColorProfile) {
        if let Some(d) = self.selected_config_mut() {
            d.color_profile = profile;
        }
    }

    /// Set gamma for a specific channel on the selected display.
    pub fn set_gamma(&mut self, channel: GammaChannel, value: f32) {
        let clamped = value.clamp(0.2, 5.0);
        if let Some(d) = self.selected_config_mut() {
            match channel {
                GammaChannel::Red => d.gamma.red = clamped,
                GammaChannel::Green => d.gamma.green = clamped,
                GammaChannel::Blue => d.gamma.blue = clamped,
                GammaChannel::All => {
                    d.gamma.red = clamped;
                    d.gamma.green = clamped;
                    d.gamma.blue = clamped;
                }
            }
        }
    }

    /// Reset gamma to defaults for the selected display.
    pub fn reset_gamma(&mut self) {
        if let Some(d) = self.selected_config_mut() {
            d.gamma = GammaSettings::default();
        }
    }

    /// Set night light schedule.
    pub fn set_night_light_schedule(&mut self, schedule: NightLightSchedule) {
        self.night_light.schedule = schedule;
    }

    /// Set night light temperature.
    pub fn set_night_light_temperature(&mut self, temp: u32) {
        self.night_light.temperature = ColorTemperature(temp.clamp(1000, 15000));
    }

    /// Set rotation for the selected display.
    pub fn set_rotation(&mut self, degrees: u32) {
        if let Some(d) = self.selected_config_mut() {
            d.rotation = match degrees {
                0..=44 | 316..=360 => 0,
                45..=134 => 90,
                135..=224 => 180,
                225..=315 => 270,
                _ => 0,
            };
        }
    }

    /// Show a test pattern.
    pub fn show_test_pattern(&mut self, pattern: TestPattern) {
        self.active_test_pattern = Some(pattern);
    }

    /// Dismiss the test pattern.
    pub fn dismiss_test_pattern(&mut self) {
        self.active_test_pattern = None;
    }

    /// Serialize to key=value config text.
    pub fn to_config_text(&self) -> String {
        let mut out = String::with_capacity(512);
        out.push_str("# Display settings\n");

        // Night light
        out.push_str(&format!(
            "night_light_temp={}\n",
            self.night_light.temperature.0
        ));
        out.push_str(&format!(
            "night_light_transition={}\n",
            self.night_light.transition_minutes
        ));
        let sched_str = match &self.night_light.schedule {
            NightLightSchedule::Off => "off".to_string(),
            NightLightSchedule::AlwaysOn => "always".to_string(),
            NightLightSchedule::SunsetToSunrise => "sunset".to_string(),
            NightLightSchedule::Custom(w) => format!(
                "custom:{:02}:{:02}-{:02}:{:02}",
                w.start().hour(),
                w.start().minute(),
                w.end().hour(),
                w.end().minute(),
            ),
        };
        out.push_str(&format!("night_light_schedule={}\n", sched_str));

        // Per-display settings
        for (i, d) in self.displays.iter().enumerate() {
            let prefix = format!("display_{}", i);
            out.push_str(&format!("{}_{}\n", prefix, "name"));
            out.push_str(&format!("{}_brightness={}\n", prefix, d.brightness));
            out.push_str(&format!("{}_contrast={}\n", prefix, d.contrast));
            out.push_str(&format!("{}_scale={}\n", prefix, d.scale_factor));
            out.push_str(&format!("{}_gamma_r={}\n", prefix, d.gamma.red));
            out.push_str(&format!("{}_gamma_g={}\n", prefix, d.gamma.green));
            out.push_str(&format!("{}_gamma_b={}\n", prefix, d.gamma.blue));
            out.push_str(&format!("{}_profile={}\n", prefix, d.color_profile.id()));
            out.push_str(&format!("{}_rotation={}\n", prefix, d.rotation));
        }

        out
    }

    /// Render the display settings panel.
    pub fn render(
        &self,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(64);

        // Background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: p.base,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: y + 16.0,
            text: "Display Settings".to_string(),
            font_size: 18.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 32.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Tab bar
        let tab_y = y + 48.0;
        for (i, tab) in DisplaySettingsTab::ALL.iter().enumerate() {
            let tab_x = x + 16.0 + i as f32 * 140.0;
            let is_active = *tab == self.active_tab;

            if is_active {
                cmds.push(RenderCommand::FillRect {
                    x: tab_x,
                    y: tab_y,
                    width: 130.0,
                    height: 28.0,
                    color: p.surface1,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: tab_x + 8.0,
                y: tab_y + 6.0,
                text: tab.display_name().to_string(),
                font_size: 12.0,
                // A chosen tab is a selection, so it follows the accent.
                color: if is_active { p.accent } else { p.subtext0 },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(120.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Content area
        let content_y = tab_y + 40.0;
        let content_height = height - (content_y - y) - 16.0;

        match self.active_tab {
            DisplaySettingsTab::General => {
                self.render_general_tab(
                    &mut cmds,
                    p,
                    x + 16.0,
                    content_y,
                    width - 32.0,
                    content_height,
                );
            }
            DisplaySettingsTab::NightLight => {
                self.render_night_light_tab(
                    &mut cmds,
                    p,
                    x + 16.0,
                    content_y,
                    width - 32.0,
                    content_height,
                );
            }
            DisplaySettingsTab::ColorCalibration => {
                self.render_calibration_tab(
                    &mut cmds,
                    p,
                    x + 16.0,
                    content_y,
                    width - 32.0,
                    content_height,
                );
            }
            DisplaySettingsTab::TestPatterns => {
                self.render_test_patterns_tab(
                    &mut cmds,
                    p,
                    x + 16.0,
                    content_y,
                    width - 32.0,
                    content_height,
                );
            }
        }

        cmds
    }

    fn render_general_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
        _height: f32,
    ) {
        if let Some(d) = self.selected_config() {
            // Display selector
            cmds.push(RenderCommand::Text {
                x,
                y,
                text: format!(
                    "Display: {} ({})",
                    d.name,
                    if d.is_primary { "Primary" } else { "Secondary" }
                ),
                font_size: 14.0,
                color: p.text,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width),
                overflow: TextOverflow::Ellipsis,
            });

            let mut row_y = y + 30.0;

            // Resolution
            self.render_setting_row(
                cmds,
                p,
                x,
                row_y,
                width,
                "Resolution",
                &d.resolution_string(),
            );
            row_y += 28.0;

            // Scale
            self.render_setting_row(cmds, p, x, row_y, width, "Scale", &d.scale_string());
            row_y += 28.0;

            // Brightness slider
            self.render_slider_row(cmds, p, x, row_y, width, "Brightness", d.brightness, 100);
            row_y += 28.0;

            // Contrast slider
            self.render_slider_row(cmds, p, x, row_y, width, "Contrast", d.contrast, 100);
            row_y += 28.0;

            // Rotation
            let rotation_str = match d.rotation {
                0 => "Landscape",
                90 => "Portrait",
                180 => "Landscape (flipped)",
                270 => "Portrait (flipped)",
                _ => "Unknown",
            };
            self.render_setting_row(cmds, p, x, row_y, width, "Orientation", rotation_str);
        }
    }

    fn render_night_light_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
        _height: f32,
    ) {
        let nl = &self.night_light;

        cmds.push(RenderCommand::Text {
            x,
            y,
            text: "Night Light".to_string(),
            font_size: 14.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });

        let mut row_y = y + 30.0;

        // Schedule
        self.render_setting_row(
            cmds,
            p,
            x,
            row_y,
            width,
            "Schedule",
            nl.schedule.display_name(),
        );
        row_y += 28.0;

        // Temperature preview
        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: format!("Color Temperature: {}K", nl.temperature.0),
            font_size: 12.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.6),
            overflow: TextOverflow::Ellipsis,
        });

        // Temperature preview swatch
        let preview_color = nl.temperature.preview_color();
        cmds.push(RenderCommand::FillRect {
            x: x + width - 60.0,
            y: row_y - 2.0,
            width: 50.0,
            height: 20.0,
            color: preview_color,
            corner_radii: CornerRadii::all(4.0),
        });
        row_y += 28.0;

        // Transition
        self.render_setting_row(
            cmds,
            p,
            x,
            row_y,
            width,
            "Transition",
            &format!("{} min", nl.transition_minutes),
        );
    }

    fn render_calibration_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
        _height: f32,
    ) {
        if let Some(d) = self.selected_config() {
            cmds.push(RenderCommand::Text {
                x,
                y,
                text: "Color Calibration".to_string(),
                font_size: 14.0,
                color: p.text,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width),
                overflow: TextOverflow::Ellipsis,
            });

            let mut row_y = y + 30.0;

            // Color profile
            self.render_setting_row(
                cmds,
                p,
                x,
                row_y,
                width,
                "Color Profile",
                d.color_profile.display_name(),
            );
            row_y += 28.0;

            // Gamma - Red
            // The channel's own colour, not a theme colour: see the module docs.
            self.render_gamma_row(cmds, p, x, row_y, width, "Red Gamma", d.gamma.red, p.red);
            row_y += 28.0;

            // Gamma - Green
            self.render_gamma_row(
                cmds,
                p,
                x,
                row_y,
                width,
                "Green Gamma",
                d.gamma.green,
                p.green,
            );
            row_y += 28.0;

            // Gamma - Blue
            self.render_gamma_row(
                cmds,
                p,
                x,
                row_y,
                width,
                "Blue Gamma",
                d.gamma.blue,
                // `p.blue`, not `p.accent`: this one really is the blue
                // channel, and the deleted constant meant both.
                p.blue,
            );
            row_y += 28.0;

            // Reset button
            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width: 120.0,
                height: 28.0,
                color: p.surface0,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: row_y + 6.0,
                text: "Reset to Defaults".to_string(),
                font_size: 12.0,
                color: p.text,
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    fn render_test_patterns_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
        _height: f32,
    ) {
        cmds.push(RenderCommand::Text {
            x,
            y,
            text: "Test Patterns".to_string(),
            font_size: 14.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });

        let mut row_y = y + 30.0;

        for pattern in TestPattern::ALL {
            let is_active = self.active_test_pattern == Some(*pattern);

            // Button background
            // Chosen means accented; unchosen means the neutral rung.
            let bg_color = if is_active { p.accent } else { p.surface0 };
            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width: width.min(300.0),
                height: 32.0,
                color: bg_color,
                corner_radii: CornerRadii::all(4.0),
            });

            // Pattern name
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: row_y + 8.0,
                text: pattern.display_name().to_string(),
                font_size: 12.0,
                // Read off the fill rather than named beside it: the
                // deleted near-black is illegible on a light-mode accent.
                color: if is_active { p.on_accent() } else { p.text },
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 24.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Small preview of the pattern
            let preview_x = x + width.min(300.0) + 16.0;
            let preview_cmds = pattern.render(preview_x, row_y + 2.0, 80.0, 28.0);
            cmds.extend(preview_cmds);

            row_y += 40.0;
        }
    }

    fn render_setting_row(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
        label: &str,
        value: &str,
    ) {
        cmds.push(RenderCommand::Text {
            x,
            y,
            text: label.to_string(),
            font_size: 12.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.5),
            overflow: TextOverflow::Ellipsis,
        });
        cmds.push(RenderCommand::Text {
            x: x + width * 0.5,
            y,
            text: value.to_string(),
            font_size: 12.0,
            color: p.text,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.5),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_slider_row(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
        label: &str,
        value: u32,
        max_val: u32,
    ) {
        // Label
        cmds.push(RenderCommand::Text {
            x,
            y,
            text: format!("{}: {}%", label, value),
            font_size: 12.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.4),
            overflow: TextOverflow::Ellipsis,
        });

        // Track
        let track_x = x + width * 0.45;
        let track_w = width * 0.5;
        cmds.push(RenderCommand::FillRect {
            x: track_x,
            y: y + 4.0,
            width: track_w,
            height: 6.0,
            color: p.surface0,
            corner_radii: CornerRadii::all(3.0),
        });

        // Filled portion
        let fill_w = if max_val > 0 {
            track_w * (value as f32 / max_val as f32)
        } else {
            0.0
        };
        cmds.push(RenderCommand::FillRect {
            x: track_x,
            y: y + 4.0,
            width: fill_w,
            height: 6.0,
            // How much of the setting is chosen, so: the accent.
            color: p.accent,
            corner_radii: CornerRadii::all(3.0),
        });

        // Thumb
        cmds.push(RenderCommand::FillRect {
            x: track_x + fill_w - 6.0,
            y: y + 1.0,
            width: 12.0,
            height: 12.0,
            color: p.text,
            corner_radii: CornerRadii::all(6.0),
        });
    }

    fn render_gamma_row(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
        label: &str,
        value: f32,
        color: Color,
    ) {
        cmds.push(RenderCommand::Text {
            x,
            y,
            text: format!("{}: {:.2}", label, value),
            font_size: 12.0,
            color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.4),
            overflow: TextOverflow::Ellipsis,
        });

        // Visual gamma curve indicator (simplified)
        let bar_x = x + width * 0.45;
        let bar_w = width * 0.5;
        cmds.push(RenderCommand::FillRect {
            x: bar_x,
            y: y + 4.0,
            width: bar_w,
            height: 6.0,
            color: p.surface0,
            corner_radii: CornerRadii::all(3.0),
        });

        // Position indicator: gamma 1.0 = center, <1 = left, >1 = right
        let normalized = ((value - 0.2) / 4.8).clamp(0.0, 1.0);
        let indicator_x = bar_x + bar_w * normalized;
        cmds.push(RenderCommand::FillRect {
            x: indicator_x - 4.0,
            y: y + 1.0,
            width: 8.0,
            height: 12.0,
            color,
            corner_radii: CornerRadii::all(4.0),
        });
    }
}

/// Which gamma channel to adjust.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GammaChannel {
    Red,
    Green,
    Blue,
    All,
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
    // These tests assert a float equals the exact literal the code under test was
    // handed. That is the assertion meant: a tolerance would let a value that has
    // drifted pass as one that has not.
    #![allow(clippy::float_cmp)]

    use super::*;
    use crate::palette_check::assert_drawn_from;
    use appearance::AccentColor;

    /// A palette whose accent is in neither mode's roles *and* in none of the
    /// calibration instruments' output.
    ///
    /// The stock accent *is* `blue`, so a fixture built from `for_mode` alone
    /// would let "this site follows the accent" and "this site is hard-coded
    /// blue" produce identical output. That much is the usual reason for an
    /// off-palette fixture; the second clause is this module's own trap. The
    /// obvious off-palette value is magenta — and the SMPTE bars *are*
    /// magenta, so a chip whose preview happened to be `ColorBars` contributed
    /// two "accents" to a tab that drew one. The value below has no channel at
    /// 0 or at 255 and is not a grey, which is exactly what puts it outside
    /// every pattern: the bars are combinations of 0 and 255, the greyscale
    /// ramp and the checkerboard are greys, and the hue sweep is fully
    /// saturated (one channel 0, one 255) at every step.
    fn accented(light: bool) -> Palette {
        let mut p = Palette::for_mode(light);
        p.accent = Color::from_hex(0x00C8_28A0);
        assert!(
            !p.roles()
                .iter()
                .any(|(n, r)| *n != "accent" && *r == p.accent),
            "the fixture's accent collided with a role, so accent tests would \
             pass for the wrong reason"
        );
        assert!(
            !instrument_colors(&DisplaySettingsManager::default()).contains(&p.accent),
            "the fixture's accent is a colour an instrument draws, so counting \
             the accents on a tab would count a calibration target as chrome"
        );
        p
    }

    /// The fourteen named accents the appearance page offers.
    ///
    /// Needed wherever the property under test is a *function of* the accent
    /// rather than "did this follow the accent at all" — `on_accent` is a
    /// threshold, and one sample characterises a step function only by luck.
    /// See known-issues.md lesson 20.
    const OFFERED: [AccentColor; 14] = [
        AccentColor::Blue,
        AccentColor::Lavender,
        AccentColor::Teal,
        AccentColor::Green,
        AccentColor::Yellow,
        AccentColor::Peach,
        AccentColor::Pink,
        AccentColor::Mauve,
        AccentColor::Red,
        AccentColor::Rosewater,
        AccentColor::Flamingo,
        AccentColor::Maroon,
        AccentColor::Sky,
        AccentColor::Sapphire,
    ];

    /// `p` for `light` mode wearing `accent`, as the settings page would build.
    fn wearing(light: bool, accent: AccentColor) -> Palette {
        let mut p = Palette::for_mode(light);
        p.accent = if light {
            accent.color_light()
        } else {
            accent.color()
        };
        p
    }

    /// Every colour `cmds` puts on the screen, in draw order.
    fn colors(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect { color, .. }
                | RenderCommand::StrokeRect { color, .. }
                | RenderCommand::Text { color, .. } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// Every colour the four calibration instruments emit.
    ///
    /// Declared to the membership sweep rather than themed, because none of
    /// them is the shell's to theme — see the module docs. Built by calling
    /// the renderers, which would be a tautology if this were the *test* of
    /// those values; it is not. `the_test_patterns_are_the_same_in_both_modes`
    /// and `the_night_light_swatch_shows_the_temperature_not_the_theme` pin
    /// them against hand-written expectations.
    fn instrument_colors(mgr: &DisplaySettingsManager) -> Vec<Color> {
        let mut v: Vec<Color> = TestPattern::ALL
            .iter()
            .flat_map(|pat| colors(&pat.render(0.0, 0.0, 80.0, 28.0)))
            .collect();
        v.push(mgr.night_light.temperature.preview_color());
        v
    }

    /// The one colour on `cmds` that belongs to no palette at all.
    ///
    /// This is how the night-light swatch is found, and it is deliberately not
    /// an index into the command stream: an index is a claim about layout, and
    /// a heading added above the swatch would silently retarget the assertion
    /// at the heading — which is a colour that *does* follow the theme, so the
    /// test would then be asserting the opposite of what it says. Finding it
    /// by exclusion also states the property being relied on: the swatch is
    /// the only thing on the page the shell did not choose.
    fn only_off_palette(p: &Palette, cmds: &[RenderCommand]) -> Color {
        let dark = Palette::for_mode(false);
        let light = Palette::for_mode(true);
        let found: Vec<Color> = colors(cmds)
            .into_iter()
            .filter(|c| {
                ![&dark, &light, p]
                    .iter()
                    .any(|q| q.roles().iter().any(|(_, r)| *r == *c))
            })
            .collect();
        assert_eq!(
            found.len(),
            1,
            "expected exactly one off-palette colour, found {found:?}"
        );
        found[0]
    }

    /// Every pattern chip's `(fill, lettering)`, in draw order.
    ///
    /// A chip is the 32-pixel-high rounded fill and the `Text` immediately
    /// after it. Paired structurally for the same reason as `only_off_palette`
    /// above: the previews between the chips vary in length with the pattern,
    /// so only the *first* chip has a fixed index, and pinning the first chip
    /// alone would leave the other four unchecked.
    fn chips(cmds: &[RenderCommand]) -> Vec<(Color, Color)> {
        let mut out = Vec::new();
        for pair in cmds.windows(2) {
            if let (
                RenderCommand::FillRect {
                    height,
                    color: fill,
                    ..
                },
                RenderCommand::Text { color: ink, .. },
            ) = (&pair[0], &pair[1])
                && *height == 32.0
            {
                out.push((*fill, *ink));
            }
        }
        assert_eq!(
            out.len(),
            TestPattern::ALL.len(),
            "not every chip was found"
        );
        out
    }

    // ---- ColorTemperature tests ----

    #[test]
    fn test_color_temp_clamp() {
        assert_eq!(ColorTemperature(500).clamped().0, 1000);
        assert_eq!(ColorTemperature(20000).clamped().0, 15000);
        assert_eq!(ColorTemperature(6500).clamped().0, 6500);
    }

    #[test]
    fn test_color_temp_rgb_daylight() {
        let (r, g, b) = ColorTemperature::NEUTRAL.to_rgb_multiplier();
        // At 6500K, should be approximately white (all channels close to 1.0)
        assert!(r > 0.9);
        assert!(g > 0.9);
        assert!(b > 0.9);
    }

    #[test]
    fn test_color_temp_rgb_warm() {
        let (r, _g, b) = ColorTemperature::WARM.to_rgb_multiplier();
        // Warm should have more red than blue
        assert!(r > b);
    }

    #[test]
    fn test_color_temp_rgb_cool() {
        let (r, _g, b) = ColorTemperature::COOL.to_rgb_multiplier();
        // Cool should have more blue relative to the warm temp
        let (wr, _wg, _wb) = ColorTemperature::WARM.to_rgb_multiplier();
        assert!(b > 0.0);
        // At cool temps, red should be less than at warm temps
        assert!(r <= wr);
    }

    #[test]
    fn test_color_temp_preview_color() {
        let color = ColorTemperature::NEUTRAL.preview_color();
        // Should be close to white
        assert!(color.r > 200);
    }

    // ---- NightLightSchedule tests ----

    #[test]
    fn test_schedule_off() {
        let s = NightLightSchedule::Off;
        assert!(!s.is_active(12, 0));
        assert!(!s.is_active(22, 0));
    }

    #[test]
    fn test_schedule_always_on() {
        let s = NightLightSchedule::AlwaysOn;
        assert!(s.is_active(12, 0));
        assert!(s.is_active(3, 0));
    }

    #[test]
    fn test_schedule_sunset_sunrise() {
        let s = NightLightSchedule::SunsetToSunrise;
        assert!(s.is_active(22, 0)); // 10 PM
        assert!(s.is_active(3, 0)); // 3 AM
        assert!(!s.is_active(12, 0)); // Noon
    }

    #[test]
    fn test_schedule_custom_same_day() {
        let s = NightLightSchedule::Custom(DailyWindow::from_hm(14, 0, 18, 0).unwrap());
        assert!(s.is_active(15, 0));
        assert!(!s.is_active(12, 0));
        assert!(!s.is_active(20, 0));
    }

    #[test]
    fn test_schedule_custom_overnight() {
        let s = NightLightSchedule::Custom(DailyWindow::from_hm(22, 0, 6, 0).unwrap());
        assert!(s.is_active(23, 0));
        assert!(s.is_active(3, 0));
        assert!(!s.is_active(12, 0));
    }

    #[test]
    fn test_schedule_display_names() {
        assert_eq!(NightLightSchedule::Off.display_name(), "Off");
        assert_eq!(NightLightSchedule::AlwaysOn.display_name(), "Always On");
    }

    // ---- GammaSettings tests ----

    #[test]
    fn test_gamma_default_is_identity() {
        let g = GammaSettings::default();
        assert!(g.is_default());
        assert_eq!(g.red, 1.0);
    }

    #[test]
    fn test_gamma_apply_identity() {
        let g = GammaSettings::default();
        let c = Color::rgb(128, 64, 200);
        let result = g.apply(c);
        assert_eq!(result.r, 128);
        assert_eq!(result.g, 64);
        assert_eq!(result.b, 200);
    }

    #[test]
    fn test_gamma_apply_channel_extremes() {
        // 0 stays 0, 255 stays 255 regardless of gamma
        assert_eq!(GammaSettings::apply_channel(0, 2.0), 0);
        assert_eq!(GammaSettings::apply_channel(255, 2.0), 255);
    }

    #[test]
    fn test_gamma_apply_brighten() {
        // Gamma > 1.0 with our formula (1/gamma) should darken midtones
        // (since we apply value^(1/gamma))
        let bright = GammaSettings::apply_channel(128, 0.5);
        let normal = GammaSettings::apply_channel(128, 1.0);
        // gamma < 1.0 → exponent > 1.0 → darker
        assert!(bright < normal);
    }

    #[test]
    fn test_gamma_is_default_false() {
        let g = GammaSettings {
            red: 1.2,
            green: 1.0,
            blue: 1.0,
        };
        assert!(!g.is_default());
    }

    // ---- ColorProfile tests ----

    #[test]
    fn test_color_profile_roundtrip() {
        for profile in [
            ColorProfile::SRGB,
            ColorProfile::DciP3,
            ColorProfile::AdobeRGB,
            ColorProfile::Native,
            ColorProfile::Custom("My Profile".to_string()),
        ] {
            let id = profile.id();
            let parsed = ColorProfile::from_id(&id);
            assert_eq!(parsed, profile);
        }
    }

    #[test]
    fn test_color_profile_unknown_defaults_srgb() {
        let p = ColorProfile::from_id("unknown-garbage");
        assert_eq!(p, ColorProfile::SRGB);
    }

    #[test]
    fn test_color_profile_display_names() {
        assert_eq!(ColorProfile::SRGB.display_name(), "sRGB");
        assert_eq!(ColorProfile::DciP3.display_name(), "DCI-P3");
    }

    // ---- TestPattern tests ----

    #[test]
    fn test_pattern_grayscale() {
        let cmds = TestPattern::Grayscale.render(0.0, 0.0, 320.0, 100.0);
        assert_eq!(cmds.len(), 16); // 16 steps
    }

    #[test]
    fn test_pattern_color_bars() {
        let cmds = TestPattern::ColorBars.render(0.0, 0.0, 320.0, 100.0);
        assert_eq!(cmds.len(), 8); // 8 color bars
    }

    #[test]
    fn test_pattern_hue_gradient() {
        let cmds = TestPattern::HueGradient.render(0.0, 0.0, 320.0, 100.0);
        assert_eq!(cmds.len(), 24); // 24 hue steps
    }

    #[test]
    fn test_pattern_checkerboard() {
        let cmds = TestPattern::Checkerboard.render(0.0, 0.0, 100.0, 100.0);
        // 100/20 = 5 cells per axis = 25 cells
        assert_eq!(cmds.len(), 25);
    }

    #[test]
    fn test_pattern_solid_gray() {
        let cmds = TestPattern::SolidGray.render(0.0, 0.0, 320.0, 100.0);
        assert_eq!(cmds.len(), 1);
    }

    #[test]
    fn test_pattern_display_names() {
        for p in TestPattern::ALL {
            assert!(!p.display_name().is_empty());
        }
    }

    // ---- DisplayConfig tests ----

    #[test]
    fn test_display_config_default() {
        let d = DisplayConfig::default();
        assert_eq!(d.brightness, 100);
        assert_eq!(d.resolution_width, 1920);
        assert!(d.is_primary);
    }

    #[test]
    fn test_display_config_resolution_string() {
        let d = DisplayConfig {
            resolution_width: 2560,
            resolution_height: 1440,
            refresh_rate: 144,
            ..DisplayConfig::default()
        };
        assert_eq!(d.resolution_string(), "2560x1440 @ 144Hz");
    }

    #[test]
    fn test_display_config_scale_string() {
        let d = DisplayConfig {
            scale_factor: 1.5,
            ..DisplayConfig::default()
        };
        assert_eq!(d.scale_string(), "150%");
    }

    // ---- DisplaySettingsManager tests ----

    #[test]
    fn test_manager_default() {
        let mgr = DisplaySettingsManager::default();
        assert_eq!(mgr.displays.len(), 1);
        assert_eq!(mgr.selected_display, 0);
        assert!(mgr.active_test_pattern.is_none());
    }

    #[test]
    fn test_manager_select_next() {
        let mut mgr = DisplaySettingsManager::new(vec![
            DisplayConfig {
                display_id: 0,
                name: "A".to_string(),
                ..DisplayConfig::default()
            },
            DisplayConfig {
                display_id: 1,
                name: "B".to_string(),
                ..DisplayConfig::default()
            },
        ]);
        assert_eq!(mgr.selected_display, 0);
        mgr.select_next_display();
        assert_eq!(mgr.selected_display, 1);
        mgr.select_next_display();
        assert_eq!(mgr.selected_display, 0); // wraps
    }

    #[test]
    fn test_manager_set_brightness() {
        let mut mgr = DisplaySettingsManager::default();
        mgr.set_brightness(75);
        assert_eq!(mgr.selected_config().unwrap().brightness, 75);
    }

    #[test]
    fn test_manager_set_brightness_clamp() {
        let mut mgr = DisplaySettingsManager::default();
        mgr.set_brightness(200);
        assert_eq!(mgr.selected_config().unwrap().brightness, 100);
    }

    #[test]
    fn test_manager_set_contrast() {
        let mut mgr = DisplaySettingsManager::default();
        mgr.set_contrast(80);
        assert_eq!(mgr.selected_config().unwrap().contrast, 80);
    }

    #[test]
    fn test_manager_set_scale() {
        let mut mgr = DisplaySettingsManager::default();
        mgr.set_scale(1.75);
        let scale = mgr.selected_config().unwrap().scale_factor;
        assert!((scale - 1.75).abs() < 0.01);
    }

    #[test]
    fn test_manager_set_scale_clamp() {
        let mut mgr = DisplaySettingsManager::default();
        mgr.set_scale(0.1);
        assert!(mgr.selected_config().unwrap().scale_factor >= 0.5);
        mgr.set_scale(10.0);
        assert!(mgr.selected_config().unwrap().scale_factor <= 4.0);
    }

    #[test]
    fn test_manager_set_color_profile() {
        let mut mgr = DisplaySettingsManager::default();
        mgr.set_color_profile(ColorProfile::DciP3);
        assert_eq!(
            mgr.selected_config().unwrap().color_profile,
            ColorProfile::DciP3
        );
    }

    #[test]
    fn test_manager_set_gamma() {
        let mut mgr = DisplaySettingsManager::default();
        mgr.set_gamma(GammaChannel::Red, 1.5);
        let g = &mgr.selected_config().unwrap().gamma;
        assert!((g.red - 1.5).abs() < 0.01);
        assert!((g.green - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_manager_set_gamma_all() {
        let mut mgr = DisplaySettingsManager::default();
        mgr.set_gamma(GammaChannel::All, 0.8);
        let g = &mgr.selected_config().unwrap().gamma;
        assert!((g.red - 0.8).abs() < 0.01);
        assert!((g.green - 0.8).abs() < 0.01);
        assert!((g.blue - 0.8).abs() < 0.01);
    }

    #[test]
    fn test_manager_reset_gamma() {
        let mut mgr = DisplaySettingsManager::default();
        mgr.set_gamma(GammaChannel::All, 2.0);
        mgr.reset_gamma();
        assert!(mgr.selected_config().unwrap().gamma.is_default());
    }

    #[test]
    fn test_manager_night_light_schedule() {
        let mut mgr = DisplaySettingsManager::default();
        mgr.set_night_light_schedule(NightLightSchedule::AlwaysOn);
        assert_eq!(mgr.night_light.schedule, NightLightSchedule::AlwaysOn);
    }

    #[test]
    fn test_manager_night_light_temperature() {
        let mut mgr = DisplaySettingsManager::default();
        mgr.set_night_light_temperature(4000);
        assert_eq!(mgr.night_light.temperature.0, 4000);
    }

    #[test]
    fn test_manager_night_light_temperature_clamp() {
        let mut mgr = DisplaySettingsManager::default();
        mgr.set_night_light_temperature(500);
        assert_eq!(mgr.night_light.temperature.0, 1000);
    }

    #[test]
    fn test_manager_rotation() {
        let mut mgr = DisplaySettingsManager::default();
        mgr.set_rotation(90);
        assert_eq!(mgr.selected_config().unwrap().rotation, 90);
        mgr.set_rotation(180);
        assert_eq!(mgr.selected_config().unwrap().rotation, 180);
    }

    #[test]
    fn test_manager_test_pattern() {
        let mut mgr = DisplaySettingsManager::default();
        mgr.show_test_pattern(TestPattern::ColorBars);
        assert_eq!(mgr.active_test_pattern, Some(TestPattern::ColorBars));
        mgr.dismiss_test_pattern();
        assert!(mgr.active_test_pattern.is_none());
    }

    #[test]
    fn test_manager_config_roundtrip() {
        let mut mgr = DisplaySettingsManager::default();
        mgr.set_brightness(80);
        mgr.set_night_light_temperature(3500);
        let text = mgr.to_config_text();
        assert!(text.contains("night_light_temp=3500"));
        assert!(text.contains("brightness=80"));
    }

    #[test]
    fn test_manager_render_general() {
        let mgr = DisplaySettingsManager::default();
        let cmds = mgr.render(&accented(false), 0.0, 0.0, 600.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_manager_render_night_light() {
        let mut mgr = DisplaySettingsManager::default();
        mgr.active_tab = DisplaySettingsTab::NightLight;
        let cmds = mgr.render(&accented(false), 0.0, 0.0, 600.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_manager_render_calibration() {
        let mut mgr = DisplaySettingsManager::default();
        mgr.active_tab = DisplaySettingsTab::ColorCalibration;
        let cmds = mgr.render(&accented(false), 0.0, 0.0, 600.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_manager_render_test_patterns() {
        let mut mgr = DisplaySettingsManager::default();
        mgr.active_tab = DisplaySettingsTab::TestPatterns;
        let cmds = mgr.render(&accented(false), 0.0, 0.0, 600.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_tab_display_names() {
        for tab in DisplaySettingsTab::ALL {
            assert!(!tab.display_name().is_empty());
        }
    }

    // ---- Hue helper test ----

    #[test]
    fn test_hue_to_rgb_red() {
        let c = hue_to_rgb(0.0);
        assert_eq!(c.r, 255);
        assert_eq!(c.b, 0);
    }

    #[test]
    fn test_hue_to_rgb_green() {
        let c = hue_to_rgb(1.0 / 3.0);
        assert_eq!(c.g, 255);
    }

    #[test]
    fn test_hue_to_rgb_blue() {
        let c = hue_to_rgb(2.0 / 3.0);
        assert_eq!(c.b, 255);
    }

    // ---- Palette conversion ----

    #[test]
    fn every_colour_the_panel_draws_comes_from_its_palette() {
        let mut drawn = 0;
        for light in [false, true] {
            let p = accented(light);
            for &tab in DisplaySettingsTab::ALL {
                for pattern in TestPattern::ALL.iter().map(Some).chain([None]) {
                    let mut mgr = DisplaySettingsManager::default();
                    mgr.active_tab = tab;
                    mgr.active_test_pattern = pattern.copied();
                    let cmds = mgr.render(&p, 0.0, 0.0, 600.0, 400.0);
                    drawn += cmds.len();
                    assert_drawn_from(&p, &cmds, &instrument_colors(&mgr), "display_settings");
                }
            }
        }
        // Non-vacuity: a sweep over an empty render passes trivially, and the
        // whole point is that it did not.
        assert!(drawn > 200, "only {drawn} commands were swept");
    }

    #[test]
    fn none_of_the_nine_deleted_constants_is_still_drawn() {
        // Every one of these is a Mocha value, so a light render that contains
        // one is a substitution the conversion missed. The instruments are
        // excluded by construction: they are pure white, pure black and full
        // primaries, none of which is in this list.
        const DELETED: [(&str, u32); 9] = [
            ("MOCHA_BASE", 0x001E_1E2E),
            ("MOCHA_SURFACE0", 0x0031_3244),
            ("MOCHA_SURFACE1", 0x0045_475A),
            ("MOCHA_TEXT", 0x00CD_D6F4),
            ("MOCHA_SUBTEXT0", 0x00A6_ADC8),
            ("MOCHA_BLUE", 0x0089_B4FA),
            ("MOCHA_RED", 0x00F3_8BA8),
            ("MOCHA_GREEN", 0x00A6_E3A1),
            ("MOCHA_MANTLE", 0x0018_1825),
        ];
        let p = accented(true);
        for &tab in DisplaySettingsTab::ALL {
            let mut mgr = DisplaySettingsManager::default();
            mgr.active_tab = tab;
            mgr.active_test_pattern = Some(TestPattern::ColorBars);
            for c in colors(&mgr.render(&p, 0.0, 0.0, 600.0, 400.0)) {
                let rgb = (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b);
                for (name, hex) in DELETED {
                    assert_ne!(rgb, hex, "{tab:?} still draws {name} under the light theme");
                }
            }
        }
    }

    #[test]
    fn every_site_draws_the_role_it_claims() {
        // The ordered vector, not the set: half the ways this conversion can
        // go wrong leave two sites having traded roles, which a membership
        // table cannot see (module 29).
        for light in [false, true] {
            let p = accented(light);
            let mut mgr = DisplaySettingsManager::default();
            mgr.active_tab = DisplaySettingsTab::General;
            assert_eq!(
                colors(&mgr.render(&p, 0.0, 0.0, 600.0, 400.0)),
                vec![
                    p.base,     // panel
                    p.text,     // title
                    p.surface1, // the active tab's pill
                    p.accent,   // the active tab's label
                    p.subtext0, // three inactive tab labels
                    p.subtext0, p.subtext0, p.text,     // "Display: ..."
                    p.subtext0, // Resolution label / value
                    p.text, p.subtext0, // Scale
                    p.text, p.subtext0, // Brightness: label, track, fill, thumb
                    p.surface0, p.accent, p.text, p.subtext0, // Contrast
                    p.surface0, p.accent, p.text, p.subtext0, // Orientation
                    p.text,
                ],
                "the General tab in {} mode",
                if light { "light" } else { "dark" }
            );

            mgr.active_tab = DisplaySettingsTab::ColorCalibration;
            assert_eq!(
                colors(&mgr.render(&p, 0.0, 0.0, 600.0, 400.0)),
                vec![
                    p.base, p.text, p.subtext0, // General and Night Light, now inactive
                    p.subtext0,
                    p.surface1, // the pill, drawn just before the label it sits under
                    p.accent,   // this tab's label
                    p.subtext0, // Test Patterns
                    p.text,     // "Color Calibration"
                    p.subtext0, // Color Profile label / value
                    p.text, p.red, // the three gamma rows: channel, track, indicator
                    p.surface0, p.red, p.green, p.surface0, p.green, p.blue, p.surface0, p.blue,
                    p.surface0, // Reset button and its label
                    p.text,
                ],
                "the Calibration tab in {} mode",
                if light { "light" } else { "dark" }
            );
        }
    }

    #[test]
    fn the_gamma_rows_are_the_channels_and_never_the_accent() {
        // The deleted `MOCHA_BLUE` meant "chosen" at three sites and "the blue
        // channel" at this one. Under the shipped theme the accent *is* blue,
        // so the two were the same colour and the file could not say which it
        // meant; an off-palette accent is what separates them.
        for light in [false, true] {
            let p = accented(light);
            let mut mgr = DisplaySettingsManager::default();
            mgr.active_tab = DisplaySettingsTab::ColorCalibration;
            let cols = colors(&mgr.render(&p, 0.0, 0.0, 600.0, 400.0));
            for (name, role) in [("red", p.red), ("green", p.green), ("blue", p.blue)] {
                assert_eq!(
                    cols.iter().filter(|c| **c == role).count(),
                    2,
                    "the {name} gamma row should draw {name} twice — its label \
                     and its indicator — in {} mode",
                    if light { "light" } else { "dark" }
                );
            }
            // Exactly one accent on this tab: the active tab's own label. A
            // gamma row that fell back to the accent would make it two.
            assert_eq!(
                cols.iter().filter(|c| **c == p.accent).count(),
                1,
                "a calibration row followed the accent"
            );
        }
    }

    #[test]
    fn the_test_patterns_are_the_same_in_both_modes() {
        // A calibration target is a measurement, not chrome. This is pinned
        // against hand-written values rather than against the palette: white
        // is #FFFFFF because it is white, and if the palette ever grew a role
        // of that value the pattern would still have to be this.
        for pattern in TestPattern::ALL {
            let cols = colors(&pattern.render(0.0, 0.0, 160.0, 40.0));
            assert!(!cols.is_empty(), "{pattern:?} drew nothing");
            for c in &cols {
                assert_eq!(c.a, 255, "{pattern:?} emitted a transparent colour");
            }
            match pattern {
                TestPattern::Grayscale => {
                    assert_eq!(cols.first(), Some(&Color::rgb(0, 0, 0)));
                    assert_eq!(cols.last(), Some(&Color::rgb(255, 255, 255)));
                    for c in &cols {
                        assert!(c.r == c.g && c.g == c.b, "a grey step was tinted");
                    }
                }
                TestPattern::ColorBars => {
                    assert_eq!(
                        cols,
                        vec![
                            Color::rgb(255, 255, 255),
                            Color::rgb(255, 255, 0),
                            Color::rgb(0, 255, 255),
                            Color::rgb(0, 255, 0),
                            Color::rgb(255, 0, 255),
                            Color::rgb(255, 0, 0),
                            Color::rgb(0, 0, 255),
                            Color::rgb(0, 0, 0),
                        ]
                    );
                }
                TestPattern::Checkerboard => {
                    for c in &cols {
                        assert!(
                            *c == Color::rgb(0, 0, 0) || *c == Color::rgb(255, 255, 255),
                            "a checkerboard cell was neither black nor white"
                        );
                    }
                }
                TestPattern::SolidGray => {
                    assert_eq!(cols, vec![Color::rgb(128, 128, 128)]);
                }
                TestPattern::HueGradient => {
                    // Full saturation and value: every step pins one channel at
                    // 255 and another at 0.
                    for c in &cols {
                        let (lo, hi) = (c.r.min(c.g).min(c.b), c.r.max(c.g).max(c.b));
                        assert_eq!((lo, hi), (0, 255), "a hue step was desaturated");
                    }
                }
            }
        }
    }

    #[test]
    fn the_night_light_swatch_shows_the_temperature_not_the_theme() {
        // What this swatch is for is telling the user what 3000K will actually
        // look like. It must be identical in both modes, and it must move when
        // the temperature moves.
        let mut seen = Vec::new();
        for temp in [3000_u32, 6500, 15000] {
            let mut per_mode = Vec::new();
            for light in [false, true] {
                let p = accented(light);
                let mut mgr = DisplaySettingsManager::default();
                mgr.active_tab = DisplaySettingsTab::NightLight;
                mgr.set_night_light_temperature(temp);
                let cmds = mgr.render(&p, 0.0, 0.0, 600.0, 400.0);
                per_mode.push(only_off_palette(&p, &cmds));
            }
            assert_eq!(
                per_mode[0], per_mode[1],
                "the {temp}K swatch changed with the theme"
            );
            seen.push(per_mode[0]);
        }
        // Warm is redder than neutral, which is redder than cool. Stated as
        // the ordering rather than as three literals: the ordering is the
        // claim, and it survives a change to the black-body approximation.
        assert!(
            seen[0].b < seen[1].b && seen[1].b <= seen[2].b,
            "the swatch does not get bluer as the temperature rises: {seen:?}"
        );
        assert!(
            seen[0].r >= seen[1].r,
            "the warm swatch is not at least as red as the neutral one"
        );
    }

    #[test]
    fn a_selected_pattern_chip_is_lettered_for_its_own_fill() {
        // `MOCHA_MANTLE` — near-black — is legible on Mocha's pale blue and
        // illegible on Latte's `#1D62EC`. The endpoints are pinned by hand
        // rather than by calling `readable_on`, because a test that called the
        // renderer's own function would agree with it however wrong both were.
        //
        // Walked over all fourteen accents: `on_accent` is a threshold at luma
        // 140, and one accent samples one side of it (lesson 20).
        const NEAR_BLACK: u32 = 0x0011_111B;
        const NEAR_WHITE: u32 = 0x00EF_F1F5;
        for light in [false, true] {
            for accent in OFFERED {
                let p = wearing(light, accent);
                let mut mgr = DisplaySettingsManager::default();
                mgr.active_tab = DisplaySettingsTab::TestPatterns;
                mgr.active_test_pattern = Some(TestPattern::Grayscale);
                let cmds = mgr.render(&p, 0.0, 0.0, 600.0, 400.0);
                let (fill, ink) = chips(&cmds)[0];
                assert_eq!(fill, p.accent, "the chosen chip is not accented");
                let luma = 0.299 * f32::from(fill.r)
                    + 0.587 * f32::from(fill.g)
                    + 0.114 * f32::from(fill.b);
                let want = if luma > 140.0 { NEAR_BLACK } else { NEAR_WHITE };
                let got = (u32::from(ink.r) << 16) | (u32::from(ink.g) << 8) | u32::from(ink.b);
                assert_eq!(
                    got,
                    want,
                    "{accent:?} in {} mode: a chip of luma {luma:.0} was \
                     lettered #{got:06X}",
                    if light { "light" } else { "dark" }
                );
            }
        }
    }

    #[test]
    fn an_unchosen_chip_and_an_unchosen_tab_are_never_the_accent() {
        // The inverse of the test above, and the one that catches an "always
        // accented" fix: exactly one tab label and at most one chip may wear
        // the accent, however many of each are on screen.
        for light in [false, true] {
            let p = accented(light);
            for chosen in [None, Some(TestPattern::HueGradient)] {
                let mut mgr = DisplaySettingsManager::default();
                mgr.active_tab = DisplaySettingsTab::TestPatterns;
                mgr.active_test_pattern = chosen;
                let cols = colors(&mgr.render(&p, 0.0, 0.0, 600.0, 400.0));
                let want = 1 + usize::from(chosen.is_some());
                assert_eq!(
                    cols.iter().filter(|c| **c == p.accent).count(),
                    want,
                    "with {chosen:?} chosen in {} mode, the accent appears the \
                     wrong number of times",
                    if light { "light" } else { "dark" }
                );
                assert!(
                    cols.iter().filter(|c| **c == p.surface0).count() >= 4,
                    "the four unchosen chips are not on the neutral rung"
                );
            }
        }
    }
}
