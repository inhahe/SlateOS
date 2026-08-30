#![allow(dead_code)]
//! Internet Radio — streaming radio player for SlateOS.
//!
//! Features:
//! - Preset stations organized by genre
//! - Custom station URL entry
//! - Favorites management
//! - Recently played history
//! - Playback controls (play/stop, volume, mute)
//! - Station metadata display (name, genre, bitrate, codec, description)
//! - Genre categories with station browsing
//! - Sleep timer
//! - Recording simulation (save current stream)
//! - Now playing visualization (simulated spectrum)
//! - Station search

use guitk::color::Color;
use guitk::listview::ListViewport;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::rng::{seeded_from_system, RandomSource, SeededRng};
use guitk::scroll_window;
use guitk::style::CornerRadii;

// ── Layout ─────────────────────────────────────────────────────────────────
//
// Both scrolling lists in this window are drawn by one function and scrolled by
// another, so every distance either of them needs is named here rather than
// spelled out at each site. The genre list previously had its bottom edge
// written out twice with two different values, and drew over the search hint
// as a result.

/// Width of the tab-and-genre sidebar.
const SIDEBAR_WIDTH: f32 = 160.0;
/// Height of the now-playing bar pinned to the bottom of the window.
const PLAYER_BAR_HEIGHT: f32 = 80.0;
/// Height of one screen tab in the sidebar (Browse / Favorites / Recent).
const SIDEBAR_TAB_HEIGHT: f32 = 24.0;
/// Height of one genre row, including the gap under it.
const GENRE_ROW_HEIGHT: f32 = 20.0;
/// Height of the "[/] Search" hint pinned to the bottom of the sidebar.
const SEARCH_HINT_HEIGHT: f32 = 20.0;
/// Vertical space a list keeps for its "N more" line.
///
/// Reserved whether or not the line is drawn, so that how many rows fit does
/// not depend on how many rows fit.
const LIST_MORE_HEIGHT: f32 = 16.0;

/// Y of the sidebar's first genre row, measured from the sidebar's own top.
///
/// The title, the three screen tabs, the gap beneath them, the "Genres"
/// heading and the always-present "All Genres" row, summed once. Named because
/// the renderer walks down this distance a piece at a time while
/// [`RadioApp::genre_rows`] needs it as a single number, and two spellings of
/// one distance is one spelling too many; the renderer asserts they agree.
const GENRE_ROWS_TOP: f32 = 32.0
    + 3.0 * SIDEBAR_TAB_HEIGHT
    + 8.0
    + 16.0
    + GENRE_ROW_HEIGHT;

/// Height of one station row in the main list.
const STATION_ROW_HEIGHT: f32 = 50.0;
/// Y of the station list's first row, from the top of the list pane.
const STATION_ROWS_TOP: f32 = 30.0;

// ── Catppuccin Mocha palette ───────────────────────────────────────────────
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const SKY: Color = Color::from_hex(0x89DCEB);

// ── Genre ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Genre {
    Rock,
    Pop,
    Jazz,
    Classical,
    Electronic,
    HipHop,
    Country,
    RnB,
    Metal,
    Blues,
    Ambient,
    News,
    Talk,
    Lofi,
    World,
}

impl Genre {
    const ALL: [Self; 15] = [
        Self::Rock, Self::Pop, Self::Jazz, Self::Classical, Self::Electronic,
        Self::HipHop, Self::Country, Self::RnB, Self::Metal, Self::Blues,
        Self::Ambient, Self::News, Self::Talk, Self::Lofi, Self::World,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Rock => "Rock",
            Self::Pop => "Pop",
            Self::Jazz => "Jazz",
            Self::Classical => "Classical",
            Self::Electronic => "Electronic",
            Self::HipHop => "Hip-Hop",
            Self::Country => "Country",
            Self::RnB => "R&B",
            Self::Metal => "Metal",
            Self::Blues => "Blues",
            Self::Ambient => "Ambient",
            Self::News => "News",
            Self::Talk => "Talk",
            Self::Lofi => "Lo-Fi",
            Self::World => "World",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Rock => RED,
            Self::Pop => MAUVE,
            Self::Jazz => YELLOW,
            Self::Classical => LAVENDER,
            Self::Electronic => BLUE,
            Self::HipHop => PEACH,
            Self::Country => GREEN,
            Self::RnB => TEAL,
            Self::Metal => SUBTEXT0,
            Self::Blues => SKY,
            Self::Ambient => Color::from_hex(0x74C7EC),
            Self::News => SUBTEXT1,
            Self::Talk => OVERLAY0,
            Self::Lofi => Color::from_hex(0xF2CDCD),
            Self::World => Color::from_hex(0xF5E0DC),
        }
    }
}

// ── Audio Codec ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Codec {
    Mp3,
    Aac,
    Ogg,
    Flac,
    Opus,
    Wma,
}

impl Codec {
    fn label(self) -> &'static str {
        match self {
            Self::Mp3 => "MP3",
            Self::Aac => "AAC",
            Self::Ogg => "OGG",
            Self::Flac => "FLAC",
            Self::Opus => "Opus",
            Self::Wma => "WMA",
        }
    }
}

// ── Station ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Station {
    name: String,
    url: String,
    genre: Genre,
    bitrate_kbps: u32,
    codec: Codec,
    description: String,
    country: String,
    language: String,
}

fn preset_stations() -> Vec<Station> {
    vec![
        Station {
            name: "Classic Rock FM".into(), url: "http://classicrock.fm/stream".into(),
            genre: Genre::Rock, bitrate_kbps: 192, codec: Codec::Mp3,
            description: "The best classic rock from the 60s, 70s, and 80s".into(),
            country: "US".into(), language: "English".into(),
        },
        Station {
            name: "Indie Rock Radio".into(), url: "http://indierock.io/live".into(),
            genre: Genre::Rock, bitrate_kbps: 128, codec: Codec::Aac,
            description: "Indie and alternative rock discoveries".into(),
            country: "UK".into(), language: "English".into(),
        },
        Station {
            name: "Pop Hits Today".into(), url: "http://pophits.today/stream".into(),
            genre: Genre::Pop, bitrate_kbps: 256, codec: Codec::Mp3,
            description: "Today's biggest pop hits, 24/7".into(),
            country: "US".into(), language: "English".into(),
        },
        Station {
            name: "Smooth Jazz Cafe".into(), url: "http://smoothjazz.cafe/live".into(),
            genre: Genre::Jazz, bitrate_kbps: 320, codec: Codec::Flac,
            description: "Smooth jazz for relaxation and focus".into(),
            country: "US".into(), language: "English".into(),
        },
        Station {
            name: "Jazz FM London".into(), url: "http://jazzfm.london/stream".into(),
            genre: Genre::Jazz, bitrate_kbps: 192, codec: Codec::Aac,
            description: "London's premier jazz station".into(),
            country: "UK".into(), language: "English".into(),
        },
        Station {
            name: "Classical WQXR".into(), url: "http://wqxr.org/stream".into(),
            genre: Genre::Classical, bitrate_kbps: 320, codec: Codec::Flac,
            description: "Classical music from New York".into(),
            country: "US".into(), language: "English".into(),
        },
        Station {
            name: "BBC Radio 3".into(), url: "http://bbc.co.uk/radio3/stream".into(),
            genre: Genre::Classical, bitrate_kbps: 320, codec: Codec::Aac,
            description: "Classical, jazz, world music from the BBC".into(),
            country: "UK".into(), language: "English".into(),
        },
        Station {
            name: "Electro Beats FM".into(), url: "http://electrobeats.fm/live".into(),
            genre: Genre::Electronic, bitrate_kbps: 256, codec: Codec::Ogg,
            description: "Electronic dance music around the clock".into(),
            country: "DE".into(), language: "English".into(),
        },
        Station {
            name: "Chillwave Radio".into(), url: "http://chillwave.radio/stream".into(),
            genre: Genre::Electronic, bitrate_kbps: 192, codec: Codec::Opus,
            description: "Chill electronic vibes for any mood".into(),
            country: "NL".into(), language: "English".into(),
        },
        Station {
            name: "Beats1 Hip-Hop".into(), url: "http://beats1.hiphop/live".into(),
            genre: Genre::HipHop, bitrate_kbps: 192, codec: Codec::Mp3,
            description: "Hip-hop and rap, new and classic".into(),
            country: "US".into(), language: "English".into(),
        },
        Station {
            name: "Nashville Country".into(), url: "http://nashville.country/stream".into(),
            genre: Genre::Country, bitrate_kbps: 128, codec: Codec::Mp3,
            description: "Country music straight from Nashville".into(),
            country: "US".into(), language: "English".into(),
        },
        Station {
            name: "Soul & RnB Radio".into(), url: "http://soulrnb.radio/live".into(),
            genre: Genre::RnB, bitrate_kbps: 192, codec: Codec::Aac,
            description: "Soul, R&B, and Motown classics".into(),
            country: "US".into(), language: "English".into(),
        },
        Station {
            name: "Metal Mayhem".into(), url: "http://metalmayhem.fm/stream".into(),
            genre: Genre::Metal, bitrate_kbps: 256, codec: Codec::Mp3,
            description: "Heavy metal, thrash, death metal".into(),
            country: "SE".into(), language: "English".into(),
        },
        Station {
            name: "Delta Blues Radio".into(), url: "http://deltablues.radio/live".into(),
            genre: Genre::Blues, bitrate_kbps: 192, codec: Codec::Mp3,
            description: "Mississippi delta blues and Chicago blues".into(),
            country: "US".into(), language: "English".into(),
        },
        Station {
            name: "Ambient Worlds".into(), url: "http://ambientworlds.fm/stream".into(),
            genre: Genre::Ambient, bitrate_kbps: 256, codec: Codec::Flac,
            description: "Ambient soundscapes for meditation and sleep".into(),
            country: "JP".into(), language: "English".into(),
        },
        Station {
            name: "NPR News".into(), url: "http://npr.org/stream".into(),
            genre: Genre::News, bitrate_kbps: 64, codec: Codec::Mp3,
            description: "National Public Radio news and analysis".into(),
            country: "US".into(), language: "English".into(),
        },
        Station {
            name: "BBC World Service".into(), url: "http://bbc.co.uk/worldservice/stream".into(),
            genre: Genre::News, bitrate_kbps: 96, codec: Codec::Aac,
            description: "International news from the BBC".into(),
            country: "UK".into(), language: "English".into(),
        },
        Station {
            name: "Lo-Fi Hip Hop Beats".into(), url: "http://lofi.beats/stream".into(),
            genre: Genre::Lofi, bitrate_kbps: 128, codec: Codec::Ogg,
            description: "Lo-fi beats to relax/study to".into(),
            country: "JP".into(), language: "English".into(),
        },
        Station {
            name: "Lo-Fi Cafe".into(), url: "http://lofi.cafe/live".into(),
            genre: Genre::Lofi, bitrate_kbps: 192, codec: Codec::Opus,
            description: "Cozy lo-fi music for focus and chill".into(),
            country: "US".into(), language: "English".into(),
        },
        Station {
            name: "World Music Channel".into(), url: "http://worldmusic.ch/stream".into(),
            genre: Genre::World, bitrate_kbps: 192, codec: Codec::Mp3,
            description: "Music from every corner of the globe".into(),
            country: "CH".into(), language: "Multiple".into(),
        },
    ]
}

