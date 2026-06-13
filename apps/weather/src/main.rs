//! SlateOS Weather Application
//!
//! A weather dashboard application providing:
//! - Current weather conditions with detailed metrics
//! - Hourly forecast (24 hours) with horizontal strip layout
//! - 7-day daily forecast in a table layout
//! - Weather alerts with severity-based banner display
//! - Multiple saved locations with default selection
//! - Temperature graph (line chart of hourly temps)
//! - Settings for units (temperature, wind, pressure, time)
//! - Air quality index with color-coded display
//!
//! All weather data is simulated locally (no network required).
//! Uses the guitk library for rendering.

#![allow(dead_code)]

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
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

// ============================================================================
// Weather Conditions
// ============================================================================

/// All possible weather conditions the app can display.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WeatherCondition {
    Clear,
    PartlyCloudy,
    Cloudy,
    Overcast,
    LightRain,
    Rain,
    HeavyRain,
    Thunderstorm,
    Snow,
    LightSnow,
    Sleet,
    Fog,
    Haze,
    Windy,
    Tornado,
    Hurricane,
}

impl WeatherCondition {
    /// Human-readable description of the condition.
    pub fn description(self) -> &'static str {
        match self {
            Self::Clear => "Clear sky",
            Self::PartlyCloudy => "Partly cloudy",
            Self::Cloudy => "Cloudy",
            Self::Overcast => "Overcast",
            Self::LightRain => "Light rain",
            Self::Rain => "Rain",
            Self::HeavyRain => "Heavy rain",
            Self::Thunderstorm => "Thunderstorm",
            Self::Snow => "Snow",
            Self::LightSnow => "Light snow",
            Self::Sleet => "Sleet",
            Self::Fog => "Fog",
            Self::Haze => "Haze",
            Self::Windy => "Windy",
            Self::Tornado => "Tornado",
            Self::Hurricane => "Hurricane",
        }
    }

    /// Returns ASCII-art lines representing this weather condition for
    /// icon rendering.
    pub fn icon_lines(self) -> &'static [&'static str] {
        match self {
            Self::Clear => &[
                r"    \   /    ",
                r"     .-.     ",
                r"  - (   ) -  ",
                r"     `-'     ",
                r"    /   \    ",
            ],
            Self::PartlyCloudy => &[
                r"   \  /      ",
                r" _ /''.--.   ",
                r"   \_(    ). ",
                r"   /(___(__) ",
                r"             ",
            ],
            Self::Cloudy | Self::Overcast => &[
                r"             ",
                r"     .--.    ",
                r"  .-(    ).  ",
                r" (___.__)__) ",
                r"             ",
            ],
            Self::LightRain => &[
                r"     .--.    ",
                r"  .-(    ).  ",
                r" (___.__)__) ",
                r"  ' ' ' '   ",
                r"             ",
            ],
            Self::Rain => &[
                r"     .--.    ",
                r"  .-(    ).  ",
                r" (___.__)__) ",
                r" /' /' /' /  ",
                r"             ",
            ],
            Self::HeavyRain => &[
                r"     .--.    ",
                r"  .-(    ).  ",
                r" (___.__)__) ",
                r" /'/'/'/'/   ",
                r" /'/'/'/     ",
            ],
            Self::Thunderstorm => &[
                r"     .--.    ",
                r"  .-(    ).  ",
                r" (___.__)__) ",
                r"   /_/ /_/   ",
                r"    /  /     ",
            ],
            Self::Snow => &[
                r"     .--.    ",
                r"  .-(    ).  ",
                r" (___.__)__) ",
                r"  * * * *   ",
                r"   * * *    ",
            ],
            Self::LightSnow => &[
                r"     .--.    ",
                r"  .-(    ).  ",
                r" (___.__)__) ",
                r"   *   *    ",
                r"             ",
            ],
            Self::Sleet => &[
                r"     .--.    ",
                r"  .-(    ).  ",
                r" (___.__)__) ",
                r"  ' * ' *   ",
                r"             ",
            ],
            Self::Fog => &[
                r"             ",
                r" _ - _ - _ - ",
                r"  _ - _ - _  ",
                r" _ - _ - _ - ",
                r"             ",
            ],
            Self::Haze => &[
                r"             ",
                r"  - - - - -  ",
                r"   - - - -   ",
                r"  - - - - -  ",
                r"             ",
            ],
            Self::Windy => &[
                r"             ",
                r"  ~~~~       ",
                r"    ~~~~~    ",
                r"  ~~~~~~     ",
                r"             ",
            ],
            Self::Tornado => &[
                r"     .--.    ",
                r"  .-(    ).  ",
                r"    \\||//   ",
                r"     \\|/    ",
                r"      ||     ",
            ],
            Self::Hurricane => &[
                r"     .--.    ",
                r"  .-(    ).  ",
                r" (___@__)__) ",
                r" /'/'/'/'/   ",
                r"  ~~~~       ",
            ],
        }
    }

    /// Color hint for the weather condition icon.
    pub fn icon_color(self) -> Color {
        match self {
            Self::Clear => YELLOW,
            Self::PartlyCloudy => SUBTEXT1,
            Self::Cloudy | Self::Overcast | Self::Haze => OVERLAY0,
            Self::LightRain | Self::Rain | Self::HeavyRain => BLUE,
            Self::Thunderstorm => PEACH,
            Self::Snow | Self::LightSnow | Self::Sleet => LAVENDER,
            Self::Fog => SURFACE2,
            Self::Windy => SUBTEXT0,
            Self::Tornado | Self::Hurricane => RED,
        }
    }
}

// ============================================================================
// Wind direction
// ============================================================================

/// Compass wind direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindDirection {
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
    NW,
}

impl WindDirection {
    /// Abbreviation for display.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::N => "N",
            Self::NE => "NE",
            Self::E => "E",
            Self::SE => "SE",
            Self::S => "S",
            Self::SW => "SW",
            Self::W => "W",
            Self::NW => "NW",
        }
    }

    /// Convert a degree heading (0-359) to compass direction.
    pub fn from_degrees(deg: u16) -> Self {
        let normalized = deg % 360;
        match normalized {
            0..=22 | 338..=359 => Self::N,
            23..=67 => Self::NE,
            68..=112 => Self::E,
            113..=157 => Self::SE,
            158..=202 => Self::S,
            203..=247 => Self::SW,
            248..=292 => Self::W,
            293..=337 => Self::NW,
            _ => Self::N,
        }
    }
}

// ============================================================================
// Units & Settings
// ============================================================================

/// Temperature unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TempUnit {
    Celsius,
    Fahrenheit,
}

/// Wind speed unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindSpeedUnit {
    Kmh,
    Mph,
    Ms,
    Knots,
}

impl WindSpeedUnit {
    pub fn label(self) -> &'static str {
        match self {
            Self::Kmh => "km/h",
            Self::Mph => "mph",
            Self::Ms => "m/s",
            Self::Knots => "kn",
        }
    }
}

/// Pressure unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PressureUnit {
    Hpa,
    InHg,
    MmHg,
}

impl PressureUnit {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hpa => "hPa",
            Self::InHg => "inHg",
            Self::MmHg => "mmHg",
        }
    }
}

/// Time format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeFormat {
    H12,
    H24,
}

/// Application settings.
#[derive(Clone, Debug)]
pub struct Settings {
    pub temp_unit: TempUnit,
    pub wind_unit: WindSpeedUnit,
    pub pressure_unit: PressureUnit,
    pub time_format: TimeFormat,
    pub update_interval_min: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            temp_unit: TempUnit::Celsius,
            wind_unit: WindSpeedUnit::Kmh,
            pressure_unit: PressureUnit::Hpa,
            time_format: TimeFormat::H24,
            update_interval_min: 30,
        }
    }
}

// ============================================================================
// UV Index
// ============================================================================

/// UV index severity label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UvSeverity {
    Low,
    Moderate,
    High,
    VeryHigh,
    Extreme,
}

impl UvSeverity {
    /// Classify a numeric UV index.
    pub fn from_index(index: u8) -> Self {
        match index {
            0..=2 => Self::Low,
            3..=5 => Self::Moderate,
            6..=7 => Self::High,
            8..=10 => Self::VeryHigh,
            _ => Self::Extreme,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Moderate => "Moderate",
            Self::High => "High",
            Self::VeryHigh => "Very High",
            Self::Extreme => "Extreme",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Low => GREEN,
            Self::Moderate => YELLOW,
            Self::High => PEACH,
            Self::VeryHigh => RED,
            Self::Extreme => Color::from_hex(0xCBA6F7), // Mauve
        }
    }
}

// ============================================================================
// Air Quality Index
// ============================================================================

/// Air quality category per the AQI scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirQuality {
    Good,
    Moderate,
    UnhealthySensitive,
    Unhealthy,
    VeryUnhealthy,
    Hazardous,
}

impl AirQuality {
    /// Classify a numeric AQI value.
    pub fn from_aqi(aqi: u16) -> Self {
        match aqi {
            0..=50 => Self::Good,
            51..=100 => Self::Moderate,
            101..=150 => Self::UnhealthySensitive,
            151..=200 => Self::Unhealthy,
            201..=300 => Self::VeryUnhealthy,
            _ => Self::Hazardous,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Good => "Good",
            Self::Moderate => "Moderate",
            Self::UnhealthySensitive => "Unhealthy for Sensitive",
            Self::Unhealthy => "Unhealthy",
            Self::VeryUnhealthy => "Very Unhealthy",
            Self::Hazardous => "Hazardous",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Good => GREEN,
            Self::Moderate => YELLOW,
            Self::UnhealthySensitive => PEACH,
            Self::Unhealthy => RED,
            Self::VeryUnhealthy => Color::from_hex(0xCBA6F7),
            Self::Hazardous => Color::from_hex(0x7F1D1D),
        }
    }
}

// ============================================================================
// Weather Alerts
// ============================================================================

/// Type of weather alert.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertType {
    Thunderstorm,
    Tornado,
    Flood,
    Heat,
    Cold,
    Wind,
    Snow,
    Ice,
    Fog,
}

impl AlertType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Thunderstorm => "Thunderstorm",
            Self::Tornado => "Tornado",
            Self::Flood => "Flood",
            Self::Heat => "Heat",
            Self::Cold => "Cold",
            Self::Wind => "Wind",
            Self::Snow => "Snow",
            Self::Ice => "Ice",
            Self::Fog => "Fog",
        }
    }
}

/// Alert severity level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AlertSeverity {
    Advisory,
    Watch,
    Warning,
}

impl AlertSeverity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Advisory => "Advisory",
            Self::Watch => "Watch",
            Self::Warning => "Warning",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Advisory => YELLOW,
            Self::Watch => PEACH,
            Self::Warning => RED,
        }
    }
}

