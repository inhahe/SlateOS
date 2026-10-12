//! Open-Meteo: what the weather app asks it, and what its replies mean.
//!
//! The operator chose Open-Meteo (E-Q2, design-decisions §1236): a free
//! forecast service that needs no account or key. It is asked only once the
//! user has turned forecasts on, and what it is sent is a place's latitude and
//! longitude -- the place the user added -- or, while they search for one, the
//! name they typed. Its data is licensed CC BY 4.0, so the window credits it
//! ([`ATTRIBUTION`]) wherever its readings are shown.
//!
//! Everything here is a pure function of a reply's text, so it is tested
//! against real replies (`tests/data/`); the asking is `fetch`'s.
//!
//! # Missing values
//!
//! A reply may hold `null` where a value is not known. A reading that is not
//! known is not shown as one: an hour or a day missing a value it needs is
//! left out, rather than drawn as 0 degrees or 0% rain.

use crate::{CurrentWeather, DayForecast, HourForecast, WeatherCondition, WindDirection};
use jsonvalue::{JsonValue, json_parse};

/// The forecast service's host.
pub const FORECAST_HOST: &str = "api.open-meteo.com";

/// The place-name search's host.
pub const SEARCH_HOST: &str = "geocoding-api.open-meteo.com";

/// The air-quality service's host.
pub const AIR_HOST: &str = "air-quality-api.open-meteo.com";

/// Credit for the data, which its licence (CC BY 4.0) asks for wherever it is
/// shown.
pub const ATTRIBUTION: &str = "Weather data by Open-Meteo.com, CC BY 4.0";

/// The most places one search offers.
pub const SEARCH_RESULTS: u32 = 8;

/// The days a forecast covers.
pub const FORECAST_DAYS: u32 = 7;

/// The hours of the hourly forecast shown, from the current one.
pub const HOURS_SHOWN: usize = 24;

/// A place the weather can be asked for: what the user chose from a search.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub name: String,
    /// The region it is in (a state, a province), if the service gave one.
    pub region: String,
    pub country: String,
    pub latitude: f64,
    pub longitude: f64,
}