// ── Playback State ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlayState {
    Stopped,
    Buffering,
    Playing,
    Error,
}

// ── Sleep Timer ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SleepTimer {
    Off,
    Minutes15,
    Minutes30,
    Minutes60,
    Minutes90,
    Minutes120,
}

impl SleepTimer {
    fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Minutes15 => "15 min",
            Self::Minutes30 => "30 min",
            Self::Minutes60 => "1 hour",
            Self::Minutes90 => "1.5 hours",
            Self::Minutes120 => "2 hours",
        }
    }

    fn seconds(self) -> Option<u32> {
        match self {
            Self::Off => None,
            Self::Minutes15 => Some(900),
            Self::Minutes30 => Some(1800),
            Self::Minutes60 => Some(3600),
            Self::Minutes90 => Some(5400),
            Self::Minutes120 => Some(7200),
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Off => Self::Minutes15,
            Self::Minutes15 => Self::Minutes30,
            Self::Minutes30 => Self::Minutes60,
            Self::Minutes60 => Self::Minutes90,
            Self::Minutes90 => Self::Minutes120,
            Self::Minutes120 => Self::Off,
        }
    }
}

// ── Application State ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Browse,
    Favorites,
    Recent,
    Search,
}

struct RadioApp {
    stations: Vec<Station>,
    favorites: Vec<usize>, // indices into stations
    recent: Vec<usize>,
    max_recent: usize,

    // Genre filter
    genre_filter: Option<Genre>,
    /// Which genres the sidebar is showing, and which of them is the filter.
    ///
    /// The two move together because `genre_filter` is cycled by Left/Right
    /// without regard to what is on screen: a filter set without a matching
    /// scroll leaves the highlighted genre off the bottom of the sidebar, with
    /// no key that brings it back. `genre_scroll`, which this replaces, was
    /// written by nothing and read by nothing at all.
    ///
    /// `None` means the "All Genres" row, which is drawn above the scrolled
    /// region and so is visible at any offset.
    genre_view: ListViewport,

    /// Which stations the list pane is showing, and which of them is picked.
    ///
    /// One field rather than a selection and a scroll offset, because the rule
    /// binding them — the picked station is on screen — has to be restored on
    /// every movement, and a rule spread across four key handlers is a rule
    /// that holds in three of them. Here it held in none: `station_scroll` was
    /// read by the renderer and written by nobody, so Down past the tenth
    /// station moved a selection that stayed off screen for good.
    station_view: ListViewport,

    // Playback
    play_state: PlayState,
    current_station: Option<usize>,
    volume: u8, // 0-100
    muted: bool,
    listen_time_secs: u32, // how long currently listening

    // Sleep timer
    sleep_timer: SleepTimer,
    sleep_remaining_secs: Option<u32>,

    // Recording
    recording: bool,
    record_duration_secs: u32,

    // Spectrum visualization (simulated)
    spectrum_bars: [u8; 32],
    /// Drives the simulated spectrum animation. See `tick` for what this
    /// replaced and why the old version made the right-hand bars go still.
    spectrum_rng: SeededRng,

    // Search
    search_query: String,
    search_results: Vec<usize>,
    search_selected: usize,
    search_active: bool,

    // UI
    screen: Screen,
    status_message: String,
    width: f32,
    height: f32,
}

/// Seed used when the kernel's entropy source cannot be reached.
///
/// A per-crate constant rather than a shared one, so that two programs which
/// lose entropy on the same boot do not then produce correlated streams. The
/// bytes spell `RADIO!!!`.
const FALLBACK_SEED: u64 = 0x5241_4449_4F21_2121;

/// The tallest a spectrum bar may draw.
const SPECTRUM_CEILING: u8 = 100;

impl RadioApp {
    fn new() -> Self {
        let stations = preset_stations();
        let mut spectrum_bars = [0u8; 32];
        for (i, bar) in spectrum_bars.iter_mut().enumerate() {
            *bar = ((i as u8).wrapping_mul(7).wrapping_add(30)) % 100;
        }
        let mut app = Self {
            stations,
            favorites: Vec::new(),
            recent: Vec::new(),
            max_recent: 30,
            genre_filter: None,
            genre_view: ListViewport::new(0),
            station_view: ListViewport::new(0),
            play_state: PlayState::Stopped,
            current_station: None,
            volume: 75,
            muted: false,
            listen_time_secs: 0,
            sleep_timer: SleepTimer::Off,
            sleep_remaining_secs: None,
            recording: false,
            record_duration_secs: 0,
            spectrum_bars,
            spectrum_rng: seeded_from_system(FALLBACK_SEED),
            search_query: String::new(),
            search_results: Vec::new(),
            search_selected: 0,
            search_active: false,
            screen: Screen::Browse,
            status_message: "Select a station and press Enter to play".into(),
            width: 900.0,
            height: 650.0,
        };
        // Not `..Default::default()` on the two viewports: their heights come
        // from the window size, and a viewport whose height disagrees with the
        // pane it is drawn into is the bug this type exists to prevent.
        app.set_size(900.0, 650.0);
        app.select_station(0);
        app
    }

    /// Resizes the window, keeping both list viewports' heights in step.
    ///
    /// The single door through which `width`/`height` change. A viewport whose
    /// row count is left over from a taller window would let the selection sit
    /// below the last row actually drawn — the renderer would show one page and
    /// the key handler would believe another.
    fn set_size(&mut self, width: f32, height: f32) {
        self.width = width;
        self.height = height;
        let stations = self.filtered_stations().len();
        self.station_view.set_height(self.station_rows(), stations);
        self.genre_view
            .set_height(self.genre_rows(), Genre::ALL.len());
    }

    /// How many station rows the list pane can show at the current size.
    ///
    /// The renderer draws exactly this many, and the viewport uses it to decide
    /// when a selection has fallen off the bottom, so it is derived once.
    fn station_rows(&self) -> usize {
        let pane_h = self.height - PLAYER_BAR_HEIGHT;
        scroll_window::capacity(
            STATION_ROW_HEIGHT,
            pane_h - STATION_ROWS_TOP - LIST_MORE_HEIGHT,
        )
    }

