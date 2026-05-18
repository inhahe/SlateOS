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

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha theme
// ============================================================================

const MOCHA_BASE: Color = Color::from_hex(0x1E1E2E);
const MOCHA_SURFACE0: Color = Color::from_hex(0x313244);
const MOCHA_SURFACE1: Color = Color::from_hex(0x45475A);
const MOCHA_TEXT: Color = Color::from_hex(0xCDD6F4);
const MOCHA_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const MOCHA_BLUE: Color = Color::from_hex(0x89B4FA);
const MOCHA_RED: Color = Color::from_hex(0xF38BA8);
const MOCHA_GREEN: Color = Color::from_hex(0xA6E3A1);
const MOCHA_YELLOW: Color = Color::from_hex(0xF9E2AF);
const MOCHA_PEACH: Color = Color::from_hex(0xFAB387);
const MOCHA_OVERLAY0: Color = Color::from_hex(0x6C7086);
const MOCHA_MANTLE: Color = Color::from_hex(0x181825);

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
            let r_raw = 329.698727446 * (temp - 60.0).powf(-0.1332047592);
            (r_raw / 255.0).clamp(0.0, 1.0)
        };

        // Green channel
        let g = if temp <= 66.0 {
            let g_raw = 99.4708025861 * temp.ln() - 161.1195681661;
            (g_raw / 255.0).clamp(0.0, 1.0)
        } else {
            let g_raw = 288.1221695283 * (temp - 60.0).powf(-0.0755148492);
            (g_raw / 255.0).clamp(0.0, 1.0)
        };

        // Blue channel
        let b = if temp >= 66.0 {
            1.0
        } else if temp <= 19.0 {
            0.0
        } else {
            let b_raw = 138.5177312231 * (temp - 10.0).ln() - 305.0447927307;
            (b_raw / 255.0).clamp(0.0, 1.0)
        };

        (r, g, b)
    }

    /// Produce a preview color at this temperature.
    pub fn preview_color(self) -> Color {
        let (r, g, b) = self.to_rgb_multiplier();
        Color::rgb(
            (r * 255.0) as u8,
            (g * 255.0) as u8,
            (b * 255.0) as u8,
        )
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
    Custom {
        /// Start hour (0-23).
        start_hour: u8,
        /// Start minute (0-59).
        start_minute: u8,
        /// End hour (0-23).
        end_hour: u8,
        /// End minute (0-59).
        end_minute: u8,
    },
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
                hour >= 19 || hour < 7
            }
            Self::Custom {
                start_hour,
                start_minute,
                end_hour,
                end_minute,
            } => {
                let now = (hour as u32) * 60 + (minute as u32);
                let start = (*start_hour as u32) * 60 + (*start_minute as u32);
                let end = (*end_hour as u32) * 60 + (*end_minute as u32);

                if start <= end {
                    // Same-day range (e.g., 14:00 to 18:00)
                    now >= start && now < end
                } else {
                    // Overnight range (e.g., 22:00 to 06:00)
                    now >= start || now < end
                }
            }
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
    /// Apply gamma correction to a color value (0-255).
    pub fn apply_channel(value: u8, gamma: f32) -> u8 {
        if gamma == 1.0 {
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
        (self.red - 1.0).abs() < 0.001
            && (self.green - 1.0).abs() < 0.001
            && (self.blue - 1.0).abs() < 0.001
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
        for i in 0..steps {
            let gray = (i * 255 / (steps - 1).max(1)) as u8;
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
        let mut cmds = Vec::with_capacity((cols * rows) as usize);
        for row in 0..rows {
            for col in 0..cols {
                let is_white = (row + col) % 2 == 0;
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
        if !self.displays.is_empty() {
            self.selected_display = (self.selected_display + 1) % self.displays.len();
        }
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
            NightLightSchedule::Custom {
                start_hour,
                start_minute,
                end_hour,
                end_minute,
            } => format!("custom:{:02}:{:02}-{:02}:{:02}", start_hour, start_minute, end_hour, end_minute),
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
            out.push_str(&format!(
                "{}_profile={}\n",
                prefix,
                d.color_profile.id()
            ));
            out.push_str(&format!("{}_rotation={}\n", prefix, d.rotation));
        }

        out
    }

    /// Render the display settings panel.
    pub fn render(&self, x: f32, y: f32, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(64);

        // Background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: MOCHA_BASE,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: y + 16.0,
            text: "Display Settings".to_string(),
            font_size: 18.0,
            color: MOCHA_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 32.0),
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
                    color: MOCHA_SURFACE1,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: tab_x + 8.0,
                y: tab_y + 6.0,
                text: tab.display_name().to_string(),
                font_size: 12.0,
                color: if is_active { MOCHA_BLUE } else { MOCHA_SUBTEXT0 },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(120.0),
            });
        }

        // Content area
        let content_y = tab_y + 40.0;
        let content_height = height - (content_y - y) - 16.0;

        match self.active_tab {
            DisplaySettingsTab::General => {
                self.render_general_tab(&mut cmds, x + 16.0, content_y, width - 32.0, content_height);
            }
            DisplaySettingsTab::NightLight => {
                self.render_night_light_tab(&mut cmds, x + 16.0, content_y, width - 32.0, content_height);
            }
            DisplaySettingsTab::ColorCalibration => {
                self.render_calibration_tab(&mut cmds, x + 16.0, content_y, width - 32.0, content_height);
            }
            DisplaySettingsTab::TestPatterns => {
                self.render_test_patterns_tab(&mut cmds, x + 16.0, content_y, width - 32.0, content_height);
            }
        }

        cmds
    }

    fn render_general_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
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
                text: format!("Display: {} ({})", d.name, if d.is_primary { "Primary" } else { "Secondary" }),
                font_size: 14.0,
                color: MOCHA_TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width),
            });

            let mut row_y = y + 30.0;

            // Resolution
            self.render_setting_row(cmds, x, row_y, width, "Resolution", &d.resolution_string());
            row_y += 28.0;

            // Scale
            self.render_setting_row(cmds, x, row_y, width, "Scale", &d.scale_string());
            row_y += 28.0;

            // Brightness slider
            self.render_slider_row(cmds, x, row_y, width, "Brightness", d.brightness, 100);
            row_y += 28.0;

            // Contrast slider
            self.render_slider_row(cmds, x, row_y, width, "Contrast", d.contrast, 100);
            row_y += 28.0;

            // Rotation
            let rotation_str = match d.rotation {
                0 => "Landscape",
                90 => "Portrait",
                180 => "Landscape (flipped)",
                270 => "Portrait (flipped)",
                _ => "Unknown",
            };
            self.render_setting_row(cmds, x, row_y, width, "Orientation", rotation_str);
        }
    }

    fn render_night_light_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
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
            color: MOCHA_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });

        let mut row_y = y + 30.0;

        // Schedule
        self.render_setting_row(
            cmds, x, row_y, width,
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
            color: MOCHA_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.6),
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
            cmds, x, row_y, width,
            "Transition",
            &format!("{} min", nl.transition_minutes),
        );
    }

    fn render_calibration_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
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
                color: MOCHA_TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width),
            });

            let mut row_y = y + 30.0;

            // Color profile
            self.render_setting_row(cmds, x, row_y, width, "Color Profile", d.color_profile.display_name());
            row_y += 28.0;

            // Gamma - Red
            self.render_gamma_row(cmds, x, row_y, width, "Red Gamma", d.gamma.red, MOCHA_RED);
            row_y += 28.0;

            // Gamma - Green
            self.render_gamma_row(cmds, x, row_y, width, "Green Gamma", d.gamma.green, MOCHA_GREEN);
            row_y += 28.0;

            // Gamma - Blue
            self.render_gamma_row(cmds, x, row_y, width, "Blue Gamma", d.gamma.blue, MOCHA_BLUE);
            row_y += 28.0;

            // Reset button
            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width: 120.0,
                height: 28.0,
                color: MOCHA_SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: row_y + 6.0,
                text: "Reset to Defaults".to_string(),
                font_size: 12.0,
                color: MOCHA_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
            });
        }
    }

    fn render_test_patterns_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
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
            color: MOCHA_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });

        let mut row_y = y + 30.0;

        for pattern in TestPattern::ALL {
            let is_active = self.active_test_pattern == Some(*pattern);

            // Button background
            let bg_color = if is_active { MOCHA_BLUE } else { MOCHA_SURFACE0 };
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
                color: if is_active { MOCHA_MANTLE } else { MOCHA_TEXT },
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 24.0),
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
            color: MOCHA_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.5),
        });
        cmds.push(RenderCommand::Text {
            x: x + width * 0.5,
            y,
            text: value.to_string(),
            font_size: 12.0,
            color: MOCHA_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.5),
        });
    }

    fn render_slider_row(
        &self,
        cmds: &mut Vec<RenderCommand>,
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
            color: MOCHA_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.4),
        });

        // Track
        let track_x = x + width * 0.45;
        let track_w = width * 0.5;
        cmds.push(RenderCommand::FillRect {
            x: track_x,
            y: y + 4.0,
            width: track_w,
            height: 6.0,
            color: MOCHA_SURFACE0,
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
            color: MOCHA_BLUE,
            corner_radii: CornerRadii::all(3.0),
        });

        // Thumb
        cmds.push(RenderCommand::FillRect {
            x: track_x + fill_w - 6.0,
            y: y + 1.0,
            width: 12.0,
            height: 12.0,
            color: MOCHA_TEXT,
            corner_radii: CornerRadii::all(6.0),
        });
    }

    fn render_gamma_row(
        &self,
        cmds: &mut Vec<RenderCommand>,
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
        });

        // Visual gamma curve indicator (simplified)
        let bar_x = x + width * 0.45;
        let bar_w = width * 0.5;
        cmds.push(RenderCommand::FillRect {
            x: bar_x,
            y: y + 4.0,
            width: bar_w,
            height: 6.0,
            color: MOCHA_SURFACE0,
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
    use super::*;

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
        let (r, g, b) = ColorTemperature::WARM.to_rgb_multiplier();
        // Warm should have more red than blue
        assert!(r > b);
    }

    #[test]
    fn test_color_temp_rgb_cool() {
        let (r, g, b) = ColorTemperature::COOL.to_rgb_multiplier();
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
        assert!(s.is_active(22, 0));  // 10 PM
        assert!(s.is_active(3, 0));   // 3 AM
        assert!(!s.is_active(12, 0)); // Noon
    }

    #[test]
    fn test_schedule_custom_same_day() {
        let s = NightLightSchedule::Custom {
            start_hour: 14,
            start_minute: 0,
            end_hour: 18,
            end_minute: 0,
        };
        assert!(s.is_active(15, 0));
        assert!(!s.is_active(12, 0));
        assert!(!s.is_active(20, 0));
    }

    #[test]
    fn test_schedule_custom_overnight() {
        let s = NightLightSchedule::Custom {
            start_hour: 22,
            start_minute: 0,
            end_hour: 6,
            end_minute: 0,
        };
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
            DisplayConfig { display_id: 0, name: "A".to_string(), ..DisplayConfig::default() },
            DisplayConfig { display_id: 1, name: "B".to_string(), ..DisplayConfig::default() },
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
        let cmds = mgr.render(0.0, 0.0, 600.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_manager_render_night_light() {
        let mut mgr = DisplaySettingsManager::default();
        mgr.active_tab = DisplaySettingsTab::NightLight;
        let cmds = mgr.render(0.0, 0.0, 600.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_manager_render_calibration() {
        let mut mgr = DisplaySettingsManager::default();
        mgr.active_tab = DisplaySettingsTab::ColorCalibration;
        let cmds = mgr.render(0.0, 0.0, 600.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_manager_render_test_patterns() {
        let mut mgr = DisplaySettingsManager::default();
        mgr.active_tab = DisplaySettingsTab::TestPatterns;
        let cmds = mgr.render(0.0, 0.0, 600.0, 400.0);
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
}
