//! Where the weather comes from: Open-Meteo, once the user turns forecasts on.
//!
//! The operator's answer to E-Q2 (design-decisions §1236): no program
//! contacts a website by default unless that is what it is for, and the
//! weather is named -- not by default, and not asking where the user lives.
//! So nothing here sends anything until [`WeatherApp::set_forecasts`] has
//! turned forecasts on, which only the user's own press does: the button on
//! the panel that explains what turning them on sends, or the Settings row
//! that says the same. Turned on, the app asks Open-Meteo for the places the
//! user adds -- each place's latitude and longitude, and while they search,
//! the name they typed -- and for nothing else.
//!
//! A request runs on a thread of its own ([`crate::fetch`] blocks) and its answer
//! wakes the window. At most two are out at once, one of each kind: the
//! forecast of the place shown, and a search. Each kind has its own slot, so
//! a search never costs the forecast its answer, and a newer request of a
//! kind replaces the older one. An answer that arrives for a place no longer
//! shown is dropped rather than shown under another name.
//!
//! The forecast shown is asked for again every [`REFRESH`]; one that could not
//! be had is tried again after [`RETRY`] -- not at once, which against a
//! server that is down would be a request a second.
//!
//! The switch and the places are kept in the user's settings, `weather.yaml`,
//! beside the units: `forecasts: true`; `places:` holding each place's name,
//! latitude and longitude under `p0`, `p1`, ... in the order shown; and
//! `default:`, the place shown when the window opens.

use crate::openmeteo::{self, Forecast, Place};
use crate::{CONFIG_NAME, Location, WeatherApp};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::task::Waker;
use std::time::{Duration, Instant};

/// How often a shown forecast is asked for again, while the window is open.
pub const REFRESH: Duration = Duration::from_mins(30);

/// How soon a forecast that could not be had is asked for again, unasked.
/// The network that was down when the machine woke is usually back within
/// minutes; until then the window says why there is no new forecast.
pub const RETRY: Duration = Duration::from_mins(5);

/// How a request is made: [`crate::fetch::get`], or a test's stand-in.
pub type Fetcher = fn(&str, &str) -> Result<String, String>;

/// A forecast asked for: the place, named and placed as it was when asked.
#[derive(Clone, Debug, PartialEq)]
pub struct ForecastFor {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
}

/// A request out on its thread: what it asked, and where its answer comes.
struct Pending<A, T> {
    asked: A,
    answer: Receiver<Result<T, String>>,
}

/// The forecast source's state.
pub struct Source {
    /// Whether forecasts are on. Off until the user turns them on.
    pub on: bool,
    /// How requests are made.
    pub fetcher: Fetcher,
    /// The forecast request out, and the air quality with it.
    forecast: Option<Pending<ForecastFor, (Forecast, Option<u16>)>>,
    /// The search out -- beside the forecast request, not in its place.
    search: Option<Pending<String, Vec<Place>>>,
    /// When the forecast shown arrived, for the half-hourly refresh.
    pub fetched_at: Option<Instant>,
    /// When the forecast was last asked for, answered or not: one that failed
    /// is asked for again [`RETRY`] after this.
    pub tried_at: Option<Instant>,
    /// When the forecast shown is for, in the place's own time.
    pub observed: Option<String>,
    /// Why the last forecast request failed, said in the window until the
    /// next one goes out.
    pub error: Option<String>,
    /// Why the last search failed, said under the search box. Apart from
    /// [`error`](Self::error): a search that failed says nothing about the
    /// forecast shown.
    pub search_error: Option<String>,
    /// The places the last search offered.
    pub found: Vec<Place>,
    /// The name the last search was for, to say what was searched.
    pub searched: Option<String>,
    /// Wakes the window when an answer arrives.
    waker: Option<Waker>,
}

/// How requests are made unless a test says otherwise: over the network --
/// or, under test, nowhere, so that no test reaches the network by
/// forgetting to put a stand-in in.
#[cfg(not(test))]
const DEFAULT_FETCHER: Fetcher = crate::fetch::get;
#[cfg(test)]
const DEFAULT_FETCHER: Fetcher = no_network;