impl Place {
    /// How the place is named in a list: "Berlin, State of Berlin, Germany".
    #[must_use]
    pub fn label(&self) -> String {
        [
            self.name.as_str(),
            self.region.as_str(),
            self.country.as_str(),
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
    }
}

/// The request path for a place-name search: `/v1/search?name=...`.
#[must_use]
pub fn search_path(name: &str) -> String {
    format!(
        "/v1/search?name={}&count={SEARCH_RESULTS}&language=en&format=json",
        percent_encode(name.trim())
    )
}

/// The request path for a place's forecast: the current conditions, the next
/// hours and the next days, in the place's own time zone.
#[must_use]
pub fn forecast_path(latitude: f64, longitude: f64) -> String {
    format!(
        "/v1/forecast?latitude={latitude}&longitude={longitude}\
         &current=temperature_2m,apparent_temperature,relative_humidity_2m,dew_point_2m,\
weather_code,wind_speed_10m,wind_direction_10m,pressure_msl,visibility,uv_index\
         &hourly=temperature_2m,weather_code,precipitation_probability\
         &daily=weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max,\
wind_speed_10m_max,wind_direction_10m_dominant,sunrise,sunset\
         &timezone=auto&forecast_days={FORECAST_DAYS}"
    )
}

/// The request path for a place's air quality now, on the US index the
/// app's categories are drawn from.
#[must_use]
pub fn air_path(latitude: f64, longitude: f64) -> String {
    format!("/v1/air-quality?latitude={latitude}&longitude={longitude}&current=us_aqi")
}

/// `text` for a URL's query: every byte but the unreserved ones as `%XX`.
#[must_use]
pub fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The places a search's reply offers. A reply with no `results` -- the
/// service's answer when nothing matches -- is no places, not an error.
///
/// # Errors
///
/// A reply that is not JSON, or one whose results are not places.
pub fn read_places(body: &str) -> Result<Vec<Place>, String> {
    let reply = json_parse(body).map_err(|e| format!("the reply is not JSON: {e}"))?;
    service_error(&reply)?;
    let Some(results) = reply.get("results") else {
        return Ok(Vec::new());
    };
    let results = results.as_array().ok_or("its results are not a list")?;
    let mut places = Vec::new();
    for result in results {
        let text = |key: &str| {
            result
                .get(key)
                .and_then(JsonValue::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let (Some(latitude), Some(longitude)) = (
            result.get("latitude").and_then(JsonValue::as_f64),
            result.get("longitude").and_then(JsonValue::as_f64),
        ) else {
            continue;
        };
        let name = text("name");
        if name.is_empty() {
            continue;
        }
        places.push(Place {
            name,
            region: text("admin1"),
            country: text("country"),
            latitude,
            longitude,
        });
    }
    Ok(places)
}

/// A forecast as the app shows it.
#[derive(Clone, Debug)]
pub struct Forecast {
    /// The conditions now -- without the air quality, which another service
    /// answers ([`read_us_aqi`]).
    pub current: CurrentWeather,
    pub hourly: Vec<HourForecast>,
    pub daily: Vec<DayForecast>,
    /// When the current conditions are for, in the place's own time:
    /// `2026-10-10T03:00`.
    pub observed: String,
}

/// A forecast reply, read.
///
/// # Errors
///
/// A reply that is not JSON, says the service refused the request, or lacks
/// the current conditions.
pub fn read_forecast(body: &str) -> Result<Forecast, String> {
    let reply = json_parse(body).map_err(|e| format!("the reply is not JSON: {e}"))?;
    service_error(&reply)?;
    let now = reply
        .get("current")
        .ok_or("the reply has no current conditions")?;
    let number = |key: &str| now.get(key).and_then(JsonValue::as_f64);
    let need = |key: &str| number(key).ok_or_else(|| format!("the current {key} is missing"));
    let observed = now
        .get("time")
        .and_then(JsonValue::as_str)
        .ok_or("the current conditions have no time")?
        .to_owned();

    let daily = reply.get("daily");
    let sun = |key: &str| {
        daily
            .and_then(|d| d.get(key))
            .and_then(JsonValue::as_array)
            .and_then(|times| times.first())
            .and_then(JsonValue::as_str)
            .and_then(hour_minute)
    };

    let current = CurrentWeather {
        temp_c: as_f32(need("temperature_2m")?),
        feels_like_c: as_f32(need("apparent_temperature")?),
        condition: condition(whole(need("weather_code")?)),
        humidity_pct: percent(need("relative_humidity_2m")?),
        dew_point_c: as_f32(need("dew_point_2m")?),
        wind_speed_kmh: as_f32(need("wind_speed_10m")?),
        wind_dir: WindDirection::from_degrees(whole(need("wind_direction_10m")?) % 360),
        pressure_hpa: as_f32(need("pressure_msl")?),
        visibility_km: as_f32(need("visibility")? / 1000.0),
        uv_index: u8::try_from(whole(need("uv_index")?)).unwrap_or(u8::MAX),
        sunrise: sun("sunrise").ok_or("the reply has no sunrise")?,
        sunset: sun("sunset").ok_or("the reply has no sunset")?,
        aqi: None,
    };
    Ok(Forecast {
        current,
        hourly: read_hours(reply.get("hourly"), &observed),
        daily: read_days(daily),
        observed,
    })
}

/// The next [`HOURS_SHOWN`] hours from the hour `observed` falls in.
fn read_hours(hourly: Option<&JsonValue>, observed: &str) -> Vec<HourForecast> {
    let Some(hourly) = hourly else {
        return Vec::new();
    };
    let column = |key: &str| {
        hourly
            .get(key)
            .and_then(JsonValue::as_array)
            .map_or(&[][..], Vec::as_slice)
    };
    let (times, temps, codes, rain) = (
        column("time"),
        column("temperature_2m"),
        column("weather_code"),
        column("precipitation_probability"),
    );
    // The hour the observation is in: its time with the minutes at :00.
    let this_hour = observed.get(..13).map(|h| format!("{h}:00"));
    let start = times
        .iter()
        .position(|t| t.as_str() == this_hour.as_deref())
        .unwrap_or(0);
    let mut hours = Vec::new();
    for i in start..times.len() {
        if hours.len() == HOURS_SHOWN {
            break;
        }
        let (Some(time), Some(temp), Some(code), Some(rain)) = (
            times.get(i).and_then(JsonValue::as_str),
            temps.get(i).and_then(JsonValue::as_f64),
            codes.get(i).and_then(JsonValue::as_f64),
            rain.get(i).and_then(JsonValue::as_f64),
        ) else {
            continue;
        };
        let Some((hour, _)) = hour_minute(time) else {
            continue;
        };
        hours.push(HourForecast {
            hour,
            temp_c: as_f32(temp),
            condition: condition(whole(code)),
            precip_pct: percent(rain),
        });
    }
    hours
}

/// The days of the forecast, the first named "Today".
fn read_days(daily: Option<&JsonValue>) -> Vec<DayForecast> {
    let Some(daily) = daily else {
        return Vec::new();
    };
    let column = |key: &str| {
        daily
            .get(key)
            .and_then(JsonValue::as_array)
            .map_or(&[][..], Vec::as_slice)
    };
    let dates = column("time");
    let mut days = Vec::new();
    for (i, date) in dates.iter().enumerate() {
        let at = |key: &str| column(key).get(i).and_then(JsonValue::as_f64);
        let (Some(date), Some(code), Some(high), Some(low), Some(rain), Some(wind), Some(dir)) = (
            date.as_str(),
            at("weather_code"),
            at("temperature_2m_max"),
            at("temperature_2m_min"),
            at("precipitation_probability_max"),
            at("wind_speed_10m_max"),
            at("wind_direction_10m_dominant"),
        ) else {
            continue;
        };
        let Some(weekday) = weekday(date) else {
            continue;
        };
        days.push(DayForecast {
            day_name: if i == 0 {
                String::from("Today")
            } else {
                weekday.to_owned()
            },
            high_c: as_f32(high),
            low_c: as_f32(low),
            condition: condition(whole(code)),
            precip_pct: percent(rain),
            wind_speed_kmh: as_f32(wind),
            wind_dir: WindDirection::from_degrees(whole(dir) % 360),
        });
    }
    days
}

/// The US air-quality index now, from an air-quality reply.
///
/// # Errors
///
/// A reply that is not JSON, says the service refused, or has no index.
pub fn read_us_aqi(body: &str) -> Result<u16, String> {
    let reply = json_parse(body).map_err(|e| format!("the reply is not JSON: {e}"))?;
    service_error(&reply)?;
    let index = reply
        .get("current")
        .and_then(|c| c.get("us_aqi"))
        .and_then(JsonValue::as_f64)
        .ok_or("the reply has no air-quality index")?;
    Ok(whole(index))
}

/// The service's own refusal -- `{"error": true, "reason": "..."}` -- as an
/// error, in its words.
fn service_error(reply: &JsonValue) -> Result<(), String> {
    if reply.get("error").and_then(JsonValue::as_bool) == Some(true) {
        let why = reply
            .get("reason")
            .and_then(JsonValue::as_str)
            .unwrap_or("no reason given");
        return Err(format!("Open-Meteo refused the request: {why}"));
    }
    Ok(())
}

/// What a WMO weather-interpretation code (the `weather_code` Open-Meteo
/// gives) looks like, in the app's words. A code this does not know is
/// overcast -- the one appearance that claims nothing in particular.
#[must_use]
pub fn condition(code: u16) -> WeatherCondition {
    match code {
        0 | 1 => WeatherCondition::Clear,
        2 => WeatherCondition::PartlyCloudy,
        45 | 48 => WeatherCondition::Fog,
        51 | 53 | 55 | 61 | 80 => WeatherCondition::LightRain,
        63 | 81 => WeatherCondition::Rain,
        65 | 82 => WeatherCondition::HeavyRain,
        56 | 57 | 66 | 67 => WeatherCondition::Sleet,
        71 | 77 | 85 => WeatherCondition::LightSnow,
        73 | 75 | 86 => WeatherCondition::Snow,
        95 | 96 | 99 => WeatherCondition::Thunderstorm,
        _ => WeatherCondition::Overcast,
    }
}

/// The weekday's short name of a `YYYY-MM-DD` date, in the Gregorian
/// calendar (Sakamoto's method).
#[must_use]
pub fn weekday(date: &str) -> Option<&'static str> {
    const NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTH: [i64; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let mut parts = date.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: usize = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=31).contains(&day) {
        return None;
    }
    let offset = *MONTH.get(month.checked_sub(1)?)?;
    let y = if month < 3 {
        year.checked_sub(1)?
    } else {
        year
    };
    let sum = y
        .checked_add(y.div_euclid(4))?
        .checked_sub(y.div_euclid(100))?
        .checked_add(y.div_euclid(400))?
        .checked_add(offset)?
        .checked_add(day)?;
    NAMES.get(usize::try_from(sum.rem_euclid(7)).ok()?).copied()
}

/// The hour and minute of a `YYYY-MM-DDTHH:MM` time.
#[must_use]
pub fn hour_minute(time: &str) -> Option<(u8, u8)> {
    let (_, clock) = time.split_once('T')?;
    let (hour, minute) = clock.split_once(':')?;
    let hour: u8 = hour.parse().ok()?;
    let minute: u8 = minute.get(..2)?.parse().ok()?;
    (hour < 24 && minute < 60).then_some((hour, minute))
}

/// `value` as an `f32`, which every reading in the app is.
#[allow(
    clippy::cast_possible_truncation,
    reason = "a weather reading -- tens or thousands -- loses nothing an f32 cannot hold"
)]
fn as_f32(value: f64) -> f32 {
    value as f32
}