    /// How many genre rows fit in the sidebar at the current size.
    ///
    /// Stops above the search hint, not at the sidebar's bottom edge: the two
    /// were confused before, so a short window drew the last genres straight
    /// over the hint. The "N more" line's space is subtracted unconditionally
    /// so the count does not depend on whether the line turns out to be needed.
    fn genre_rows(&self) -> usize {
        let sidebar_h = self.height - PLAYER_BAR_HEIGHT;
        scroll_window::capacity(
            GENRE_ROW_HEIGHT,
            sidebar_h - GENRE_ROWS_TOP - SEARCH_HINT_HEIGHT - LIST_MORE_HEIGHT,
        )
    }

    /// The picked station's position in the filtered list, or 0 when nothing is
    /// picked — which happens only when the filtered list is empty, where every
    /// caller's `get` returns `None` regardless.
    fn selected_station(&self) -> usize {
        self.station_view.selected().unwrap_or(0)
    }

    /// Picks the station at `index` in the filtered list and scrolls to it.
    fn select_station(&mut self, index: usize) {
        let len = self.filtered_stations().len();
        self.station_view.select(Some(index), len);
    }

    /// Sets the genre filter and scrolls the sidebar so the choice is on screen.
    ///
    /// Both Left and Right funnel through here: they differ only in which genre
    /// they pick, and everything after that — the reveal, the station list
    /// jumping back to the top, the status line — is the same in both and was
    /// duplicated in both.
    fn set_genre_filter(&mut self, genre: Option<Genre>) {
        self.genre_filter = genre;
        let index = genre.and_then(|g| Genre::ALL.iter().position(|&x| x == g));
        self.genre_view.select(index, Genre::ALL.len());
        // The filter decides which stations exist, so a position into the old
        // list would name a different station or none at all.
        self.select_station(0);
        let label = genre.map_or("All", Genre::label);
        self.status_message = format!("Genre: {label}");
    }

    /// Get filtered station list for current view.
    fn filtered_stations(&self) -> Vec<usize> {
        match self.screen {
            Screen::Browse => {
                if let Some(genre) = self.genre_filter {
                    self.stations.iter().enumerate()
                        .filter(|(_, s)| s.genre == genre)
                        .map(|(i, _)| i)
                        .collect()
                } else {
                    (0..self.stations.len()).collect()
                }
            }
            Screen::Favorites => self.favorites.clone(),
            Screen::Recent => self.recent.clone(),
            Screen::Search => self.search_results.clone(),
        }
    }

    // ── Playback ───────────────────────────────────────────────────────

    fn play_station(&mut self, station_idx: usize) {
        if station_idx >= self.stations.len() {
            return;
        }
        self.current_station = Some(station_idx);
        self.play_state = PlayState::Playing;
        self.listen_time_secs = 0;
        self.add_to_recent(station_idx);
        if let Some(s) = self.stations.get(station_idx) {
            self.status_message = format!("Playing: {}", s.name);
        }
    }

    fn stop(&mut self) {
        self.play_state = PlayState::Stopped;
        self.listen_time_secs = 0;
        self.recording = false;
        self.record_duration_secs = 0;
        self.status_message = "Stopped".into();
    }

    fn toggle_play(&mut self) {
        match self.play_state {
            PlayState::Stopped => {
                let filtered = self.filtered_stations();
                if let Some(&idx) = filtered.get(self.selected_station()) {
                    self.play_station(idx);
                }
            }
            PlayState::Playing | PlayState::Buffering => self.stop(),
            PlayState::Error => {
                // Retry
                if let Some(idx) = self.current_station {
                    self.play_station(idx);
                }
            }
        }
    }

    fn volume_up(&mut self) {
        self.volume = self.volume.saturating_add(5).min(100);
        self.muted = false;
        self.status_message = format!("Volume: {}%", self.volume);
    }

    fn volume_down(&mut self) {
        self.volume = self.volume.saturating_sub(5);
        self.status_message = format!("Volume: {}%", self.volume);
    }

    fn toggle_mute(&mut self) {
        self.muted = !self.muted;
        self.status_message = if self.muted {
            "Muted".into()
        } else {
            format!("Volume: {}%", self.volume)
        };
    }

    // ── Recent/Favorites ───────────────────────────────────────────────

    fn add_to_recent(&mut self, idx: usize) {
        self.recent.retain(|&i| i != idx);
        self.recent.insert(0, idx);
        if self.recent.len() > self.max_recent {
            self.recent.truncate(self.max_recent);
        }
    }

    fn toggle_favorite(&mut self) {
        let filtered = self.filtered_stations();
        if let Some(&idx) = filtered.get(self.selected_station()) {
            if self.favorites.contains(&idx) {
                self.favorites.retain(|&i| i != idx);
                if let Some(s) = self.stations.get(idx) {
                    self.status_message = format!("Removed '{}' from favorites", s.name);
                }
            } else {
                self.favorites.push(idx);
                if let Some(s) = self.stations.get(idx) {
                    self.status_message = format!("Added '{}' to favorites", s.name);
                }
            }
        }
    }

    fn is_current_favorite(&self) -> bool {
        let filtered = self.filtered_stations();
        filtered.get(self.selected_station())
            .map(|idx| self.favorites.contains(idx))
            .unwrap_or(false)
    }

    // ── Timer ──────────────────────────────────────────────────────────

    fn set_sleep_timer(&mut self) {
        self.sleep_timer = self.sleep_timer.next();
        self.sleep_remaining_secs = self.sleep_timer.seconds();
        self.status_message = format!("Sleep timer: {}", self.sleep_timer.label());
    }

    fn tick(&mut self) {
        if self.play_state == PlayState::Playing {
            self.listen_time_secs = self.listen_time_secs.saturating_add(1);

            // Update spectrum (simulated).
            //
            // This used to advance one glibc LCG per frame
            // (`* 1103515245 + 12345`) and then read bar `i`'s height out of
            // bits `i..i+6` of that single state. Two things went wrong with
            // that, both visible on screen:
            //
            // - **The right-hand bars went still.** Bar 31 has only one bit of
            //   the state left above it, so it alternated between exactly two
            //   heights forever; bar 30 had four, bar 29 eight, bar 28 sixteen.
            //   Measured over 64 frames the distinct-height counts across the
            //   32 bars ran ... 45, 30, 16, 8, 4, 2. A spectrum analyser whose
            //   right quarter is frozen is the one thing it must not be.
            // - **The left-hand bars pinned at the ceiling.** `base` is 60 at
            //   the left and the noise reached 63, so `min(100)` clipped
            //   roughly a third of frames flat against the top.
            //
            // Now each bar draws its own value, sized so the jitter fills the
            // space between its base and the ceiling exactly, with nothing
            // clipped and no bar sharing bits with its neighbour.
            for (i, bar) in self.spectrum_bars.iter_mut().enumerate() {
                let base = if i < 8 {
                    60u8
                } else if i < 16 {
                    45
                } else {
                    30
                };
                // Saturating rather than `-`/`+`: every base is below the
                // ceiling today, but an edit that raised one above it should
                // quietly draw a flat bar, not underflow to a headroom of 256.
                let headroom =
                    usize::from(SPECTRUM_CEILING.saturating_sub(base)).saturating_add(1);
                let noise = u8::try_from(self.spectrum_rng.below(headroom)).unwrap_or(0);
                *bar = base.saturating_add(noise).min(SPECTRUM_CEILING);
            }

            // Recording timer
            if self.recording {
                self.record_duration_secs = self.record_duration_secs.saturating_add(1);
            }
        }

        // Sleep timer countdown
        if let Some(ref mut remaining) = self.sleep_remaining_secs {
            if *remaining > 0 {
                *remaining = remaining.saturating_sub(1);
            }
            if *remaining == 0 {
                self.stop();
                self.sleep_timer = SleepTimer::Off;
                self.sleep_remaining_secs = None;
                self.status_message = "Sleep timer: playback stopped".into();
            }
        }
    }

    // ── Recording ──────────────────────────────────────────────────────

    fn toggle_recording(&mut self) {
        if self.play_state != PlayState::Playing {
            self.status_message = "Must be playing to record".into();
            return;
        }
        self.recording = !self.recording;
        if self.recording {
            self.record_duration_secs = 0;
            self.status_message = "Recording started".into();
        } else {
            self.status_message = format!(
                "Recording saved ({})",
                Self::format_time(self.record_duration_secs)
            );
        }
    }

    // ── Search ─────────────────────────────────────────────────────────