/// A weather alert.
#[derive(Clone, Debug)]
pub struct WeatherAlert {
    pub alert_type: AlertType,
    pub severity: AlertSeverity,
    pub title: String,
    pub description: String,
}

// ============================================================================
// Data Models
// ============================================================================

/// Current weather data for a location.
#[derive(Clone, Debug)]
pub struct CurrentWeather {
    pub temp_c: f32,
    pub feels_like_c: f32,
    pub condition: WeatherCondition,
    pub humidity_pct: u8,
    pub dew_point_c: f32,
    pub wind_speed_kmh: f32,
    pub wind_dir: WindDirection,
    pub pressure_hpa: f32,
    pub visibility_km: f32,
    pub uv_index: u8,
    pub sunrise: (u8, u8),
    pub sunset: (u8, u8),
    pub aqi: u16,
}

/// One hour of hourly forecast data.
#[derive(Clone, Debug)]
pub struct HourForecast {
    pub hour: u8,
    pub temp_c: f32,
    pub condition: WeatherCondition,
    pub precip_pct: u8,
}

/// One day of daily forecast data.
#[derive(Clone, Debug)]
pub struct DayForecast {
    pub day_name: String,
    pub high_c: f32,
    pub low_c: f32,
    pub condition: WeatherCondition,
    pub precip_pct: u8,
    pub wind_speed_kmh: f32,
    pub wind_dir: WindDirection,
}

/// A saved location.
#[derive(Clone, Debug)]
pub struct Location {
    pub name: String,
    pub is_default: bool,
}

// ============================================================================
// Unit Conversion Helpers
// ============================================================================

/// Convert Celsius to Fahrenheit.
pub fn c_to_f(c: f32) -> f32 {
    c * 9.0 / 5.0 + 32.0
}

/// Convert km/h to mph.
pub fn kmh_to_mph(kmh: f32) -> f32 {
    kmh * 0.621_371
}

/// Convert km/h to m/s.
pub fn kmh_to_ms(kmh: f32) -> f32 {
    kmh / 3.6
}

/// Convert km/h to knots.
pub fn kmh_to_knots(kmh: f32) -> f32 {
    kmh * 0.539_957
}

/// Convert km to miles.
pub fn km_to_miles(km: f32) -> f32 {
    km * 0.621_371
}

/// Convert hPa to inHg.
pub fn hpa_to_inhg(hpa: f32) -> f32 {
    hpa * 0.029_53
}

/// Convert hPa to mmHg.
pub fn hpa_to_mmhg(hpa: f32) -> f32 {
    hpa * 0.750_062
}

/// Format temperature with the given unit.
pub fn format_temp(c: f32, unit: TempUnit) -> String {
    // Round half away from zero (e.g. 22.5 -> 23) rather than relying on the
    // formatter's round-half-to-even, which surprises users (22.5 -> 22).
    match unit {
        TempUnit::Celsius => format!("{:.0}\u{00B0}C", c.round()),
        TempUnit::Fahrenheit => format!("{:.0}\u{00B0}F", c_to_f(c).round()),
    }
}

/// Format wind speed with the given unit.
pub fn format_wind(kmh: f32, unit: WindSpeedUnit) -> String {
    match unit {
        WindSpeedUnit::Kmh => format!("{:.0} km/h", kmh),
        WindSpeedUnit::Mph => format!("{:.0} mph", kmh_to_mph(kmh)),
        WindSpeedUnit::Ms => format!("{:.1} m/s", kmh_to_ms(kmh)),
        WindSpeedUnit::Knots => format!("{:.0} kn", kmh_to_knots(kmh)),
    }
}

/// Format pressure with the given unit.
pub fn format_pressure(hpa: f32, unit: PressureUnit) -> String {
    match unit {
        PressureUnit::Hpa => format!("{:.0} hPa", hpa),
        PressureUnit::InHg => format!("{:.2} inHg", hpa_to_inhg(hpa)),
        PressureUnit::MmHg => format!("{:.0} mmHg", hpa_to_mmhg(hpa)),
    }
}

/// Format visibility with the temperature unit (metric vs imperial).
pub fn format_visibility(km: f32, unit: TempUnit) -> String {
    match unit {
        TempUnit::Celsius => format!("{:.1} km", km),
        TempUnit::Fahrenheit => format!("{:.1} mi", km_to_miles(km)),
    }
}

/// Format hour in the given time format.
pub fn format_hour(hour: u8, fmt: TimeFormat) -> String {
    match fmt {
        TimeFormat::H24 => format!("{hour:02}:00"),
        TimeFormat::H12 => {
            let period = if hour < 12 { "AM" } else { "PM" };
            let h12 = match hour {
                0 => 12,
                13..=23 => hour - 12,
                _ => hour,
            };
            format!("{h12}:00 {period}")
        }
    }
}

/// Format sunrise/sunset time.
pub fn format_time(h: u8, m: u8, fmt: TimeFormat) -> String {
    match fmt {
        TimeFormat::H24 => format!("{h:02}:{m:02}"),
        TimeFormat::H12 => {
            let period = if h < 12 { "AM" } else { "PM" };
            let h12 = match h {
                0 => 12,
                13..=23 => h - 12,
                _ => h,
            };
            format!("{h12}:{m:02} {period}")
        }
    }
}

// ============================================================================
// Sample data generation
// ============================================================================

/// Generate sample current weather data.
pub fn sample_current_weather() -> CurrentWeather {
    CurrentWeather {
        temp_c: 22.5,
        feels_like_c: 24.0,
        condition: WeatherCondition::PartlyCloudy,
        humidity_pct: 58,
        dew_point_c: 13.8,
        wind_speed_kmh: 15.0,
        wind_dir: WindDirection::SW,
        pressure_hpa: 1013.25,
        visibility_km: 10.0,
        uv_index: 5,
        sunrise: (6, 15),
        sunset: (20, 45),
        aqi: 42,
    }
}

/// Generate sample hourly forecast (24 hours).
pub fn sample_hourly_forecast() -> Vec<HourForecast> {
    let temps = [
        18.0, 17.5, 17.0, 16.5, 16.0, 16.5, 17.0, 18.5, 20.0, 21.5, 23.0, 24.0, 24.5, 25.0, 24.5,
        24.0, 23.0, 22.0, 21.0, 20.0, 19.0, 18.5, 18.0, 17.5,
    ];
    let conditions = [
        WeatherCondition::Clear,
        WeatherCondition::Clear,
        WeatherCondition::Clear,
        WeatherCondition::Clear,
        WeatherCondition::Clear,
        WeatherCondition::Clear,
        WeatherCondition::PartlyCloudy,
        WeatherCondition::PartlyCloudy,
        WeatherCondition::PartlyCloudy,
        WeatherCondition::Cloudy,
        WeatherCondition::Cloudy,
        WeatherCondition::Cloudy,
        WeatherCondition::PartlyCloudy,
        WeatherCondition::PartlyCloudy,
        WeatherCondition::LightRain,
        WeatherCondition::LightRain,
        WeatherCondition::Rain,
        WeatherCondition::Rain,
        WeatherCondition::LightRain,
        WeatherCondition::Cloudy,
        WeatherCondition::Cloudy,
        WeatherCondition::PartlyCloudy,
        WeatherCondition::Clear,
        WeatherCondition::Clear,
    ];
    let precips = [
        0, 0, 0, 0, 0, 0, 5, 5, 10, 10, 15, 15, 10, 10, 40, 50, 70, 65, 40, 20, 10, 5, 0, 0,
    ];
    (0..24)
        .map(|i| HourForecast {
            hour: i as u8,
            temp_c: temps[i],
            condition: conditions[i],
            precip_pct: precips[i],
        })
        .collect()
}

/// Generate sample 7-day daily forecast.
pub fn sample_daily_forecast() -> Vec<DayForecast> {
    let days = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    let highs = [25.0, 27.0, 23.0, 20.0, 22.0, 26.0, 28.0];
    let lows = [15.0, 16.0, 14.0, 12.0, 13.0, 15.0, 17.0];
    let conditions = [
        WeatherCondition::PartlyCloudy,
        WeatherCondition::Clear,
        WeatherCondition::Rain,
        WeatherCondition::Cloudy,
        WeatherCondition::LightRain,
        WeatherCondition::Clear,
        WeatherCondition::Clear,
    ];
    let precips: [u8; 7] = [10, 0, 80, 30, 45, 0, 5];
    let winds = [12.0, 8.0, 25.0, 18.0, 15.0, 10.0, 7.0];
    let dirs = [
        WindDirection::SW,
        WindDirection::S,
        WindDirection::W,
        WindDirection::NW,
        WindDirection::N,
        WindDirection::SE,
        WindDirection::E,
    ];
    (0..7)
        .map(|i| DayForecast {
            day_name: days[i].to_string(),
            high_c: highs[i],
            low_c: lows[i],
            condition: conditions[i],
            precip_pct: precips[i],
            wind_speed_kmh: winds[i],
            wind_dir: dirs[i],
        })
        .collect()
}

/// Generate sample alerts.
pub fn sample_alerts() -> Vec<WeatherAlert> {
    vec![WeatherAlert {
        alert_type: AlertType::Thunderstorm,
        severity: AlertSeverity::Watch,
        title: "Thunderstorm Watch".to_string(),
        description: "Thunderstorms expected this afternoon. Stay alert.".to_string(),
    }]
}

/// Default saved locations.
pub fn default_locations() -> Vec<Location> {
    vec![
        Location {
            name: "New York, NY".to_string(),
            is_default: true,
        },
        Location {
            name: "London, UK".to_string(),
            is_default: false,
        },
        Location {
            name: "Tokyo, JP".to_string(),
            is_default: false,
        },
    ]
}

// ============================================================================
// Application State
// ============================================================================

/// Active tab / view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveView {
    Dashboard,
    HourlyDetail,
    DailyDetail,
    Alerts,
    Locations,
    SettingsView,
}

/// Main application state.
pub struct WeatherApp {
    pub current: CurrentWeather,
    pub hourly: Vec<HourForecast>,
    pub daily: Vec<DayForecast>,
    pub alerts: Vec<WeatherAlert>,
    pub locations: Vec<Location>,
    pub active_location_idx: usize,
    pub settings: Settings,
    pub active_view: ActiveView,
    pub hourly_scroll_offset: f32,
    pub width: f32,
    pub height: f32,
}