/// A whole-number field (a code, a direction, an index), rounded; 0 for
/// anything negative, which none of these can be.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=u16::MAX first"
)]
fn whole(value: f64) -> u16 {
    value.round().clamp(0.0, f64::from(u16::MAX)) as u16
}

/// A percentage, held to 0..=100.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=100 first"
)]
fn percent(value: f64) -> u8 {
    value.round().clamp(0.0, 100.0) as u8
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    const SEARCH: &str = include_str!("../tests/data/openmeteo-search-berlin.json");
    const FORECAST: &str = include_str!("../tests/data/openmeteo-forecast-berlin.json");
    const AIR: &str = include_str!("../tests/data/openmeteo-air-berlin.json");

    #[test]
    fn a_search_reply_is_read_as_places() {
        let places = read_places(SEARCH).unwrap();
        assert_eq!(places.len(), 3);
        assert_eq!(places[0].label(), "Berlin, State of Berlin, Germany");
        assert!((places[0].latitude - 52.524_37).abs() < 1e-9);
        assert!((places[0].longitude - 13.410_53).abs() < 1e-9);
        assert_eq!(places[1].country, "United States");
        // Nothing found: the service leaves `results` out.
        assert_eq!(
            read_places(r#"{"generationtime_ms":0.3}"#).unwrap(),
            Vec::new()
        );
        assert!(read_places("<html>").is_err());
    }

    #[test]
    fn a_forecast_reply_is_read_as_the_app_shows_it() {
        let f = read_forecast(FORECAST).unwrap();
        assert_eq!(f.observed, "2026-10-10T03:00");
        let c = &f.current;
        assert!((c.temp_c - 11.9).abs() < 1e-4);
        assert!((c.feels_like_c - 9.4).abs() < 1e-4);
        assert_eq!(
            c.condition,
            WeatherCondition::LightRain,
            "code 80, slight showers"
        );
        assert_eq!(c.humidity_pct, 87);
        assert_eq!(c.wind_dir, WindDirection::SW, "220 degrees");
        assert!((c.visibility_km - 22.98).abs() < 1e-3, "metres become km");
        assert_eq!((c.sunrise, c.sunset), ((7, 22), (18, 22)));
        assert_eq!(c.aqi, None, "the air quality is another service's");

        // From the observation's hour on, a day's worth.
        assert_eq!(f.hourly.len(), HOURS_SHOWN);
        assert_eq!(f.hourly[0].hour, 3);
        assert_eq!(f.hourly[1].hour, 4);

        assert_eq!(f.daily.len(), 7);
        assert_eq!(f.daily[0].day_name, "Today");
        assert_eq!(f.daily[1].day_name, "Sun", "2026-10-11 is a Sunday");
        assert!((f.daily[0].high_c - 16.6).abs() < 1e-4);
        assert_eq!(f.daily[0].precip_pct, 85);
        assert_eq!(f.daily[2].condition, WeatherCondition::Overcast, "code 3");
    }

    #[test]
    fn an_hour_or_a_day_with_a_value_missing_is_left_out() {
        let reply = FORECAST.replacen(
            "\"precipitation_probability\":[",
            "\"precipitation_probability\":[null,null,null,null,",
            1,
        );
        // Four nulls ahead of the column: midnight to 03:00 have no rain
        // chance, so the hours shown start at 04:00, a day's worth of them.
        let f = read_forecast(&reply).unwrap();
        assert_eq!(f.hourly[0].hour, 4, "an hour with no rain chance was shown");
        assert_eq!(f.hourly.len(), HOURS_SHOWN);
        let reply = FORECAST.replacen(
            "\"temperature_2m_max\":[16.6",
            "\"temperature_2m_max\":[null",
            1,
        );
        let f = read_forecast(&reply).unwrap();
        assert_eq!(f.daily.len(), 6);
        assert_ne!(
            f.daily[0].day_name, "Today",
            "the day with no high was shown"
        );
    }

    #[test]
    fn the_air_quality_reply_gives_the_us_index() {
        assert_eq!(read_us_aqi(AIR).unwrap(), 23);
        assert!(read_us_aqi(r#"{"current":{}}"#).is_err());
    }

    #[test]
    fn the_service_refusing_says_so_in_its_own_words() {
        let refused = r#"{"error":true,"reason":"Latitude must be in range of -90 to 90"}"#;
        for err in [
            read_forecast(refused).unwrap_err(),
            read_places(refused).unwrap_err(),
            read_us_aqi(refused).unwrap_err(),
        ] {
            assert!(err.contains("Latitude must be in range"), "{err}");
        }
    }

    #[test]
    fn a_place_name_is_percent_encoded() {
        assert_eq!(percent_encode("Berlin"), "Berlin");
        assert_eq!(percent_encode("São Paulo"), "S%C3%A3o%20Paulo");
        assert_eq!(percent_encode("a&b=c"), "a%26b%3Dc");
        assert!(search_path("  Köln ").starts_with("/v1/search?name=K%C3%B6ln&count=8"));
    }

    #[test]
    fn the_forecast_asks_for_what_the_reader_reads() {
        let path = forecast_path(52.5, 13.4);
        for field in [
            "latitude=52.5",
            "longitude=13.4",
            "apparent_temperature",
            "precipitation_probability",
            "wind_direction_10m_dominant",
            "sunrise",
            "timezone=auto",
            "forecast_days=7",
        ] {
            assert!(path.contains(field), "{field} is not asked for: {path}");
        }
        assert!(!path.contains(' '), "{path}");
        assert_eq!(
            air_path(1.5, -2.0),
            "/v1/air-quality?latitude=1.5&longitude=-2&current=us_aqi"
        );
    }

    #[test]
    fn wmo_codes_read_as_the_weather_they_name() {
        assert_eq!(condition(0), WeatherCondition::Clear);
        assert_eq!(condition(2), WeatherCondition::PartlyCloudy);
        assert_eq!(condition(3), WeatherCondition::Overcast);
        assert_eq!(condition(45), WeatherCondition::Fog);
        assert_eq!(condition(65), WeatherCondition::HeavyRain);
        assert_eq!(condition(67), WeatherCondition::Sleet);
        assert_eq!(condition(75), WeatherCondition::Snow);
        assert_eq!(condition(99), WeatherCondition::Thunderstorm);
        assert_eq!(condition(1234), WeatherCondition::Overcast);
    }

    #[test]
    fn a_date_reads_as_its_weekday() {
        assert_eq!(weekday("2026-10-10"), Some("Sat"));
        assert_eq!(weekday("2000-02-29"), Some("Tue"));
        assert_eq!(weekday("1970-01-01"), Some("Thu"));
        assert_eq!(weekday("2024-03-01"), Some("Fri"));
        assert_eq!(weekday("2026-13-01"), None);
        assert_eq!(weekday("2026-10"), None);
        assert_eq!(hour_minute("2026-10-10T07:22"), Some((7, 22)));
        assert_eq!(hour_minute("2026-10-10T25:00"), None);
        assert_eq!(hour_minute("07:22"), None);
    }
}