    fn perform_search(&mut self) {
        self.search_results.clear();
        self.search_selected = 0;
        let query = self.search_query.trim().to_lowercase();
        if query.is_empty() {
            return;
        }

        for (i, station) in self.stations.iter().enumerate() {
            if station.name.to_lowercase().contains(&query)
                || station.genre.label().to_lowercase().contains(&query)
                || station.description.to_lowercase().contains(&query)
                || station.country.to_lowercase().contains(&query)
            {
                self.search_results.push(i);
            }
        }

        self.status_message = if self.search_results.is_empty() {
            format!("No stations found for '{}'", self.search_query)
        } else {
            format!("{} stations found", self.search_results.len())
        };
    }

    // ── Helpers ─────────────────────────────────────────────────────────

    fn format_time(secs: u32) -> String {
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        let s = secs % 60;
        if h > 0 {
            format!("{h}:{m:02}:{s:02}")
        } else {
            format!("{m}:{s:02}")
        }
    }

    // ── Input ──────────────────────────────────────────────────────────

    fn handle_key(&mut self, key: &str, ctrl: bool, _shift: bool) {
        // Search mode
        if self.search_active {
            match key {
                "Escape" => {
                    self.search_active = false;
                    self.search_query.clear();
                    self.search_results.clear();
                    self.screen = Screen::Browse;
                }
                "Return" | "Enter"
                    if !self.search_results.is_empty() => {
                        self.screen = Screen::Search;
                        self.search_active = false;
                    }
                "BackSpace" => {
                    self.search_query.pop();
                    self.perform_search();
                }
                _ if key.len() == 1 && !ctrl => {
                    self.search_query.push_str(key);
                    self.perform_search();
                }
                _ => {}
            }
            return;
        }

        match key {
            // Playback
            " " | "Return" | "Enter" => self.toggle_play(),
            "s" if !ctrl => self.stop(),

            // Volume
            "+" | "=" => self.volume_up(),
            "-" => self.volume_down(),
            "m" if !ctrl => self.toggle_mute(),

            // Navigation. Every arm scrolls the list to keep the new selection
            // on screen, which is the whole reason it goes through the viewport
            // rather than assigning an index: moving a selection the list does
            // not follow is how Down used to walk off the bottom of the pane.
            //
            // A page is now a windowful rather than a fixed five rows, so
            // PageDown lands on the row after the last one visible instead of
            // somewhere in the middle of the page it was already showing.
            "Up" => {
                let len = self.filtered_stations().len();
                self.station_view.select_prev(len);
            }
            "Down" => {
                let len = self.filtered_stations().len();
                self.station_view.select_next(len);
            }
            "PageUp" | "Prior" => {
                let len = self.filtered_stations().len();
                self.station_view.page_up(len);
            }
            "PageDown" | "Next" => {
                let len = self.filtered_stations().len();
                self.station_view.page_down(len);
            }

            // Genre filter
            "Left" if self.screen == Screen::Browse => {
                // Cycle genre filter backward, wrapping through "All".
                let previous = match self.genre_filter {
                    None => Some(Genre::World),
                    Some(g) => {
                        let idx = Genre::ALL.iter().position(|&x| x == g).unwrap_or(0);
                        if idx == 0 {
                            None
                        } else {
                            Genre::ALL.get(idx.saturating_sub(1)).copied()
                        }
                    }
                };
                self.set_genre_filter(previous);
            }
            "Right" if self.screen == Screen::Browse => {
                let next = match self.genre_filter {
                    None => Some(Genre::Rock),
                    Some(g) => {
                        let idx = Genre::ALL.iter().position(|&x| x == g).unwrap_or(0);
                        let next = idx.saturating_add(1);
                        if next >= Genre::ALL.len() {
                            None
                        } else {
                            Genre::ALL.get(next).copied()
                        }
                    }
                };
                self.set_genre_filter(next);
            }

            // Favorite
            "f" if !ctrl => self.toggle_favorite(),

            // Screen switching
            // A screen switch replaces the list rather than editing it, so the
            // old position names nothing; `select_station` scrolls back to the
            // top as well as picking the first row.
            "1" => { self.screen = Screen::Browse; self.select_station(0); }
            "2" => { self.screen = Screen::Favorites; self.select_station(0); }
            "3" => { self.screen = Screen::Recent; self.select_station(0); }

            // Search
            "/" => {
                self.search_active = true;
                self.search_query.clear();
                self.status_message = "Type to search stations...".into();
            }

            // Sleep timer
            "t" if !ctrl => self.set_sleep_timer(),

            // Recording
            "r" if !ctrl => self.toggle_recording(),

            _ => {}
        }
    }

