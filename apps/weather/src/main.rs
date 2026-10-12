//! Slate OS Weather Application
//!
//! **Forecasts come from Open-Meteo, and only once the user turns them on.**
//! The operator's answer to E-Q2 (design-decisions §1236) is that no program
//! contacts a website by default unless that is what it is for. So the window
//! opens on `FORECASTS_OFF_LINES` -- what turning forecasts on sends, to whom,
//! when, and that it goes in plain text -- and a button that turns them on.
//! Nothing is sent before that press, or the Settings row that does the same;
//! [`source`] holds that rule and the requests, [`openmeteo`] what is asked
//! and how the replies read, and [`fetch`] the HTTP exchange itself.
//!
//! Turned on, the user searches for a place by name on the Locations tab
//! (Open-Meteo's geocoding), adds one of the places offered, and the window
//! asks for its forecast and air quality: when a place is added or chosen,
//! every half hour while the window is open, five minutes after a request
//! that failed, and when the user presses R. The places and the switch are
//! kept in the user's settings (`settingsfile`, `weather.yaml`) beside the
//! units, and a change made in one window reaches the others.
//!
//! **No panel shows a default that would be read as a reading.** With no
//! forecast in -- forecasts off, no place chosen, the request out, or failed
//! -- the window says which and returns before the dashboard; an index the
//! air-quality service did not give is "Not known", not 0. And it gives no
//! severe-weather warnings, since Open-Meteo has none to give: the Alerts tab
//! says so, because an empty alerts list is read as "no warnings in force".
//! (It used to open on invented weather for New York, London and Tokyo -- a
//! confident lie about something people make decisions with.)
//!
//! Every frame showing Open-Meteo's data credits it (CC BY 4.0), along the
//! bottom with the time the forecast is for. Every control answers the
//! pointer: the renderer records a hit box where it draws each one
//! (`guitk::frame::Frame`).
//!
//! What the window draws, once a forecast is in:
//! - Current conditions with detailed metrics
//! - Hourly forecast (24 hours) with horizontal strip layout
//! - 7-day daily forecast in a table layout
//! - Temperature graph (line chart of hourly temps)
//! - Air quality index with color-coded display
//! - Saved places, searched for by name, with a default
//! - Settings for units (temperature, wind, pressure, time) and the forecasts
//!   switch, kept between sessions
//!
//! Uses the guitk library for rendering.

mod fetch;
mod openmeteo;
mod source;

use appearance::Palette;
use appearance::Surface;
use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use guitk::textedit;
use guitk::textinput::TextInput;
use guitk::wheel;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::Duration;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

// ============================================================================
// Alert card metrics
// ============================================================================