/// A test's default: every request refused.
#[cfg(test)]
fn no_network(host: &str, _path: &str) -> Result<String, String> {
    Err(format!("{host}: tests do not reach the network"))
}

impl Default for Source {
    fn default() -> Self {
        Self {
            on: false,
            fetcher: DEFAULT_FETCHER,
            forecast: None,
            search: None,
            fetched_at: None,
            tried_at: None,
            observed: None,
            error: None,
            search_error: None,
            found: Vec::new(),
            searched: None,
            waker: None,
        }
    }
}

impl Source {
    /// The forecast request out, if one is.
    #[must_use]
    pub fn forecast_asked(&self) -> Option<&ForecastFor> {
        self.forecast.as_ref().map(|p| &p.asked)
    }

    /// The name the search out is for, if one is.
    #[must_use]
    pub fn search_asked(&self) -> Option<&str> {
        self.search.as_ref().map(|p| p.asked.as_str())
    }

    /// Whether any request is out.
    #[must_use]
    pub fn waiting(&self) -> bool {
        self.forecast.is_some() || self.search.is_some()
    }

    /// Keep the waker the window was given.
    pub fn attach_waker(&mut self, waker: Waker) {
        self.waker = Some(waker);
    }
}

/// Run `work` on a thread of its own, waking the window when it is done.
fn spawn_request<T: Send + 'static>(
    waker: Option<Waker>,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<Receiver<Result<T, String>>, String> {
    let (tx, answer) = mpsc::channel();
    std::thread::Builder::new()
        .name(String::from("weather-fetch"))
        .spawn(move || {
            // The window may have closed, or asked something newer, and
            // dropped the receiver; then there is nobody to tell.
            let _nobody_left = tx.send(work());
            if let Some(waker) = waker {
                waker.wake();
            }
        })
        .map_err(|e| format!("the request could not be started: {e}"))?;
    Ok(answer)
}

/// Take the answer to the request in `slot` if it has come, and the request
/// with it. A thread that ended without answering -- only a panic would -- is
/// answered `ended`, so the window is not left asking for ever.
fn answered<A, T>(slot: &mut Option<Pending<A, T>>, ended: &str) -> Option<(A, Result<T, String>)> {
    let answer = match slot.as_ref()?.answer.try_recv() {
        Ok(answer) => answer,
        Err(TryRecvError::Empty) => return None,
        Err(TryRecvError::Disconnected) => Err(ended.to_owned()),
    };
    slot.take().map(|pending| (pending.asked, answer))
}

impl WeatherApp {
    /// Turn forecasts on or off, keep the choice, and -- turned on, with a
    /// place to show -- ask for its weather.
    ///
    /// Turned off, what was fetched goes from the window too: a forecast
    /// left on screen would stop being refreshed and go stale without a word.
    pub fn set_forecasts(&mut self, on: bool) {
        self.source.on = on;
        if on {
            self.ask_forecast();
        } else {
            self.stop_fetching();
        }
        self.store_places();
    }

    /// Forecasts off: the requests out are dropped -- their answers have
    /// nobody to go to -- and what was fetched or found goes from the window.
    pub fn stop_fetching(&mut self) {
        self.source.on = false;
        self.source.forecast = None;
        self.source.search = None;
        self.source.found.clear();
        self.source.searched = None;
        self.source.search_error = None;
        self.forget_weather();
    }

    /// Clear what was fetched: the place it was for is no longer shown, or
    /// forecasts are off. Everything the window draws from a forecast goes,
    /// with when it arrived and when it was last asked for -- so nothing is
    /// due again for weather not shown -- and why the last request failed.
    pub fn forget_weather(&mut self) {
        self.current = None;
        self.hourly.clear();
        self.daily.clear();
        self.source.fetched_at = None;
        self.source.tried_at = None;
        self.source.observed = None;
        self.source.error = None;
    }