    // ── Rendering ──────────────────────────────────────────────────────

    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0, y: 0.0, width: self.width, height: self.height,
            color: BASE, corner_radii: CornerRadii::ZERO,
        });

        // Layout:
        // [Genre sidebar 160px] [Station list] [Now Playing bar 80px at bottom]
        let sidebar_w = SIDEBAR_WIDTH;
        let player_h = PLAYER_BAR_HEIGHT;
        let main_x = sidebar_w;
        let main_w = self.width - sidebar_w;
        let main_h = self.height - player_h;

        // Genre sidebar
        self.render_sidebar(&mut cmds, 0.0, 0.0, sidebar_w, main_h);

        // Station list
        self.render_station_list(&mut cmds, main_x, 0.0, main_w, main_h);

        // Now playing bar
        self.render_player_bar(&mut cmds, 0.0, main_h, self.width, player_h);

        // Search overlay
        if self.search_active {
            self.render_search_overlay(&mut cmds);
        }

        cmds
    }

    fn render_sidebar(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, h: f32) {
        // `genre_rows` sizes the genre list from `self.height`; if the sidebar
        // it is drawn into were some other height the list would stop in the
        // wrong place, which is the class of bug this whole function was in.
        debug_assert!(
            ((self.height - PLAYER_BAR_HEIGHT) - h).abs() < 0.01,
            "the sidebar must be the one `genre_rows` sized the genre list for"
        );
        cmds.push(RenderCommand::FillRect {
            x, y, width: w, height: h,
            color: MANTLE, corner_radii: CornerRadii::ZERO,
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: x + 12.0, y: y + 10.0,
            text: "Internet Radio".into(), font_size: 14.0,
            color: BLUE, font_weight: FontWeightHint::Bold,
            max_width: Some(w - 24.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Tabs
        let tabs = [
            ("1:Browse", Screen::Browse),
            ("2:Favorites", Screen::Favorites),
            ("3:Recent", Screen::Recent),
        ];
        let mut ty = y + 32.0;
        for (label, scr) in &tabs {
            let active = self.screen == *scr;
            if active {
                cmds.push(RenderCommand::FillRect {
                    x: x + 4.0, y: ty, width: w - 8.0, height: 20.0,
                    color: SURFACE0, corner_radii: CornerRadii::all(4.0),
                });
            }
            cmds.push(RenderCommand::Text {
                x: x + 12.0, y: ty + 4.0,
                text: label.to_string(), font_size: 10.0,
                color: if active { TEXT_COLOR } else { SUBTEXT0 },
                font_weight: if active { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(w - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
            ty += SIDEBAR_TAB_HEIGHT;
        }

        ty += 8.0;

        // Genre filter (only in Browse)
        if self.screen == Screen::Browse {
            cmds.push(RenderCommand::Text {
                x: x + 12.0, y: ty,
                text: "Genres [Left/Right]".into(), font_size: 10.0,
                color: OVERLAY0, font_weight: FontWeightHint::Bold,
                max_width: Some(w - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
            ty += 16.0;

            // All genres option
            let all_active = self.genre_filter.is_none();
            if all_active {
                cmds.push(RenderCommand::FillRect {
                    x: x + 6.0, y: ty, width: w - 12.0, height: 18.0,
                    color: SURFACE0, corner_radii: CornerRadii::all(3.0),
                });
            }
            cmds.push(RenderCommand::Text {
                x: x + 14.0, y: ty + 3.0,
                text: "All Genres".into(), font_size: 10.0,
                color: if all_active { TEXT_COLOR } else { SUBTEXT0 },
                font_weight: FontWeightHint::Regular,
                max_width: Some(w - 28.0),
                overflow: TextOverflow::Ellipsis,
            });
            ty += GENRE_ROW_HEIGHT;

            // The distance walked down to here, in pieces, is the one
            // `genre_rows` needs as a single number. Assert rather than
            // recompute, so a change to any piece above is caught here instead
            // of quietly shifting where the list is allowed to stop.
            debug_assert!(
                (ty - (y + GENRE_ROWS_TOP)).abs() < 0.01,
                "GENRE_ROWS_TOP must be the y the genre rows actually start at"
            );

            // Bounded by where the search hint begins, not by the sidebar's
            // bottom edge: the old `ty + 18.0 > y + h` let the last genres draw
            // straight over the hint at `y + h - SEARCH_HINT_HEIGHT`.
            let window =
                scroll_window::visible_count(Genre::ALL.len(), self.genre_rows(), self.genre_view.first_visible());
            for (row, genre) in Genre::ALL
                .get(window.start..window.end())
                .unwrap_or_default()
                .iter()
                .enumerate()
            {
                let gy = ty + (row as f32) * GENRE_ROW_HEIGHT;
                let active = self.genre_filter == Some(*genre);
                if active {
                    cmds.push(RenderCommand::FillRect {
                        x: x + 6.0, y: gy, width: w - 12.0, height: 18.0,
                        color: SURFACE0, corner_radii: CornerRadii::all(3.0),
                    });
                }
                // Genre color dot
                cmds.push(RenderCommand::FillRect {
                    x: x + 10.0, y: gy + 5.0, width: 8.0, height: 8.0,
                    color: genre.color(), corner_radii: CornerRadii::all(4.0),
                });
                cmds.push(RenderCommand::Text {
                    x: x + 22.0, y: gy + 3.0,
                    text: genre.label().to_string(), font_size: 10.0,
                    color: if active { TEXT_COLOR } else { SUBTEXT0 },
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(w - 36.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            let hidden = Genre::ALL.len().saturating_sub(window.count);
            if hidden > 0 {
                cmds.push(RenderCommand::Text {
                    x: x + 22.0,
                    y: ty + (window.count as f32) * GENRE_ROW_HEIGHT + 3.0,
                    text: format!("{hidden} more"),
                    font_size: 9.0,
                    color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(w - 36.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }
        }

        // Search hint
        cmds.push(RenderCommand::Text {
            x: x + 12.0, y: y + h - SEARCH_HINT_HEIGHT,
            text: "[/] Search".into(), font_size: 9.0,
            color: OVERLAY0, font_weight: FontWeightHint::Regular,
            max_width: Some(w - 24.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Separator
        cmds.push(RenderCommand::FillRect {
            x: x + w - 1.0, y, width: 1.0, height: h,
            color: SURFACE0, corner_radii: CornerRadii::ZERO,
        });
    }

    fn render_station_list(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, h: f32) {
        let filtered = self.filtered_stations();
        let title = match self.screen {
            Screen::Browse => {
                let genre_label = self.genre_filter.map(|g| g.label()).unwrap_or("All");
                format!("{} ({} stations)", genre_label, filtered.len())
            }
            Screen::Favorites => format!("Favorites ({} stations)", filtered.len()),
            Screen::Recent => format!("Recently Played ({} stations)", filtered.len()),
            Screen::Search => format!("Search Results ({} stations)", filtered.len()),
        };

        cmds.push(RenderCommand::Text {
            x: x + 12.0, y: y + 8.0,
            text: title, font_size: 13.0,
            color: TEXT_COLOR, font_weight: FontWeightHint::Bold,
            max_width: Some(w - 24.0),
            overflow: TextOverflow::Ellipsis,
        });

        let start_y = y + STATION_ROWS_TOP;
        let row_h = STATION_ROW_HEIGHT;
        // The viewport decides where the selection may go; this pane decides
        // where rows land. If the two ever disagreed about how many rows fit,
        // the selection could sit below the last row drawn — which is exactly
        // the state the old code lived in permanently.
        debug_assert!(
            ((self.height - PLAYER_BAR_HEIGHT) - h).abs() < 0.01,
            "the station pane must be the one `station_rows` sized the viewport for"
        );

        if filtered.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + 20.0, y: start_y + 10.0,
                text: "No stations".into(), font_size: 12.0,
                color: OVERLAY0, font_weight: FontWeightHint::Regular,
                max_width: Some(w - 40.0),
                overflow: TextOverflow::Ellipsis,
            });
            return;
        }

        // `visible_range` re-derives the window against the length it is given,
        // so a list that shrank since the last keypress — a favorite removed
        // while the Favorites screen is up — shows its last page rather than
        // blank space.
        let window = self.station_view.visible_range(filtered.len());
        let shown = window.len();
        // Enumerate *after* the skip so `row` is the position on screen and needs
        // no subtraction to become a y-coordinate; the absolute index the
        // selection is compared against is reconstructed by adding the scroll
        // back on. Enumerating first and subtracting is the same number by a
        // route that underflows if the two ever disagree.
        let first = window.start;
        for (row, &station_idx) in filtered
            .get(window)
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            if let Some(station) = self.stations.get(station_idx) {
                let ry = start_y + (row as f32) * row_h;
                let is_sel = self.station_view.selected() == Some(first.saturating_add(row));
                let is_playing = self.current_station == Some(station_idx)
                    && self.play_state == PlayState::Playing;

                if is_sel {
                    cmds.push(RenderCommand::FillRect {
                        x: x + 4.0, y: ry, width: w - 8.0, height: row_h - 4.0,
                        color: SURFACE0, corner_radii: CornerRadii::all(6.0),
                    });
                }

                // Playing indicator
                if is_playing {
                    cmds.push(RenderCommand::FillRect {
                        x: x + 8.0, y: ry + 8.0, width: 4.0, height: row_h - 20.0,
                        color: GREEN, corner_radii: CornerRadii::all(2.0),
                    });
                }

                // Station name
                cmds.push(RenderCommand::Text {
                    x: x + 18.0, y: ry + 4.0,
                    text: station.name.clone(), font_size: 13.0,
                    color: if is_playing { GREEN } else if is_sel { TEXT_COLOR } else { SUBTEXT1 },
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(w - 100.0),
                    overflow: TextOverflow::Ellipsis,
                });

                // Genre badge
                cmds.push(RenderCommand::FillRect {
                    x: x + w - 80.0, y: ry + 4.0, width: 60.0, height: 16.0,
                    color: station.genre.color(),
                    corner_radii: CornerRadii::all(8.0),
                });
                cmds.push(RenderCommand::Text {
                    x: x + w - 72.0, y: ry + 6.0,
                    text: station.genre.label().to_string(), font_size: 8.0,
                    color: CRUST, font_weight: FontWeightHint::Bold,
                    max_width: Some(52.0),
                    overflow: TextOverflow::Ellipsis,
                });

                // Info line
                let fav_mark = if self.favorites.contains(&station_idx) { " *" } else { "" };
                cmds.push(RenderCommand::Text {
                    x: x + 18.0, y: ry + 20.0,
                    text: format!("{}kbps {} | {}{}", station.bitrate_kbps, station.codec.label(), station.country, fav_mark),
                    font_size: 9.0, color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(w - 40.0),
                    overflow: TextOverflow::Ellipsis,
                });

                // Description
                cmds.push(RenderCommand::Text {
                    x: x + 18.0, y: ry + 32.0,
                    text: station.description.clone(), font_size: 9.0,
                    color: SUBTEXT0, font_weight: FontWeightHint::Regular,
                    max_width: Some(w - 40.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }
        }

        // A list that is hiding rows has to say so, or a station that exists
        // and is simply below the fold is indistinguishable from one that does
        // not exist. The space for this line is subtracted from the row budget
        // unconditionally, so drawing it can never push a row off the bottom.
        let hidden = filtered.len().saturating_sub(shown);
        if hidden > 0 {
            cmds.push(RenderCommand::Text {
                x: x + 18.0,
                y: start_y + (shown as f32) * row_h,
                text: format!("{hidden} more"),
                font_size: 9.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(w - 40.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    fn render_player_bar(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, h: f32) {
        cmds.push(RenderCommand::FillRect {
            x, y, width: w, height: h,
            color: CRUST, corner_radii: CornerRadii::ZERO,
        });

        // Separator
        cmds.push(RenderCommand::FillRect {
            x, y, width: w, height: 1.0,
            color: SURFACE0, corner_radii: CornerRadii::ZERO,
        });

        if let Some(idx) = self.current_station {
            if let Some(station) = self.stations.get(idx) {
                // Station name
                cmds.push(RenderCommand::Text {
                    x: x + 12.0, y: y + 8.0,
                    text: station.name.clone(), font_size: 14.0,
                    color: if self.play_state == PlayState::Playing { GREEN } else { TEXT_COLOR },
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(250.0),
                    overflow: TextOverflow::Ellipsis,
                });

                // Status
                let status = match self.play_state {
                    PlayState::Stopped => "Stopped",
                    PlayState::Buffering => "Buffering...",
                    PlayState::Playing => "Playing",
                    PlayState::Error => "Error",
                };
                cmds.push(RenderCommand::Text {
                    x: x + 12.0, y: y + 26.0,
                    text: format!("{} | {} | {}kbps", status, Self::format_time(self.listen_time_secs), station.bitrate_kbps),
                    font_size: 10.0, color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(250.0),
                    overflow: TextOverflow::Ellipsis,
                });

                // Spectrum visualization
                if self.play_state == PlayState::Playing {
                    let spec_x = x + 280.0;
                    let spec_w: f32 = 200.0;
                    let bar_w = spec_w / 32.0;
                    for (i, &val) in self.spectrum_bars.iter().enumerate() {
                        let bar_h = (val as f32) * 0.4;
                        cmds.push(RenderCommand::FillRect {
                            x: spec_x + (i as f32) * bar_w,
                            y: y + h - 10.0 - bar_h,
                            width: bar_w - 1.0,
                            height: bar_h,
                            color: BLUE,
                            corner_radii: CornerRadii::ZERO,
                        });
                    }
                }
            }
        } else {
            cmds.push(RenderCommand::Text {
                x: x + 12.0, y: y + 20.0,
                text: "No station playing — Select and press Enter".into(),
                font_size: 12.0, color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(400.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Volume
        let vol_x = w - 180.0;
        let vol_label = if self.muted { "Muted".into() } else { format!("Vol: {}%", self.volume) };
        cmds.push(RenderCommand::Text {
            x: vol_x, y: y + 8.0,
            text: vol_label, font_size: 10.0,
            color: if self.muted { RED } else { SUBTEXT1 },
            font_weight: FontWeightHint::Regular,
            max_width: Some(80.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Volume bar
        cmds.push(RenderCommand::FillRect {
            x: vol_x, y: y + 22.0, width: 80.0, height: 4.0,
            color: SURFACE0, corner_radii: CornerRadii::all(2.0),
        });
        let vol_fill = if self.muted { 0.0 } else { (self.volume as f32) * 0.8 };
        cmds.push(RenderCommand::FillRect {
            x: vol_x, y: y + 22.0, width: vol_fill, height: 4.0,
            color: GREEN, corner_radii: CornerRadii::all(2.0),
        });

        // Controls hint
        cmds.push(RenderCommand::Text {
            x: vol_x, y: y + 34.0,
            text: "[Space] Play/Stop [+/-] Vol [M] Mute".into(),
            font_size: 8.0, color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(170.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Sleep timer
        if let Some(remaining) = self.sleep_remaining_secs {
            cmds.push(RenderCommand::Text {
                x: vol_x, y: y + 48.0,
                text: format!("Sleep: {}", Self::format_time(remaining)),
                font_size: 9.0, color: YELLOW,
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Recording indicator
        if self.recording {
            cmds.push(RenderCommand::FillRect {
                x: vol_x + 100.0, y: y + 8.0, width: 8.0, height: 8.0,
                color: RED, corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: vol_x + 112.0, y: y + 8.0,
                text: format!("REC {}", Self::format_time(self.record_duration_secs)),
                font_size: 9.0, color: RED,
                font_weight: FontWeightHint::Bold,
                max_width: Some(80.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Status
        cmds.push(RenderCommand::Text {
            x: x + 12.0, y: y + h - 16.0,
            text: self.status_message.clone(), font_size: 9.0,
            color: SUBTEXT0, font_weight: FontWeightHint::Regular,
            max_width: Some(w - 24.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_search_overlay(&self, cmds: &mut Vec<RenderCommand>) {
        let sw: f32 = 400.0;
        let sh: f32 = 44.0;
        let sx = (self.width - sw) / 2.0;
        let sy: f32 = 40.0;

        cmds.push(RenderCommand::FillRect {
            x: sx, y: sy, width: sw, height: sh,
            color: SURFACE1, corner_radii: CornerRadii::all(8.0),
        });

        let display = if self.search_query.is_empty() {
            "Type to search stations...".to_string()
        } else {
            format!("{}|", self.search_query)
        };
        cmds.push(RenderCommand::Text {
            x: sx + 12.0, y: sy + 8.0,
            text: display, font_size: 14.0,
            color: if self.search_query.is_empty() { OVERLAY0 } else { TEXT_COLOR },
            font_weight: FontWeightHint::Regular,
            max_width: Some(sw - 24.0),
            overflow: TextOverflow::Ellipsis,
        });

        if !self.search_results.is_empty() {
            cmds.push(RenderCommand::Text {
                x: sx + 12.0, y: sy + 28.0,
                text: format!("{} results — Enter to view", self.search_results.len()),
                font_size: 10.0, color: GREEN,
                font_weight: FontWeightHint::Regular,
                max_width: Some(sw - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }
}

fn main() {
    let _app = RadioApp::new();
}

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the line
    // that did it -- that is the diagnosis. The defensive lints exist to keep
    // panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    #[test]
    fn test_preset_stations() {
        let stations = preset_stations();
        assert!(stations.len() >= 15);
    }

    #[test]
    fn test_all_genres_represented() {
        let stations = preset_stations();
        let genres_present: Vec<Genre> = stations.iter().map(|s| s.genre).collect();
        // At least Rock, Pop, Jazz, Classical, Electronic should be present
        assert!(genres_present.contains(&Genre::Rock));
        assert!(genres_present.contains(&Genre::Jazz));
        assert!(genres_present.contains(&Genre::Classical));
    }

    #[test]
    fn test_genre_labels() {
        assert_eq!(Genre::Rock.label(), "Rock");
        assert_eq!(Genre::Electronic.label(), "Electronic");
        assert_eq!(Genre::Lofi.label(), "Lo-Fi");
    }

    #[test]
    fn test_codec_labels() {
        assert_eq!(Codec::Mp3.label(), "MP3");
        assert_eq!(Codec::Flac.label(), "FLAC");
    }

    #[test]
    fn test_app_creation() {
        let app = RadioApp::new();
        assert!(!app.stations.is_empty());
        assert_eq!(app.play_state, PlayState::Stopped);
        assert_eq!(app.volume, 75);
        assert!(!app.muted);
    }

    #[test]
    fn test_play_station() {
        let mut app = RadioApp::new();
        app.play_station(0);
        assert_eq!(app.play_state, PlayState::Playing);
        assert_eq!(app.current_station, Some(0));
        assert!(app.recent.contains(&0));
    }

    #[test]
    fn test_stop() {
        let mut app = RadioApp::new();
        app.play_station(0);
        app.stop();
        assert_eq!(app.play_state, PlayState::Stopped);
    }

    #[test]
    fn test_toggle_play() {
        let mut app = RadioApp::new();
        app.toggle_play(); // should play first station
        assert_eq!(app.play_state, PlayState::Playing);
        app.toggle_play(); // should stop
        assert_eq!(app.play_state, PlayState::Stopped);
    }

    #[test]
    fn test_volume() {
        let mut app = RadioApp::new();
        let before = app.volume;
        app.volume_up();
        assert_eq!(app.volume, before + 5);
        app.volume_down();
        assert_eq!(app.volume, before);
    }

    #[test]
    fn test_volume_bounds() {
        let mut app = RadioApp::new();
        app.volume = 100;
        app.volume_up();
        assert_eq!(app.volume, 100);
        app.volume = 0;
        app.volume_down();
        assert_eq!(app.volume, 0);
    }

    #[test]
    fn test_mute() {
        let mut app = RadioApp::new();
        assert!(!app.muted);
        app.toggle_mute();
        assert!(app.muted);
        app.toggle_mute();
        assert!(!app.muted);
    }

    #[test]
    fn test_favorites() {
        let mut app = RadioApp::new();
        assert!(!app.is_current_favorite());
        app.toggle_favorite();
        assert!(app.is_current_favorite());
        app.toggle_favorite();
        assert!(!app.is_current_favorite());
    }

    #[test]
    fn test_recent() {
        let mut app = RadioApp::new();
        app.add_to_recent(0);
        app.add_to_recent(1);
        assert_eq!(app.recent.len(), 2);
        assert_eq!(app.recent.first(), Some(&1)); // most recent first
    }

    #[test]
    fn test_recent_no_dupes() {
        let mut app = RadioApp::new();
        app.add_to_recent(0);
        app.add_to_recent(1);
        app.add_to_recent(0);
        assert_eq!(app.recent.len(), 2);
        assert_eq!(app.recent.first(), Some(&0));
    }

    #[test]
    fn test_sleep_timer_cycle() {
        let t = SleepTimer::Off;
        assert_eq!(t.next(), SleepTimer::Minutes15);
        assert_eq!(t.next().next(), SleepTimer::Minutes30);
    }

    #[test]
    fn test_sleep_timer_seconds() {
        assert_eq!(SleepTimer::Off.seconds(), None);
        assert_eq!(SleepTimer::Minutes15.seconds(), Some(900));
        assert_eq!(SleepTimer::Minutes60.seconds(), Some(3600));
    }

    #[test]
    fn test_set_sleep_timer() {
        let mut app = RadioApp::new();
        app.set_sleep_timer();
        assert_eq!(app.sleep_timer, SleepTimer::Minutes15);
        assert_eq!(app.sleep_remaining_secs, Some(900));
    }

    #[test]
    fn test_tick_increments_listen_time() {
        let mut app = RadioApp::new();
        app.play_station(0);
        app.tick();
        assert_eq!(app.listen_time_secs, 1);
    }

    #[test]
    fn test_tick_no_increment_when_stopped() {
        let mut app = RadioApp::new();
        app.tick();
        assert_eq!(app.listen_time_secs, 0);
    }

    #[test]
    fn test_sleep_timer_stops_playback() {
        let mut app = RadioApp::new();
        app.play_station(0);
        app.sleep_remaining_secs = Some(1);
        app.tick(); // decrements to 0
        app.tick(); // triggers stop
        assert_eq!(app.play_state, PlayState::Stopped);
    }

    #[test]
    fn test_recording() {
        let mut app = RadioApp::new();
        app.play_station(0);
        app.toggle_recording();
        assert!(app.recording);
        app.tick();
        assert_eq!(app.record_duration_secs, 1);
        app.toggle_recording();
        assert!(!app.recording);
    }

    #[test]
    fn test_recording_requires_playing() {
        let mut app = RadioApp::new();
        app.toggle_recording();
        assert!(!app.recording); // can't record when stopped
    }

    #[test]
    fn test_search() {
        let mut app = RadioApp::new();
        app.search_query = "jazz".into();
        app.perform_search();
        assert!(!app.search_results.is_empty());
        for &idx in &app.search_results {
            let station = app.stations.get(idx).unwrap();
            let matches = station.name.to_lowercase().contains("jazz")
                || station.genre.label().to_lowercase().contains("jazz")
                || station.description.to_lowercase().contains("jazz");
            assert!(matches);
        }
    }

    #[test]
    fn test_search_no_results() {
        let mut app = RadioApp::new();
        app.search_query = "xyzzyplugh".into();
        app.perform_search();
        assert!(app.search_results.is_empty());
    }

    #[test]
    fn test_search_empty() {
        let mut app = RadioApp::new();
        app.search_query = String::new();
        app.perform_search();
        assert!(app.search_results.is_empty());
    }

    #[test]
    fn test_genre_filter() {
        let mut app = RadioApp::new();
        app.genre_filter = Some(Genre::Jazz);
        let filtered = app.filtered_stations();
        for &idx in &filtered {
            assert_eq!(app.stations.get(idx).unwrap().genre, Genre::Jazz);
        }
    }

    #[test]
    fn test_no_genre_filter() {
        let app = RadioApp::new();
        let filtered = app.filtered_stations();
        assert_eq!(filtered.len(), app.stations.len());
    }

    #[test]
    fn test_format_time() {
        assert_eq!(RadioApp::format_time(0), "0:00");
        assert_eq!(RadioApp::format_time(61), "1:01");
        assert_eq!(RadioApp::format_time(3661), "1:01:01");
    }

    #[test]
    fn test_key_space_plays() {
        let mut app = RadioApp::new();
        app.handle_key(" ", false, false);
        assert_eq!(app.play_state, PlayState::Playing);
    }

    #[test]
    fn test_key_volume() {
        let mut app = RadioApp::new();
        let before = app.volume;
        app.handle_key("+", false, false);
        assert_eq!(app.volume, before + 5);
        app.handle_key("-", false, false);
        assert_eq!(app.volume, before);
    }

    #[test]
    fn test_key_mute() {
        let mut app = RadioApp::new();
        app.handle_key("m", false, false);
        assert!(app.muted);
    }

    #[test]
    fn test_key_favorite() {
        let mut app = RadioApp::new();
        app.handle_key("f", false, false);
        assert!(app.is_current_favorite());
    }

    #[test]
    fn test_key_screen_switch() {
        let mut app = RadioApp::new();
        app.handle_key("2", false, false);
        assert_eq!(app.screen, Screen::Favorites);
        app.handle_key("3", false, false);
        assert_eq!(app.screen, Screen::Recent);
        app.handle_key("1", false, false);
        assert_eq!(app.screen, Screen::Browse);
    }

    #[test]
    fn test_key_search() {
        let mut app = RadioApp::new();
        app.handle_key("/", false, false);
        assert!(app.search_active);
    }

    #[test]
    fn test_key_navigation() {
        let mut app = RadioApp::new();
        app.handle_key("Down", false, false);
        assert_eq!(app.selected_station(), 1);
        app.handle_key("Up", false, false);
        assert_eq!(app.selected_station(), 0);
    }

    #[test]
    fn test_render_browse() {
        let app = RadioApp::new();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_playing() {
        let mut app = RadioApp::new();
        app.play_station(0);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_favorites() {
        let mut app = RadioApp::new();
        app.screen = Screen::Favorites;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_search_overlay() {
        let mut app = RadioApp::new();
        app.search_active = true;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_spectrum_updates() {
        let mut app = RadioApp::new();
        app.play_station(0);
        let before = app.spectrum_bars;
        app.tick();
        // Spectrum should change
        assert_ne!(app.spectrum_bars, before);
    }

    /// Every bar's height, for each of `frames` consecutive ticks of one
    /// playing app -- so, one generator's stream, not one frame each off
    /// `frames` fresh apps. The defect this catches is a *within-stream* one:
    /// re-seeding between samples would hide it completely.
    fn spectrum_frames(frames: usize) -> Vec<[u8; 32]> {
        let mut app = RadioApp::new();
        app.play_station(0);
        (0..frames)
            .map(|_| {
                app.tick();
                app.spectrum_bars
            })
            .collect()
    }

    #[test]
    fn every_bar_of_the_spectrum_actually_moves() {
        // The old animation read bar `i` from bits `i..i+6` of a single LCG
        // state, so the bars at the right ran out of state to read: measured
        // over 64 frames, the distinct-height counts across the 32 bars ended
        // ... 16, 8, 4, 2. Bar 31 had two heights and nothing else, for the
        // whole life of the program.
        let frames = spectrum_frames(64);
        for i in 0..32 {
            let mut heights: Vec<u8> = Vec::new();
            for f in &frames {
                if !heights.contains(&f[i]) {
                    heights.push(f[i]);
                }
            }
            assert!(
                heights.len() >= 12,
                "bar {i} took only {} distinct heights over 64 frames: {heights:?}",
                heights.len()
            );
        }
    }

    #[test]
    fn no_bar_is_pinned_against_the_ceiling() {
        // `base + noise` used to be clipped by `min(100)`, and with a base of
        // 60 and noise up to 63 the left-hand bars sat flat against the top
        // for roughly a third of frames. The jitter is now sized to the
        // headroom, so the ceiling is reachable but not a resting place.
        let frames = spectrum_frames(64);
        for i in 0..32 {
            let at_ceiling = frames.iter().filter(|f| f[i] == SPECTRUM_CEILING).count();
            assert!(
                at_ceiling <= 12,
                "bar {i} was flat against the ceiling in {at_ceiling} of 64 frames"
            );
            assert!(
                frames.iter().all(|f| f[i] <= SPECTRUM_CEILING),
                "bar {i} drew above the ceiling"
            );
        }
    }

    #[cfg(not(unix))]
    #[test]
    fn a_fresh_app_is_seeded_by_the_system_and_not_by_a_literal() {
        // A host `cargo test` has no SlateOS kernel to ask, so
        // `seeded_from_system` takes the fallback -- which is what makes this
        // checkable. Asserting *which* seed, not that two apps differ: a
        // variety check would pass on the old hardcoded 42 and fail on the fix.
        let draws = |mut rng: SeededRng| -> Vec<usize> {
            (0..12).map(|_| rng.below(1000)).collect()
        };
        let from_system = draws(RadioApp::new().spectrum_rng);
        assert_eq!(
            from_system,
            draws(SeededRng::new(FALLBACK_SEED)),
            "a fresh app did not ask the system for its seed"
        );
        assert_ne!(
            from_system,
            draws(SeededRng::new(42)),
            "a fresh app still animates from a literal"
        );
    }

    // ── Scrolling lists ────────────────────────────────────────────────────
    //
    // Both lists in this window used to be drawn from state that nothing
    // maintained: `station_scroll` was read by the renderer and written by
    // nobody, so the selection walked off the bottom of the pane and stayed
    // there, and `genre_scroll` was neither read nor written, so the genre
    // list simply ran past its own bottom edge and over the search hint.
    //
    // These tests are phrased as questions about what is *drawn*, because
    // that is the thing that was wrong. A test that only asked what the
    // selection index was would have passed throughout.

    /// Station names as they appear in the list pane, top to bottom.
    ///
    /// Keyed on the pane's own x and the row title's font size rather than a
    /// y range: a filter on position is exactly the thing that silently
    /// returns nothing when the geometry it assumes has moved.
    fn drawn_stations(app: &RadioApp) -> Vec<String> {
        app.render()
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text { x, text, font_size, .. }
                    if (x - (SIDEBAR_WIDTH + 18.0)).abs() < 0.01
                        && (font_size - 13.0).abs() < 0.01 =>
                {
                    Some(text)
                }
                _ => None,
            })
            .collect()
    }

    /// Genre labels drawn in the sidebar, with the y each was drawn at.
    fn drawn_genres(app: &RadioApp) -> Vec<(String, f32)> {
        app.render()
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text { x, y, text, font_size, .. }
                    if (x - 22.0).abs() < 0.01 && (font_size - 10.0).abs() < 0.01 =>
                {
                    Some((text, y))
                }
                _ => None,
            })
            .collect()
    }

    /// The "N more" line a list draws when it is hiding rows, if any.
    fn more_line(app: &RadioApp, x: f32) -> Option<String> {
        app.render().into_iter().find_map(|c| match c {
            RenderCommand::Text { x: cx, text, font_size, .. }
                if (cx - x).abs() < 0.01
                    && (font_size - 9.0).abs() < 0.01
                    && text.ends_with(" more") =>
            {
                Some(text)
            }
            _ => None,
        })
    }

    #[test]
    fn no_station_row_is_drawn_past_the_bottom_of_the_list_pane() {
        for height in [200.0_f32, 300.0, 450.0, 650.0, 900.0] {
            let mut app = RadioApp::new();
            app.set_size(900.0, height);
            let drawn = drawn_stations(&app);
            let pane_h = height - PLAYER_BAR_HEIGHT;
            let bottom = STATION_ROWS_TOP + (drawn.len() as f32) * STATION_ROW_HEIGHT;
            assert!(
                bottom <= pane_h,
                "at {height}px, {} rows reach {bottom} in a {pane_h}px pane",
                drawn.len()
            );
        }
    }

    /// The regression test for the scroll offset that nothing wrote. Before
    /// the fix the list showed rows 0..10 no matter where the selection was.
    #[test]
    fn the_station_list_follows_the_selection_down() {
        let mut app = RadioApp::new();
        let total = app.filtered_stations().len();
        assert!(total > app.station_rows(), "need a list longer than the pane");

        for step in 0..total {
            let selected = app.selected_station();
            assert_eq!(selected, step, "Down should advance one row at a time");
            let name = app
                .filtered_stations()
                .get(selected)
                .and_then(|&i| app.stations.get(i))
                .map(|s| s.name.clone())
                .expect("selected station exists");
            assert!(
                drawn_stations(&app).contains(&name),
                "station {step} ({name}) is selected but not drawn"
            );
            app.handle_key("Down", false, false);
        }
    }

    #[test]
    fn the_station_list_scrolls_back_up_with_the_selection() {
        let mut app = RadioApp::new();
        let total = app.filtered_stations().len();
        for _ in 0..total {
            app.handle_key("Down", false, false);
        }
        for _ in 0..total {
            app.handle_key("Up", false, false);
        }
        assert_eq!(app.selected_station(), 0);
        let first = app
            .stations
            .first()
            .map(|s| s.name.clone())
            .expect("stations exist");
        assert_eq!(
            drawn_stations(&app).first(),
            Some(&first),
            "scrolling back to the first station should put it back on top"
        );
    }

    #[test]
    fn paging_through_the_station_list_never_leaves_the_selection_off_screen() {
        let mut app = RadioApp::new();
        for key in ["PageDown", "PageDown", "PageDown", "PageUp", "PageDown", "PageUp"] {
            app.handle_key(key, false, false);
            let name = app
                .filtered_stations()
                .get(app.selected_station())
                .and_then(|&i| app.stations.get(i))
                .map(|s| s.name.clone())
                .expect("selected station exists");
            assert!(
                drawn_stations(&app).contains(&name),
                "after {key} the selection ({name}) is off screen"
            );
        }
    }

    #[test]
    fn a_station_list_that_is_hiding_stations_says_so() {
        let app = RadioApp::new();
        let total = app.filtered_stations().len();
        let shown = drawn_stations(&app).len();
        assert!(shown < total, "the default window should not fit every station");
        assert_eq!(
            more_line(&app, SIDEBAR_WIDTH + 18.0),
            Some(format!("{} more", total - shown)),
            "a list with rows below the fold must say how many"
        );
    }

    #[test]
    fn a_station_list_that_fits_says_nothing() {
        let mut app = RadioApp::new();
        app.set_size(900.0, 2000.0);
        assert_eq!(drawn_stations(&app).len(), app.filtered_stations().len());
        assert_eq!(more_line(&app, SIDEBAR_WIDTH + 18.0), None);
    }

    #[test]
    fn switching_screens_scrolls_the_station_list_back_to_the_top() {
        let mut app = RadioApp::new();
        for _ in 0..15 {
            app.handle_key("Down", false, false);
        }
        assert!(app.station_view.first_visible() > 0, "should have scrolled");
        app.handle_key("1", false, false);
        assert_eq!(app.selected_station(), 0);
        assert_eq!(
            app.station_view.first_visible(),
            0,
            "a screen switch replaces the list, so the old position names nothing"
        );
    }

    /// The genre list used to stop at the sidebar's bottom edge, which is
    /// *below* the search hint pinned 20px above it.
    #[test]
    fn no_genre_row_is_drawn_over_the_search_hint() {
        for height in [300.0_f32, 400.0, 500.0, 650.0, 900.0] {
            let mut app = RadioApp::new();
            app.set_size(900.0, height);
            let hint_top = (height - PLAYER_BAR_HEIGHT) - SEARCH_HINT_HEIGHT;
            for (label, y) in drawn_genres(&app) {
                assert!(
                    y + GENRE_ROW_HEIGHT <= hint_top,
                    "at {height}px the genre {label} at y={y} reaches into the \
                     search hint at {hint_top}"
                );
            }
        }
    }

    #[test]
    fn the_genre_sidebar_follows_the_genre_filter() {
        let mut app = RadioApp::new();
        // Short enough that the genre list cannot show all of `Genre::ALL`.
        app.set_size(900.0, 400.0);
        assert!(app.genre_rows() < Genre::ALL.len(), "need an overflowing list");

        for expected in Genre::ALL {
            app.handle_key("Right", false, false);
            assert_eq!(app.genre_filter, Some(expected));
            let labels: Vec<String> = drawn_genres(&app).into_iter().map(|(l, _)| l).collect();
            assert!(
                labels.iter().any(|l| l == expected.label()),
                "the filter is {} but the sidebar shows {labels:?}",
                expected.label()
            );
        }
    }

    #[test]
    fn cycling_genres_backwards_also_keeps_the_choice_on_screen() {
        let mut app = RadioApp::new();
        app.set_size(900.0, 400.0);
        for _ in 0..Genre::ALL.len() {
            app.handle_key("Left", false, false);
            if let Some(genre) = app.genre_filter {
                let labels: Vec<String> =
                    drawn_genres(&app).into_iter().map(|(l, _)| l).collect();
                assert!(
                    labels.iter().any(|l| l == genre.label()),
                    "the filter is {} but the sidebar shows {labels:?}",
                    genre.label()
                );
            }
        }
    }

    #[test]
    fn a_genre_list_that_is_hiding_genres_says_so() {
        let mut app = RadioApp::new();
        app.set_size(900.0, 400.0);
        let shown = drawn_genres(&app).len();
        assert!(shown < Genre::ALL.len(), "the short window should hide genres");
        assert_eq!(
            more_line(&app, 22.0),
            Some(format!("{} more", Genre::ALL.len() - shown)),
            "a genre list with rows below the fold must say how many"
        );
    }

    #[test]
    fn a_genre_list_that_fits_shows_every_genre_and_says_nothing() {
        let app = RadioApp::new();
        let labels: Vec<String> = drawn_genres(&app).into_iter().map(|(l, _)| l).collect();
        assert_eq!(
            labels.len(),
            Genre::ALL.len(),
            "the default window is tall enough for all {} genres",
            Genre::ALL.len()
        );
        assert_eq!(more_line(&app, 22.0), None);
    }

    /// The genre list is only drawn on the Browse screen, so its scroll
    /// position must not make the *station* list's "N more" line ambiguous —
    /// the two are told apart by x, and this pins that apart-ness down.
    #[test]
    fn the_two_more_lines_are_distinguishable() {
        let mut app = RadioApp::new();
        app.set_size(900.0, 400.0);
        assert!(more_line(&app, 22.0).is_some(), "genres are hidden at 400px");
        assert!(
            more_line(&app, SIDEBAR_WIDTH + 18.0).is_some(),
            "stations are hidden at 400px too"
        );
        assert_ne!(
            more_line(&app, 22.0),
            more_line(&app, SIDEBAR_WIDTH + 18.0),
            "two lists hiding different numbers of rows must report differently"
        );
    }

}