impl WeatherApp {
    /// Create a new app with sample data.
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            current: sample_current_weather(),
            hourly: sample_hourly_forecast(),
            daily: sample_daily_forecast(),
            alerts: sample_alerts(),
            locations: default_locations(),
            active_location_idx: 0,
            settings: Settings::default(),
            active_view: ActiveView::Dashboard,
            hourly_scroll_offset: 0.0,
            width,
            height,
        }
    }

    /// Get the name of the currently active location.
    pub fn active_location_name(&self) -> &str {
        self.locations
            .get(self.active_location_idx)
            .map(|l| l.name.as_str())
            .unwrap_or("Unknown")
    }

    /// Switch to a different location by index.
    pub fn set_active_location(&mut self, idx: usize) {
        if idx < self.locations.len() {
            self.active_location_idx = idx;
        }
    }

    /// Add a new location.
    pub fn add_location(&mut self, name: String) {
        let is_default = self.locations.is_empty();
        self.locations.push(Location { name, is_default });
    }

    /// Remove a location by index. Returns true if removed.
    pub fn remove_location(&mut self, idx: usize) -> bool {
        if idx >= self.locations.len() {
            return false;
        }
        let was_default = self.locations[idx].is_default;
        self.locations.remove(idx);
        // If we removed the active location, clamp the index
        if self.active_location_idx >= self.locations.len() && !self.locations.is_empty() {
            self.active_location_idx = self.locations.len() - 1;
        }
        // If we removed the default, promote the first location
        if was_default && let Some(loc) = self.locations.first_mut() {
            loc.is_default = true;
        }
        true
    }

    /// Reorder a location: move from `from` to `to`.
    pub fn reorder_location(&mut self, from: usize, to: usize) -> bool {
        if from >= self.locations.len() || to >= self.locations.len() {
            return false;
        }
        let item = self.locations.remove(from);
        self.locations.insert(to, item);
        // Update active index to follow the item if it was selected
        if self.active_location_idx == from {
            self.active_location_idx = to;
        } else if from < self.active_location_idx && to >= self.active_location_idx {
            self.active_location_idx = self.active_location_idx.saturating_sub(1);
        } else if from > self.active_location_idx && to <= self.active_location_idx {
            self.active_location_idx = self
                .active_location_idx
                .saturating_add(1)
                .min(self.locations.len().saturating_sub(1));
        }
        true
    }

    /// Set the default location by index.
    pub fn set_default_location(&mut self, idx: usize) -> bool {
        if idx >= self.locations.len() {
            return false;
        }
        for (i, loc) in self.locations.iter_mut().enumerate() {
            loc.is_default = i == idx;
        }
        true
    }

    /// Toggle temperature unit between C and F.
    pub fn toggle_temp_unit(&mut self) {
        self.settings.temp_unit = match self.settings.temp_unit {
            TempUnit::Celsius => TempUnit::Fahrenheit,
            TempUnit::Fahrenheit => TempUnit::Celsius,
        };
    }

    /// Cycle wind speed unit.
    pub fn cycle_wind_unit(&mut self) {
        self.settings.wind_unit = match self.settings.wind_unit {
            WindSpeedUnit::Kmh => WindSpeedUnit::Mph,
            WindSpeedUnit::Mph => WindSpeedUnit::Ms,
            WindSpeedUnit::Ms => WindSpeedUnit::Knots,
            WindSpeedUnit::Knots => WindSpeedUnit::Kmh,
        };
    }

    /// Cycle pressure unit.
    pub fn cycle_pressure_unit(&mut self) {
        self.settings.pressure_unit = match self.settings.pressure_unit {
            PressureUnit::Hpa => PressureUnit::InHg,
            PressureUnit::InHg => PressureUnit::MmHg,
            PressureUnit::MmHg => PressureUnit::Hpa,
        };
    }

    /// Toggle time format.
    pub fn toggle_time_format(&mut self) {
        self.settings.time_format = match self.settings.time_format {
            TimeFormat::H12 => TimeFormat::H24,
            TimeFormat::H24 => TimeFormat::H12,
        };
    }

    /// Set update interval (clamped to 5..=120 minutes).
    pub fn set_update_interval(&mut self, minutes: u32) {
        self.settings.update_interval_min = minutes.clamp(5, 120);
    }

    /// Scroll the hourly strip left/right.
    pub fn scroll_hourly(&mut self, delta: f32) {
        self.hourly_scroll_offset = (self.hourly_scroll_offset + delta).max(0.0);
        let max_scroll = (self.hourly.len() as f32 * 80.0).max(0.0);
        if self.hourly_scroll_offset > max_scroll {
            self.hourly_scroll_offset = max_scroll;
        }
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the entire weather application into render commands.
    pub fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Alert banner (if any)
        let content_y = self.render_alerts_banner(&mut cmds, 0.0);

        // Title bar
        let title_y = self.render_title_bar(&mut cmds, content_y);

        // Main content area depends on active view
        match self.active_view {
            ActiveView::Dashboard => self.render_dashboard(&mut cmds, title_y),
            ActiveView::HourlyDetail => self.render_hourly_detail(&mut cmds, title_y),
            ActiveView::DailyDetail => self.render_daily_detail(&mut cmds, title_y),
            ActiveView::Alerts => self.render_alerts_view(&mut cmds, title_y),
            ActiveView::Locations => self.render_locations_view(&mut cmds, title_y),
            ActiveView::SettingsView => self.render_settings_view(&mut cmds, title_y),
        }

        cmds
    }

    /// Render alert banner at the top. Returns the Y position after the banner.
    fn render_alerts_banner(&self, cmds: &mut Vec<RenderCommand>, y: f32) -> f32 {
        if self.alerts.is_empty() {
            return y;
        }

        let banner_height = 36.0;
        for (i, alert) in self.alerts.iter().enumerate() {
            let by = y + i as f32 * banner_height;
            let bg_color = alert.severity.color();

            cmds.push(RenderCommand::FillRect {
                x: 0.0,
                y: by,
                width: self.width,
                height: banner_height,
                color: Color::rgba(bg_color.r, bg_color.g, bg_color.b, 40),
                corner_radii: CornerRadii::ZERO,
            });

            // Left accent bar
            cmds.push(RenderCommand::FillRect {
                x: 0.0,
                y: by,
                width: 4.0,
                height: banner_height,
                color: bg_color,
                corner_radii: CornerRadii::ZERO,
            });

            // Severity icon placeholder
            cmds.push(RenderCommand::Text {
                x: 12.0,
                y: by + 10.0,
                text: format!(
                    "[{}] {} - {}",
                    alert.severity.label(),
                    alert.title,
                    alert.description,
                ),
                font_size: 13.0,
                color: bg_color,
                font_weight: FontWeightHint::Bold,
                max_width: Some(self.width - 24.0),
            });
        }

        y + self.alerts.len() as f32 * banner_height
    }

    /// Render the title bar. Returns the Y position after the title.
    fn render_title_bar(&self, cmds: &mut Vec<RenderCommand>, y: f32) -> f32 {
        let title_height = 50.0;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.width,
            height: title_height,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // App title
        cmds.push(RenderCommand::Text {
            x: 16.0,
            y: y + 15.0,
            text: "Weather".to_string(),
            font_size: 20.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Active location
        cmds.push(RenderCommand::Text {
            x: 120.0,
            y: y + 19.0,
            text: self.active_location_name().to_string(),
            font_size: 14.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Nav tabs
        let tabs = [
            ("Dashboard", ActiveView::Dashboard),
            ("Hourly", ActiveView::HourlyDetail),
            ("7-Day", ActiveView::DailyDetail),
            ("Alerts", ActiveView::Alerts),
            ("Locations", ActiveView::Locations),
            ("Settings", ActiveView::SettingsView),
        ];
        let mut tx = self.width - 16.0;
        for (label, view) in tabs.iter().rev() {
            let text_width = label.len() as f32 * 7.5;
            tx -= text_width + 16.0;
            let is_active = *view == self.active_view;

            if is_active {
                cmds.push(RenderCommand::FillRect {
                    x: tx - 4.0,
                    y: y + 8.0,
                    width: text_width + 24.0,
                    height: 30.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(6.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: tx + 8.0,
                y: y + 16.0,
                text: label.to_string(),
                font_size: 13.0,
                color: if is_active { BLUE } else { SUBTEXT0 },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
            });
        }

        y + title_height
    }

    /// Render the dashboard view (main overview).
    fn render_dashboard(&self, cmds: &mut Vec<RenderCommand>, y: f32) {
        let padding = 16.0;
        let mut cy = y + padding;

        // Current weather card
        cy = self.render_current_weather_card(cmds, padding, cy);
        cy += padding;

        // Hourly strip
        cy = self.render_hourly_strip(cmds, padding, cy);
        cy += padding;

        // Temperature graph
        cy = self.render_temp_graph(cmds, padding, cy);
        cy += padding;

        // Daily forecast table
        cy = self.render_daily_table(cmds, padding, cy);
        cy += padding;

        // Air quality card
        self.render_air_quality_card(cmds, padding, cy);
    }

    /// Render the current weather card. Returns the Y after the card.
    fn render_current_weather_card(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32) -> f32 {
        let card_w = self.width - x * 2.0;
        let card_h = 200.0;

        // Card shadow
        cmds.push(RenderCommand::BoxShadow {
            x,
            y,
            width: card_w,
            height: card_h,
            offset_x: 0.0,
            offset_y: 2.0,
            blur: 8.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 40),
            corner_radii: CornerRadii::all(12.0),
        });

        // Card background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: card_w,
            height: card_h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(12.0),
        });

        let inner_x = x + 20.0;
        let inner_y = y + 16.0;

        // Section label
        cmds.push(RenderCommand::Text {
            x: inner_x,
            y: inner_y,
            text: "Current Weather".to_string(),
            font_size: 12.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Large temperature
        cmds.push(RenderCommand::Text {
            x: inner_x,
            y: inner_y + 24.0,
            text: format_temp(self.current.temp_c, self.settings.temp_unit),
            font_size: 48.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Feels like
        cmds.push(RenderCommand::Text {
            x: inner_x,
            y: inner_y + 80.0,
            text: format!(
                "Feels like {}",
                format_temp(self.current.feels_like_c, self.settings.temp_unit)
            ),
            font_size: 13.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Condition text
        cmds.push(RenderCommand::Text {
            x: inner_x,
            y: inner_y + 100.0,
            text: self.current.condition.description().to_string(),
            font_size: 14.0,
            color: SUBTEXT1,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Weather icon (ASCII art rendered as text lines)
        let icon_x = inner_x + 200.0;
        let icon_color = self.current.condition.icon_color();
        for (i, line) in self.current.condition.icon_lines().iter().enumerate() {
            cmds.push(RenderCommand::Text {
                x: icon_x,
                y: inner_y + 30.0 + i as f32 * 16.0,
                text: line.to_string(),
                font_size: 14.0,
                color: icon_color,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Details grid (right side of card)
        let detail_x = icon_x + 180.0;
        let details = self.current_weather_details();
        for (i, (label, value)) in details.iter().enumerate() {
            let row = i / 2;
            let col = i % 2;
            let dx = detail_x + col as f32 * 160.0;
            let dy = inner_y + 16.0 + row as f32 * 36.0;

            cmds.push(RenderCommand::Text {
                x: dx,
                y: dy,
                text: label.to_string(),
                font_size: 11.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cmds.push(RenderCommand::Text {
                x: dx,
                y: dy + 14.0,
                text: value.to_string(),
                font_size: 13.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }

        y + card_h
    }

    /// Collect current weather detail label-value pairs.
    fn current_weather_details(&self) -> Vec<(&'static str, String)> {
        let uv_severity = UvSeverity::from_index(self.current.uv_index);
        vec![
            ("Humidity", format!("{}%", self.current.humidity_pct)),
            (
                "Wind",
                format!(
                    "{} {}",
                    format_wind(self.current.wind_speed_kmh, self.settings.wind_unit),
                    self.current.wind_dir.as_str()
                ),
            ),
            (
                "Pressure",
                format_pressure(self.current.pressure_hpa, self.settings.pressure_unit),
            ),
            (
                "Visibility",
                format_visibility(self.current.visibility_km, self.settings.temp_unit),
            ),
            (
                "Dew Point",
                format_temp(self.current.dew_point_c, self.settings.temp_unit),
            ),
            (
                "UV Index",
                format!("{} ({})", self.current.uv_index, uv_severity.label()),
            ),
            (
                "Sunrise",
                format_time(
                    self.current.sunrise.0,
                    self.current.sunrise.1,
                    self.settings.time_format,
                ),
            ),
            (
                "Sunset",
                format_time(
                    self.current.sunset.0,
                    self.current.sunset.1,
                    self.settings.time_format,
                ),
            ),
        ]
    }

    /// Render the hourly forecast strip. Returns Y after.
    fn render_hourly_strip(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32) -> f32 {
        let strip_w = self.width - x * 2.0;
        let strip_h = 120.0;
        let item_w = 72.0;
        let item_gap = 8.0;

        // Card
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: strip_w,
            height: strip_h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(12.0),
        });

        // Section label
        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: y + 12.0,
            text: "Hourly Forecast".to_string(),
            font_size: 12.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Clip the scrollable area
        let scroll_y = y + 30.0;
        let scroll_h = strip_h - 34.0;
        cmds.push(RenderCommand::PushClip {
            x: x + 8.0,
            y: scroll_y,
            width: strip_w - 16.0,
            height: scroll_h,
        });

        for (i, hf) in self.hourly.iter().enumerate() {
            let ix = x + 12.0 + i as f32 * (item_w + item_gap) - self.hourly_scroll_offset;

            // Skip if off-screen
            if ix + item_w < x || ix > x + strip_w {
                continue;
            }

            // Item background
            cmds.push(RenderCommand::FillRect {
                x: ix,
                y: scroll_y + 4.0,
                width: item_w,
                height: scroll_h - 8.0,
                color: SURFACE1,
                corner_radii: CornerRadii::all(8.0),
            });

            // Hour label
            cmds.push(RenderCommand::Text {
                x: ix + 8.0,
                y: scroll_y + 10.0,
                text: format_hour(hf.hour, self.settings.time_format),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(item_w - 16.0),
            });

            // Temperature
            cmds.push(RenderCommand::Text {
                x: ix + 8.0,
                y: scroll_y + 28.0,
                text: format_temp(hf.temp_c, self.settings.temp_unit),
                font_size: 16.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(item_w - 16.0),
            });

            // Condition mini icon (first line of ASCII art)
            let icon_lines = hf.condition.icon_lines();
            if let Some(first_line) = icon_lines.first() {
                cmds.push(RenderCommand::Text {
                    x: ix + 4.0,
                    y: scroll_y + 48.0,
                    text: first_line.to_string(),
                    font_size: 9.0,
                    color: hf.condition.icon_color(),
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(item_w - 8.0),
                });
            }

            // Precipitation chance
            if hf.precip_pct > 0 {
                cmds.push(RenderCommand::Text {
                    x: ix + 8.0,
                    y: scroll_y + 64.0,
                    text: format!("{}%", hf.precip_pct),
                    font_size: 11.0,
                    color: BLUE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(item_w - 16.0),
                });
            }
        }

        cmds.push(RenderCommand::PopClip);

        y + strip_h
    }

    /// Render the temperature graph. Returns Y after.
    fn render_temp_graph(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32) -> f32 {
        let graph_w = self.width - x * 2.0;
        let graph_h = 160.0;
        let plot_x = x + 50.0;
        let plot_y = y + 36.0;
        let plot_w = graph_w - 70.0;
        let plot_h = graph_h - 56.0;

        // Card
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: graph_w,
            height: graph_h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(12.0),
        });

        // Section label
        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: y + 12.0,
            text: "Temperature (24h)".to_string(),
            font_size: 12.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        if self.hourly.is_empty() {
            return y + graph_h;
        }

        // Find temp range
        let min_temp = self
            .hourly
            .iter()
            .map(|h| h.temp_c)
            .fold(f32::INFINITY, f32::min);
        let max_temp = self
            .hourly
            .iter()
            .map(|h| h.temp_c)
            .fold(f32::NEG_INFINITY, f32::max);
        let temp_range = (max_temp - min_temp).max(1.0);

        // Y-axis labels
        for i in 0..=4 {
            let frac = i as f32 / 4.0;
            let temp = min_temp + temp_range * (1.0 - frac);
            let ly = plot_y + plot_h * frac;

            cmds.push(RenderCommand::Text {
                x: x + 8.0,
                y: ly - 6.0,
                text: format_temp(temp, self.settings.temp_unit),
                font_size: 10.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(40.0),
            });

            // Grid line
            cmds.push(RenderCommand::Line {
                x1: plot_x,
                y1: ly,
                x2: plot_x + plot_w,
                y2: ly,
                color: Color::rgba(OVERLAY0.r, OVERLAY0.g, OVERLAY0.b, 30),
                width: 1.0,
            });
        }

        // Plot line segments
        let point_count = self.hourly.len();
        if point_count >= 2 {
            let step = plot_w / (point_count as f32 - 1.0);
            for i in 0..point_count - 1 {
                let t1 = self.hourly[i].temp_c;
                let t2 = self.hourly[i + 1].temp_c;
                let x1 = plot_x + i as f32 * step;
                let y1 = plot_y + plot_h * (1.0 - (t1 - min_temp) / temp_range);
                let x2 = plot_x + (i + 1) as f32 * step;
                let y2 = plot_y + plot_h * (1.0 - (t2 - min_temp) / temp_range);

                cmds.push(RenderCommand::Line {
                    x1,
                    y1,
                    x2,
                    y2,
                    color: BLUE,
                    width: 2.0,
                });
            }
        }

        // X-axis hour labels (every 4 hours)
        let point_count_f = point_count as f32;
        let step = if point_count > 1 {
            plot_w / (point_count_f - 1.0)
        } else {
            0.0
        };
        for i in (0..point_count).step_by(4) {
            let lx = plot_x + i as f32 * step;
            cmds.push(RenderCommand::Text {
                x: lx - 10.0,
                y: plot_y + plot_h + 6.0,
                text: format_hour(self.hourly[i].hour, self.settings.time_format),
                font_size: 10.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(40.0),
            });
        }

        y + graph_h
    }

    /// Render the daily forecast table. Returns Y after.
    fn render_daily_table(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32) -> f32 {
        let table_w = self.width - x * 2.0;
        let header_h = 34.0;
        let row_h = 36.0;
        let table_h = header_h + self.daily.len() as f32 * row_h + 16.0;

        // Card
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: table_w,
            height: table_h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(12.0),
        });

        // Section label
        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: y + 12.0,
            text: "7-Day Forecast".to_string(),
            font_size: 12.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Column positions
        let col_day = x + 16.0;
        let col_cond = x + 80.0;
        let col_high = x + 220.0;
        let col_low = x + 300.0;
        let col_precip = x + 380.0;
        let col_wind = x + 450.0;

        // Header row
        let hy = y + header_h;
        let headers = [
            (col_day, "Day"),
            (col_cond, "Condition"),
            (col_high, "High"),
            (col_low, "Low"),
            (col_precip, "Precip"),
            (col_wind, "Wind"),
        ];
        for (hx, label) in &headers {
            cmds.push(RenderCommand::Text {
                x: *hx,
                y: hy,
                text: label.to_string(),
                font_size: 11.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }

        // Divider line
        cmds.push(RenderCommand::Line {
            x1: x + 12.0,
            y1: hy + 16.0,
            x2: x + table_w - 12.0,
            y2: hy + 16.0,
            color: SURFACE1,
            width: 1.0,
        });

        // Data rows
        for (i, day) in self.daily.iter().enumerate() {
            let ry = hy + 22.0 + i as f32 * row_h;

            // Alternating row background
            if i % 2 == 1 {
                cmds.push(RenderCommand::FillRect {
                    x: x + 8.0,
                    y: ry - 4.0,
                    width: table_w - 16.0,
                    height: row_h,
                    color: Color::rgba(SURFACE1.r, SURFACE1.g, SURFACE1.b, 40),
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: col_day,
                y: ry,
                text: day.day_name.clone(),
                font_size: 13.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            cmds.push(RenderCommand::Text {
                x: col_cond,
                y: ry,
                text: day.condition.description().to_string(),
                font_size: 13.0,
                color: SUBTEXT1,
                font_weight: FontWeightHint::Regular,
                max_width: Some(130.0),
            });

            cmds.push(RenderCommand::Text {
                x: col_high,
                y: ry,
                text: format_temp(day.high_c, self.settings.temp_unit),
                font_size: 13.0,
                color: PEACH,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            cmds.push(RenderCommand::Text {
                x: col_low,
                y: ry,
                text: format_temp(day.low_c, self.settings.temp_unit),
                font_size: 13.0,
                color: BLUE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            cmds.push(RenderCommand::Text {
                x: col_precip,
                y: ry,
                text: format!("{}%", day.precip_pct),
                font_size: 13.0,
                color: if day.precip_pct > 50 { BLUE } else { SUBTEXT0 },
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            cmds.push(RenderCommand::Text {
                x: col_wind,
                y: ry,
                text: format!(
                    "{} {}",
                    format_wind(day.wind_speed_kmh, self.settings.wind_unit),
                    day.wind_dir.as_str()
                ),
                font_size: 13.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(120.0),
            });
        }

        y + table_h
    }

    /// Render the air quality card. Returns Y after.
    fn render_air_quality_card(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32) -> f32 {
        let card_w = self.width - x * 2.0;
        let card_h = 80.0;
        let aq = AirQuality::from_aqi(self.current.aqi);

        // Card
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: card_w,
            height: card_h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(12.0),
        });

        // Section label
        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: y + 12.0,
            text: "Air Quality".to_string(),
            font_size: 12.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // AQI number
        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: y + 34.0,
            text: format!("AQI: {}", self.current.aqi),
            font_size: 24.0,
            color: aq.color(),
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Category label
        cmds.push(RenderCommand::Text {
            x: x + 140.0,
            y: y + 40.0,
            text: aq.label().to_string(),
            font_size: 16.0,
            color: aq.color(),
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Color-coded bar
        let bar_x = x + 16.0;
        let bar_y = y + 62.0;
        let bar_w = card_w - 32.0;
        let bar_h = 6.0;

        // Background bar
        cmds.push(RenderCommand::FillRect {
            x: bar_x,
            y: bar_y,
            width: bar_w,
            height: bar_h,
            color: SURFACE1,
            corner_radii: CornerRadii::all(3.0),
        });

        // Filled portion (AQI 0-500 scale)
        let fill_frac = (self.current.aqi as f32 / 500.0).min(1.0);
        let fill_w = bar_w * fill_frac;
        if fill_w > 0.0 {
            cmds.push(RenderCommand::FillRect {
                x: bar_x,
                y: bar_y,
                width: fill_w,
                height: bar_h,
                color: aq.color(),
                corner_radii: CornerRadii::all(3.0),
            });
        }

        y + card_h
    }

    /// Render hourly detail view.
    fn render_hourly_detail(&self, cmds: &mut Vec<RenderCommand>, y: f32) {
        let padding = 16.0;
        let mut cy = y + padding;

        cmds.push(RenderCommand::Text {
            x: padding,
            y: cy,
            text: "Hourly Forecast Detail".to_string(),
            font_size: 18.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cy += 30.0;

        let col_hour = padding;
        let col_temp = padding + 100.0;
        let col_cond = padding + 200.0;
        let col_precip = padding + 380.0;

        // Header
        let headers = [
            (col_hour, "Hour"),
            (col_temp, "Temp"),
            (col_cond, "Condition"),
            (col_precip, "Precip %"),
        ];
        for (hx, label) in &headers {
            cmds.push(RenderCommand::Text {
                x: *hx,
                y: cy,
                text: label.to_string(),
                font_size: 12.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
        cy += 20.0;

        cmds.push(RenderCommand::Line {
            x1: padding,
            y1: cy,
            x2: self.width - padding,
            y2: cy,
            color: SURFACE1,
            width: 1.0,
        });
        cy += 8.0;

        for hf in &self.hourly {
            cmds.push(RenderCommand::Text {
                x: col_hour,
                y: cy,
                text: format_hour(hf.hour, self.settings.time_format),
                font_size: 13.0,
                color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cmds.push(RenderCommand::Text {
                x: col_temp,
                y: cy,
                text: format_temp(hf.temp_c, self.settings.temp_unit),
                font_size: 13.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            cmds.push(RenderCommand::Text {
                x: col_cond,
                y: cy,
                text: hf.condition.description().to_string(),
                font_size: 13.0,
                color: SUBTEXT1,
                font_weight: FontWeightHint::Regular,
                max_width: Some(170.0),
            });
            cmds.push(RenderCommand::Text {
                x: col_precip,
                y: cy,
                text: format!("{}%", hf.precip_pct),
                font_size: 13.0,
                color: if hf.precip_pct > 30 { BLUE } else { SUBTEXT0 },
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cy += 24.0;
        }
    }

    /// Render daily detail view.
    fn render_daily_detail(&self, cmds: &mut Vec<RenderCommand>, y: f32) {
        let padding = 16.0;
        let mut cy = y + padding;

        cmds.push(RenderCommand::Text {
            x: padding,
            y: cy,
            text: "7-Day Forecast Detail".to_string(),
            font_size: 18.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cy += 36.0;

        for day in &self.daily {
            // Day card
            let card_h = 100.0;
            cmds.push(RenderCommand::FillRect {
                x: padding,
                y: cy,
                width: self.width - padding * 2.0,
                height: card_h,
                color: SURFACE0,
                corner_radii: CornerRadii::all(10.0),
            });

            cmds.push(RenderCommand::Text {
                x: padding + 16.0,
                y: cy + 12.0,
                text: day.day_name.clone(),
                font_size: 16.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            cmds.push(RenderCommand::Text {
                x: padding + 16.0,
                y: cy + 36.0,
                text: day.condition.description().to_string(),
                font_size: 13.0,
                color: SUBTEXT1,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            cmds.push(RenderCommand::Text {
                x: padding + 200.0,
                y: cy + 12.0,
                text: format!(
                    "H: {}  L: {}",
                    format_temp(day.high_c, self.settings.temp_unit),
                    format_temp(day.low_c, self.settings.temp_unit)
                ),
                font_size: 14.0,
                color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            cmds.push(RenderCommand::Text {
                x: padding + 200.0,
                y: cy + 36.0,
                text: format!(
                    "Precip: {}%  Wind: {} {}",
                    day.precip_pct,
                    format_wind(day.wind_speed_kmh, self.settings.wind_unit),
                    day.wind_dir.as_str()
                ),
                font_size: 13.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(self.width - padding * 2.0 - 220.0),
            });

            // Render condition icon lines
            let icon_x = self.width - padding - 160.0;
            let icon_color = day.condition.icon_color();
            for (j, line) in day.condition.icon_lines().iter().enumerate() {
                cmds.push(RenderCommand::Text {
                    x: icon_x,
                    y: cy + 14.0 + j as f32 * 14.0,
                    text: line.to_string(),
                    font_size: 11.0,
                    color: icon_color,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }

            cy += card_h + 12.0;
        }
    }

    /// Render alerts view.
    fn render_alerts_view(&self, cmds: &mut Vec<RenderCommand>, y: f32) {
        let padding = 16.0;
        let mut cy = y + padding;

        cmds.push(RenderCommand::Text {
            x: padding,
            y: cy,
            text: "Weather Alerts".to_string(),
            font_size: 18.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cy += 36.0;

        if self.alerts.is_empty() {
            cmds.push(RenderCommand::Text {
                x: padding,
                y: cy,
                text: "No active alerts.".to_string(),
                font_size: 14.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            return;
        }

        for alert in &self.alerts {
            let card_h = 90.0;
            let severity_color = alert.severity.color();

            // Card background
            cmds.push(RenderCommand::FillRect {
                x: padding,
                y: cy,
                width: self.width - padding * 2.0,
                height: card_h,
                color: SURFACE0,
                corner_radii: CornerRadii::all(10.0),
            });

            // Severity stripe
            cmds.push(RenderCommand::FillRect {
                x: padding,
                y: cy,
                width: 5.0,
                height: card_h,
                color: severity_color,
                corner_radii: CornerRadii {
                    top_left: 10.0,
                    top_right: 0.0,
                    bottom_right: 0.0,
                    bottom_left: 10.0,
                },
            });

            cmds.push(RenderCommand::Text {
                x: padding + 20.0,
                y: cy + 12.0,
                text: format!(
                    "{} {} - {}",
                    alert.alert_type.label(),
                    alert.severity.label(),
                    alert.title,
                ),
                font_size: 15.0,
                color: severity_color,
                font_weight: FontWeightHint::Bold,
                max_width: Some(self.width - padding * 2.0 - 40.0),
            });

            cmds.push(RenderCommand::Text {
                x: padding + 20.0,
                y: cy + 40.0,
                text: alert.description.clone(),
                font_size: 13.0,
                color: SUBTEXT1,
                font_weight: FontWeightHint::Regular,
                max_width: Some(self.width - padding * 2.0 - 40.0),
            });

            cy += card_h + 12.0;
        }
    }

    /// Render locations view.
    fn render_locations_view(&self, cmds: &mut Vec<RenderCommand>, y: f32) {
        let padding = 16.0;
        let mut cy = y + padding;

        cmds.push(RenderCommand::Text {
            x: padding,
            y: cy,
            text: "Saved Locations".to_string(),
            font_size: 18.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cy += 36.0;

        for (i, loc) in self.locations.iter().enumerate() {
            let row_h = 48.0;
            let is_active = i == self.active_location_idx;

            // Row background
            cmds.push(RenderCommand::FillRect {
                x: padding,
                y: cy,
                width: self.width - padding * 2.0,
                height: row_h,
                color: if is_active { SURFACE1 } else { SURFACE0 },
                corner_radii: CornerRadii::all(8.0),
            });

            // Active indicator
            if is_active {
                cmds.push(RenderCommand::FillRect {
                    x: padding,
                    y: cy,
                    width: 4.0,
                    height: row_h,
                    color: BLUE,
                    corner_radii: CornerRadii {
                        top_left: 8.0,
                        top_right: 0.0,
                        bottom_right: 0.0,
                        bottom_left: 8.0,
                    },
                });
            }

            // Location name
            cmds.push(RenderCommand::Text {
                x: padding + 20.0,
                y: cy + 8.0,
                text: loc.name.clone(),
                font_size: 15.0,
                color: TEXT,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(self.width - padding * 2.0 - 100.0),
            });

            // Default badge
            if loc.is_default {
                cmds.push(RenderCommand::FillRect {
                    x: padding + 20.0,
                    y: cy + 28.0,
                    width: 56.0,
                    height: 16.0,
                    color: Color::rgba(BLUE.r, BLUE.g, BLUE.b, 40),
                    corner_radii: CornerRadii::all(4.0),
                });
                cmds.push(RenderCommand::Text {
                    x: padding + 26.0,
                    y: cy + 30.0,
                    text: "Default".to_string(),
                    font_size: 10.0,
                    color: BLUE,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }

            cy += row_h + 8.0;
        }
    }

    /// Render settings view.
    fn render_settings_view(&self, cmds: &mut Vec<RenderCommand>, y: f32) {
        let padding = 16.0;
        let mut cy = y + padding;

        cmds.push(RenderCommand::Text {
            x: padding,
            y: cy,
            text: "Settings".to_string(),
            font_size: 18.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cy += 36.0;

        let settings_items: Vec<(&str, String)> = vec![
            (
                "Temperature Unit",
                match self.settings.temp_unit {
                    TempUnit::Celsius => "Celsius (\u{00B0}C)".to_string(),
                    TempUnit::Fahrenheit => "Fahrenheit (\u{00B0}F)".to_string(),
                },
            ),
            (
                "Wind Speed Unit",
                self.settings.wind_unit.label().to_string(),
            ),
            (
                "Pressure Unit",
                self.settings.pressure_unit.label().to_string(),
            ),
            (
                "Time Format",
                match self.settings.time_format {
                    TimeFormat::H12 => "12-hour".to_string(),
                    TimeFormat::H24 => "24-hour".to_string(),
                },
            ),
            (
                "Update Interval",
                format!("{} min", self.settings.update_interval_min),
            ),
        ];

        for (label, value) in &settings_items {
            let row_h = 50.0;

            cmds.push(RenderCommand::FillRect {
                x: padding,
                y: cy,
                width: self.width - padding * 2.0,
                height: row_h,
                color: SURFACE0,
                corner_radii: CornerRadii::all(8.0),
            });

            cmds.push(RenderCommand::Text {
                x: padding + 16.0,
                y: cy + 10.0,
                text: label.to_string(),
                font_size: 13.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            cmds.push(RenderCommand::Text {
                x: padding + 16.0,
                y: cy + 28.0,
                text: value.clone(),
                font_size: 15.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            cy += row_h + 8.0;
        }
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let app = WeatherApp::new(900.0, 800.0);
    let cmds = app.render();

    // In the actual OS, these commands would be submitted to the compositor.
    // For now, we just verify the app produces valid render output.
    let _cmd_count = cmds.len();
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- WeatherCondition tests ---

    #[test]
    fn test_condition_description() {
        assert_eq!(WeatherCondition::Clear.description(), "Clear sky");
        assert_eq!(WeatherCondition::Thunderstorm.description(), "Thunderstorm");
        assert_eq!(WeatherCondition::Hurricane.description(), "Hurricane");
    }

    #[test]
    fn test_condition_icon_lines_nonempty() {
        let conditions = [
            WeatherCondition::Clear,
            WeatherCondition::PartlyCloudy,
            WeatherCondition::Cloudy,
            WeatherCondition::Overcast,
            WeatherCondition::LightRain,
            WeatherCondition::Rain,
            WeatherCondition::HeavyRain,
            WeatherCondition::Thunderstorm,
            WeatherCondition::Snow,
            WeatherCondition::LightSnow,
            WeatherCondition::Sleet,
            WeatherCondition::Fog,
            WeatherCondition::Haze,
            WeatherCondition::Windy,
            WeatherCondition::Tornado,
            WeatherCondition::Hurricane,
        ];
        for cond in &conditions {
            let lines = cond.icon_lines();
            assert!(!lines.is_empty(), "{cond:?} should have icon lines");
            assert_eq!(lines.len(), 5, "{cond:?} should have 5 icon lines");
        }
    }

    #[test]
    fn test_condition_icon_color_is_opaque() {
        let conditions = [
            WeatherCondition::Clear,
            WeatherCondition::Rain,
            WeatherCondition::Tornado,
        ];
        for cond in &conditions {
            let c = cond.icon_color();
            assert_eq!(c.a, 255, "{cond:?} icon color should be fully opaque");
        }
    }

    #[test]
    fn test_condition_cloudy_and_overcast_share_icon() {
        let cloudy = WeatherCondition::Cloudy.icon_lines();
        let overcast = WeatherCondition::Overcast.icon_lines();
        assert_eq!(cloudy, overcast);
    }

    // --- WindDirection tests ---

    #[test]
    fn test_wind_dir_as_str() {
        assert_eq!(WindDirection::N.as_str(), "N");
        assert_eq!(WindDirection::NE.as_str(), "NE");
        assert_eq!(WindDirection::SW.as_str(), "SW");
    }

    #[test]
    fn test_wind_dir_from_degrees_north() {
        assert_eq!(WindDirection::from_degrees(0), WindDirection::N);
        assert_eq!(WindDirection::from_degrees(10), WindDirection::N);
        assert_eq!(WindDirection::from_degrees(350), WindDirection::N);
        assert_eq!(WindDirection::from_degrees(360), WindDirection::N);
    }

    #[test]
    fn test_wind_dir_from_degrees_all() {
        assert_eq!(WindDirection::from_degrees(45), WindDirection::NE);
        assert_eq!(WindDirection::from_degrees(90), WindDirection::E);
        assert_eq!(WindDirection::from_degrees(135), WindDirection::SE);
        assert_eq!(WindDirection::from_degrees(180), WindDirection::S);
        assert_eq!(WindDirection::from_degrees(225), WindDirection::SW);
        assert_eq!(WindDirection::from_degrees(270), WindDirection::W);
        assert_eq!(WindDirection::from_degrees(315), WindDirection::NW);
    }

    #[test]
    fn test_wind_dir_from_degrees_wraps() {
        assert_eq!(WindDirection::from_degrees(720), WindDirection::N);
        assert_eq!(WindDirection::from_degrees(450), WindDirection::E);
    }

    // --- Unit conversion tests ---

    #[test]
    fn test_c_to_f_freezing() {
        let f = c_to_f(0.0);
        assert!((f - 32.0).abs() < 0.01);
    }

    #[test]
    fn test_c_to_f_boiling() {
        let f = c_to_f(100.0);
        assert!((f - 212.0).abs() < 0.01);
    }

    #[test]
    fn test_c_to_f_body_temp() {
        let f = c_to_f(37.0);
        assert!((f - 98.6).abs() < 0.1);
    }

    #[test]
    fn test_kmh_to_mph() {
        let mph = kmh_to_mph(100.0);
        assert!((mph - 62.14).abs() < 0.1);
    }

    #[test]
    fn test_kmh_to_ms() {
        let ms = kmh_to_ms(36.0);
        assert!((ms - 10.0).abs() < 0.01);
    }

    #[test]
    fn test_kmh_to_knots() {
        let kn = kmh_to_knots(100.0);
        assert!((kn - 54.0).abs() < 0.1);
    }

    #[test]
    fn test_km_to_miles() {
        let mi = km_to_miles(1.609);
        assert!((mi - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_hpa_to_inhg() {
        let inhg = hpa_to_inhg(1013.25);
        assert!((inhg - 29.92).abs() < 0.1);
    }

    #[test]
    fn test_hpa_to_mmhg() {
        let mmhg = hpa_to_mmhg(1013.25);
        assert!((mmhg - 760.0).abs() < 1.0);
    }

    // --- Format helpers tests ---

    #[test]
    fn test_format_temp_celsius() {
        let s = format_temp(22.5, TempUnit::Celsius);
        assert!(s.contains("23")); // rounded
        assert!(s.contains("\u{00B0}C"));
    }

    #[test]
    fn test_format_temp_fahrenheit() {
        let s = format_temp(0.0, TempUnit::Fahrenheit);
        assert!(s.contains("32"));
        assert!(s.contains("\u{00B0}F"));
    }

    #[test]
    fn test_format_wind_kmh() {
        let s = format_wind(15.0, WindSpeedUnit::Kmh);
        assert_eq!(s, "15 km/h");
    }

    #[test]
    fn test_format_wind_mph() {
        let s = format_wind(100.0, WindSpeedUnit::Mph);
        assert!(s.contains("mph"));
    }

    #[test]
    fn test_format_wind_ms() {
        let s = format_wind(36.0, WindSpeedUnit::Ms);
        assert!(s.contains("10.0 m/s"));
    }

    #[test]
    fn test_format_wind_knots() {
        let s = format_wind(100.0, WindSpeedUnit::Knots);
        assert!(s.contains("kn"));
    }

    #[test]
    fn test_format_pressure_hpa() {
        let s = format_pressure(1013.0, PressureUnit::Hpa);
        assert_eq!(s, "1013 hPa");
    }

    #[test]
    fn test_format_pressure_inhg() {
        let s = format_pressure(1013.25, PressureUnit::InHg);
        assert!(s.contains("inHg"));
    }

    #[test]
    fn test_format_pressure_mmhg() {
        let s = format_pressure(1013.25, PressureUnit::MmHg);
        assert!(s.contains("mmHg"));
    }

    #[test]
    fn test_format_visibility_metric() {
        let s = format_visibility(10.0, TempUnit::Celsius);
        assert_eq!(s, "10.0 km");
    }

    #[test]
    fn test_format_visibility_imperial() {
        let s = format_visibility(10.0, TempUnit::Fahrenheit);
        assert!(s.contains("mi"));
    }

    #[test]
    fn test_format_hour_24h() {
        assert_eq!(format_hour(0, TimeFormat::H24), "00:00");
        assert_eq!(format_hour(13, TimeFormat::H24), "13:00");
        assert_eq!(format_hour(23, TimeFormat::H24), "23:00");
    }

    #[test]
    fn test_format_hour_12h() {
        assert_eq!(format_hour(0, TimeFormat::H12), "12:00 AM");
        assert_eq!(format_hour(12, TimeFormat::H12), "12:00 PM");
        assert_eq!(format_hour(13, TimeFormat::H12), "1:00 PM");
        assert_eq!(format_hour(23, TimeFormat::H12), "11:00 PM");
    }

    #[test]
    fn test_format_time_24h() {
        assert_eq!(format_time(6, 15, TimeFormat::H24), "06:15");
        assert_eq!(format_time(20, 45, TimeFormat::H24), "20:45");
    }

    #[test]
    fn test_format_time_12h() {
        assert_eq!(format_time(6, 15, TimeFormat::H12), "6:15 AM");
        assert_eq!(format_time(20, 45, TimeFormat::H12), "8:45 PM");
    }

    #[test]
    fn test_format_time_12h_midnight() {
        assert_eq!(format_time(0, 0, TimeFormat::H12), "12:00 AM");
    }

    #[test]
    fn test_format_time_12h_noon() {
        assert_eq!(format_time(12, 0, TimeFormat::H12), "12:00 PM");
    }

    // --- UV Severity tests ---

    #[test]
    fn test_uv_severity_low() {
        assert_eq!(UvSeverity::from_index(0), UvSeverity::Low);
        assert_eq!(UvSeverity::from_index(2), UvSeverity::Low);
    }

    #[test]
    fn test_uv_severity_moderate() {
        assert_eq!(UvSeverity::from_index(3), UvSeverity::Moderate);
        assert_eq!(UvSeverity::from_index(5), UvSeverity::Moderate);
    }

    #[test]
    fn test_uv_severity_high() {
        assert_eq!(UvSeverity::from_index(6), UvSeverity::High);
        assert_eq!(UvSeverity::from_index(7), UvSeverity::High);
    }

    #[test]
    fn test_uv_severity_very_high() {
        assert_eq!(UvSeverity::from_index(8), UvSeverity::VeryHigh);
        assert_eq!(UvSeverity::from_index(10), UvSeverity::VeryHigh);
    }

    #[test]
    fn test_uv_severity_extreme() {
        assert_eq!(UvSeverity::from_index(11), UvSeverity::Extreme);
        assert_eq!(UvSeverity::from_index(15), UvSeverity::Extreme);
    }

    #[test]
    fn test_uv_severity_labels() {
        assert_eq!(UvSeverity::Low.label(), "Low");
        assert_eq!(UvSeverity::Extreme.label(), "Extreme");
    }

    #[test]
    fn test_uv_severity_colors_are_opaque() {
        for sev in &[
            UvSeverity::Low,
            UvSeverity::Moderate,
            UvSeverity::High,
            UvSeverity::VeryHigh,
            UvSeverity::Extreme,
        ] {
            assert_eq!(sev.color().a, 255);
        }
    }

    // --- Air Quality tests ---

    #[test]
    fn test_aqi_good() {
        assert_eq!(AirQuality::from_aqi(0), AirQuality::Good);
        assert_eq!(AirQuality::from_aqi(50), AirQuality::Good);
    }

    #[test]
    fn test_aqi_moderate() {
        assert_eq!(AirQuality::from_aqi(51), AirQuality::Moderate);
        assert_eq!(AirQuality::from_aqi(100), AirQuality::Moderate);
    }

    #[test]
    fn test_aqi_unhealthy_sensitive() {
        assert_eq!(AirQuality::from_aqi(101), AirQuality::UnhealthySensitive);
        assert_eq!(AirQuality::from_aqi(150), AirQuality::UnhealthySensitive);
    }

    #[test]
    fn test_aqi_unhealthy() {
        assert_eq!(AirQuality::from_aqi(151), AirQuality::Unhealthy);
        assert_eq!(AirQuality::from_aqi(200), AirQuality::Unhealthy);
    }

    #[test]
    fn test_aqi_very_unhealthy() {
        assert_eq!(AirQuality::from_aqi(201), AirQuality::VeryUnhealthy);
        assert_eq!(AirQuality::from_aqi(300), AirQuality::VeryUnhealthy);
    }

    #[test]
    fn test_aqi_hazardous() {
        assert_eq!(AirQuality::from_aqi(301), AirQuality::Hazardous);
        assert_eq!(AirQuality::from_aqi(500), AirQuality::Hazardous);
    }

    #[test]
    fn test_aqi_labels() {
        assert_eq!(AirQuality::Good.label(), "Good");
        assert_eq!(
            AirQuality::UnhealthySensitive.label(),
            "Unhealthy for Sensitive"
        );
        assert_eq!(AirQuality::Hazardous.label(), "Hazardous");
    }

    #[test]
    fn test_aqi_colors_are_opaque() {
        for aq in &[
            AirQuality::Good,
            AirQuality::Moderate,
            AirQuality::UnhealthySensitive,
            AirQuality::Unhealthy,
            AirQuality::VeryUnhealthy,
            AirQuality::Hazardous,
        ] {
            assert_eq!(aq.color().a, 255);
        }
    }

    // --- Alert tests ---

    #[test]
    fn test_alert_type_labels() {
        assert_eq!(AlertType::Thunderstorm.label(), "Thunderstorm");
        assert_eq!(AlertType::Tornado.label(), "Tornado");
        assert_eq!(AlertType::Fog.label(), "Fog");
    }

    #[test]
    fn test_alert_severity_order() {
        assert!(AlertSeverity::Advisory < AlertSeverity::Watch);
        assert!(AlertSeverity::Watch < AlertSeverity::Warning);
    }

    #[test]
    fn test_alert_severity_labels() {
        assert_eq!(AlertSeverity::Advisory.label(), "Advisory");
        assert_eq!(AlertSeverity::Warning.label(), "Warning");
    }

    #[test]
    fn test_alert_severity_colors() {
        // Advisory = YELLOW, Watch = PEACH, Warning = RED
        assert_eq!(AlertSeverity::Advisory.color(), YELLOW);
        assert_eq!(AlertSeverity::Watch.color(), PEACH);
        assert_eq!(AlertSeverity::Warning.color(), RED);
    }

    // --- Sample data tests ---

    #[test]
    fn test_sample_current_weather_valid() {
        let cw = sample_current_weather();
        assert!(cw.humidity_pct <= 100);
        assert!(cw.uv_index <= 15);
        assert!(cw.sunrise.0 < 24 && cw.sunrise.1 < 60);
        assert!(cw.sunset.0 < 24 && cw.sunset.1 < 60);
    }

    #[test]
    fn test_sample_hourly_has_24_entries() {
        let hf = sample_hourly_forecast();
        assert_eq!(hf.len(), 24);
    }

    #[test]
    fn test_sample_hourly_hours_sequential() {
        let hf = sample_hourly_forecast();
        for (i, h) in hf.iter().enumerate() {
            assert_eq!(h.hour as usize, i);
        }
    }

    #[test]
    fn test_sample_daily_has_7_entries() {
        let df = sample_daily_forecast();
        assert_eq!(df.len(), 7);
    }

    #[test]
    fn test_sample_daily_high_gte_low() {
        let df = sample_daily_forecast();
        for day in &df {
            assert!(
                day.high_c >= day.low_c,
                "High ({}) should be >= Low ({})",
                day.high_c,
                day.low_c
            );
        }
    }

    #[test]
    fn test_sample_alerts_nonempty() {
        let alerts = sample_alerts();
        assert!(!alerts.is_empty());
    }

    #[test]
    fn test_default_locations() {
        let locs = default_locations();
        assert_eq!(locs.len(), 3);
        assert!(locs[0].is_default);
        assert!(!locs[1].is_default);
    }

    // --- Settings tests ---

    #[test]
    fn test_settings_default() {
        let s = Settings::default();
        assert_eq!(s.temp_unit, TempUnit::Celsius);
        assert_eq!(s.wind_unit, WindSpeedUnit::Kmh);
        assert_eq!(s.pressure_unit, PressureUnit::Hpa);
        assert_eq!(s.time_format, TimeFormat::H24);
        assert_eq!(s.update_interval_min, 30);
    }

    #[test]
    fn test_wind_unit_labels() {
        assert_eq!(WindSpeedUnit::Kmh.label(), "km/h");
        assert_eq!(WindSpeedUnit::Mph.label(), "mph");
        assert_eq!(WindSpeedUnit::Ms.label(), "m/s");
        assert_eq!(WindSpeedUnit::Knots.label(), "kn");
    }

    #[test]
    fn test_pressure_unit_labels() {
        assert_eq!(PressureUnit::Hpa.label(), "hPa");
        assert_eq!(PressureUnit::InHg.label(), "inHg");
        assert_eq!(PressureUnit::MmHg.label(), "mmHg");
    }

    // --- WeatherApp tests ---

    #[test]
    fn test_app_new() {
        let app = WeatherApp::new(800.0, 600.0);
        assert_eq!(app.width, 800.0);
        assert_eq!(app.height, 600.0);
        assert_eq!(app.active_view, ActiveView::Dashboard);
        assert_eq!(app.hourly_scroll_offset, 0.0);
    }

    #[test]
    fn test_app_active_location_name() {
        let app = WeatherApp::new(800.0, 600.0);
        assert_eq!(app.active_location_name(), "New York, NY");
    }

    #[test]
    fn test_app_set_active_location() {
        let mut app = WeatherApp::new(800.0, 600.0);
        app.set_active_location(1);
        assert_eq!(app.active_location_name(), "London, UK");
    }

    #[test]
    fn test_app_set_active_location_out_of_bounds() {
        let mut app = WeatherApp::new(800.0, 600.0);
        app.set_active_location(100);
        assert_eq!(app.active_location_idx, 0); // unchanged
    }

    #[test]
    fn test_app_add_location() {
        let mut app = WeatherApp::new(800.0, 600.0);
        let initial = app.locations.len();
        app.add_location("Paris, FR".to_string());
        assert_eq!(app.locations.len(), initial + 1);
        assert_eq!(
            app.locations.last().map(|l| l.name.as_str()),
            Some("Paris, FR")
        );
        assert!(!app.locations.last().map(|l| l.is_default).unwrap_or(true));
    }

    #[test]
    fn test_app_add_location_to_empty() {
        let mut app = WeatherApp::new(800.0, 600.0);
        app.locations.clear();
        app.add_location("Only City".to_string());
        assert!(app.locations[0].is_default);
    }

    #[test]
    fn test_app_remove_location() {
        let mut app = WeatherApp::new(800.0, 600.0);
        assert!(app.remove_location(1));
        assert_eq!(app.locations.len(), 2);
    }

    #[test]
    fn test_app_remove_location_out_of_bounds() {
        let mut app = WeatherApp::new(800.0, 600.0);
        assert!(!app.remove_location(100));
    }

    #[test]
    fn test_app_remove_default_promotes_first() {
        let mut app = WeatherApp::new(800.0, 600.0);
        // locations[0] is default
        app.remove_location(0);
        assert!(app.locations[0].is_default);
    }

    #[test]
    fn test_app_remove_active_clamps() {
        let mut app = WeatherApp::new(800.0, 600.0);
        app.set_active_location(2); // last
        app.remove_location(2);
        assert!(app.active_location_idx < app.locations.len());
    }

    #[test]
    fn test_app_reorder_location() {
        let mut app = WeatherApp::new(800.0, 600.0);
        assert!(app.reorder_location(0, 2));
        assert_eq!(app.locations[2].name, "New York, NY");
    }

    #[test]
    fn test_app_reorder_out_of_bounds() {
        let mut app = WeatherApp::new(800.0, 600.0);
        assert!(!app.reorder_location(0, 100));
    }

    #[test]
    fn test_app_set_default_location() {
        let mut app = WeatherApp::new(800.0, 600.0);
        assert!(app.set_default_location(2));
        assert!(app.locations[2].is_default);
        assert!(!app.locations[0].is_default);
    }

    #[test]
    fn test_app_set_default_out_of_bounds() {
        let mut app = WeatherApp::new(800.0, 600.0);
        assert!(!app.set_default_location(100));
    }

    #[test]
    fn test_app_toggle_temp_unit() {
        let mut app = WeatherApp::new(800.0, 600.0);
        assert_eq!(app.settings.temp_unit, TempUnit::Celsius);
        app.toggle_temp_unit();
        assert_eq!(app.settings.temp_unit, TempUnit::Fahrenheit);
        app.toggle_temp_unit();
        assert_eq!(app.settings.temp_unit, TempUnit::Celsius);
    }

    #[test]
    fn test_app_cycle_wind_unit() {
        let mut app = WeatherApp::new(800.0, 600.0);
        assert_eq!(app.settings.wind_unit, WindSpeedUnit::Kmh);
        app.cycle_wind_unit();
        assert_eq!(app.settings.wind_unit, WindSpeedUnit::Mph);
        app.cycle_wind_unit();
        assert_eq!(app.settings.wind_unit, WindSpeedUnit::Ms);
        app.cycle_wind_unit();
        assert_eq!(app.settings.wind_unit, WindSpeedUnit::Knots);
        app.cycle_wind_unit();
        assert_eq!(app.settings.wind_unit, WindSpeedUnit::Kmh);
    }

    #[test]
    fn test_app_cycle_pressure_unit() {
        let mut app = WeatherApp::new(800.0, 600.0);
        assert_eq!(app.settings.pressure_unit, PressureUnit::Hpa);
        app.cycle_pressure_unit();
        assert_eq!(app.settings.pressure_unit, PressureUnit::InHg);
        app.cycle_pressure_unit();
        assert_eq!(app.settings.pressure_unit, PressureUnit::MmHg);
        app.cycle_pressure_unit();
        assert_eq!(app.settings.pressure_unit, PressureUnit::Hpa);
    }

    #[test]
    fn test_app_toggle_time_format() {
        let mut app = WeatherApp::new(800.0, 600.0);
        assert_eq!(app.settings.time_format, TimeFormat::H24);
        app.toggle_time_format();
        assert_eq!(app.settings.time_format, TimeFormat::H12);
        app.toggle_time_format();
        assert_eq!(app.settings.time_format, TimeFormat::H24);
    }

    #[test]
    fn test_app_set_update_interval() {
        let mut app = WeatherApp::new(800.0, 600.0);
        app.set_update_interval(60);
        assert_eq!(app.settings.update_interval_min, 60);
    }

    #[test]
    fn test_app_set_update_interval_clamped_low() {
        let mut app = WeatherApp::new(800.0, 600.0);
        app.set_update_interval(1);
        assert_eq!(app.settings.update_interval_min, 5);
    }

    #[test]
    fn test_app_set_update_interval_clamped_high() {
        let mut app = WeatherApp::new(800.0, 600.0);
        app.set_update_interval(999);
        assert_eq!(app.settings.update_interval_min, 120);
    }

    #[test]
    fn test_app_scroll_hourly_positive() {
        let mut app = WeatherApp::new(800.0, 600.0);
        app.scroll_hourly(100.0);
        assert!((app.hourly_scroll_offset - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_app_scroll_hourly_no_negative() {
        let mut app = WeatherApp::new(800.0, 600.0);
        app.scroll_hourly(-100.0);
        assert_eq!(app.hourly_scroll_offset, 0.0);
    }

    #[test]
    fn test_app_scroll_hourly_capped() {
        let mut app = WeatherApp::new(800.0, 600.0);
        app.scroll_hourly(100_000.0);
        let max = app.hourly.len() as f32 * 80.0;
        assert!(app.hourly_scroll_offset <= max);
    }

    // --- Rendering tests ---

    #[test]
    fn test_render_produces_commands() {
        let app = WeatherApp::new(900.0, 800.0);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_starts_with_background() {
        let app = WeatherApp::new(900.0, 800.0);
        let cmds = app.render();
        match &cmds[0] {
            RenderCommand::FillRect { x, y, color, .. } => {
                assert_eq!(*x, 0.0);
                assert_eq!(*y, 0.0);
                assert_eq!(*color, BASE);
            }
            _ => panic!("First command should be a FillRect background"),
        }
    }

    #[test]
    fn test_render_has_text_commands() {
        let app = WeatherApp::new(900.0, 800.0);
        let cmds = app.render();
        let has_text = cmds.iter().any(|c| matches!(c, RenderCommand::Text { .. }));
        assert!(has_text, "Render output should contain text commands");
    }

    #[test]
    fn test_render_has_line_commands() {
        let app = WeatherApp::new(900.0, 800.0);
        let cmds = app.render();
        let has_lines = cmds.iter().any(|c| matches!(c, RenderCommand::Line { .. }));
        assert!(has_lines, "Dashboard should have line commands (graph)");
    }

    #[test]
    fn test_render_alert_banner_when_alerts() {
        let app = WeatherApp::new(900.0, 800.0);
        let cmds = app.render();
        // Should have at least one text command with severity label
        let has_alert_text = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text.contains("[Watch]")
            } else {
                false
            }
        });
        assert!(has_alert_text, "Should render alert banner text");
    }

    #[test]
    fn test_render_no_alert_banner_when_empty() {
        let mut app = WeatherApp::new(900.0, 800.0);
        app.alerts.clear();
        let cmds = app.render();
        let has_alert_text = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text.contains("[Watch]")
                    || text.contains("[Warning]")
                    || text.contains("[Advisory]")
            } else {
                false
            }
        });
        assert!(!has_alert_text, "Should not have alert text when no alerts");
    }

    #[test]
    fn test_render_dashboard_view() {
        let app = WeatherApp::new(900.0, 800.0);
        let cmds = app.render();
        let has_current = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text.contains("Current Weather")
            } else {
                false
            }
        });
        assert!(has_current, "Dashboard should show Current Weather label");
    }

    #[test]
    fn test_render_hourly_view() {
        let mut app = WeatherApp::new(900.0, 800.0);
        app.active_view = ActiveView::HourlyDetail;
        let cmds = app.render();
        let has_hourly_label = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text.contains("Hourly Forecast Detail")
            } else {
                false
            }
        });
        assert!(has_hourly_label);
    }

    #[test]
    fn test_render_daily_view() {
        let mut app = WeatherApp::new(900.0, 800.0);
        app.active_view = ActiveView::DailyDetail;
        let cmds = app.render();
        let has_daily = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text.contains("7-Day Forecast Detail")
            } else {
                false
            }
        });
        assert!(has_daily);
    }

    #[test]
    fn test_render_alerts_view_empty() {
        let mut app = WeatherApp::new(900.0, 800.0);
        app.active_view = ActiveView::Alerts;
        app.alerts.clear();
        let cmds = app.render();
        let has_no_alerts = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text.contains("No active alerts")
            } else {
                false
            }
        });
        assert!(has_no_alerts);
    }

    #[test]
    fn test_render_locations_view() {
        let mut app = WeatherApp::new(900.0, 800.0);
        app.active_view = ActiveView::Locations;
        let cmds = app.render();
        let has_locations = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text.contains("Saved Locations")
            } else {
                false
            }
        });
        assert!(has_locations);
    }

    #[test]
    fn test_render_settings_view() {
        let mut app = WeatherApp::new(900.0, 800.0);
        app.active_view = ActiveView::SettingsView;
        let cmds = app.render();
        let has_settings = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text == "Settings"
            } else {
                false
            }
        });
        assert!(has_settings);
    }

    #[test]
    fn test_render_settings_shows_units() {
        let mut app = WeatherApp::new(900.0, 800.0);
        app.active_view = ActiveView::SettingsView;
        let cmds = app.render();
        let has_temp_unit = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text.contains("Temperature Unit")
            } else {
                false
            }
        });
        assert!(has_temp_unit);
    }

    #[test]
    fn test_render_locations_shows_default_badge() {
        let mut app = WeatherApp::new(900.0, 800.0);
        app.active_view = ActiveView::Locations;
        let cmds = app.render();
        let has_default_badge = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text == "Default"
            } else {
                false
            }
        });
        assert!(has_default_badge);
    }

    #[test]
    fn test_render_daily_table_has_header_labels() {
        let app = WeatherApp::new(900.0, 800.0);
        let cmds = app.render();
        let headers = ["Day", "Condition", "High", "Low", "Precip", "Wind"];
        for hdr in &headers {
            let found = cmds.iter().any(|c| {
                if let RenderCommand::Text { text, .. } = c {
                    text == *hdr
                } else {
                    false
                }
            });
            assert!(found, "Should have header label: {hdr}");
        }
    }

    #[test]
    fn test_render_air_quality_shows_aqi() {
        let app = WeatherApp::new(900.0, 800.0);
        let cmds = app.render();
        let has_aqi = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text.contains("AQI:")
            } else {
                false
            }
        });
        assert!(has_aqi);
    }

    #[test]
    fn test_render_dashboard_box_shadow() {
        let app = WeatherApp::new(900.0, 800.0);
        let cmds = app.render();
        let has_shadow = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::BoxShadow { .. }));
        assert!(has_shadow, "Dashboard should have box shadow for cards");
    }

    #[test]
    fn test_render_hourly_strip_clipping() {
        let app = WeatherApp::new(900.0, 800.0);
        let cmds = app.render();
        let has_push_clip = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::PushClip { .. }));
        let has_pop_clip = cmds.iter().any(|c| matches!(c, RenderCommand::PopClip));
        assert!(has_push_clip, "Hourly strip should push clip");
        assert!(has_pop_clip, "Hourly strip should pop clip");
    }

    #[test]
    fn test_current_weather_details_count() {
        let app = WeatherApp::new(900.0, 800.0);
        let details = app.current_weather_details();
        assert_eq!(details.len(), 8); // 8 detail pairs
    }

    #[test]
    fn test_current_weather_details_labels() {
        let app = WeatherApp::new(900.0, 800.0);
        let details = app.current_weather_details();
        let labels: Vec<&str> = details.iter().map(|(l, _)| *l).collect();
        assert!(labels.contains(&"Humidity"));
        assert!(labels.contains(&"Wind"));
        assert!(labels.contains(&"Pressure"));
        assert!(labels.contains(&"Visibility"));
        assert!(labels.contains(&"Dew Point"));
        assert!(labels.contains(&"UV Index"));
        assert!(labels.contains(&"Sunrise"));
        assert!(labels.contains(&"Sunset"));
    }

    #[test]
    fn test_render_with_fahrenheit() {
        let mut app = WeatherApp::new(900.0, 800.0);
        app.settings.temp_unit = TempUnit::Fahrenheit;
        let cmds = app.render();
        let has_f = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text.contains("\u{00B0}F")
            } else {
                false
            }
        });
        assert!(has_f, "Should display Fahrenheit temperatures");
    }

    #[test]
    fn test_render_empty_hourly() {
        let mut app = WeatherApp::new(900.0, 800.0);
        app.hourly.clear();
        // Should not panic
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_empty_daily() {
        let mut app = WeatherApp::new(900.0, 800.0);
        app.daily.clear();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_single_hourly_entry() {
        let mut app = WeatherApp::new(900.0, 800.0);
        app.hourly = vec![HourForecast {
            hour: 12,
            temp_c: 20.0,
            condition: WeatherCondition::Clear,
            precip_pct: 0,
        }];
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_all_views_no_panic() {
        let views = [
            ActiveView::Dashboard,
            ActiveView::HourlyDetail,
            ActiveView::DailyDetail,
            ActiveView::Alerts,
            ActiveView::Locations,
            ActiveView::SettingsView,
        ];
        for view in &views {
            let mut app = WeatherApp::new(900.0, 800.0);
            app.active_view = *view;
            let cmds = app.render();
            assert!(
                !cmds.is_empty(),
                "View {view:?} should produce render commands"
            );
        }
    }
}