    /// Ask for the places matching `name`. Nothing is sent with forecasts
    /// off, or for an empty name. A search already out is replaced.
    pub fn search_places(&mut self, name: &str) {
        let name = name.trim();
        if !self.source.on || name.is_empty() {
            return;
        }
        self.source.found.clear();
        self.source.searched = Some(name.to_owned());
        self.source.search_error = None;
        let fetcher = self.source.fetcher;
        let path = openmeteo::search_path(name);
        let spawned = spawn_request(self.source.waker.clone(), move || {
            fetcher(openmeteo::SEARCH_HOST, &path).and_then(|body| openmeteo::read_places(&body))
        });
        match spawned {
            Ok(answer) => {
                self.source.search = Some(Pending {
                    asked: name.to_owned(),
                    answer,
                });
            }
            Err(why) => {
                self.source.search = None;
                self.source.search_error = Some(format!(
                    "Could not search for \u{201c}{name}\u{201d}: {why}"
                ));
            }
        }
    }

    /// Ask for the weather at the place shown, if forecasts are on, replacing
    /// a forecast request already out. With no place shown, nothing is out
    /// afterwards: an answer would have no place to be shown under.
    pub fn ask_forecast(&mut self) {
        if !self.source.on {
            return;
        }
        let Some(location) = self.locations.get(self.active_location_idx) else {
            self.source.forecast = None;
            return;
        };
        let asked = ForecastFor {
            name: location.name.clone(),
            latitude: location.latitude,
            longitude: location.longitude,
        };
        let (latitude, longitude) = (asked.latitude, asked.longitude);
        let fetcher = self.source.fetcher;
        self.source.tried_at = Some(Instant::now());
        self.source.error = None;
        let spawned = spawn_request(self.source.waker.clone(), move || {
            let forecast = fetcher(
                openmeteo::FORECAST_HOST,
                &openmeteo::forecast_path(latitude, longitude),
            )
            .and_then(|body| openmeteo::read_forecast(&body))?;
            // A second service; the forecast stands without it.
            let air = fetcher(
                openmeteo::AIR_HOST,
                &openmeteo::air_path(latitude, longitude),
            )
            .and_then(|body| openmeteo::read_us_aqi(&body))
            .ok();
            Ok((forecast, air))
        });
        match spawned {
            Ok(answer) => self.source.forecast = Some(Pending { asked, answer }),
            Err(why) => {
                self.source.forecast = None;
                self.source.error = Some(format!("Could not get the forecast: {why}"));
            }
        }
    }

    /// Take in the answers that have come. Whether anything shown changed.
    pub fn pump(&mut self) -> bool {
        let mut changed = false;
        if let Some((name, answer)) = answered(
            &mut self.source.search,
            "the search ended without an answer",
        ) {
            match answer {
                Ok(places) => self.source.found = places,
                Err(why) => {
                    self.source.search_error = Some(format!(
                        "Could not search for \u{201c}{name}\u{201d}: {why}"
                    ));
                }
            }
            changed = true;
        }
        if let Some((asked, answer)) = answered(
            &mut self.source.forecast,
            "the request ended without an answer",
        ) {
            // An answer for a place no longer shown is not this window's
            // answer any more.
            if self.still_shown(&asked) {
                match answer {
                    Ok((forecast, air)) => {
                        let mut current = forecast.current;
                        current.aqi = air;
                        self.current = Some(current);
                        self.hourly = forecast.hourly;
                        self.daily = forecast.daily;
                        self.source.observed = Some(forecast.observed);
                        self.source.fetched_at = Some(Instant::now());
                    }
                    Err(why) => {
                        self.source.error = Some(format!("Could not get the forecast: {why}"));
                    }
                }
                changed = true;
            }
        }
        changed
    }

    /// Whether `asked` is for the place the window shows now.
    fn still_shown(&self, asked: &ForecastFor) -> bool {
        self.source.on
            && self
                .locations
                .get(self.active_location_idx)
                .is_some_and(|l| {
                    same_spot(l.latitude, asked.latitude) && same_spot(l.longitude, asked.longitude)
                })
    }