/// Point size of an alert's description text.
const ALERT_BODY_FONT_SIZE: f32 = 13.0;
/// Line-to-line spacing of an alert's description, which is wrapped to the card.
const ALERT_BODY_LINE_HEIGHT: f32 = 18.0;
/// Offset from the top of an alert card to the first line of its description,
/// leaving room for the title above it.
const ALERT_BODY_TOP: f32 = 40.0;

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
    /// The colour a condition's icon is drawn in.
    ///
    /// The icon is a glyph, so this is text, and every caller draws it as
    /// such -- which is what lets the ink go here rather than at three draw
    /// sites (837; `gui/appearance/colour-methods.py`).
    ///
    /// `subtext0`/`subtext1` are floored in the palette already and
    /// `overlay0` is deliberately not -- it is the faintest legible mark. Fog
    /// asked for `surface2`, a *background* rung, which is a legibility bug
    /// rather than a style: inked, it becomes a mark you can actually see.
    pub fn icon_color(self, pal: &Palette) -> Color {
        match self {
            Self::Clear => pal.ink(pal.yellow),
            Self::PartlyCloudy => pal.subtext1,
            Self::Cloudy | Self::Overcast | Self::Haze => pal.overlay0,
            Self::LightRain | Self::Rain | Self::HeavyRain => pal.ink(pal.blue),
            Self::Thunderstorm => pal.ink(pal.peach),
            Self::Snow | Self::LightSnow | Self::Sleet => pal.ink(pal.lavender),
            Self::Fog => pal.ink(pal.surface2),
            Self::Windy => pal.subtext0,
            Self::Tornado | Self::Hurricane => pal.ink(pal.red),
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub temp_unit: TempUnit,
    pub wind_unit: WindSpeedUnit,
    pub pressure_unit: PressureUnit,
    pub time_format: TimeFormat,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            temp_unit: TempUnit::Celsius,
            wind_unit: WindSpeedUnit::Kmh,
            pressure_unit: PressureUnit::Hpa,
            time_format: TimeFormat::H24,
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

    pub fn color(self, pal: &Palette) -> Color {
        match self {
            Self::Low => pal.green,
            Self::Moderate => pal.yellow,
            Self::High => pal.peach,
            Self::VeryHigh => pal.red,
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

    pub fn color(self, pal: &Palette) -> Color {
        match self {
            Self::Good => pal.green,
            Self::Moderate => pal.yellow,
            Self::UnhealthySensitive => pal.peach,
            Self::Unhealthy => pal.red,
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

    pub fn color(self, pal: &Palette) -> Color {
        match self {
            Self::Advisory => pal.yellow,
            Self::Watch => pal.peach,
            Self::Warning => pal.red,
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
    /// The US air-quality index, if the air-quality service answered: it is a
    /// second request to a second service, and a reading it did not give is
    /// not shown as one.
    pub aqi: Option<u16>,
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

/// A saved location: a place the user chose from a search, and where it is,
/// which is what the forecast service is asked about.
#[derive(Clone, Debug)]
pub struct Location {
    pub name: String,
    pub is_default: bool,
    pub latitude: f64,
    pub longitude: f64,
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
                13..=23 => hour.saturating_sub(12),
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
                13..=23 => h.saturating_sub(12),
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
#[cfg(test)]
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
        aqi: Some(42),
    }
}

/// Generate sample hourly forecast (24 hours).
#[cfg(test)]
pub fn sample_hourly_forecast() -> Vec<HourForecast> {
    // One row per hour rather than three parallel arrays indexed in lockstep.
    // The arrays could not be checked against each other: adding a temperature
    // without adding a precipitation left the shorter one to be indexed out of
    // range, which is a panic in a function every screen of the app calls.
    // A table cannot be half-edited — a row is short and it does not compile.
    const HOURS: [(f32, WeatherCondition, u8); 24] = [
        (18.0, WeatherCondition::Clear, 0),
        (17.5, WeatherCondition::Clear, 0),
        (17.0, WeatherCondition::Clear, 0),
        (16.5, WeatherCondition::Clear, 0),
        (16.0, WeatherCondition::Clear, 0),
        (16.5, WeatherCondition::Clear, 0),
        (17.0, WeatherCondition::PartlyCloudy, 5),
        (18.5, WeatherCondition::PartlyCloudy, 5),
        (20.0, WeatherCondition::PartlyCloudy, 10),
        (21.5, WeatherCondition::Cloudy, 10),
        (23.0, WeatherCondition::Cloudy, 15),
        (24.0, WeatherCondition::Cloudy, 15),
        (24.5, WeatherCondition::PartlyCloudy, 10),
        (25.0, WeatherCondition::PartlyCloudy, 10),
        (24.5, WeatherCondition::LightRain, 40),
        (24.0, WeatherCondition::LightRain, 50),
        (23.0, WeatherCondition::Rain, 70),
        (22.0, WeatherCondition::Rain, 65),
        (21.0, WeatherCondition::LightRain, 40),
        (20.0, WeatherCondition::Cloudy, 20),
        (19.0, WeatherCondition::Cloudy, 10),
        (18.5, WeatherCondition::PartlyCloudy, 5),
        (18.0, WeatherCondition::Clear, 0),
        (17.5, WeatherCondition::Clear, 0),
    ];
    HOURS
        .into_iter()
        .enumerate()
        .map(|(i, (temp_c, condition, precip_pct))| HourForecast {
            // The table is 24 long, so this cannot truncate; `try_from` says so
            // in the code rather than in a comment.
            hour: u8::try_from(i).unwrap_or(0),
            temp_c,
            condition,
            precip_pct,
        })
        .collect()
}

/// Generate sample 7-day daily forecast.
#[cfg(test)]
pub fn sample_daily_forecast() -> Vec<DayForecast> {
    // One row per day, for the reason above: seven parallel arrays were seven
    // chances for one of them to be a different length than the rest. rustfmt
    // puts each field on its own line, so this reads as a list rather than a
    // grid — the property that matters is still there, though: a row with a
    // missing field does not compile, where a short array did not even warn.
    const DAYS: [(&str, f32, f32, WeatherCondition, u8, f32, WindDirection); 7] = [
        (
            "Mon",
            25.0,
            15.0,
            WeatherCondition::PartlyCloudy,
            10,
            12.0,
            WindDirection::SW,
        ),
        (
            "Tue",
            27.0,
            16.0,
            WeatherCondition::Clear,
            0,
            8.0,
            WindDirection::S,
        ),
        (
            "Wed",
            23.0,
            14.0,
            WeatherCondition::Rain,
            80,
            25.0,
            WindDirection::W,
        ),
        (
            "Thu",
            20.0,
            12.0,
            WeatherCondition::Cloudy,
            30,
            18.0,
            WindDirection::NW,
        ),
        (
            "Fri",
            22.0,
            13.0,
            WeatherCondition::LightRain,
            45,
            15.0,
            WindDirection::N,
        ),
        (
            "Sat",
            26.0,
            15.0,
            WeatherCondition::Clear,
            0,
            10.0,
            WindDirection::SE,
        ),
        (
            "Sun",
            28.0,
            17.0,
            WeatherCondition::Clear,
            5,
            7.0,
            WindDirection::E,
        ),
    ];
    DAYS.into_iter()
        .map(
            |(day_name, high_c, low_c, condition, precip_pct, wind_speed_kmh, wind_dir)| {
                DayForecast {
                    day_name: day_name.to_owned(),
                    high_c,
                    low_c,
                    condition,
                    precip_pct,
                    wind_speed_kmh,
                    wind_dir,
                }
            },
        )
        .collect()
}

/// Generate sample alerts.
#[cfg(test)]
pub fn sample_alerts() -> Vec<WeatherAlert> {
    vec![WeatherAlert {
        alert_type: AlertType::Thunderstorm,
        severity: AlertSeverity::Watch,
        title: "Thunderstorm Watch".to_string(),
        description: "Thunderstorms expected this afternoon. Stay alert.".to_string(),
    }]
}

/// What the window says while forecasts are off, in place of a forecast:
/// what turning them on would send, to whom and when, so the button under it
/// is pressed knowing (design-decisions §1236, the operator's answer to E-Q2).
///
/// The last line is about warnings, as it was when this app could fetch
/// nothing. A weather app is the one program here with a channel whose whole
/// purpose is to make somebody change their plans for safety, and Open-Meteo
/// gives no warnings -- so an empty warnings list must not read as "none in
/// force", with forecasts on or off.
const FORECASTS_OFF_LINES: [&str; 4] = [
    "Forecasts are off.",
    "Turned on, this app asks Open-Meteo (open-meteo.com) for the weather at the places you add: it sends each place's latitude and longitude, and the names you search for.",
    "It asks when you add or choose a place, and every half hour while the window is open. The requests go in plain text, so others on your network could see which places.",
    "It gives no severe-weather warnings, on or off: Open-Meteo has none to give. Silence here is not an all-clear.",
];

/// The longest place name the search box takes, in characters.
const SEARCH_CAPACITY: usize = 100;

/// The size the search box's text is drawn at.
const SEARCH_FONT: f32 = 14.0;

/// Default saved locations.
#[cfg(test)]
pub fn default_locations() -> Vec<Location> {
    vec![
        Location {
            name: "New York, NY".to_string(),
            is_default: true,
            latitude: 40.714_27,
            longitude: -74.005_97,
        },
        Location {
            name: "London, UK".to_string(),
            is_default: false,
            latitude: 51.508_53,
            longitude: -0.125_74,
        },
        Location {
            name: "Tokyo, JP".to_string(),
            is_default: false,
            latitude: 35.6895,
            longitude: 139.691_71,
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

/// The keys this program answers, raised by `F1` or `?`.
///
/// One thing takes typed text: the place search on the Places tab, while
/// forecasts are on. There the letters, the digits and `?` type into it, and
/// `F1` still raises this list; on every other tab `?` is free, and
/// design-decisions 863 says to bind it where it is.
///
/// The app named none of these before the list existed.
const SHORTCUTS: &[(&str, &str)] = &[
    ("1-6", "Dashboard, hourly, daily, alerts, places, settings"),
    ("Tab", "The next view"),
    ("Up / Down", "Another place"),
    ("Left / Right", "Scroll the hourly strip"),
    ("Home", "Back to the start of it"),
    ("U", "Celsius or Fahrenheit"),
    ("W", "Wind units"),
    ("P", "Pressure units"),
    ("T", "24-hour or 12-hour"),
    ("R", "Ask for the forecast again"),
    ("Enter", "Search for the place typed, on the places tab"),
    ("F1 / ?", "This list"),
];

/// Main application state.
pub struct WeatherApp {
    /// The current observation, if anything has fetched one.
    ///
    /// `Option` since 2026-09-15. It was a bare `CurrentWeather` filled by
    /// `sample_current_weather()`, so the dashboard always had a temperature,
    /// a feels-like, a humidity, a UV index and an air-quality reading to
    /// show. A plain struct has no way to say "not fetched", so the only
    /// answer it could give was a made-up one -- the same shape as the sound
    /// recorder's `available_bytes: u64`, which is why the fix is the same.
    pub current: Option<CurrentWeather>,
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
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
    /// Whether the shortcut card is up.
    show_help: bool,
    /// What the pointer is over, so it can be drawn lit.
    hover: Option<Target>,
    /// Every box the last paint recorded, for hover and the wheel.
    last_hits: Vec<(Target, Rect)>,
    /// The wheel's remainder over the hourly strip.
    wheel: wheel::Accumulator,
    /// Why the units could not be kept, when they could not.
    settings_error: Option<String>,
    /// Where the weather comes from, once forecasts are on (`source.rs`).
    pub source: source::Source,
    /// The place-name search on the Locations tab: its text and caret.
    search: TextInput,
    /// What the search box's Ctrl+C and Ctrl+X took, for its Ctrl+V.
    clipboard: String,
}

impl WeatherApp {
    /// Create a new app. It knows no weather and no locations.
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            show_help: false,
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            // All empty. Nothing here can reach a weather service.
            //
            // `locations` went with the rest: it defaulted to "New York, NY",
            // marked as the user's default, which is a claim about where they
            // are.
            current: None,
            hourly: Vec::new(),
            daily: Vec::new(),
            alerts: Vec::new(),
            locations: Vec::new(),
            active_location_idx: 0,
            settings: Settings::default(),
            active_view: ActiveView::Dashboard,
            hourly_scroll_offset: 0.0,
            width,
            height,
            hover: None,
            last_hits: Vec::new(),
            wheel: wheel::Accumulator::default(),
            settings_error: None,
            // Off: nothing is sent anywhere until the user turns forecasts
            // on (design-decisions §1236).
            source: source::Source::default(),
            search: TextInput::new(),
            clipboard: String::new(),
        }
    }

    /// An app holding the observation and forecast `new` used to invent.
    ///
    /// `#[cfg(test)]`. Most of this app's tests are about the hourly strip,
    /// the temperature graph, the alert cards, the location list and the
    /// settings -- all of which need *weather*, not specifically invented
    /// weather. Before 2026-09-15 they got it from `new`, which is the defect:
    /// the forecast production could reach was the forecast that shipped.
    #[cfg(test)]
    pub fn with_sample_weather(width: f32, height: f32) -> Self {
        let mut app = Self::new(width, height);
        app.current = Some(sample_current_weather());
        app.hourly = sample_hourly_forecast();
        app.daily = sample_daily_forecast();
        app.alerts = sample_alerts();
        app.locations = default_locations();
        // Weather shown means forecasts are on. The fetcher is still the
        // tests' own, which reaches nothing.
        app.source.on = true;
        app
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

    /// Add a location by name, at no particular place: for the tests of the
    /// list itself. A place the user adds comes from a search, with where it
    /// is ([`Self::add_place`]).
    #[cfg(test)]
    pub fn add_location(&mut self, name: String) {
        let is_default = self.locations.is_empty();
        self.locations.push(Location {
            name,
            is_default,
            latitude: 0.0,
            longitude: 0.0,
        });
    }

    /// Remove place `index` from the window's list, keep the list, and show
    /// the place that takes its turn -- asking for its weather when that is a
    /// different place from the one shown.
    pub fn remove_place(&mut self, index: usize) -> bool {
        let shown_before = self
            .locations
            .get(self.active_location_idx)
            .map(|l| (l.latitude, l.longitude));
        if !self.remove_location(index) {
            return false;
        }
        let shown_after = self
            .locations
            .get(self.active_location_idx)
            .map(|l| (l.latitude, l.longitude));
        if shown_after != shown_before {
            self.forget_weather();
            self.ask_forecast();
        }
        self.store_places();
        true
    }

    /// Remove a location by index. Returns true if removed.
    pub fn remove_location(&mut self, idx: usize) -> bool {
        // `get` rather than a bounds test and then an index: one expression
        // that cannot disagree with itself.
        let Some(loc) = self.locations.get(idx) else {
            return false;
        };
        let was_default = loc.is_default;
        self.locations.remove(idx);
        if idx < self.active_location_idx {
            // A place above the one shown went: the one shown moved up a
            // row, and is still the one shown. Left as it was, the index
            // named the place below it.
            self.active_location_idx = self.active_location_idx.saturating_sub(1);
        } else if self.active_location_idx >= self.locations.len() {
            // The one shown went, and it was the last: the new last is shown.
            // Otherwise the place after it took its row, and is shown.
            self.active_location_idx = self.locations.len().saturating_sub(1);
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

    /// Scroll the hourly strip left/right.
    pub fn scroll_hourly(&mut self, delta: f32) {
        self.hourly_scroll_offset = (self.hourly_scroll_offset + delta).max(0.0);
        let max_scroll = (self.hourly.len() as f32 * 80.0).max(0.0);
        if self.hourly_scroll_offset > max_scroll {
            self.hourly_scroll_offset = max_scroll;
        }
    }

    // ========================================================================
    // Events
    // ========================================================================

    /// Route a compositor event into the app.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Key(key_ev) => self.handle_key(key_ev),
            Event::Resize { width, height } => {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a window dimension is far below f32's integer-exact range"
                )]
                {
                    self.width = *width as f32;
                    self.height = *height as f32;
                }
                // Not `Consumed`: a resize is not by itself a reason to redraw.
                EventResult::Ignored
            }
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            // A unit changed in another window, and the desktop says so: this
            // window follows. Read at startup only, it showed the old unit
            // until it was opened again.
            Event::SettingsChanged { group } if group.file_name() == CONFIG_NAME => {
                let units = self.reread_units();
                let places = self.reread_places();
                if units || places {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            // An answer the waker's wake did not bring in, and the half-hourly
            // refresh of the forecast shown -- or the retry of one that failed.
            Event::Tick { .. } => {
                let mut changed = self.pump();
                if self.refresh_due(std::time::Instant::now()) {
                    self.ask_forecast();
                    // What the window says changes: it is asking.
                    changed = true;
                }
                if changed {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }

    /// Read the forecast switch and the places again after the desktop said
    /// `weather.yaml` changed -- turned on, a place added or removed, in
    /// another window. Whether anything changed. Turned off there, this
    /// window stops showing what it fetched; turned on there, or its place
    /// changed, this window asks for the forecast of the place it now shows.
    fn reread_places(&mut self) -> bool {
        // A place as the file holds it. `set_f64` writes the shortest
        // spelling that reads back as the same number, so a place this
        // window wrote compares equal, bit for bit, when it comes back.
        let spot = |l: &Location| (l.name.clone(), l.latitude.to_bits(), l.longitude.to_bits());
        let was_on = self.source.on;
        let before: Vec<_> = self.locations.iter().map(spot).collect();
        let shown_before = before.get(self.active_location_idx).cloned();
        self.load_places(&settingsfile::load(CONFIG_NAME));
        let after: Vec<_> = self.locations.iter().map(spot).collect();
        // The place this window shows stays shown while the list still has
        // it. `load_places` shows the file's default -- the place a window
        // opens on -- and the watcher announces every write, this window's
        // own among them: without this, adding a place, or going to another
        // and changing a unit, came straight back as a jump to the default.
        if let Some(i) = shown_before
            .as_ref()
            .and_then(|shown| after.iter().position(|a| a == shown))
        {
            self.active_location_idx = i;
        }
        let shown_after = after.get(self.active_location_idx).cloned();
        if was_on && !self.source.on {
            // Off in another window: the same as off here, without writing
            // the file back.
            self.stop_fetching();
        } else if self.source.on && (!was_on || shown_after != shown_before) {
            self.forget_weather();
            self.ask_forecast();
        }
        was_on != self.source.on || before != after
    }

    /// Apply a key press.
    ///
    /// The app had no input handling at all before it was wired to the
    /// compositor — every view, unit and location it can display was reachable
    /// only by a caller constructing the state directly. These bindings are
    /// what makes the existing mutators reachable by the person using it.
    pub fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if !key.pressed {
            return EventResult::Ignored;
        }
        // The place-name search on the Locations tab, while forecasts are on,
        // has the keys a text box has -- Ctrl+A, C, X and V among them, and
        // what AltGr types -- so it comes before the plain-keys rule below.
        // Enter searches; Escape empties it. The unit keys are letters, so
        // they type here and change units on the other tabs.
        if self.active_view == ActiveView::Locations && self.source.on && !self.show_help {
            if key.key == Key::Enter && textline::is_plain(key.modifiers) {
                let name = self.search.text().to_owned();
                self.search_places(&name);
                return EventResult::Consumed;
            }
            if key.key == Key::Escape && !self.search.text().is_empty() {
                self.search.set_text("");
                return EventResult::Consumed;
            }
            let edit = textline::apply_key(
                &mut self.search,
                key,
                SEARCH_CAPACITY,
                &self.clipboard,
                SEARCH_FONT,
            );
            if let Some(copied) = edit.copied {
                self.clipboard = copied;
            }
            if edit.handled {
                return EventResult::Consumed;
            }
        }
        // Every binding is on a key, taken plain -- nothing held but Shift. A
        // chord with Ctrl, Alt or the Windows key is the window's or the
        // desktop's and arrives carrying its key: Alt+U changed the
        // temperature unit, and Ctrl+T the clock.
        if !textline::is_plain(key.modifiers) {
            return EventResult::Ignored;
        }

        if key.key == Key::F1 || (key.key == Key::Slash && key.modifiers.shift) {
            self.show_help = !self.show_help;
            return EventResult::Consumed;
        }
        if self.show_help {
            // Modal: letting keys through would mean changing the units of a
            // reading the reader cannot see.
            if matches!(key.key, Key::Escape | Key::Enter | Key::F1) {
                self.show_help = false;
            }
            return EventResult::Consumed;
        }

        let views = [
            ActiveView::Dashboard,
            ActiveView::HourlyDetail,
            ActiveView::DailyDetail,
            ActiveView::Alerts,
            ActiveView::Locations,
            ActiveView::SettingsView,
        ];
        match key.key {
            // The six views, in the order the number row reads.
            Key::Num1 => self.set_view(views[0]),
            Key::Num2 => self.set_view(views[1]),
            Key::Num3 => self.set_view(views[2]),
            Key::Num4 => self.set_view(views[3]),
            Key::Num5 => self.set_view(views[4]),
            Key::Num6 => self.set_view(views[5]),
            Key::Tab => {
                let next = views
                    .iter()
                    .position(|v| *v == self.active_view)
                    .and_then(|i| i.checked_add(1))
                    .map_or(0, |i| if i < views.len() { i } else { 0 });
                self.set_view(*views.get(next).unwrap_or(&ActiveView::Dashboard))
            }
            // The hourly strip is the only thing on screen wider than the
            // window, so the horizontal arrows belong to it.
            Key::Left => self.scroll_and_report(-Self::HOURLY_SCROLL_STEP),
            Key::Right => self.scroll_and_report(Self::HOURLY_SCROLL_STEP),
            Key::Home => self.scroll_to_start(),
            // Locations, which is what the vertical arrows mean everywhere else
            // in the app's own list views.
            Key::Up => self.step_location(-1),
            Key::Down => self.step_location(1),
            // Unit toggles, on the initial of the thing they change.
            Key::U => {
                self.change_setting(Setting::Temperature);
                EventResult::Consumed
            }
            Key::W => {
                self.change_setting(Setting::Wind);
                EventResult::Consumed
            }
            Key::P => {
                self.change_setting(Setting::Pressure);
                EventResult::Consumed
            }
            Key::T => {
                self.change_setting(Setting::Time);
                EventResult::Consumed
            }
            // Ask for the forecast again now, rather than at the half hour.
            Key::R if self.source.on && !self.locations.is_empty() => {
                self.ask_forecast();
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// How far one arrow press moves the hourly strip.
    ///
    /// One item's width plus its gap, so a press advances by exactly one hour
    /// rather than by a number of pixels that happens to look right.
    const HOURLY_SCROLL_STEP: f32 = 92.0;

    /// Switch views, reporting whether anything changed.
    fn set_view(&mut self, view: ActiveView) -> EventResult {
        if self.active_view == view {
            // Already here. Answering `Consumed` would redraw an identical
            // frame every time the user pressed the key for the view they are
            // already looking at.
            return EventResult::Ignored;
        }
        self.active_view = view;
        EventResult::Consumed
    }

    /// Scroll the hourly strip, reporting whether it actually moved.
    fn scroll_and_report(&mut self, delta: f32) -> EventResult {
        let before = self.hourly_scroll_offset;
        self.scroll_hourly(delta);
        if (self.hourly_scroll_offset - before).abs() < f32::EPSILON {
            EventResult::Ignored
        } else {
            EventResult::Consumed
        }
    }

    /// Return the hourly strip to its first hour.
    fn scroll_to_start(&mut self) -> EventResult {
        if self.hourly_scroll_offset <= 0.0 {
            return EventResult::Ignored;
        }
        self.hourly_scroll_offset = 0.0;
        EventResult::Consumed
    }

    /// Move to the next or previous saved location.
    ///
    /// Stops at either end rather than wrapping: a list of three cities that
    /// loops is a list the user cannot tell they have reached the end of.
    fn step_location(&mut self, delta: isize) -> EventResult {
        let Ok(current) = isize::try_from(self.active_location_idx) else {
            return EventResult::Ignored;
        };
        let Some(next) = current.checked_add(delta) else {
            return EventResult::Ignored;
        };
        let Ok(next) = usize::try_from(next) else {
            return EventResult::Ignored;
        };
        if next >= self.locations.len() || next == self.active_location_idx {
            return EventResult::Ignored;
        }
        self.show_location(next);
        EventResult::Consumed
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the entire weather application into render commands.
    ///
    /// Named `render_commands` and not `render`: at equal arity an inherent
    /// method silently wins method lookup over `oswindow::app::App::render`, so
    /// an app that keeps the name draws nothing and reports no error.
    #[cfg(test)]
    pub fn render_commands(&self) -> Vec<RenderCommand> {
        self.frame().into_tree().commands
    }

    /// Draw the window, recording every control where it is drawn: both the
    /// picture and the hit test.
    fn frame(&self) -> Frame<Target> {
        let mut f = Frame::new(self.width, self.height);
        self.draw(&mut f);
        // Over everything, because it is the one thing a reader asked for --
        // and whether or not anything was fetched. It was drawn after the
        // fetched views only, so with nothing fetched F1 raised a card that
        // was never drawn and that swallowed every key until it was closed.
        if self.show_help {
            guitk::shortcut::render_card(
                &mut f,
                &self.palette,
                (self.width, self.height),
                0.0,
                SHORTCUTS,
                "F1 or ? closes this",
            );
            f.hit(
                Target::HelpCard,
                Rect::new(0.0, 0.0, self.width, self.height),
            );
        }
        f
    }

    fn draw(&self, cmds: &mut Frame<Target>) {
        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });

        // Alert banner (if any)
        let content_y = self.render_alerts_banner(cmds, 0.0);

        // Title bar
        let title_y = self.render_title_bar(cmds, content_y);

        // The settings are not a reading, so they are shown whether or not
        // anything was fetched: the units a reading will be given in are the
        // user's to choose now. Before, the Settings tab showed the same
        // "cannot fetch" as every other, and U, W, P and T changed values
        // nobody could see.
        if self.active_view == ActiveView::SettingsView {
            self.render_settings_view(cmds, title_y);
            return;
        }
        // Where places are searched for and chosen, which is how a forecast
        // comes: shown whether or not one has.
        if self.active_view == ActiveView::Locations {
            self.render_locations_view(cmds, title_y);
            return;
        }
        if !self.source.on {
            self.render_forecasts_off(cmds, title_y);
            return;
        }

        // Nothing has come yet, so there is no view to draw. Every panel
        // below reports a reading, and a panel with no reading to report
        // either shows a default -- 0 degrees, Clear -- or an empty space, and
        // both are read as observations.
        let Some(current) = self.current.clone() else {
            self.render_no_forecast_yet(cmds, title_y);
            return;
        };

        // Main content area depends on active view
        match self.active_view {
            ActiveView::Dashboard => self.render_dashboard(cmds, title_y, &current),
            ActiveView::HourlyDetail => self.render_hourly_detail(cmds, title_y),
            ActiveView::DailyDetail => self.render_daily_detail(cmds, title_y),
            ActiveView::Alerts => self.render_alerts_view(cmds, title_y),
            ActiveView::Locations => self.render_locations_view(cmds, title_y),
            ActiveView::SettingsView => self.render_settings_view(cmds, title_y),
        }
        self.render_source_line(cmds);
    }

    /// `lines`, each wrapped to the window, from `y` down; the first larger
    /// and in `first_colour`. Returns the y below the last.
    fn paragraphs(
        &self,
        cmds: &mut Frame<Target>,
        y: f32,
        lines: &[&str],
        first_colour: Color,
    ) -> f32 {
        let x = 16.0;
        let width = (self.width - x * 2.0).max(0.0);
        let mut cy = y;
        for (i, line) in lines.iter().enumerate() {
            let (size, weight, colour) = if i == 0 {
                (15.0, FontWeightHint::Bold, first_colour)
            } else {
                (12.0, FontWeightHint::Regular, self.palette.subtext0)
            };
            for piece in text::wrap(line, width, size, weight) {
                cmds.push(RenderCommand::Text {
                    x,
                    y: cy,
                    text: piece,
                    color: colour,
                    font_size: size,
                    font_weight: weight,
                    max_width: Some(width),
                    overflow: TextOverflow::Clip,
                });
                cy += text::line_height(size, weight);
            }
            cy += 6.0;
        }
        cy
    }

    /// A button: `label` in a rounded box at (`x`, `y`), recorded as `target`.
    /// Returns its right edge.
    fn button(&self, cmds: &mut Frame<Target>, x: f32, y: f32, label: &str, target: Target) -> f32 {
        let w = text::measure(label, 13.0, FontWeightHint::Bold) + 28.0;
        let rect = Rect::new(x, y, w, 30.0);
        self.palette.push_surface(
            cmds,
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            8.0,
            if self.hover == Some(target) {
                Surface::Selected
            } else {
                Surface::Card
            },
        );
        cmds.push(RenderCommand::Text {
            x: rect.x + 14.0,
            y: rect.y + (rect.h - text::line_height(13.0, FontWeightHint::Bold)) / 2.0,
            text: label.to_string(),
            color: self.palette.text,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cmds.hit(target, rect);
        rect.right()
    }

    /// While forecasts are off: what turning them on sends, and the button
    /// that does it.
    fn render_forecasts_off(&self, cmds: &mut Frame<Target>, y: f32) {
        let below = self.paragraphs(
            cmds,
            y + 16.0,
            &FORECASTS_OFF_LINES,
            self.palette.ink(self.palette.yellow),
        );
        self.button(cmds, 16.0, below + 6.0, "Turn on forecasts", Target::TurnOn);
    }

    /// Forecasts on, and no forecast shown: no place chosen yet, one being
    /// asked for, or why the last request failed.
    fn render_no_forecast_yet(&self, cmds: &mut Frame<Target>, y: f32) {
        let name = self
            .locations
            .get(self.active_location_idx)
            .map(|l| l.name.as_str());
        let Some(name) = name else {
            let below = self.paragraphs(
                cmds,
                y + 16.0,
                &[
                    "No place chosen.",
                    "Search for a place on the Locations tab and choose it; its weather is shown here.",
                ],
                self.palette.text,
            );
            self.button(
                cmds,
                16.0,
                below + 6.0,
                "Choose a place",
                Target::Tab(ActiveView::Locations),
            );
            return;
        };
        if self.source.forecast_asked().is_some() {
            let asking = format!("Asking Open-Meteo for the weather at {name}\u{2026}");
            let lines: [&str; 1] = [&asking];
            self.paragraphs(cmds, y + 16.0, &lines, self.palette.text);
            return;
        }
        // Why there is none, and what happens next; or, with no reason
        // known, a way to ask. Nothing out and nothing failed is not a state
        // the window gets into, but a panel that said "Asking" over a
        // request nobody made would wait for ever.
        let again = format!(
            "It is asked for again in {} minutes, or now with Try again.",
            source::RETRY.as_secs() / 60
        );
        let (lines, colour) = match &self.source.error {
            Some(error) => (
                vec!["No forecast.", error.as_str(), again.as_str()],
                self.palette.ink(self.palette.red),
            ),
            None => (vec!["No forecast yet."], self.palette.text),
        };
        let below = self.paragraphs(cmds, y + 16.0, &lines, colour);
        self.button(cmds, 16.0, below + 6.0, "Try again", Target::Retry);
    }

    /// Along the bottom while a forecast is shown: whose data it is -- on
    /// every frame that shows it, which its licence asks for -- when it is
    /// for, and what is happening to it: being asked for again, or why a
    /// newer one could not be had, on a line of its own above.
    fn render_source_line(&self, cmds: &mut Frame<Target>) {
        let when = self
            .source
            .observed
            .as_deref()
            .and_then(openmeteo::hour_minute)
            .map(|(h, m)| format!(" \u{00b7} for {h:02}:{m:02} there"))
            .unwrap_or_default();
        let next = if self.source.forecast_asked().is_some() {
            " \u{00b7} asking again\u{2026}"
        } else {
            " \u{00b7} R asks again"
        };
        let mut lines = vec![(
            format!("{}{when}{next}", openmeteo::ATTRIBUTION),
            self.palette.subtext0,
        )];
        if let Some(error) = &self.source.error {
            lines.insert(
                0,
                (
                    format!(
                        "{error} \u{2014} this is the forecast from before; asked for again in {} minutes",
                        source::RETRY.as_secs() / 60
                    ),
                    self.palette.ink(self.palette.red),
                ),
            );
        }
        #[expect(clippy::cast_precision_loss, reason = "one or two lines")]
        let mut y = (self.height - 22.0 * lines.len() as f32).max(0.0);
        // Over whatever a long view drew down there, so the lines can be read.
        let strip = (y - 6.0).max(0.0);
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: strip,
            width: self.width,
            height: (self.height - strip).max(0.0),
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });
        for (text, color) in lines {
            cmds.push(RenderCommand::Text {
                x: 16.0,
                y,
                text,
                color,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some((self.width - 32.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            y += 22.0;
        }
    }

    /// Render alert banner at the top. Returns the Y position after the banner.
    fn render_alerts_banner(&self, cmds: &mut Frame<Target>, y: f32) -> f32 {
        if self.alerts.is_empty() {
            return y;
        }

        let banner_height = 36.0;
        for (i, alert) in self.alerts.iter().enumerate() {
            let by = y + i as f32 * banner_height;
            let bg_color = alert.severity.color(&self.palette);

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
                // Fills the banner above at alpha 40, so the headline is on
                // the page rather than on a solid ground: inked, not grounded.
                color: self.palette.ink(bg_color),
                font_weight: FontWeightHint::Bold,
                max_width: Some(self.width - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        y + self.alerts.len() as f32 * banner_height
    }

    /// Render the title bar. Returns the Y position after the title.
    fn render_title_bar(&self, cmds: &mut Frame<Target>, y: f32) -> f32 {
        let title_height = 50.0;

        self.palette
            .push_surface(cmds, 0.0, y, self.width, title_height, 0.0, Surface::Card);

        // App title
        cmds.push(RenderCommand::Text {
            x: 16.0,
            y: y + 15.0,
            text: "Weather".to_string(),
            font_size: 20.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Active location
        cmds.push(RenderCommand::Text {
            x: 120.0,
            y: y + 19.0,
            text: self.active_location_name().to_string(),
            font_size: 14.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
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
            // Measured at the widest weight the tab can take: the active one
            // is drawn bold, and the strip is laid out right to left, so
            // sizing to the current weight walked every tab sideways whenever
            // the selection moved.
            let text_width = text::measure(label, 13.0, FontWeightHint::Bold).max(text::measure(
                label,
                13.0,
                FontWeightHint::Regular,
            ));
            tx -= text_width + 16.0;
            let is_active = *view == self.active_view;
            let tab = Rect::new(tx - 4.0, y + 8.0, text_width + 24.0, 30.0);

            if is_active || self.hover == Some(Target::Tab(*view)) {
                self.palette
                    .push_surface(cmds, tab.x, tab.y, tab.w, tab.h, 6.0, Surface::Selected);
            }
            cmds.hit(Target::Tab(*view), tab);

            cmds.push(RenderCommand::Text {
                x: tx + 8.0,
                y: y + 16.0,
                text: label.to_string(),
                font_size: 13.0,
                color: if is_active {
                    self.palette.ink(self.palette.blue)
                } else {
                    self.palette.subtext0
                },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        y + title_height
    }

    /// Render the dashboard view (main overview).
    fn render_dashboard(&self, cmds: &mut Frame<Target>, y: f32, current: &CurrentWeather) {
        let padding = 16.0;
        let mut cy = y + padding;

        // Current weather card
        cy = self.render_current_weather_card(cmds, padding, cy, current);
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
        self.render_air_quality_card(cmds, padding, cy, current);
    }

    /// Render the current weather card. Returns the Y after the card.
    fn render_current_weather_card(
        &self,
        cmds: &mut Frame<Target>,
        x: f32,
        y: f32,
        current: &CurrentWeather,
    ) -> f32 {
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
        self.palette
            .push_surface(cmds, x, y, card_w, card_h, 12.0, Surface::Card);

        let inner_x = x + 20.0;
        let inner_y = y + 16.0;

        // Section label
        cmds.push(RenderCommand::Text {
            x: inner_x,
            y: inner_y,
            text: "Current Weather".to_string(),
            font_size: 12.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Large temperature
        cmds.push(RenderCommand::Text {
            x: inner_x,
            y: inner_y + 24.0,
            text: format_temp(current.temp_c, self.settings.temp_unit),
            font_size: 48.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Feels like
        cmds.push(RenderCommand::Text {
            x: inner_x,
            y: inner_y + 80.0,
            text: format!(
                "Feels like {}",
                format_temp(current.feels_like_c, self.settings.temp_unit)
            ),
            font_size: 13.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Condition text
        cmds.push(RenderCommand::Text {
            x: inner_x,
            y: inner_y + 100.0,
            text: current.condition.description().to_string(),
            font_size: 14.0,
            color: self.palette.subtext1,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Weather icon (ASCII art rendered as text lines)
        let icon_x = inner_x + 200.0;
        let icon_color = current.condition.icon_color(&self.palette);
        for (i, line) in current.condition.icon_lines().iter().enumerate() {
            cmds.push(RenderCommand::Text {
                x: icon_x,
                y: inner_y + 30.0 + i as f32 * 16.0,
                text: line.to_string(),
                font_size: 14.0,
                color: icon_color,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        // Details grid (right side of card)
        let detail_x = icon_x + 180.0;
        let details = self.current_weather_details(current);
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
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cmds.push(RenderCommand::Text {
                x: dx,
                y: dy + 14.0,
                text: value.to_owned(),
                font_size: 13.0,
                color: self.palette.text,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        y + card_h
    }

    /// Collect current weather detail label-value pairs.
    fn current_weather_details(&self, current: &CurrentWeather) -> Vec<(&'static str, String)> {
        let uv_severity = UvSeverity::from_index(current.uv_index);
        vec![
            ("Humidity", format!("{}%", current.humidity_pct)),
            (
                "Wind",
                format!(
                    "{} {}",
                    format_wind(current.wind_speed_kmh, self.settings.wind_unit),
                    current.wind_dir.as_str()
                ),
            ),
            (
                "Pressure",
                format_pressure(current.pressure_hpa, self.settings.pressure_unit),
            ),
            (
                "Visibility",
                format_visibility(current.visibility_km, self.settings.temp_unit),
            ),
            (
                "Dew Point",
                format_temp(current.dew_point_c, self.settings.temp_unit),
            ),
            (
                "UV Index",
                format!("{} ({})", current.uv_index, uv_severity.label()),
            ),
            (
                "Sunrise",
                format_time(
                    current.sunrise.0,
                    current.sunrise.1,
                    self.settings.time_format,
                ),
            ),
            (
                "Sunset",
                format_time(
                    current.sunset.0,
                    current.sunset.1,
                    self.settings.time_format,
                ),
            ),
        ]
    }

    /// Render the hourly forecast strip. Returns Y after.
    fn render_hourly_strip(&self, cmds: &mut Frame<Target>, x: f32, y: f32) -> f32 {
        let strip_w = self.width - x * 2.0;
        let strip_h = 120.0;
        let item_w = 72.0;
        let item_gap = 8.0;

        // Card
        self.palette
            .push_surface(cmds, x, y, strip_w, strip_h, 12.0, Surface::Card);

        // Section label
        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: y + 12.0,
            text: "Hourly Forecast".to_string(),
            font_size: 12.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Clip the scrollable area
        let scroll_y = y + 30.0;
        let scroll_h = strip_h - 34.0;
        // The wheel scrolls it, sideways: it is the one thing on screen wider
        // than the window.
        cmds.hit(Target::HourlyStrip, Rect::new(x, y, strip_w, strip_h));
        cmds.clip(Rect::new(
            x + 8.0,
            scroll_y,
            (strip_w - 16.0).max(0.0),
            scroll_h,
        ));

        for (i, hf) in self.hourly.iter().enumerate() {
            let ix = x + 12.0 + i as f32 * (item_w + item_gap) - self.hourly_scroll_offset;

            // Skip if off-screen
            if ix + item_w < x || ix > x + strip_w {
                continue;
            }

            // Item background
            self.palette.push_surface(
                cmds,
                ix,
                scroll_y + 4.0,
                item_w,
                scroll_h - 8.0,
                8.0,
                Surface::Card,
            );

            // Hour label
            cmds.push(RenderCommand::Text {
                x: ix + 8.0,
                y: scroll_y + 10.0,
                text: format_hour(hf.hour, self.settings.time_format),
                font_size: 11.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(item_w - 16.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Temperature
            cmds.push(RenderCommand::Text {
                x: ix + 8.0,
                y: scroll_y + 28.0,
                text: format_temp(hf.temp_c, self.settings.temp_unit),
                font_size: 16.0,
                color: self.palette.text,
                font_weight: FontWeightHint::Bold,
                max_width: Some(item_w - 16.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Condition mini icon (first line of ASCII art)
            let icon_lines = hf.condition.icon_lines();
            if let Some(first_line) = icon_lines.first() {
                cmds.push(RenderCommand::Text {
                    x: ix + 4.0,
                    y: scroll_y + 48.0,
                    text: first_line.to_string(),
                    font_size: 9.0,
                    color: hf.condition.icon_color(&self.palette),
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(item_w - 8.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            // Precipitation chance
            if hf.precip_pct > 0 {
                cmds.push(RenderCommand::Text {
                    x: ix + 8.0,
                    y: scroll_y + 64.0,
                    text: format!("{}%", hf.precip_pct),
                    font_size: 11.0,
                    color: self.palette.ink(self.palette.blue),
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(item_w - 16.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }
        }

        cmds.unclip();

        y + strip_h
    }

    /// Render the temperature graph. Returns Y after.
    fn render_temp_graph(&self, cmds: &mut Frame<Target>, x: f32, y: f32) -> f32 {
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
            color: self.palette.surface0,
            corner_radii: CornerRadii::all(12.0),
        });

        // Section label
        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: y + 12.0,
            text: "Temperature (24h)".to_string(),
            font_size: 12.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
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
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(40.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Grid line
            cmds.push(RenderCommand::Line {
                x1: plot_x,
                y1: ly,
                x2: plot_x + plot_w,
                y2: ly,
                color: Color::rgba(
                    self.palette.overlay0.r,
                    self.palette.overlay0.g,
                    self.palette.overlay0.b,
                    30,
                ),
                width: 1.0,
            });
        }

        // Plot line segments
        let point_count = self.hourly.len();
        if point_count >= 2 {
            #[allow(
                clippy::cast_precision_loss,
                reason = "24 hours of forecast; the count is nowhere near f32's \
                          integer-exact range"
            )]
            let step = plot_w / (point_count as f32 - 1.0);
            // Over adjacent pairs rather than an index and its successor: the
            // pairing is what the loop is about, and `windows` cannot run off
            // the end the way `0..len - 1` does when `len` is 0.
            for (i, pair) in self.hourly.windows(2).enumerate() {
                let (Some(h1), Some(h2)) = (pair.first(), pair.get(1)) else {
                    continue;
                };
                let t1 = h1.temp_c;
                let t2 = h2.temp_c;
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a point index within a 24-point plot"
                )]
                let (xi, xi_next) = (i as f32, i.saturating_add(1) as f32);
                let x1 = plot_x + xi * step;
                let y1 = plot_y + plot_h * (1.0 - (t1 - min_temp) / temp_range);
                let x2 = plot_x + xi_next * step;
                let y2 = plot_y + plot_h * (1.0 - (t2 - min_temp) / temp_range);

                cmds.push(RenderCommand::Line {
                    x1,
                    y1,
                    x2,
                    y2,
                    color: self.palette.blue,
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
        for (i, hour) in self.hourly.iter().enumerate().step_by(4) {
            #[allow(
                clippy::cast_precision_loss,
                reason = "a point index within a 24-point plot"
            )]
            let lx = plot_x + i as f32 * step;
            cmds.push(RenderCommand::Text {
                x: lx - 10.0,
                y: plot_y + plot_h + 6.0,
                text: format_hour(hour.hour, self.settings.time_format),
                font_size: 10.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(40.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        y + graph_h
    }

    /// Render the daily forecast table. Returns Y after.
    fn render_daily_table(&self, cmds: &mut Frame<Target>, x: f32, y: f32) -> f32 {
        let table_w = self.width - x * 2.0;
        let header_h = 34.0;
        let row_h = 36.0;
        let table_h = header_h + self.daily.len() as f32 * row_h + 16.0;

        // Card
        self.palette
            .push_surface(cmds, x, y, table_w, table_h, 12.0, Surface::Card);

        // Section label
        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: y + 12.0,
            text: "7-Day Forecast".to_string(),
            font_size: 12.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
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
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        // Divider line
        cmds.push(RenderCommand::Line {
            x1: x + 12.0,
            y1: hy + 16.0,
            x2: x + table_w - 12.0,
            y2: hy + 16.0,
            color: self.palette.surface1,
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
                    color: Color::rgba(
                        self.palette.surface1.r,
                        self.palette.surface1.g,
                        self.palette.surface1.b,
                        40,
                    ),
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: col_day,
                y: ry,
                text: day.day_name.clone(),
                font_size: 13.0,
                color: self.palette.text,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            cmds.push(RenderCommand::Text {
                x: col_cond,
                y: ry,
                text: day.condition.description().to_string(),
                font_size: 13.0,
                color: self.palette.subtext1,
                font_weight: FontWeightHint::Regular,
                max_width: Some(130.0),
                overflow: TextOverflow::Ellipsis,
            });

            cmds.push(RenderCommand::Text {
                x: col_high,
                y: ry,
                text: format_temp(day.high_c, self.settings.temp_unit),
                font_size: 13.0,
                color: self.palette.ink(self.palette.peach),
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            cmds.push(RenderCommand::Text {
                x: col_low,
                y: ry,
                text: format_temp(day.low_c, self.settings.temp_unit),
                font_size: 13.0,
                color: self.palette.ink(self.palette.blue),
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            cmds.push(RenderCommand::Text {
                x: col_precip,
                y: ry,
                text: format!("{}%", day.precip_pct),
                font_size: 13.0,
                color: if day.precip_pct > 50 {
                    self.palette.ink(self.palette.blue)
                } else {
                    self.palette.subtext0
                },
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
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
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(120.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        y + table_h
    }

    /// Render the air quality card. Returns Y after.
    fn render_air_quality_card(
        &self,
        cmds: &mut Frame<Target>,
        x: f32,
        y: f32,
        current: &CurrentWeather,
    ) -> f32 {
        let card_w = self.width - x * 2.0;
        let card_h = 80.0;

        // Card
        self.palette
            .push_surface(cmds, x, y, card_w, card_h, 12.0, Surface::Card);

        // Section label
        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: y + 12.0,
            text: "Air Quality".to_string(),
            font_size: 12.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // An index nobody gave is not drawn as one: 0 would read as "Good".
        let Some(aqi) = current.aqi else {
            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: y + 38.0,
                text: "Not known -- the air-quality service did not answer".to_string(),
                font_size: 13.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some((card_w - 32.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            return y + card_h;
        };
        let aq = AirQuality::from_aqi(aqi);

        // AQI number
        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: y + 34.0,
            text: format!("AQI: {aqi}"),
            font_size: 24.0,
            color: self.palette.ink(aq.color(&self.palette)),
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Category label
        cmds.push(RenderCommand::Text {
            x: x + 140.0,
            y: y + 40.0,
            text: aq.label().to_string(),
            font_size: 16.0,
            color: self.palette.ink(aq.color(&self.palette)),
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
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
            color: self.palette.surface1,
            corner_radii: CornerRadii::all(3.0),
        });

        // Filled portion (AQI 0-500 scale)
        let fill_frac = (f32::from(aqi) / 500.0).min(1.0);
        let fill_w = bar_w * fill_frac;
        if fill_w > 0.0 {
            cmds.push(RenderCommand::FillRect {
                x: bar_x,
                y: bar_y,
                width: fill_w,
                height: bar_h,
                color: aq.color(&self.palette),
                corner_radii: CornerRadii::all(3.0),
            });
        }

        y + card_h
    }

    /// Render hourly detail view.
    fn render_hourly_detail(&self, cmds: &mut Frame<Target>, y: f32) {
        let padding = 16.0;
        let mut cy = y + padding;

        cmds.push(RenderCommand::Text {
            x: padding,
            y: cy,
            text: "Hourly Forecast Detail".to_string(),
            font_size: 18.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
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
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
        cy += 20.0;

        cmds.push(RenderCommand::Line {
            x1: padding,
            y1: cy,
            x2: self.width - padding,
            y2: cy,
            color: self.palette.surface1,
            width: 1.0,
        });
        cy += 8.0;

        for hf in &self.hourly {
            cmds.push(RenderCommand::Text {
                x: col_hour,
                y: cy,
                text: format_hour(hf.hour, self.settings.time_format),
                font_size: 13.0,
                color: self.palette.text,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cmds.push(RenderCommand::Text {
                x: col_temp,
                y: cy,
                text: format_temp(hf.temp_c, self.settings.temp_unit),
                font_size: 13.0,
                color: self.palette.text,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cmds.push(RenderCommand::Text {
                x: col_cond,
                y: cy,
                text: hf.condition.description().to_string(),
                font_size: 13.0,
                color: self.palette.subtext1,
                font_weight: FontWeightHint::Regular,
                max_width: Some(170.0),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.push(RenderCommand::Text {
                x: col_precip,
                y: cy,
                text: format!("{}%", hf.precip_pct),
                font_size: 13.0,
                color: if hf.precip_pct > 30 {
                    self.palette.ink(self.palette.blue)
                } else {
                    self.palette.subtext0
                },
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cy += 24.0;
        }
    }

    /// Render daily detail view.
    fn render_daily_detail(&self, cmds: &mut Frame<Target>, y: f32) {
        let padding = 16.0;
        let mut cy = y + padding;

        cmds.push(RenderCommand::Text {
            x: padding,
            y: cy,
            text: "7-Day Forecast Detail".to_string(),
            font_size: 18.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += 36.0;

        for day in &self.daily {
            // Day card
            let card_h = 100.0;
            self.palette.push_surface(
                cmds,
                padding,
                cy,
                self.width - padding * 2.0,
                card_h,
                10.0,
                Surface::Card,
            );

            cmds.push(RenderCommand::Text {
                x: padding + 16.0,
                y: cy + 12.0,
                text: day.day_name.clone(),
                font_size: 16.0,
                color: self.palette.text,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            cmds.push(RenderCommand::Text {
                x: padding + 16.0,
                y: cy + 36.0,
                text: day.condition.description().to_string(),
                font_size: 13.0,
                color: self.palette.subtext1,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
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
                color: self.palette.text,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
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
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(self.width - padding * 2.0 - 220.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Render condition icon lines
            let icon_x = self.width - padding - 160.0;
            let icon_color = day.condition.icon_color(&self.palette);
            for (j, line) in day.condition.icon_lines().iter().enumerate() {
                cmds.push(RenderCommand::Text {
                    x: icon_x,
                    y: cy + 14.0 + j as f32 * 14.0,
                    text: line.to_string(),
                    font_size: 11.0,
                    color: icon_color,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }

            cy += card_h + 12.0;
        }
    }

    /// Render alerts view.
    fn render_alerts_view(&self, cmds: &mut Frame<Target>, y: f32) {
        let padding = 16.0;
        let mut cy = y + padding;

        cmds.push(RenderCommand::Text {
            x: padding,
            y: cy,
            text: "Weather Alerts".to_string(),
            font_size: 18.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += 36.0;

        // Open-Meteo gives no warnings, so an empty list here is not "none in
        // force", and "No active alerts" -- what this said -- would have read
        // as exactly that. Said as it is, with where warnings do come from.
        if self.alerts.is_empty() {
            self.paragraphs(
                cmds,
                cy,
                &[
                    "This app gives no severe-weather warnings.",
                    "Open-Meteo, where its forecasts come from, has none to give, so nothing here means none are in force. Your national weather service gives them.",
                ],
                self.palette.ink(self.palette.yellow),
            );
            return;
        }

        for alert in &self.alerts {
            // `RenderCommand::Text` clips at `max_width` rather than wrapping,
            // so the description used to come out as its first line and no
            // more. An alert's description is the part that says what to
            // actually do about the weather, so it is wrapped and the card
            // grows to hold it — the alerts below are a stacked list, and a
            // card that did not grow would be overlapped by the next one.
            let text_width = self.width - padding * 2.0 - 40.0;
            let description = text::wrap(
                &alert.description,
                text_width,
                ALERT_BODY_FONT_SIZE,
                FontWeightHint::Regular,
            );
            let body_height = description.len() as f32 * ALERT_BODY_LINE_HEIGHT;
            // 90.0 keeps the familiar card size for the one- and two-line
            // descriptions that are the common case.
            let card_h = (ALERT_BODY_TOP + body_height + 12.0).max(90.0);
            let severity_color = alert.severity.color(&self.palette);

            // Card background
            self.palette.push_surface(
                cmds,
                padding,
                cy,
                self.width - padding * 2.0,
                card_h,
                10.0,
                Surface::Card,
            );

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
                // The card behind it is this colour too, at a lower alpha,
                // so the headline is effectively on the page. 837.
                color: self.palette.ink(severity_color),
                font_weight: FontWeightHint::Bold,
                max_width: Some(self.width - padding * 2.0 - 40.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Description, one command per wrapped line.
            for (n, line) in description.iter().enumerate() {
                cmds.push(RenderCommand::Text {
                    x: padding + 20.0,
                    y: cy + ALERT_BODY_TOP + n as f32 * ALERT_BODY_LINE_HEIGHT,
                    text: line.clone(),
                    font_size: ALERT_BODY_FONT_SIZE,
                    color: self.palette.subtext1,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(text_width),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            cy += card_h + 12.0;
        }
    }

    /// Render locations view.
    fn render_locations_view(&self, cmds: &mut Frame<Target>, y: f32) {
        let padding = 16.0;
        let mut cy = y + padding;
        let width = (self.width - padding * 2.0).max(0.0);

        cmds.push(RenderCommand::Text {
            x: padding,
            y: cy,
            text: "Places".to_string(),
            font_size: 18.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += 36.0;

        if self.source.on {
            cy = self.render_place_search(cmds, padding, cy, width);
        } else {
            // Searching asks Open-Meteo too, so it waits for forecasts.
            let below = self.paragraphs(
                cmds,
                cy,
                &[
                    "Forecasts are off.",
                    "Searching for a place sends its name to Open-Meteo, so it waits until forecasts are on.",
                ],
                self.palette.ink(self.palette.yellow),
            );
            self.button(
                cmds,
                padding,
                below + 2.0,
                "Turn on forecasts",
                Target::TurnOn,
            );
            cy = below + 48.0;
        }

        for (i, loc) in self.locations.iter().enumerate() {
            let row_h = 48.0;
            let is_active = i == self.active_location_idx;

            // Row background
            self.palette.push_surface(
                cmds,
                padding,
                cy,
                width,
                row_h,
                8.0,
                if is_active || self.hover == Some(Target::Location(i)) {
                    Surface::Selected
                } else {
                    Surface::Card
                },
            );
            let remove_w = text::measure("Remove", 12.0, FontWeightHint::Bold) + 20.0;
            let remove = Rect::new(padding + width - remove_w - 10.0, cy + 10.0, remove_w, 28.0);
            cmds.hit(
                Target::Location(i),
                Rect::new(padding, cy, (width - remove_w - 20.0).max(0.0), row_h),
            );

            // Active indicator
            if is_active {
                cmds.push(RenderCommand::FillRect {
                    x: padding,
                    y: cy,
                    width: 4.0,
                    height: row_h,
                    color: self.palette.blue,
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
                color: self.palette.text,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some((width - remove_w - 50.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });

            // Default badge
            if loc.is_default {
                cmds.push(RenderCommand::FillRect {
                    x: padding + 20.0,
                    y: cy + 28.0,
                    width: 56.0,
                    height: 16.0,
                    color: Color::rgba(
                        self.palette.blue.r,
                        self.palette.blue.g,
                        self.palette.blue.b,
                        40,
                    ),
                    corner_radii: CornerRadii::all(4.0),
                });
                cmds.push(RenderCommand::Text {
                    x: padding + 26.0,
                    y: cy + 30.0,
                    text: "Default".to_string(),
                    font_size: 10.0,
                    color: self.palette.ink(self.palette.blue),
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }

            // Take it out of the list.
            self.palette.push_surface(
                cmds,
                remove.x,
                remove.y,
                remove.w,
                remove.h,
                6.0,
                if self.hover == Some(Target::RemovePlace(i)) {
                    Surface::Selected
                } else {
                    Surface::Card
                },
            );
            cmds.push(RenderCommand::Text {
                x: remove.x + 10.0,
                y: remove.y + (remove.h - text::line_height(12.0, FontWeightHint::Bold)) / 2.0,
                text: "Remove".to_string(),
                font_size: 12.0,
                color: self.palette.text,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cmds.hit(Target::RemovePlace(i), remove);

            cy += row_h + 8.0;
        }
    }

    /// The place-name search: the box, its button, and what it found -- or
    /// that it is asking, found nothing, or failed. Returns the y below.
    fn render_place_search(&self, cmds: &mut Frame<Target>, x: f32, y: f32, width: f32) -> f32 {
        let go_w = text::measure("Search", 13.0, FontWeightHint::Bold) + 28.0;
        let field = Rect::new(x, y, (width - go_w - 8.0).max(0.0), 32.0);
        guitk::field::draw(
            cmds,
            &self.palette,
            field,
            guitk::field::State {
                hovered: self.hover == Some(Target::SearchBox),
                focused: true,
                disabled: false,
                invalid: false,
            },
            guitk::style::FOCUS_RING_WIDTH,
        );
        cmds.hit(Target::SearchBox, field);
        let area = Rect::new(
            field.x + 10.0,
            field.y + 6.0,
            (field.w - 20.0).max(0.0),
            20.0,
        );
        let mut typed = RenderTree::new();
        if self.search.text().is_empty() {
            cmds.push(RenderCommand::Text {
                x: area.x,
                y: area.y,
                text: "Type a place's name, then Enter\u{2026}".to_string(),
                color: self.palette.subtext0,
                font_size: SEARCH_FONT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(area.w),
                overflow: TextOverflow::Ellipsis,
            });
            textedit::push_caret(
                &mut typed,
                area.x,
                area.y,
                area.h,
                self.palette.text,
                textedit::CARET_WIDTH,
            );
        } else {
            textedit::draw(
                &mut typed,
                &textedit::SingleLine {
                    text: self.search.text(),
                    cursor: self.search.cursor(),
                    selection_anchor: self.search.selection_anchor(),
                    focused: true,
                    x: area.x,
                    y: area.y,
                    width: area.w,
                    line_height: area.h,
                    font_size: SEARCH_FONT,
                    weight: FontWeightHint::Regular,
                    color: self.palette.text,
                    selection_bg: self.palette.accent,
                    selection_fg: self.palette.crust,
                    caret_width: textedit::CARET_WIDTH,
                },
            );
        }
        for command in typed.commands {
            cmds.push(command);
        }
        self.button(
            cmds,
            field.right() + 8.0,
            y + 1.0,
            "Search",
            Target::SearchGo,
        );
        let mut cy = y + 44.0;

        // What the search found, or why it shows nothing.
        let note = if let Some(name) = self.source.search_asked() {
            Some(format!(
                "Asking Open-Meteo for places called \u{201c}{name}\u{201d}\u{2026}"
            ))
        } else if let Some(error) = &self.source.search_error {
            Some(error.clone())
        } else if let (Some(name), true) = (&self.source.searched, self.source.found.is_empty()) {
            Some(format!("No place called \u{201c}{name}\u{201d}"))
        } else {
            None
        };
        if let Some(note) = note {
            cmds.push(RenderCommand::Text {
                x,
                y: cy,
                text: note,
                color: self.palette.subtext0,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
                overflow: TextOverflow::Ellipsis,
            });
            cy += 28.0;
        }
        for (i, place) in self.source.found.iter().enumerate() {
            let row = Rect::new(x, cy, width, 32.0);
            self.palette.push_surface(
                cmds,
                row.x,
                row.y,
                row.w,
                row.h,
                6.0,
                if self.hover == Some(Target::Found(i)) {
                    Surface::Selected
                } else {
                    Surface::Card
                },
            );
            cmds.push(RenderCommand::Text {
                x: row.x + 12.0,
                y: row.y + 8.0,
                text: format!("+  {}", place.label()),
                color: self.palette.text,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some((row.w - 24.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.hit(Target::Found(i), row);
            cy += 38.0;
        }
        if !self.source.found.is_empty() {
            cy += 8.0;
        }
        cy
    }

    /// Render settings view.
    fn render_settings_view(&self, cmds: &mut Frame<Target>, y: f32) {
        let padding = 16.0;
        let mut cy = y + padding;

        cmds.push(RenderCommand::Text {
            x: padding,
            y: cy,
            text: "Settings".to_string(),
            font_size: 18.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += 36.0;

        // The update interval is not here. It was shown as "30 min" with no
        // way to change it and nothing behind it -- there is no source to
        // refresh (known-issues.md,
        // TD-C-WEATHER-HAS-A-REFRESH-INTERVAL-AND-NOTHING-TO-REFRESH, whose
        // proper fix says the control should be absent until it drives
        // something).
        let settings_items: Vec<(Setting, &str, String)> = vec![
            // First, because it decides whether anything leaves the machine
            // (design-decisions §1236). Its value says what each answer
            // means, so a press on it is made knowing what it sends.
            (
                Setting::Forecasts,
                "Forecasts from Open-Meteo",
                if self.source.on {
                    "On \u{2014} each place's position goes to open-meteo.com, in plain text"
                        .to_string()
                } else {
                    "Off \u{2014} nothing is sent; on, the places you add go to open-meteo.com"
                        .to_string()
                },
            ),
            (
                Setting::Temperature,
                "Temperature Unit",
                match self.settings.temp_unit {
                    TempUnit::Celsius => "Celsius (\u{00B0}C)".to_string(),
                    TempUnit::Fahrenheit => "Fahrenheit (\u{00B0}F)".to_string(),
                },
            ),
            (
                Setting::Wind,
                "Wind Speed Unit",
                self.settings.wind_unit.label().to_string(),
            ),
            (
                Setting::Pressure,
                "Pressure Unit",
                self.settings.pressure_unit.label().to_string(),
            ),
            (
                Setting::Time,
                "Time Format",
                match self.settings.time_format {
                    TimeFormat::H12 => "12-hour".to_string(),
                    TimeFormat::H24 => "24-hour".to_string(),
                },
            ),
        ];

        for (setting, label, value) in &settings_items {
            let row_h = 50.0;
            let row = Rect::new(padding, cy, self.width - padding * 2.0, row_h);

            self.palette.push_surface(
                cmds,
                row.x,
                row.y,
                row.w,
                row.h,
                8.0,
                if self.hover == Some(Target::Setting(*setting)) {
                    Surface::Selected
                } else {
                    Surface::Card
                },
            );
            // What a press does, and the key that does the same.
            let hint = match setting.key() {
                Some(key) => format!("Click to change  ·  {key}"),
                None => "Click to change".to_string(),
            };
            let hint_w = text::measure(&hint, 12.0, FontWeightHint::Regular);
            cmds.push(RenderCommand::Text {
                x: text::right_x(&hint, row.right() - 16.0, 12.0, FontWeightHint::Regular),
                y: cy + 18.0,
                text: hint,
                font_size: 12.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cmds.hit(Target::Setting(*setting), row);

            cmds.push(RenderCommand::Text {
                x: padding + 16.0,
                y: cy + 10.0,
                text: label.to_string(),
                font_size: 13.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Up to the hint, not under it: the forecast row's value is a
            // sentence, which a narrow window ends with an ellipsis.
            cmds.push(RenderCommand::Text {
                x: padding + 16.0,
                y: cy + 28.0,
                text: value.clone(),
                font_size: 15.0,
                color: self.palette.text,
                font_weight: FontWeightHint::Bold,
                max_width: Some((row.w - 48.0 - hint_w).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });

            cy += row_h + 8.0;
        }

        if let Some(error) = &self.settings_error {
            cmds.push(RenderCommand::Text {
                x: padding,
                y: cy + 4.0,
                text: error.clone(),
                font_size: 12.0,
                color: self.palette.ink(self.palette.red),
                font_weight: FontWeightHint::Regular,
                max_width: Some(self.width - padding * 2.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }
}

// ============================================================================
// Pointer targets
// ============================================================================

/// Everything in the window a pointer can press, as the renderer records it.
///
/// The weather app drew six tabs, a settings list and a list of places, and
/// handled no pointer event (`known-issues.md` →
/// `TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Tab(ActiveView),
    /// A settings row: a press moves it to its next value.
    Setting(Setting),
    /// A saved place: a press makes it the one shown.
    Location(usize),
    /// The button that takes a saved place out of the list.
    RemovePlace(usize),
    /// The hourly strip, which the wheel scrolls.
    HourlyStrip,
    HelpCard,
    /// "Turn on forecasts", on the panel that says what that sends.
    TurnOn,
    /// Ask again, after a request failed.
    Retry,
    /// The place-name search box on the Locations tab.
    SearchBox,
    /// Its Search button.
    SearchGo,
    /// A place a search offered: a press adds it and shows its weather.
    Found(usize),
}

/// A row of the settings view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    /// Whether forecasts are on: whether anything is asked of Open-Meteo.
    Forecasts,
    Temperature,
    Wind,
    Pressure,
    Time,
}

impl Setting {
    /// The key that changes the same thing, if one does. Forecasts have
    /// none: turning them on sends things away, and a stray key should not.
    fn key(self) -> Option<&'static str> {
        match self {
            Self::Forecasts => None,
            Self::Temperature => Some("U"),
            Self::Wind => Some("W"),
            Self::Pressure => Some("P"),
            Self::Time => Some("T"),
        }
    }
}

/// Where the units, the forecast switch and the places are kept
/// (`settingsfile`): `weather.yaml`.
const CONFIG_NAME: &str = "weather";

impl WeatherApp {
    /// Move `setting` to its next value, and keep the choice.
    fn change_setting(&mut self, setting: Setting) {
        match setting {
            // Kept by `set_forecasts` itself, with the places.
            Setting::Forecasts => {
                self.set_forecasts(!self.source.on);
                return;
            }
            Setting::Temperature => self.toggle_temp_unit(),
            Setting::Wind => self.cycle_wind_unit(),
            Setting::Pressure => self.cycle_pressure_unit(),
            Setting::Time => self.toggle_time_format(),
        }
        self.store_units();
    }

    /// The units as words, for the settings file.
    fn unit_words(&self) -> [(&'static str, &'static str); 4] {
        [
            (
                "temperature",
                match self.settings.temp_unit {
                    TempUnit::Celsius => "celsius",
                    TempUnit::Fahrenheit => "fahrenheit",
                },
            ),
            (
                "wind",
                match self.settings.wind_unit {
                    WindSpeedUnit::Kmh => "kmh",
                    WindSpeedUnit::Mph => "mph",
                    WindSpeedUnit::Ms => "ms",
                    WindSpeedUnit::Knots => "knots",
                },
            ),
            (
                "pressure",
                match self.settings.pressure_unit {
                    PressureUnit::Hpa => "hpa",
                    PressureUnit::InHg => "inhg",
                    PressureUnit::MmHg => "mmhg",
                },
            ),
            (
                "time",
                match self.settings.time_format {
                    TimeFormat::H24 => "24h",
                    TimeFormat::H12 => "12h",
                },
            ),
        ]
    }

    /// Keep the units in the user's settings, so a choice outlives the window.
    /// They were reset to Celsius, km/h, hPa and 24-hour at every start.
    fn store_units(&mut self) {
        let mut doc = settingsfile::load(CONFIG_NAME);
        for (key, word) in self.unit_words() {
            doc.set_str(&["units", key], word);
        }
        self.settings_error = settingsfile::store(CONFIG_NAME, &doc)
            .err()
            .map(|e| format!("The units could not be kept for next time: {e}"));
    }

    /// Read the units from the user's settings. A word this does not know
    /// leaves that unit at its default.
    pub fn load_units(&mut self, doc: &yamldoc::Document) {
        let word = |key: &str| doc.get_str(&["units", key]);
        match word("temperature").as_deref() {
            Some("celsius") => self.settings.temp_unit = TempUnit::Celsius,
            Some("fahrenheit") => self.settings.temp_unit = TempUnit::Fahrenheit,
            _ => {}
        }
        match word("wind").as_deref() {
            Some("kmh") => self.settings.wind_unit = WindSpeedUnit::Kmh,
            Some("mph") => self.settings.wind_unit = WindSpeedUnit::Mph,
            Some("ms") => self.settings.wind_unit = WindSpeedUnit::Ms,
            Some("knots") => self.settings.wind_unit = WindSpeedUnit::Knots,
            _ => {}
        }
        match word("pressure").as_deref() {
            Some("hpa") => self.settings.pressure_unit = PressureUnit::Hpa,
            Some("inhg") => self.settings.pressure_unit = PressureUnit::InHg,
            Some("mmhg") => self.settings.pressure_unit = PressureUnit::MmHg,
            _ => {}
        }
        match word("time").as_deref() {
            Some("24h") => self.settings.time_format = TimeFormat::H24,
            Some("12h") => self.settings.time_format = TimeFormat::H12,
            _ => {}
        }
    }

    /// Read the units again after the desktop said `weather.yaml` changed --
    /// a unit changed in another window, or a hand edit (§1418, §1434). From
    /// the defaults, as at startup, so a file deleted reads as them. Whether
    /// anything changed.
    fn reread_units(&mut self) -> bool {
        let before = std::mem::take(&mut self.settings);
        self.load_units(&settingsfile::load(CONFIG_NAME));
        before != self.settings
    }

    /// What is under `(x, y)` in the frame last shown.
    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        if self.last_hits.is_empty() {
            return self.frame().hit_test(x, y);
        }
        self.last_hits
            .iter()
            .rev()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(target, _)| *target)
    }

    /// Route a pointer event.
    fn handle_mouse(&mut self, event: &MouseEvent) -> EventResult {
        // The card is modal: a press anywhere puts it away, and nothing under
        // it hears one.
        if self.show_help {
            if matches!(event.kind, MouseEventKind::Press(_)) {
                self.show_help = false;
                return EventResult::Consumed;
            }
            return EventResult::Ignored;
        }
        match event.kind {
            MouseEventKind::Press(MouseButton::Left) => self
                .frame()
                .hit_test(event.x, event.y)
                .map_or(EventResult::Ignored, |target| self.activate(target)),
            MouseEventKind::Move => {
                let over = self.target_at(event.x, event.y);
                if over == self.hover {
                    return EventResult::Ignored;
                }
                self.hover = over;
                EventResult::Consumed
            }
            MouseEventKind::Leave => {
                if self.hover.take().is_some() {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            MouseEventKind::Scroll { dx, dy } => {
                if self.target_at(event.x, event.y) != Some(Target::HourlyStrip) {
                    return EventResult::Ignored;
                }
                // Sideways either way: the vertical wheel is the one most
                // people have, and the strip only goes across.
                let turn = if dx.abs() > dy.abs() { -dx } else { dy };
                let rows = self.wheel.rows(turn);
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "a wheel turn is a handful of notches"
                )]
                let delta = rows as f32 * Self::HOURLY_SCROLL_STEP;
                self.scroll_and_report(delta)
            }
            _ => EventResult::Ignored,
        }
    }

    /// Do what pressing `target` means.
    fn activate(&mut self, target: Target) -> EventResult {
        match target {
            Target::Tab(view) => self.set_view(view),
            Target::Setting(setting) => {
                self.change_setting(setting);
                EventResult::Consumed
            }
            // The place already shown is shown; asking for it again is Try
            // again's, or R's.
            Target::Location(i) => {
                if i >= self.locations.len() || i == self.active_location_idx {
                    return EventResult::Ignored;
                }
                self.show_location(i);
                EventResult::Consumed
            }
            Target::RemovePlace(i) => {
                if self.remove_place(i) {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Target::HelpCard => {
                self.show_help = false;
                EventResult::Consumed
            }
            Target::HourlyStrip => EventResult::Ignored,
            // The user's own press, on the panel that says what it sends.
            Target::TurnOn => {
                self.set_forecasts(true);
                EventResult::Consumed
            }
            Target::Retry => {
                self.ask_forecast();
                EventResult::Consumed
            }
            // The box has the keys while the tab is up; a press says so by
            // putting the caret at the end, where typing goes.
            Target::SearchBox => {
                let end = self.search.text().len();
                self.search.set_selection_anchor(None);
                self.search.set_cursor(text::TextCursor::from(end));
                EventResult::Consumed
            }
            Target::SearchGo => {
                let name = self.search.text().to_owned();
                self.search_places(&name);
                EventResult::Consumed
            }
            Target::Found(i) => {
                let Some(place) = self.source.found.get(i).cloned() else {
                    return EventResult::Ignored;
                };
                self.add_place(&place);
                self.search.set_text("");
                EventResult::Consumed
            }
        }
    }
}

// ============================================================================
// Entry point
// ============================================================================

impl App for WeatherApp {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    fn title(&self) -> String {
        match self.locations.get(self.active_location_idx) {
            Some(place) => format!("Weather — {}", place.name),
            None => String::from("Weather"),
        }
    }

    fn attach_waker(&mut self, waker: std::task::Waker) {
        self.source.attach_waker(waker);
    }

    /// An answer has come.
    fn on_wake(&mut self) -> Response {
        if self.pump() {
            Response::Redraw
        } else {
            Response::Idle
        }
    }

    fn initial_size(&self) -> (u32, u32) {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "both are positive constants well inside u32"
        )]
        {
            (self.width as u32, self.height as u32)
        }
    }

    /// A clock only while there is something to wait for: an answer out (the
    /// waker brings it, and the clock is the fallback for a wake that is
    /// missed), or the next time the place shown is asked for again -- half
    /// an hour after its forecast came, five minutes after a request failed.
    /// With forecasts off, or nothing asked, no clock at all -- one would wake
    /// the machine on a schedule to redraw numbers that cannot change
    /// (`known-issues.md` lesson 47).
    fn tick_interval(&self) -> Option<Duration> {
        if self.source.waiting() {
            return Some(Duration::from_millis(500));
        }
        self.next_ask().map(|at| {
            at.saturating_duration_since(std::time::Instant::now())
                .max(Duration::from_secs(1))
        })
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match self.handle_event(event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // Reconciled with the size we are handed rather than trusted from the
        // last `Resize`: the compositor may grant a size that was never asked
        // for, and the first frame is drawn before any `Resize` arrives.
        self.width = width;
        self.height = height;
        let frame = self.frame();
        self.last_hits = frame.hits().to_vec();
        frame.into_tree()
    }
}

fn main() -> ExitCode {
    let mut weather = WeatherApp::new(900.0, 800.0);
    // The units, the forecast switch and the places the user chose last
    // time. `new` reads no file, so a test's window starts with forecasts off
    // whatever the user running the tests has chosen.
    let settings = settingsfile::load(CONFIG_NAME);
    weather.load_units(&settings);
    weather.load_places(&settings);
    // Turned on in an earlier session: the user's own choice, kept.
    weather.ask_forecast();
    app::launch("weather", &mut weather)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that overflows, indexes out of range or unwraps a `None` should
    // fail loudly and point at the line that did it — that is the diagnosis.
    // The defensive lints exist to keep panics out of code that runs on a
    // user's data, which this is not.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    // ------------------------------------------------------------------
    // Events
    //
    // The app had no input handling at all until it was wired to the
    // compositor, so every one of these covers a path with no prior coverage.
    // ------------------------------------------------------------------

    use guitk::event::Modifiers;

    fn press(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        })
    }

    /// Every string the window draws, joined.
    fn drawn(app: &WeatherApp) -> String {
        app.render_commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// **Every key the list advertises is one this program answers.**
    ///
    /// The hourly strip has to be scrollable for `Left`/`Right`/`Home` to have
    /// work, which is what the sample weather provides -- an app with no
    /// forecast in it refuses them correctly.
    #[test]
    fn every_advertised_key_does_something() {
        settingsfile::testing::with_scratch_config("wx_advertised", |_| {
            for (label, what) in SHORTCUTS {
                for stroke in guitk::shortcut::keystrokes(label).unwrap_or_else(|e| panic!("{e}")) {
                    // Two views, because `set_view` deliberately answers `Ignored`
                    // for the view you are already looking at -- pressing `2` on
                    // the hourly page redraws nothing, on purpose. So no single
                    // state can answer all six digits, and the union of these two
                    // answers every one. Enter is the place search's, so it is
                    // tried on the places tab -- and only Enter: the search box
                    // there takes every letter, and would answer for a letter
                    // whose binding was broken.
                    let views: &[ActiveView] = if stroke.key == Key::Enter {
                        &[ActiveView::Locations]
                    } else {
                        &[ActiveView::Dashboard, ActiveView::HourlyDetail]
                    };
                    let answered = views.iter().copied().any(|view| {
                        let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
                        app.active_view = view;
                        app.scroll_hourly(40.0);
                        // Off the first place, so `Up` has somewhere to go
                        // back to -- stepping past either end is refused, and
                        // correctly.
                        app.step_location(1);
                        app.handle_event(&Event::Key(stroke.clone())) == EventResult::Consumed
                    });
                    assert!(
                        answered,
                        "the list advertises {label:?} for {what:?}, and nothing answers {:?}",
                        stroke.key
                    );
                }
            }
        });
    }

    /// **The shortcut list reaches the window, and nothing acts behind it.**
    #[test]
    fn the_shortcut_list_reaches_the_window() {
        settingsfile::testing::with_scratch_config("wx_card", |_| {
            let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
            assert!(
                !drawn(&app).contains("F1 or ? closes this"),
                "the list is up before anybody asked for it"
            );

            app.handle_event(&press(Key::F1));
            let shown = drawn(&app);
            for (keys, what) in SHORTCUTS {
                assert!(shown.contains(keys), "{keys:?} never reached the window");
                assert!(shown.contains(what), "{what:?} never reached the window");
            }

            // `U` behind the card must not change the units of a reading the
            // reader cannot see.
            let units = app.settings.temp_unit;
            app.handle_event(&press(Key::U));
            assert_eq!(
                app.settings.temp_unit, units,
                "U changed the units through the card"
            );

            app.handle_event(&press(Key::Escape));
            assert!(
                !drawn(&app).contains("F1 or ? closes this"),
                "Escape did not close it"
            );

            app.handle_event(&press(Key::U));
            assert_ne!(
                app.settings.temp_unit, units,
                "control: U does nothing even with the card down"
            );
        });
    }

    #[test]
    fn the_number_row_reaches_every_view() {
        let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
        let expected = [
            (Key::Num2, ActiveView::HourlyDetail),
            (Key::Num3, ActiveView::DailyDetail),
            (Key::Num4, ActiveView::Alerts),
            (Key::Num5, ActiveView::Locations),
            (Key::Num6, ActiveView::SettingsView),
            (Key::Num1, ActiveView::Dashboard),
        ];
        for (k, view) in expected {
            assert_eq!(app.handle_event(&press(k)), EventResult::Consumed);
            assert_eq!(app.active_view, view, "{k:?} went to the wrong view");
        }
    }

    #[test]
    fn asking_for_the_view_already_shown_is_not_a_redraw() {
        // Answering `Consumed` here would redraw an identical frame on every
        // press of the key for the view already on screen.
        let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
        assert_eq!(app.active_view, ActiveView::Dashboard);
        assert_eq!(app.handle_event(&press(Key::Num1)), EventResult::Ignored);
    }

    #[test]
    fn tab_cycles_the_views_and_comes_back_round() {
        let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
        let first = app.active_view;
        let mut seen = vec![first];
        for _ in 0..5 {
            app.handle_event(&press(Key::Tab));
            seen.push(app.active_view);
        }
        assert_eq!(seen.len(), 6, "six views");
        for (i, a) in seen.iter().enumerate() {
            for b in seen.iter().skip(i + 1) {
                assert_ne!(a, b, "Tab visited a view twice before visiting them all");
            }
        }
        app.handle_event(&press(Key::Tab));
        assert_eq!(app.active_view, first, "Tab should wrap to the start");
    }

    #[test]
    fn a_key_the_app_has_no_use_for_is_not_consumed() {
        // An app that consumes everything stops the compositor routing keys
        // anywhere else, and redraws on each one.
        let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
        assert_eq!(app.handle_event(&press(Key::F9)), EventResult::Ignored);
    }

    #[test]
    fn a_key_release_does_nothing() {
        let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
        let release = Event::Key(KeyEvent {
            key: Key::Num5,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        assert_eq!(app.handle_event(&release), EventResult::Ignored);
        assert_eq!(app.active_view, ActiveView::Dashboard);
    }

    #[test]
    fn the_horizontal_arrows_scroll_the_hourly_strip_and_stop_at_the_start() {
        let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
        assert_eq!(app.handle_event(&press(Key::Left)), EventResult::Ignored);
        assert!(app.hourly_scroll_offset <= 0.0, "already at the start");
        assert_eq!(app.handle_event(&press(Key::Right)), EventResult::Consumed);
        assert!(app.hourly_scroll_offset > 0.0);
        assert_eq!(app.handle_event(&press(Key::Left)), EventResult::Consumed);
        assert!(
            app.hourly_scroll_offset <= 0.0,
            "one step back from one step forward is the start"
        );
    }

    #[test]
    fn home_returns_the_strip_to_its_first_hour() {
        let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
        for _ in 0..5 {
            app.handle_event(&press(Key::Right));
        }
        assert!(app.hourly_scroll_offset > 0.0);
        assert_eq!(app.handle_event(&press(Key::Home)), EventResult::Consumed);
        assert!((app.hourly_scroll_offset - 0.0).abs() < f32::EPSILON);
        // And once there, Home is not a redraw.
        assert_eq!(app.handle_event(&press(Key::Home)), EventResult::Ignored);
    }

    #[test]
    fn the_vertical_arrows_walk_the_saved_locations_without_wrapping() {
        let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
        let n = app.locations.len();
        assert!(n >= 2, "the default locations should have more than one");
        assert_eq!(app.active_location_idx, 0);
        // Up at the top stays put rather than jumping to the last city.
        assert_eq!(app.handle_event(&press(Key::Up)), EventResult::Ignored);
        assert_eq!(app.active_location_idx, 0);
        for i in 1..n {
            assert_eq!(app.handle_event(&press(Key::Down)), EventResult::Consumed);
            assert_eq!(app.active_location_idx, i);
        }
        // And Down at the bottom likewise.
        assert_eq!(app.handle_event(&press(Key::Down)), EventResult::Ignored);
        assert_eq!(app.active_location_idx, n - 1);
    }

    #[test]
    fn the_unit_keys_change_the_units_they_are_named_for() {
        settingsfile::testing::with_scratch_config("wx_unit_keys", |_| {
            let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
            let before = app.settings.clone();
            app.handle_event(&press(Key::U));
            assert_ne!(app.settings.temp_unit, before.temp_unit);
            assert_eq!(
                app.settings.wind_unit, before.wind_unit,
                "the temperature key should not touch the wind unit"
            );
            app.handle_event(&press(Key::W));
            assert_ne!(app.settings.wind_unit, before.wind_unit);
            app.handle_event(&press(Key::P));
            assert_ne!(app.settings.pressure_unit, before.pressure_unit);
            app.handle_event(&press(Key::T));
            assert_ne!(app.settings.time_format, before.time_format);
        });
    }

    /// **A key held with Ctrl, Alt or the Windows key is not the window's**:
    /// each such chord is the window's or the desktop's and arrives carrying
    /// its key -- Alt+U changed the temperature unit, Ctrl+T the clock,
    /// Windows+2 the view and Alt+Down the location.
    #[test]
    fn a_key_held_with_a_modifier_is_not_the_windows() {
        settingsfile::testing::with_scratch_config("wx_chords", |_| {
            let altgr = Modifiers {
                alt: true,
                ..Modifiers::ctrl()
            };
            let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
            let state = |app: &WeatherApp| {
                (
                    app.settings.clone(),
                    app.active_view,
                    app.active_location_idx,
                    app.show_help,
                    app.hourly_scroll_offset.to_bits(),
                )
            };
            let before = state(&app);
            for m in [
                Modifiers::ctrl(),
                Modifiers::alt(),
                Modifiers::super_key(),
                altgr,
            ] {
                for k in [
                    Key::U,
                    Key::W,
                    Key::P,
                    Key::T,
                    Key::Num2,
                    Key::Tab,
                    Key::Down,
                    Key::Right,
                    Key::F1,
                ] {
                    let chord = Event::Key(KeyEvent {
                        key: k,
                        pressed: true,
                        modifiers: m,
                        text: String::new(),
                    });
                    assert_eq!(
                        app.handle_event(&chord),
                        EventResult::Ignored,
                        "{m:?} {k:?} was taken"
                    );
                    assert!(state(&app) == before, "{m:?} {k:?} changed the window");
                }
            }
        });
    }

    #[test]
    fn a_resize_is_taken_but_is_not_itself_a_redraw() {
        let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
        let ev = Event::Resize {
            width: 1280,
            height: 1024,
        };
        assert_eq!(app.handle_event(&ev), EventResult::Ignored);
        assert!((app.width - 1280.0).abs() < f32::EPSILON);
        assert!((app.height - 1024.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_title_names_the_location_on_screen() {
        // The window title is the only place the active city appears outside
        // the app's own chrome, so it has to follow the selection.
        let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
        let first = app.title();
        assert!(first.contains(app.active_location_name()));
        app.handle_event(&press(Key::Down));
        assert!(app.title().contains(app.active_location_name()));
        assert_ne!(first, app.title(), "the title should follow the location");
    }

    #[test]
    fn every_view_renders_without_panicking_at_an_awkward_size() {
        // A view reachable by a keystroke that panics when drawn is a crash the
        // user reaches by pressing a number.
        for view in [
            ActiveView::Dashboard,
            ActiveView::HourlyDetail,
            ActiveView::DailyDetail,
            ActiveView::Alerts,
            ActiveView::Locations,
            ActiveView::SettingsView,
        ] {
            for (w, h) in [(1.0, 1.0), (320.0, 240.0), (3840.0, 2160.0)] {
                let mut app = WeatherApp::with_sample_weather(w, h);
                app.active_view = view;
                let tree = app.render(w, h);
                assert!(
                    !tree.commands.is_empty(),
                    "{view:?} drew nothing at {w}x{h}"
                );
            }
        }
    }
    use super::*;

    /// A fresh app knows no weather, no location, and issues no alert.
    ///
    /// `new` used to fill all five: an observation, an hourly forecast, a
    /// daily forecast, a saved location of "New York, NY" marked as the
    /// user's default, and a "Thunderstorm Watch -- Thunderstorms expected
    /// this afternoon. Stay alert."
    ///
    /// The alert is the one that matters beyond today. A weather app is the
    /// only program in this sweep with a channel whose entire purpose is to
    /// make somebody change their plans for safety, and a fabricated warning
    /// teaches the user that this app *has* such a channel -- so its silence
    /// tomorrow reads as "no warnings in force" rather than "not connected".
    /// The false alert is a one-day problem; the false confidence in the
    /// channel outlives it.
    #[test]
    fn a_fresh_app_knows_no_weather_and_issues_no_alert() {
        let app = WeatherApp::new(900.0, 800.0);
        assert!(
            app.current.is_none(),
            "an observation appeared from nowhere"
        );
        assert!(
            app.hourly.is_empty(),
            "an hourly forecast appeared from nowhere"
        );
        assert!(
            app.daily.is_empty(),
            "a daily forecast appeared from nowhere"
        );
        assert!(
            app.locations.is_empty(),
            "the app decided where the user lives"
        );
        assert!(app.alerts.is_empty(), "a severe-weather alert was invented");
    }

    /// **Forecasts are off, and the window says what turning them on sends**
    /// -- to whom, what, when, and in the open -- and that there are no
    /// warnings either way (design-decisions §1236).
    #[test]
    fn the_window_says_forecasts_are_off_and_what_turning_them_on_sends() {
        let app = WeatherApp::new(900.0, 800.0);
        assert!(!app.source.on, "forecasts are on in a new window");
        let texts: Vec<String> = app
            .render_commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        // Wrapped to the window, so read as one text.
        let said = texts.join(" ");
        for phrase in [
            "Forecasts are off.",
            "open-meteo.com",
            "latitude and longitude",
            "names you search for",
            "every half hour",
            "plain text",
            "Silence here is not an all-clear",
        ] {
            assert!(
                said.contains(phrase),
                "the window never said {phrase:?}: {said}"
            );
        }
        assert!(
            texts.iter().any(|t| t == "Turn on forecasts"),
            "no way to turn them on"
        );

        // And no temperature is drawn. A default `CurrentWeather` would render
        // as a perfectly plausible 0 degrees and Clear, which is a reading.
        assert!(
            !texts.iter().any(|t| t.contains('\u{00B0}')),
            "a temperature was drawn for an observation that was never taken",
        );
    }

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
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let conditions = [
            WeatherCondition::Clear,
            WeatherCondition::Rain,
            WeatherCondition::Tornado,
        ];
        for cond in &conditions {
            let c = cond.icon_color(&pal);
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
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        for sev in &[
            UvSeverity::Low,
            UvSeverity::Moderate,
            UvSeverity::High,
            UvSeverity::VeryHigh,
            UvSeverity::Extreme,
        ] {
            assert_eq!(sev.color(&pal).a, 255);
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
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        for aq in &[
            AirQuality::Good,
            AirQuality::Moderate,
            AirQuality::UnhealthySensitive,
            AirQuality::Unhealthy,
            AirQuality::VeryUnhealthy,
            AirQuality::Hazardous,
        ] {
            assert_eq!(aq.color(&pal).a, 255);
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
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        // Advisory = pal.yellow, Watch = pal.peach, Warning = pal.red
        assert_eq!(AlertSeverity::Advisory.color(&pal), pal.yellow);
        assert_eq!(AlertSeverity::Watch.color(&pal), pal.peach);
        assert_eq!(AlertSeverity::Warning.color(&pal), pal.red);
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
        let app = WeatherApp::with_sample_weather(800.0, 600.0);
        assert_eq!(app.width, 800.0);
        assert_eq!(app.height, 600.0);
        assert_eq!(app.active_view, ActiveView::Dashboard);
        assert_eq!(app.hourly_scroll_offset, 0.0);
    }

    #[test]
    fn test_app_active_location_name() {
        let app = WeatherApp::with_sample_weather(800.0, 600.0);
        assert_eq!(app.active_location_name(), "New York, NY");
    }

    #[test]
    fn test_app_set_active_location() {
        let mut app = WeatherApp::with_sample_weather(800.0, 600.0);
        app.set_active_location(1);
        assert_eq!(app.active_location_name(), "London, UK");
    }

    #[test]
    fn test_app_set_active_location_out_of_bounds() {
        let mut app = WeatherApp::with_sample_weather(800.0, 600.0);
        app.set_active_location(100);
        assert_eq!(app.active_location_idx, 0); // unchanged
    }

    #[test]
    fn test_app_add_location() {
        let mut app = WeatherApp::with_sample_weather(800.0, 600.0);
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
        let mut app = WeatherApp::with_sample_weather(800.0, 600.0);
        app.locations.clear();
        app.add_location("Only City".to_string());
        assert!(app.locations[0].is_default);
    }

    #[test]
    fn test_app_remove_location() {
        let mut app = WeatherApp::with_sample_weather(800.0, 600.0);
        assert!(app.remove_location(1));
        assert_eq!(app.locations.len(), 2);
    }

    #[test]
    fn test_app_remove_location_out_of_bounds() {
        let mut app = WeatherApp::with_sample_weather(800.0, 600.0);
        assert!(!app.remove_location(100));
    }

    #[test]
    fn test_app_remove_default_promotes_first() {
        let mut app = WeatherApp::with_sample_weather(800.0, 600.0);
        // locations[0] is default
        app.remove_location(0);
        assert!(app.locations[0].is_default);
    }

    #[test]
    fn test_app_remove_active_clamps() {
        let mut app = WeatherApp::with_sample_weather(800.0, 600.0);
        app.set_active_location(2); // last
        app.remove_location(2);
        assert!(app.active_location_idx < app.locations.len());
    }

    /// **Removing a place above the one shown leaves it shown.** The index
    /// stayed put while the list moved up under it, so it named the place
    /// below, and the window jumped there -- and asked for its weather.
    #[test]
    fn removing_a_place_above_the_one_shown_leaves_it_shown() {
        let mut app = WeatherApp::with_sample_weather(800.0, 600.0);
        app.set_active_location(1);
        let shown = app.locations[1].name.clone();
        assert!(app.remove_location(0));
        assert_eq!(app.locations[app.active_location_idx].name, shown);
        // Below it: nothing moves.
        app.set_active_location(0);
        let shown = app.locations[0].name.clone();
        assert!(app.remove_location(1));
        assert_eq!(app.locations[app.active_location_idx].name, shown);
    }

    #[test]
    fn test_app_reorder_location() {
        let mut app = WeatherApp::with_sample_weather(800.0, 600.0);
        assert!(app.reorder_location(0, 2));
        assert_eq!(app.locations[2].name, "New York, NY");
    }

    #[test]
    fn test_app_reorder_out_of_bounds() {
        let mut app = WeatherApp::with_sample_weather(800.0, 600.0);
        assert!(!app.reorder_location(0, 100));
    }

    #[test]
    fn test_app_set_default_location() {
        let mut app = WeatherApp::with_sample_weather(800.0, 600.0);
        assert!(app.set_default_location(2));
        assert!(app.locations[2].is_default);
        assert!(!app.locations[0].is_default);
    }

    #[test]
    fn test_app_set_default_out_of_bounds() {
        let mut app = WeatherApp::with_sample_weather(800.0, 600.0);
        assert!(!app.set_default_location(100));
    }

    #[test]
    fn test_app_toggle_temp_unit() {
        let mut app = WeatherApp::with_sample_weather(800.0, 600.0);
        assert_eq!(app.settings.temp_unit, TempUnit::Celsius);
        app.toggle_temp_unit();
        assert_eq!(app.settings.temp_unit, TempUnit::Fahrenheit);
        app.toggle_temp_unit();
        assert_eq!(app.settings.temp_unit, TempUnit::Celsius);
    }

    #[test]
    fn test_app_cycle_wind_unit() {
        let mut app = WeatherApp::with_sample_weather(800.0, 600.0);
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
        let mut app = WeatherApp::with_sample_weather(800.0, 600.0);
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
        let mut app = WeatherApp::with_sample_weather(800.0, 600.0);
        assert_eq!(app.settings.time_format, TimeFormat::H24);
        app.toggle_time_format();
        assert_eq!(app.settings.time_format, TimeFormat::H12);
        app.toggle_time_format();
        assert_eq!(app.settings.time_format, TimeFormat::H24);
    }

    #[test]
    fn test_app_scroll_hourly_positive() {
        let mut app = WeatherApp::with_sample_weather(800.0, 600.0);
        app.scroll_hourly(100.0);
        assert!((app.hourly_scroll_offset - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_app_scroll_hourly_no_negative() {
        let mut app = WeatherApp::with_sample_weather(800.0, 600.0);
        app.scroll_hourly(-100.0);
        assert_eq!(app.hourly_scroll_offset, 0.0);
    }

    #[test]
    fn test_app_scroll_hourly_capped() {
        let mut app = WeatherApp::with_sample_weather(800.0, 600.0);
        app.scroll_hourly(100_000.0);
        let max = app.hourly.len() as f32 * 80.0;
        assert!(app.hourly_scroll_offset <= max);
    }

    // --- Rendering tests ---

    #[test]
    fn test_render_produces_commands() {
        let app = WeatherApp::with_sample_weather(900.0, 800.0);
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_starts_with_background() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let app = WeatherApp::with_sample_weather(900.0, 800.0);
        let cmds = app.render_commands();
        match &cmds[0] {
            RenderCommand::FillRect { x, y, color, .. } => {
                assert_eq!(*x, 0.0);
                assert_eq!(*y, 0.0);
                assert_eq!(*color, pal.base);
            }
            _ => panic!("First command should be a FillRect background"),
        }
    }

    #[test]
    fn test_render_has_text_commands() {
        let app = WeatherApp::with_sample_weather(900.0, 800.0);
        let cmds = app.render_commands();
        let has_text = cmds.iter().any(|c| matches!(c, RenderCommand::Text { .. }));
        assert!(has_text, "Render output should contain text commands");
    }

    #[test]
    fn test_render_has_line_commands() {
        let app = WeatherApp::with_sample_weather(900.0, 800.0);
        let cmds = app.render_commands();
        let has_lines = cmds.iter().any(|c| matches!(c, RenderCommand::Line { .. }));
        assert!(has_lines, "Dashboard should have line commands (graph)");
    }

    #[test]
    fn test_render_alert_banner_when_alerts() {
        let app = WeatherApp::with_sample_weather(900.0, 800.0);
        let cmds = app.render_commands();
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
        let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
        app.alerts.clear();
        let cmds = app.render_commands();
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
        let app = WeatherApp::with_sample_weather(900.0, 800.0);
        let cmds = app.render_commands();
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
        let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
        app.active_view = ActiveView::HourlyDetail;
        let cmds = app.render_commands();
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
        let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
        app.active_view = ActiveView::DailyDetail;
        let cmds = app.render_commands();
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
        let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
        app.active_view = ActiveView::Alerts;
        app.alerts.clear();
        let cmds = app.render_commands();
        let has_no_alerts = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text.contains("This app gives no severe-weather warnings")
            } else {
                false
            }
        });
        assert!(has_no_alerts);
    }

    const LONG_ALERT: &str = "Damaging winds and hail up to two centimetres \
        are expected between four and nine this evening. Secure loose objects \
        outdoors, stay away from windows, and avoid travel on exposed roads \
        until the warning is lifted.";

    /// An app showing the alerts view, with one alert carrying `description`.
    fn app_with_alert(description: &str) -> WeatherApp {
        let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
        app.active_view = ActiveView::Alerts;
        app.alerts = vec![WeatherAlert {
            alert_type: AlertType::Thunderstorm,
            severity: AlertSeverity::Warning,
            title: "Severe Thunderstorm".to_string(),
            description: description.to_string(),
        }];
        app
    }

    /// The `(y, text)` of every alert-description line drawn.
    fn alert_body_lines(app: &WeatherApp) -> Vec<(f32, String)> {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        app.render_commands()
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    y,
                    text,
                    font_size,
                    color,
                    ..
                } if (font_size - ALERT_BODY_FONT_SIZE).abs() < 0.01 && color == pal.subtext1 => {
                    Some((y, text))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_long_alert_description_is_wrapped_not_truncated() {
        // `RenderCommand::Text` clips at `max_width`, so the description used
        // to show its first line only — and the description is the part of an
        // alert that says what to actually do about the weather.
        let app = app_with_alert(LONG_ALERT);
        let lines = alert_body_lines(&app);
        assert!(
            lines.len() > 1,
            "the description was drawn as {} command(s)",
            lines.len()
        );
        let drawn: String = lines
            .iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        for word in LONG_ALERT.split_whitespace() {
            assert!(drawn.contains(word), "the alert lost the word {word:?}");
        }
    }

    /// The alert card's rectangle, however the theme drew it.
    ///
    /// Filled under Cards, outlined under Borders, and the outline reports a
    /// rectangle half a line smaller than the one asked for -- `logical_rect`
    /// undoes that. The 10px corner is what tells an alert card from the
    /// full-width title strip above it, which is also a card and also wide;
    /// colour used to make that distinction and cannot any more, because under
    /// Borders every card is the same outline.
    fn alert_card(app: &WeatherApp, pal: &Palette) -> Option<(f32, f32)> {
        app.render_commands().into_iter().find_map(|c| {
            let (_, y, w, h) = appearance::logical_rect(&c)?;
            let (paints_a_card, radius) = match &c {
                RenderCommand::FillRect {
                    color,
                    corner_radii,
                    ..
                } => (*color == pal.surface0, corner_radii.top_left),
                RenderCommand::StrokeRect {
                    color,
                    corner_radii,
                    ..
                } => (*color == pal.border, corner_radii.top_left),
                _ => (false, 0.0),
            };
            (paints_a_card && w > 400.0 && (radius - 10.0).abs() < 0.01).then_some((y, h))
        })
    }

    #[test]
    fn an_alert_card_grows_to_hold_its_description() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        // Alerts are a stacked list, so a card that did not grow would be
        // overlapped by the next one drawn beneath it.
        //
        // The growing description is built by repetition rather than written
        // out, because how many lines a given sentence wraps to is a fact
        // about the host's fonts, not about this app. `LONG_ALERT` used to
        // wrap to four lines; once `text::wrap` started measuring glyphs
        // instead of estimating from byte counts it wrapped to two, which is
        // under the card's 90px floor — so this test quietly became a check
        // that 90.0 > 90.0 and failed. A repeated phrase overflows the floor
        // whatever the face measures.
        let short = app_with_alert("Winds gusting to 60 km/h.");
        let long = app_with_alert(&"Secure loose objects outdoors. ".repeat(40));

        let card_height = |app: &WeatherApp| -> f32 {
            alert_card(app, &pal)
                .expect("the alerts view drew no card")
                .1
        };

        // The floor covers two lines, so growth is only observable past it.
        // Checked separately from the assertion below so that a description
        // which stopped being long enough reports that, rather than looking
        // like the card refusing to grow.
        let drawn = alert_body_lines(&long).len();
        assert!(
            drawn > 2,
            "the growth check needs a description past the 90px floor, got {drawn} line(s)"
        );
        let long_h = card_height(&long);
        assert!(
            long_h > card_height(&short),
            "a {drawn}-line description got the same {long_h}px card as a one-liner"
        );
        let body_bottom = alert_body_lines(&long)
            .iter()
            .map(|(y, _)| y + ALERT_BODY_LINE_HEIGHT)
            .fold(f32::MIN, f32::max);
        let card_top = alert_card(&long, &pal)
            .expect("the alerts view drew no card")
            .0;
        assert!(
            body_bottom <= card_top + long_h,
            "the description ends at {body_bottom}, past the bottom of its card"
        );
    }

    #[test]
    fn test_render_locations_view() {
        let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
        app.active_view = ActiveView::Locations;
        let cmds = app.render_commands();
        let has_locations = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text == "Places"
            } else {
                false
            }
        });
        assert!(has_locations);
    }

    #[test]
    fn test_render_settings_view() {
        let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
        app.active_view = ActiveView::SettingsView;
        let cmds = app.render_commands();
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
        let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
        app.active_view = ActiveView::SettingsView;
        let cmds = app.render_commands();
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
        let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
        app.active_view = ActiveView::Locations;
        let cmds = app.render_commands();
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
        let app = WeatherApp::with_sample_weather(900.0, 800.0);
        let cmds = app.render_commands();
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
        let app = WeatherApp::with_sample_weather(900.0, 800.0);
        let cmds = app.render_commands();
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
        let app = WeatherApp::with_sample_weather(900.0, 800.0);
        let cmds = app.render_commands();
        let has_shadow = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::BoxShadow { .. }));
        assert!(has_shadow, "Dashboard should have box shadow for cards");
    }

    #[test]
    fn test_render_hourly_strip_clipping() {
        let app = WeatherApp::with_sample_weather(900.0, 800.0);
        let cmds = app.render_commands();
        let has_push_clip = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::PushClip { .. }));
        let has_pop_clip = cmds.iter().any(|c| matches!(c, RenderCommand::PopClip));
        assert!(has_push_clip, "Hourly strip should push clip");
        assert!(has_pop_clip, "Hourly strip should pop clip");
    }

    #[test]
    fn test_current_weather_details_count() {
        let app = WeatherApp::with_sample_weather(900.0, 800.0);
        let details = app.current_weather_details(&sample_current_weather());
        assert_eq!(details.len(), 8); // 8 detail pairs
    }

    #[test]
    fn test_current_weather_details_labels() {
        let app = WeatherApp::with_sample_weather(900.0, 800.0);
        let details = app.current_weather_details(&sample_current_weather());
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
        let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
        app.settings.temp_unit = TempUnit::Fahrenheit;
        let cmds = app.render_commands();
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
        let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
        app.hourly.clear();
        // Should not panic
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_empty_daily() {
        let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
        app.daily.clear();
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_single_hourly_entry() {
        let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
        app.hourly = vec![HourForecast {
            hour: 12,
            temp_c: 20.0,
            condition: WeatherCondition::Clear,
            precip_pct: 0,
        }];
        let cmds = app.render_commands();
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
            let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
            app.active_view = *view;
            let cmds = app.render_commands();
            assert!(
                !cmds.is_empty(),
                "View {view:?} should produce render commands"
            );
        }
    }

    // -- Following the user's theme -------------------------------------------

    /// The window draws in the user's colours rather than in constants of its
    /// own.
    ///
    /// Asserted on the rectangles emitted, not on the `palette` field: a field
    /// that was assigned proves nothing a user would see.
    #[test]
    fn the_window_draws_in_the_theme_it_is_given() {
        fn theme(
            mode: appearance::ThemeMode,
            contrast: Option<appearance::HighContrastScheme>,
        ) -> Palette {
            Palette::from_settings(&appearance::AppearanceSettings {
                theme_mode: mode,
                high_contrast: contrast,
                ..appearance::AppearanceSettings::default()
            })
        }

        fn fills(app: &mut WeatherApp) -> Vec<Color> {
            app.render(1000.0, 700.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let mut app = WeatherApp::with_sample_weather(1000.0, 700.0);

        app.theme_changed(&theme(appearance::ThemeMode::Dark, None));
        let dark = fills(&mut app);
        assert!(!dark.is_empty(), "the window drew no filled rectangles");

        app.theme_changed(&theme(appearance::ThemeMode::Light, None));
        let light = fills(&mut app);
        assert_eq!(dark.len(), light.len(), "the theme changed the layout");
        assert_ne!(
            dark, light,
            "the window drew identically on the dark and light themes, so it \
             is still painting from constants"
        );

        // High contrast is the case a hardcoded palette fails silently: the
        // user asks for maximum legibility and this window alone ignores them.
        app.theme_changed(&theme(
            appearance::ThemeMode::Dark,
            Some(appearance::HighContrastScheme::WhiteOnBlack),
        ));
        assert_ne!(
            dark,
            fills(&mut app),
            "high contrast reached every other surface but not this window"
        );
    }

    // ------------------------------------------------------------------
    // The pointer, and what is shown when nothing was fetched
    //
    // `TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED`.
    // ------------------------------------------------------------------

    use guitk::probe::{self, Probe};

    impl Probe for WeatherApp {
        type Target = Target;
        type Outcome = EventResult;
        const SIZE: (f32, f32) = (900.0, 800.0);

        /// Drawn at the app's own size, which these tests leave at `SIZE`.
        fn draw(&self, _size: (f32, f32)) -> Frame<Target> {
            self.frame()
        }

        fn click_at(
            &mut self,
            x: f32,
            y: f32,
            button: MouseButton,
            _size: (f32, f32),
        ) -> EventResult {
            self.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(button),
            }))
        }

        fn key_at(&mut self, key: &KeyEvent, _size: (f32, f32)) -> EventResult {
            self.handle_event(&Event::Key(key.clone()))
        }

        fn scroll_at(&mut self, x: f32, y: f32, dy: f32, _size: (f32, f32)) -> Option<EventResult> {
            Some(self.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Scroll { dx: 0.0, dy },
            })))
        }
    }

    fn mouse(x: f32, y: f32, kind: MouseEventKind) -> Event {
        Event::Mouse(MouseEvent { x, y, kind })
    }

    /// With nothing fetched -- which is always, today -- F1 raised a card that
    /// was never drawn and that swallowed every key until it was closed.
    #[test]
    fn with_nothing_fetched_the_shortcut_card_is_drawn_and_a_press_closes_it() {
        let mut app = WeatherApp::new(900.0, 800.0);
        assert!(app.current.is_none());
        app.handle_event(&press(Key::F1));
        let shown = drawn(&app);
        for (keys, what) in SHORTCUTS {
            assert!(shown.contains(keys), "{keys:?} is not on the card");
            assert!(shown.contains(what), "{what:?} is not on the card");
        }
        // The card is the whole window while it is up; a press on it anywhere
        // puts it away.
        assert_eq!(
            probe::click(&mut app, Target::HelpCard),
            EventResult::Consumed
        );
        assert!(!app.show_help);
        // And keys reach the app again.
        assert_eq!(app.handle_event(&press(Key::Num2)), EventResult::Consumed);
    }

    /// The settings are not a reading, so they are shown with nothing fetched
    /// -- and a press on a row changes it.
    #[test]
    fn with_nothing_fetched_the_settings_can_be_seen_and_changed() {
        settingsfile::testing::with_scratch_config("wx_settings_view", |_| {
            let mut app = WeatherApp::new(900.0, 800.0);
            assert_eq!(
                probe::click(&mut app, Target::Tab(ActiveView::SettingsView)),
                EventResult::Consumed
            );
            let shown = drawn(&app);
            assert!(shown.contains("Temperature Unit"), "{shown}");
            assert!(shown.contains("Celsius"));
            assert!(
                !shown.contains(FORECASTS_OFF_LINES[0]),
                "the settings are hidden behind the forecasts-off notice"
            );
            probe::click(&mut app, Target::Setting(Setting::Temperature));
            assert_eq!(app.settings.temp_unit, TempUnit::Fahrenheit);
            assert!(drawn(&app).contains("Fahrenheit"));
            probe::click(&mut app, Target::Setting(Setting::Wind));
            assert_eq!(app.settings.wind_unit, WindSpeedUnit::Mph);
            probe::click(&mut app, Target::Setting(Setting::Pressure));
            assert_eq!(app.settings.pressure_unit, PressureUnit::InHg);
            probe::click(&mut app, Target::Setting(Setting::Time));
            assert_eq!(app.settings.time_format, TimeFormat::H12);
            // The other views still say forecasts are off.
            probe::click(&mut app, Target::Tab(ActiveView::Dashboard));
            assert!(drawn(&app).contains(FORECASTS_OFF_LINES[0]));
        });
    }

    /// The interval was shown as "30 min" with no way to change it and
    /// nothing to refresh.
    #[test]
    fn the_update_interval_is_not_offered_while_nothing_refreshes() {
        let mut app = WeatherApp::new(900.0, 800.0);
        app.active_view = ActiveView::SettingsView;
        assert!(!drawn(&app).contains("Update Interval"));
    }

    #[test]
    fn the_units_are_kept_between_sessions() {
        settingsfile::testing::with_scratch_config("wx_units_kept", |_| {
            let mut app = WeatherApp::new(900.0, 800.0);
            app.handle_event(&press(Key::U));
            app.handle_event(&press(Key::W));
            app.handle_event(&press(Key::W));
            app.handle_event(&press(Key::T));
            assert_eq!(app.settings_error, None);
            let mut next = WeatherApp::new(900.0, 800.0);
            next.load_units(&settingsfile::load(CONFIG_NAME));
            assert_eq!(next.settings.temp_unit, TempUnit::Fahrenheit);
            assert_eq!(next.settings.wind_unit, WindSpeedUnit::Ms);
            assert_eq!(next.settings.pressure_unit, PressureUnit::Hpa);
            assert_eq!(next.settings.time_format, TimeFormat::H12);
        });
    }

    /// **A unit changed in one window reaches the others**, when the desktop
    /// says `weather.yaml` changed (§1434): read at startup only, the other
    /// windows showed the old unit until opened again. Another program's
    /// announcement is not this one's; a window's own save announced back
    /// changes nothing; a file deleted reads as the defaults.
    #[test]
    fn a_unit_changed_in_one_window_reaches_the_others() {
        settingsfile::testing::with_scratch_config("wx_units_reread", |dir| {
            let announce = |name: &[u8]| Event::SettingsChanged {
                group: guitk::event::SettingsGroup::Program(
                    guitk::event::SettingsName::new(name).expect("a settings name"),
                ),
            };
            let mut first = WeatherApp::new(900.0, 800.0);
            let mut second = WeatherApp::new(900.0, 800.0);
            first.handle_event(&press(Key::U));
            assert_eq!(first.settings.temp_unit, TempUnit::Fahrenheit);

            assert_eq!(
                second.handle_event(&announce(b"notes")),
                EventResult::Ignored
            );
            assert_eq!(
                second.settings.temp_unit,
                TempUnit::Celsius,
                "another file was read"
            );
            assert_eq!(
                second.handle_event(&announce(b"weather")),
                EventResult::Consumed
            );
            assert_eq!(
                second.settings.temp_unit,
                TempUnit::Fahrenheit,
                "the unit did not reach it"
            );
            assert_eq!(
                first.handle_event(&announce(b"weather")),
                EventResult::Ignored,
                "a window's own save, announced back, changed what it shows"
            );

            std::fs::remove_file(dir.join("slateos").join("weather.yaml"))
                .expect("delete the file");
            second.handle_event(&announce(b"weather"));
            assert_eq!(
                second.settings,
                Settings::default(),
                "a deleted file kept its units"
            );
        });
    }

    /// Each unit key keeps its own change, pressed alone. Every store writes
    /// all four units, so in a sequence one key that forgot to keep its
    /// change was covered by the next key's store -- which is how a
    /// mutation that dropped U's store survived the test above.
    #[test]
    fn each_unit_key_keeps_its_own_change() {
        for (key, tag) in [
            (Key::U, "wx_u"),
            (Key::W, "wx_w"),
            (Key::P, "wx_p"),
            (Key::T, "wx_t"),
        ] {
            settingsfile::testing::with_scratch_config(tag, |_| {
                let mut app = WeatherApp::new(900.0, 800.0);
                let before = format!("{:?}", app.settings);
                app.handle_event(&press(key));
                let after = format!("{:?}", app.settings);
                assert_ne!(before, after, "{key:?} changed no unit");
                let mut next = WeatherApp::new(900.0, 800.0);
                next.load_units(&settingsfile::load(CONFIG_NAME));
                assert_eq!(
                    format!("{:?}", next.settings),
                    after,
                    "{key:?}'s change was not kept"
                );
            });
        }
    }

    #[test]
    fn a_unit_word_nobody_knows_leaves_that_unit_at_its_default() {
        let doc = yamldoc::Document::parse("units:\n  temperature: kelvin\n  wind: knots\n");
        let mut app = WeatherApp::new(900.0, 800.0);
        app.load_units(&doc);
        assert_eq!(app.settings.temp_unit, TempUnit::Celsius);
        assert_eq!(app.settings.wind_unit, WindSpeedUnit::Knots);
    }

    #[test]
    fn every_tab_is_a_button() {
        let mut app = WeatherApp::new(900.0, 800.0);
        for view in [
            ActiveView::HourlyDetail,
            ActiveView::DailyDetail,
            ActiveView::Alerts,
            ActiveView::Locations,
            ActiveView::SettingsView,
            ActiveView::Dashboard,
        ] {
            assert_eq!(
                probe::click(&mut app, Target::Tab(view)),
                EventResult::Consumed
            );
            assert_eq!(app.active_view, view);
        }
        assert_eq!(
            probe::click(&mut app, Target::Tab(ActiveView::Dashboard)),
            EventResult::Ignored,
            "the view already shown"
        );
    }

    #[test]
    fn a_place_is_chosen_by_pressing_it() {
        // Choosing a place keeps the choice, in a scratch copy of the settings.
        settingsfile::testing::with_scratch_config("wx_choose", |_| {
            let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
            app.active_view = ActiveView::Locations;
            assert_eq!(
                probe::click(&mut app, Target::Location(2)),
                EventResult::Consumed
            );
            assert_eq!(app.active_location_idx, 2);
            assert!(oswindow::app::App::title(&app).contains(&app.locations[2].name));
            assert_eq!(
                probe::click(&mut app, Target::Location(2)),
                EventResult::Ignored,
                "the place already shown"
            );
        });
    }

    #[test]
    fn the_wheel_scrolls_the_hourly_strip_either_way() {
        let mut app = WeatherApp::with_sample_weather(900.0, 800.0);
        assert_eq!(
            probe::scroll_at_point(&mut app, Target::HourlyStrip, -1.0),
            EventResult::Consumed
        );
        let scrolled = app.hourly_scroll_offset;
        assert!(scrolled > 0.0, "a notch moved nothing");
        let (x, y) = probe::rect_of(&app, Target::HourlyStrip).unwrap().centre();
        app.handle_event(&mouse(x, y, MouseEventKind::Scroll { dx: 1.0, dy: 0.0 }));
        assert!(
            app.hourly_scroll_offset > scrolled,
            "the sideways wheel moved nothing"
        );
        // And over something else, the wheel does not move the strip.
        let (tx, ty) = probe::rect_of(&app, Target::Tab(ActiveView::Alerts))
            .unwrap()
            .centre();
        assert_eq!(
            app.handle_event(&mouse(tx, ty, MouseEventKind::Scroll { dx: 0.0, dy: -1.0 })),
            EventResult::Ignored
        );
    }

    #[test]
    fn the_pointer_lights_the_tab_it_is_over() {
        let mut app = WeatherApp::new(900.0, 800.0);
        let (x, y) = probe::rect_of(&app, Target::Tab(ActiveView::Alerts))
            .unwrap()
            .centre();
        assert_eq!(
            app.handle_event(&mouse(x, y, MouseEventKind::Move)),
            EventResult::Consumed
        );
        assert_eq!(app.hover, Some(Target::Tab(ActiveView::Alerts)));
        assert_eq!(
            app.handle_event(&mouse(x, y, MouseEventKind::Move)),
            EventResult::Ignored
        );
        assert_eq!(
            app.handle_event(&mouse(x, y, MouseEventKind::Leave)),
            EventResult::Consumed
        );
        assert_eq!(app.hover, None);
    }

    /// **Every control drawn is the one a press on it reaches**, and between
    /// them the states draw every kind there is.
    #[test]
    fn every_control_drawn_is_the_one_a_press_on_it_reaches() {
        let mut settings = WeatherApp::new(900.0, 800.0);
        settings.active_view = ActiveView::SettingsView;
        let dashboard = WeatherApp::with_sample_weather(900.0, 800.0);
        let mut places = WeatherApp::with_sample_weather(900.0, 800.0);
        places.active_view = ActiveView::Locations;
        let mut kinds = std::collections::BTreeSet::new();
        for (what, app) in [
            ("the settings", settings),
            ("the dashboard", dashboard),
            ("the places", places),
        ] {
            let frame = app.frame();
            for (target, rect) in frame.hits() {
                kinds.insert(probe::variant_name(*target));
                let (x, y) = rect.centre();
                assert_eq!(
                    frame.hit_test(x, y),
                    Some(*target),
                    "{target:?} in {what} is covered by something else"
                );
            }
        }
        let mut help = WeatherApp::new(900.0, 800.0);
        help.handle_event(&press(Key::F1));
        for (target, _) in help.frame().hits() {
            kinds.insert(probe::variant_name(*target));
        }
        for kind in ["Tab", "Setting", "Location", "HourlyStrip", "HelpCard"] {
            assert!(kinds.contains(kind), "no state draws a {kind}: {kinds:?}");
        }
    }

    // ------------------------------------------------------------------
    // Forecasts from Open-Meteo, off until the user turns them on
    // (design-decisions §1236). The requests go to a stand-in that answers
    // with the service's own replies (`tests/data/`); no test reaches the
    // network, and the default stand-in refuses everything.
    // ------------------------------------------------------------------

    const SEARCH_REPLY: &str = include_str!("../tests/data/openmeteo-search-berlin.json");
    const FORECAST_REPLY: &str = include_str!("../tests/data/openmeteo-forecast-berlin.json");
    const AIR_REPLY: &str = include_str!("../tests/data/openmeteo-air-berlin.json");

    /// Open-Meteo as it answered on 2026-10-10.
    fn open_meteo(host: &str, _path: &str) -> Result<String, String> {
        match host {
            openmeteo::SEARCH_HOST => Ok(SEARCH_REPLY.to_owned()),
            openmeteo::FORECAST_HOST => Ok(FORECAST_REPLY.to_owned()),
            openmeteo::AIR_HOST => Ok(AIR_REPLY.to_owned()),
            other => Err(format!("{other} was never asked")),
        }
    }

    /// Open-Meteo with its air-quality service down.
    fn no_air(host: &str, path: &str) -> Result<String, String> {
        if host == openmeteo::AIR_HOST {
            Err(String::from(
                "air-quality-api.open-meteo.com could not be reached",
            ))
        } else {
            open_meteo(host, path)
        }
    }

    /// Open-Meteo with its geocoding service down.
    fn no_search(host: &str, path: &str) -> Result<String, String> {
        if host == openmeteo::SEARCH_HOST {
            Err(String::from("geocoding-api.open-meteo.com timed out"))
        } else {
            open_meteo(host, path)
        }
    }

    /// Nobody answering.
    fn unreachable(host: &str, _path: &str) -> Result<String, String> {
        Err(format!("{host} could not be reached: connection refused"))
    }

    /// Open-Meteo, slowly: each forecast answered after a fifth of a second,
    /// so that a test can do something while one is out.
    fn slow_forecast(host: &str, path: &str) -> Result<String, String> {
        if host == openmeteo::FORECAST_HOST {
            std::thread::sleep(Duration::from_millis(200));
        }
        open_meteo(host, path)
    }

    /// Berlin, as a search would have offered it.
    fn berlin() -> openmeteo::Place {
        openmeteo::Place {
            name: String::from("Berlin"),
            region: String::from("State of Berlin"),
            country: String::from("Germany"),
            latitude: 52.524_37,
            longitude: 13.410_53,
        }
    }

    /// Paris, likewise.
    fn paris() -> openmeteo::Place {
        openmeteo::Place {
            name: String::from("Paris"),
            region: String::from("\u{ce}le-de-France"),
            country: String::from("France"),
            latitude: 48.853_41,
            longitude: 2.3488,
        }
    }

    /// The desktop saying `weather.yaml` changed.
    fn weather_yaml_changed() -> Event {
        Event::SettingsChanged {
            group: guitk::event::SettingsGroup::Program(
                guitk::event::SettingsName::new(b"weather").expect("a settings name"),
            ),
        }
    }

    /// A key as a keyboard sends it: the key, and the text it types.
    fn letter(key: Key, text: &str) -> Event {
        Event::Key(KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: text.to_owned(),
        })
    }

    /// Wait until no request is out, taking in each answer as it comes.
    fn settle(app: &mut WeatherApp) {
        for _ in 0..500 {
            app.pump();
            if !app.source.waiting() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("a request was never answered");
    }

    /// Whether the forecast request out is for the place at `latitude`.
    fn asking_at(app: &WeatherApp, latitude: f64) -> bool {
        app.source
            .forecast_asked()
            .is_some_and(|asked| (asked.latitude - latitude).abs() < 1e-9)
    }

    /// A window with forecasts on, Berlin its one place, answered by `fetcher`,
    /// its forecast taken in.
    fn showing_berlin(fetcher: source::Fetcher) -> WeatherApp {
        let mut app = WeatherApp::new(900.0, 800.0);
        app.source.fetcher = fetcher;
        app.source.on = true;
        app.add_place(&berlin());
        settle(&mut app);
        app
    }

    #[test]
    fn nothing_is_sent_until_forecasts_are_turned_on() {
        settingsfile::testing::with_scratch_config("wx_off", |_| {
            let mut app = WeatherApp::new(900.0, 800.0);
            app.source.fetcher = open_meteo;
            app.locations.push(Location {
                name: String::from("Berlin"),
                is_default: true,
                latitude: 52.524_37,
                longitude: 13.410_53,
            });
            app.ask_forecast();
            app.search_places("Berlin");
            app.handle_event(&letter(Key::R, "r"));
            assert!(
                !app.source.waiting(),
                "a request went out with forecasts off"
            );
            // Typing on the Locations tab goes nowhere while they are off:
            // the unit keys keep their letters.
            app.active_view = ActiveView::Locations;
            app.handle_event(&letter(Key::U, "u"));
            assert_eq!(app.settings.temp_unit, TempUnit::Fahrenheit);
            assert_eq!(app.search.text(), "");
            assert_eq!(
                oswindow::app::App::tick_interval(&app),
                None,
                "a clock with nothing to wait for"
            );
        });
    }

    #[test]
    fn turning_forecasts_on_asks_for_the_place_shown_and_keeps_the_choice() {
        settingsfile::testing::with_scratch_config("wx_turn_on", |_| {
            let mut app = WeatherApp::new(900.0, 800.0);
            app.source.fetcher = open_meteo;
            app.locations.push(Location {
                name: String::from("Berlin, State of Berlin, Germany"),
                is_default: true,
                latitude: 52.524_37,
                longitude: 13.410_53,
            });
            assert_eq!(
                probe::click(&mut app, Target::TurnOn),
                EventResult::Consumed
            );
            assert!(app.source.on);
            assert!(
                asking_at(&app, 52.524_37),
                "turning them on did not ask for the place shown: {:?}",
                app.source.forecast_asked()
            );
            assert_eq!(
                oswindow::app::App::tick_interval(&app),
                Some(Duration::from_millis(500)),
                "no clock to fall back on while the answer is out"
            );
            assert!(drawn(&app).contains("Asking Open-Meteo for the weather at Berlin"));
            settle(&mut app);

            let now = app.current.clone().expect("the forecast was not taken in");
            assert!((now.temp_c - 11.9).abs() < 1e-4);
            assert_eq!(now.aqi, Some(23));
            assert_eq!(app.daily.len(), 7);
            let shown = drawn(&app);
            assert!(
                shown.contains(openmeteo::ATTRIBUTION),
                "the data is not credited: {shown}"
            );
            assert!(shown.contains("for 03:00 there"), "{shown}");
            assert_eq!(
                oswindow::app::App::title(&app),
                "Weather \u{2014} Berlin, State of Berlin, Germany"
            );
            // Kept: the next window starts with them on.
            let mut next = WeatherApp::new(900.0, 800.0);
            next.load_places(&settingsfile::load(CONFIG_NAME));
            assert!(next.source.on);
            assert_eq!(next.locations.len(), 1);
            assert!((next.locations[0].longitude - 13.410_53).abs() < 1e-9);
        });
    }

    #[test]
    fn a_place_is_found_by_name_and_added_with_where_it_is() {
        settingsfile::testing::with_scratch_config("wx_search", |_| {
            let mut app = WeatherApp::new(900.0, 800.0);
            app.source.fetcher = open_meteo;
            app.source.on = true;
            app.active_view = ActiveView::Locations;
            for ch in "Berlin".chars() {
                app.handle_event(&Event::Key(probe::typing(&ch.to_string())));
            }
            assert_eq!(app.search.text(), "Berlin");
            app.handle_event(&press(Key::Enter));
            assert_eq!(app.source.search_asked(), Some("Berlin"));
            settle(&mut app);
            assert_eq!(app.source.found.len(), 3);
            assert!(drawn(&app).contains("+  Berlin, State of Berlin, Germany"));

            assert_eq!(
                probe::click(&mut app, Target::Found(0)),
                EventResult::Consumed
            );
            assert_eq!(app.locations.len(), 1);
            assert_eq!(app.locations[0].name, "Berlin, State of Berlin, Germany");
            assert!(app.locations[0].is_default);
            assert!((app.locations[0].latitude - 52.524_37).abs() < 1e-9);
            assert!(app.source.found.is_empty(), "the offers stayed up");
            assert_eq!(app.search.text(), "", "the search was left in the box");
            assert!(asking_at(&app, 52.524_37));
            settle(&mut app);
            assert!(app.current.is_some());

            // The same place again is shown, not added twice.
            app.add_place(&berlin());
            assert_eq!(app.locations.len(), 1);
        });
    }

    #[test]
    fn a_search_that_finds_nothing_says_so() {
        #[expect(clippy::unnecessary_wraps, reason = "a Fetcher's shape")]
        fn nothing(_host: &str, _path: &str) -> Result<String, String> {
            Ok(String::from(r#"{"generationtime_ms":0.2}"#))
        }
        let mut app = WeatherApp::new(900.0, 800.0);
        app.source.fetcher = nothing;
        app.source.on = true;
        app.active_view = ActiveView::Locations;
        app.search_places("Nowhereville");
        settle(&mut app);
        assert!(drawn(&app).contains("No place called \u{201c}Nowhereville\u{201d}"));
    }

    /// **A search does not cost the forecast its answer.** One slot held
    /// both, so a search started while the forecast was out replaced it:
    /// the forecast's answer was dropped, and nothing asked for it again.
    #[test]
    fn a_search_while_the_forecast_is_out_leaves_it_out() {
        settingsfile::testing::with_scratch_config("wx_both_out", |_| {
            let mut app = WeatherApp::new(900.0, 800.0);
            app.source.fetcher = slow_forecast;
            app.source.on = true;
            app.add_place(&berlin());
            assert!(asking_at(&app, 52.524_37));
            app.search_places("Paris");
            assert!(
                asking_at(&app, 52.524_37),
                "the search took the forecast's place"
            );
            assert_eq!(app.source.search_asked(), Some("Paris"));
            settle(&mut app);
            assert!(app.current.is_some(), "the forecast's answer was dropped");
            assert_eq!(app.source.found.len(), 3);
        });
    }

    /// **A search that failed says nothing about the forecast.** One error
    /// held both, so the dashboard called a current forecast "from before"
    /// because a search had timed out.
    #[test]
    fn a_failed_search_is_said_under_the_box_and_not_over_the_forecast() {
        settingsfile::testing::with_scratch_config("wx_search_fails", |_| {
            let mut app = showing_berlin(no_search);
            app.search_places("Paris");
            settle(&mut app);
            assert!(app.source.error.is_none(), "the forecast was marked stale");
            assert!(!drawn(&app).contains("from before"));
            app.active_view = ActiveView::Locations;
            assert!(drawn(&app).contains("Could not search for \u{201c}Paris\u{201d}"));
        });
    }

    #[test]
    fn a_failed_forecast_says_why_and_try_again_asks_again() {
        settingsfile::testing::with_scratch_config("wx_fail", |_| {
            let mut app = showing_berlin(unreachable);
            assert!(app.current.is_none());
            let shown = drawn(&app);
            assert!(shown.contains("connection refused"), "{shown}");
            assert!(shown.contains("Try again"));
            app.source.fetcher = open_meteo;
            assert_eq!(probe::click(&mut app, Target::Retry), EventResult::Consumed);
            settle(&mut app);
            assert!(app.current.is_some(), "trying again did not get it");
            assert!(app.source.error.is_none());
        });
    }

    /// **A failed request is tried again in five minutes, not every second.**
    /// The refresh was due from the time the last forecast *arrived*, which a
    /// failure does not move: once due, it stayed due, and the one-second
    /// clock asked a server that was down once a second.
    #[test]
    fn a_failed_refresh_is_tried_again_after_a_while_not_at_once() {
        settingsfile::testing::with_scratch_config("wx_retry", |_| {
            let mut app = showing_berlin(open_meteo);
            app.source.fetched_at =
                std::time::Instant::now().checked_sub(source::REFRESH + Duration::from_secs(1));
            app.source.fetcher = unreachable;
            assert_eq!(
                app.handle_event(&Event::Tick { elapsed_ms: 1 }),
                EventResult::Consumed,
                "the line along the bottom was not redrawn to say it is asking"
            );
            assert!(drawn(&app).contains("asking again"));
            settle(&mut app);
            assert!(app.source.error.is_some());
            assert!(
                app.current.is_some(),
                "the forecast from before went with the failure"
            );
            let shown = drawn(&app);
            assert!(
                shown.contains("this is the forecast from before"),
                "{shown}"
            );
            assert!(
                shown.contains(openmeteo::ATTRIBUTION),
                "the error took the credit's place"
            );

            let now = std::time::Instant::now();
            assert!(!app.refresh_due(now), "asked again straight after failing");
            let interval = oswindow::app::App::tick_interval(&app).expect("no clock");
            assert!(
                interval > source::RETRY.saturating_sub(Duration::from_secs(5))
                    && interval <= source::RETRY,
                "{interval:?}"
            );
            app.handle_event(&Event::Tick { elapsed_ms: 1 });
            assert!(app.source.forecast_asked().is_none());
            // Five minutes on, it is.
            app.source.tried_at = now.checked_sub(source::RETRY);
            assert!(app.refresh_due(now));
        });
    }

    #[test]
    fn the_forecast_stands_when_the_air_quality_service_does_not_answer() {
        settingsfile::testing::with_scratch_config("wx_no_air", |_| {
            let app = showing_berlin(no_air);
            let now = app
                .current
                .clone()
                .expect("the forecast fell with the air quality");
            assert_eq!(now.aqi, None, "an index nobody gave was shown");
            assert!(drawn(&app).contains("Not known"));
        });
    }

    #[test]
    fn turning_forecasts_off_takes_what_was_fetched_from_the_window() {
        settingsfile::testing::with_scratch_config("wx_turn_off", |_| {
            let mut app = showing_berlin(open_meteo);
            assert!(app.current.is_some());
            probe::click(&mut app, Target::Tab(ActiveView::SettingsView));
            probe::click(&mut app, Target::Setting(Setting::Forecasts));
            assert!(!app.source.on);
            assert!(
                app.current.is_none(),
                "a forecast stayed up that will never be refreshed"
            );
            probe::click(&mut app, Target::Tab(ActiveView::Dashboard));
            assert!(drawn(&app).contains(FORECASTS_OFF_LINES[0]));
            assert_eq!(oswindow::app::App::tick_interval(&app), None);
            // And off is kept, places and all.
            let mut next = WeatherApp::new(900.0, 800.0);
            next.load_places(&settingsfile::load(CONFIG_NAME));
            assert!(!next.source.on);
            assert_eq!(next.locations.len(), 1);
        });
    }

    #[test]
    fn the_forecast_is_asked_for_again_on_the_half_hour() {
        settingsfile::testing::with_scratch_config("wx_refresh", |_| {
            let mut app = showing_berlin(open_meteo);
            let interval =
                oswindow::app::App::tick_interval(&app).expect("no clock with a forecast up");
            assert!(interval > Duration::from_mins(25), "{interval:?}");
            assert!(!app.refresh_due(std::time::Instant::now()));
            app.source.fetched_at =
                std::time::Instant::now().checked_sub(source::REFRESH + Duration::from_secs(1));
            app.handle_event(&Event::Tick { elapsed_ms: 1 });
            assert!(
                asking_at(&app, 52.524_37),
                "half an hour on, nothing was asked"
            );
        });
    }

    #[test]
    fn an_answer_for_a_place_no_longer_shown_is_not_shown_under_it() {
        settingsfile::testing::with_scratch_config("wx_stale", |_| {
            let mut app = WeatherApp::new(900.0, 800.0);
            app.source.fetcher = open_meteo;
            app.source.on = true;
            app.add_place(&berlin());
            // The place moved under the request: the answer is Berlin's, not
            // this one's.
            app.locations[0].latitude = 48.856_61;
            settle(&mut app);
            assert!(
                app.current.is_none(),
                "Berlin's weather was shown for another place"
            );
        });
    }

    #[test]
    fn removing_the_place_shown_shows_the_next_and_asks_for_it() {
        settingsfile::testing::with_scratch_config("wx_remove", |_| {
            let mut app = showing_berlin(open_meteo);
            app.add_place(&paris());
            settle(&mut app);
            app.show_location(0);
            settle(&mut app);
            app.active_view = ActiveView::Locations;
            assert_eq!(
                probe::click(&mut app, Target::RemovePlace(0)),
                EventResult::Consumed
            );
            assert_eq!(app.locations.len(), 1);
            assert_eq!(app.locations[0].name, "Paris, \u{ce}le-de-France, France");
            assert!(app.locations[0].is_default, "the default went with Berlin");
            assert!(
                asking_at(&app, 48.853_41),
                "{:?}",
                app.source.forecast_asked()
            );
            // The last one gone, nothing is out: an answer would have no
            // place to be shown under.
            assert_eq!(
                probe::click(&mut app, Target::RemovePlace(0)),
                EventResult::Consumed
            );
            assert!(app.locations.is_empty());
            assert!(app.source.forecast_asked().is_none());
            assert!(drawn(&app).contains("Places"));
            app.active_view = ActiveView::Dashboard;
            assert!(drawn(&app).contains("No place chosen."));
        });
    }

    /// **A window's own save, announced back, leaves it where it was.** The
    /// settings watcher announces every write, this window's own among them,
    /// and re-reading the file showed its default place: adding a place, the
    /// window jumped back to the default and asked for its weather instead.
    #[test]
    fn a_windows_own_save_announced_back_leaves_it_showing_the_same_place() {
        settingsfile::testing::with_scratch_config("wx_echo", |_| {
            let mut app = showing_berlin(open_meteo);
            app.add_place(&paris());
            settle(&mut app);
            assert_eq!(app.active_location_idx, 1);
            assert!(!app.locations[1].is_default, "Berlin is still the default");
            assert_eq!(
                app.handle_event(&weather_yaml_changed()),
                EventResult::Ignored,
                "nothing changed, and the window said something had"
            );
            assert_eq!(app.active_location_idx, 1, "the window went back to Berlin");
            assert!(app.current.is_some());
            assert!(!app.source.waiting(), "the forecast was asked for again");
            // And after a unit is changed, which saves the file too.
            app.handle_event(&letter(Key::U, "u"));
            app.handle_event(&weather_yaml_changed());
            assert_eq!(app.active_location_idx, 1);
        });
    }

    /// **Places and the switch, changed in one window, reach the others.**
    #[test]
    fn forecasts_turned_on_and_off_in_one_window_reach_the_others() {
        settingsfile::testing::with_scratch_config("wx_two_windows", |_| {
            let mut other = WeatherApp::new(900.0, 800.0);
            other.source.fetcher = open_meteo;
            let first = showing_berlin(open_meteo);
            drop(first);

            assert_eq!(
                other.handle_event(&weather_yaml_changed()),
                EventResult::Consumed
            );
            assert!(other.source.on, "turned on there, still off here");
            assert_eq!(other.locations.len(), 1);
            assert!(
                asking_at(&other, 52.524_37),
                "turned on here, nothing was asked"
            );
            settle(&mut other);
            assert!(other.current.is_some());

            // Off in a third window: off here too, and what was fetched goes.
            let mut third = WeatherApp::new(900.0, 800.0);
            third.load_places(&settingsfile::load(CONFIG_NAME));
            third.set_forecasts(false);
            assert_eq!(
                other.handle_event(&weather_yaml_changed()),
                EventResult::Consumed
            );
            assert!(!other.source.on);
            assert!(other.current.is_none());
            assert!(!other.source.waiting());
        });
    }

    /// The Locations tab's box has the letters while forecasts are on; on
    /// every other tab they are what they always were.
    #[test]
    fn the_search_box_has_the_letters_only_on_its_own_tab() {
        settingsfile::testing::with_scratch_config("wx_keys", |_| {
            let mut app = showing_berlin(open_meteo);
            app.active_view = ActiveView::Locations;
            app.handle_event(&letter(Key::R, "r"));
            app.handle_event(&letter(Key::U, "u"));
            assert_eq!(app.search.text(), "ru");
            assert!(!app.source.waiting(), "R asked again while typing a name");
            assert_eq!(app.settings.temp_unit, TempUnit::Celsius);
            // Escape empties the box, and with it empty is nobody's.
            assert_eq!(app.handle_event(&press(Key::Escape)), EventResult::Consumed);
            assert_eq!(app.search.text(), "");

            app.active_view = ActiveView::Dashboard;
            app.handle_event(&letter(Key::U, "u"));
            assert_eq!(app.settings.temp_unit, TempUnit::Fahrenheit);
            assert_eq!(
                app.handle_event(&letter(Key::R, "r")),
                EventResult::Consumed
            );
            assert!(asking_at(&app, 52.524_37), "R did not ask again");
            settle(&mut app);
        });
    }

    /// **What the file holds is read back only as far as it can be a place.**
    /// A place without a name or a coordinate, or with one off the globe, is
    /// left out; a default naming no place is the first; and anything but
    /// `forecasts: true` is off -- the switch is the user's consent to send
    /// where they are, so a misspelt one is not taken as it.
    #[test]
    fn places_read_back_leave_out_what_cannot_be_a_place() {
        let doc = yamldoc::Document::parse(concat!(
            "forecasts: yes\n",
            "places:\n",
            "  p0:\n    name: Nowhere\n    latitude: 91.0\n    longitude: 0.0\n",
            "  p1:\n    latitude: 1.0\n    longitude: 1.0\n",
            "  p2:\n    name: Berlin\n    latitude: 52.52437\n    longitude: 13.41053\n",
            "  p3:\n    name: Far east\n    latitude: 0.0\n    longitude: 180.5\n",
            "  p4:\n    name: Paris\n    latitude: 48.85341\n    longitude: east\n",
            "  p5:\n    name: Paris\n    latitude: 48.85341\n    longitude: 2.3488\n",
            "default: 7\n",
        ));
        let mut app = WeatherApp::new(900.0, 800.0);
        app.load_places(&doc);
        assert!(!app.source.on, "`yes` was taken as the user's consent");
        let names: Vec<&str> = app.locations.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Berlin", "Paris"]);
        assert_eq!(app.active_location_idx, 0);
        assert!(app.locations[0].is_default);
        assert!(!app.locations[1].is_default);

        let doc = yamldoc::Document::parse(concat!(
            "forecasts: true\n",
            "places:\n",
            "  p0:\n    name: Berlin\n    latitude: 52.52437\n    longitude: 13.41053\n",
            "  p1:\n    name: Paris\n    latitude: 48.85341\n    longitude: 2.3488\n",
            "default: 1\n",
        ));
        app.load_places(&doc);
        assert!(app.source.on);
        assert_eq!(
            app.active_location_idx, 1,
            "the default is not the place shown"
        );
        assert!(app.locations[1].is_default);
        assert!(!app.locations[0].is_default);
    }

    /// Turned on in another window, with the same place shown here: its
    /// forecast is asked for here too, though the place did not change.
    #[test]
    fn forecasts_turned_on_elsewhere_ask_for_the_place_already_shown_here() {
        settingsfile::testing::with_scratch_config("wx_on_elsewhere", |_| {
            let mut first = WeatherApp::new(900.0, 800.0);
            first.add_place(&berlin());
            assert!(!first.source.waiting(), "asked with forecasts off");
            let mut other = WeatherApp::new(900.0, 800.0);
            other.source.fetcher = open_meteo;
            other.load_places(&settingsfile::load(CONFIG_NAME));
            assert_eq!(other.locations.len(), 1);

            first.set_forecasts(true);
            assert_eq!(
                other.handle_event(&weather_yaml_changed()),
                EventResult::Consumed
            );
            assert!(
                asking_at(&other, 52.524_37),
                "turned on there, nothing asked here"
            );
            settle(&mut other);
            settle(&mut first);
            assert!(other.current.is_some());
        });
    }
}