    /// When the place shown is next asked for without the user asking:
    /// [`REFRESH`] after its forecast arrived, or [`RETRY`] after a request
    /// that failed. `None` with forecasts off, a forecast request out, no
    /// place shown, or nothing asked for yet.
    #[must_use]
    pub fn next_ask(&self) -> Option<Instant> {
        if !self.source.on
            || self.source.forecast.is_some()
            || self.locations.get(self.active_location_idx).is_none()
        {
            return None;
        }
        if self.source.error.is_some() {
            self.source.tried_at?.checked_add(RETRY)
        } else {
            self.source.fetched_at?.checked_add(REFRESH)
        }
    }

    /// Whether the place shown is due to be asked for again at `now`.
    #[must_use]
    pub fn refresh_due(&self, now: Instant) -> bool {
        self.next_ask().is_some_and(|at| now >= at)
    }

    /// Add `place` -- one a search offered -- show it, keep it, and ask for
    /// its weather. A place already in the list is shown rather than added
    /// twice.
    pub fn add_place(&mut self, place: &Place) {
        let existing = self.locations.iter().position(|l| {
            same_spot(l.latitude, place.latitude) && same_spot(l.longitude, place.longitude)
        });
        let index = if let Some(i) = existing {
            i
        } else {
            let is_default = self.locations.is_empty();
            self.locations.push(Location {
                name: place.label(),
                is_default,
                latitude: place.latitude,
                longitude: place.longitude,
            });
            self.locations.len().saturating_sub(1)
        };
        self.source.found.clear();
        self.source.searched = None;
        self.source.search_error = None;
        self.show_location(index);
        self.store_places();
    }

    /// Show location `index`, asking for its weather -- unless it is the one
    /// shown and its forecast is up or on its way.
    pub fn show_location(&mut self, index: usize) {
        if index >= self.locations.len() {
            return;
        }
        let same = index == self.active_location_idx
            && (self.current.is_some() || self.source.forecast.is_some());
        self.active_location_idx = index;
        if !same {
            self.forget_weather();
            self.ask_forecast();
        }
    }

    /// Read the switch and the places from the user's settings. Anything but
    /// `forecasts: true` is off; a place missing its name or a coordinate, or
    /// with a coordinate off the globe, is left out.
    pub fn load_places(&mut self, doc: &yamldoc::Document) {
        self.source.on = doc.get_bool(&["forecasts"]) == Some(true);
        let mut locations = Vec::new();
        for key in doc.keys(&["places"]) {
            let key = key.as_str();
            let (Some(name), Some(latitude), Some(longitude)) = (
                doc.get_str(&["places", key, "name"]),
                doc.get_f64(&["places", key, "latitude"]),
                doc.get_f64(&["places", key, "longitude"]),
            ) else {
                continue;
            };
            if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
                continue;
            }
            locations.push(Location {
                name,
                is_default: false,
                latitude,
                longitude,
            });
        }
        // The default place is the one shown at start; the first, when the
        // file names none.
        let default = doc
            .get_i64(&["default"])
            .and_then(|i| usize::try_from(i).ok())
            .filter(|i| *i < locations.len())
            .unwrap_or(0);
        if let Some(place) = locations.get_mut(default) {
            place.is_default = true;
        }
        self.locations = locations;
        self.active_location_idx = default;
    }

    /// Keep the switch and the places in the user's settings.
    pub fn store_places(&mut self) {
        let mut doc = settingsfile::load(CONFIG_NAME);
        doc.set_bool(&["forecasts"], self.source.on);
        doc.remove(&["places"]);
        for (i, location) in self.locations.iter().enumerate() {
            let key = format!("p{i}");
            doc.set_str(&["places", &key, "name"], &location.name);
            doc.set_f64(&["places", &key, "latitude"], location.latitude);
            doc.set_f64(&["places", &key, "longitude"], location.longitude);
        }
        let default = self
            .locations
            .iter()
            .position(|l| l.is_default)
            .unwrap_or(0);
        doc.set_i64(&["default"], i64::try_from(default).unwrap_or(0));
        if let Err(e) = settingsfile::store(CONFIG_NAME, &doc) {
            self.settings_error = Some(format!("The places could not be kept for next time: {e}"));
        }
    }
}

/// Whether two coordinates are one place: closer than the six decimal places
/// the service gives, about ten centimetres.
fn same_spot(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}
