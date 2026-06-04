#![allow(dead_code)]
//! Internet Radio — streaming radio player for OurOS.
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
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

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
    genre_scroll: usize,

    // Station selection
    selected_station: usize,
    station_scroll: usize,

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
    spectrum_seed: u32,

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

impl RadioApp {
    fn new() -> Self {
        let stations = preset_stations();
        let mut spectrum_bars = [0u8; 32];
        for (i, bar) in spectrum_bars.iter_mut().enumerate() {
            *bar = ((i as u8).wrapping_mul(7).wrapping_add(30)) % 100;
        }
        Self {
            stations,
            favorites: Vec::new(),
            recent: Vec::new(),
            max_recent: 30,
            genre_filter: None,
            genre_scroll: 0,
            selected_station: 0,
            station_scroll: 0,
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
            spectrum_seed: 42,
            search_query: String::new(),
            search_results: Vec::new(),
            search_selected: 0,
            search_active: false,
            screen: Screen::Browse,
            status_message: "Select a station and press Enter to play".into(),
            width: 900.0,
            height: 650.0,
        }
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
                if let Some(&idx) = filtered.get(self.selected_station) {
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
        if let Some(&idx) = filtered.get(self.selected_station) {
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
        filtered.get(self.selected_station)
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

            // Update spectrum (simulated)
            self.spectrum_seed = self.spectrum_seed.wrapping_mul(1103515245).wrapping_add(12345);
            for (i, bar) in self.spectrum_bars.iter_mut().enumerate() {
                let noise = ((self.spectrum_seed.wrapping_shr(i as u32)) & 0x3F) as u8;
                let base = if i < 8 { 60u8 } else if i < 16 { 45 } else { 30 };
                *bar = base.saturating_add(noise).min(100);
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

            // Navigation
            "Up" => {
                self.selected_station = self.selected_station.saturating_sub(1);
            }
            "Down" => {
                let filtered = self.filtered_stations();
                let max = filtered.len().saturating_sub(1);
                let next = self.selected_station.saturating_add(1);
                if next <= max {
                    self.selected_station = next;
                }
            }
            "PageUp" | "Prior" => {
                self.selected_station = self.selected_station.saturating_sub(5);
            }
            "PageDown" | "Next" => {
                let filtered = self.filtered_stations();
                let max = filtered.len().saturating_sub(1);
                let next = self.selected_station.saturating_add(5).min(max);
                self.selected_station = next;
            }

            // Genre filter
            "Left"
                if self.screen == Screen::Browse => {
                    // Cycle genre filter backward
                    self.genre_filter = match self.genre_filter {
                        None => Some(Genre::World),
                        Some(g) => {
                            let idx = Genre::ALL.iter().position(|&x| x == g).unwrap_or(0);
                            if idx == 0 { None } else { Genre::ALL.get(idx.saturating_sub(1)).copied() }
                        }
                    };
                    self.selected_station = 0;
                    let label = self.genre_filter.map(|g| g.label()).unwrap_or("All");
                    self.status_message = format!("Genre: {label}");
                }
            "Right"
                if self.screen == Screen::Browse => {
                    self.genre_filter = match self.genre_filter {
                        None => Some(Genre::Rock),
                        Some(g) => {
                            let idx = Genre::ALL.iter().position(|&x| x == g).unwrap_or(0);
                            let next = idx.saturating_add(1);
                            if next >= Genre::ALL.len() { None } else { Genre::ALL.get(next).copied() }
                        }
                    };
                    self.selected_station = 0;
                    let label = self.genre_filter.map(|g| g.label()).unwrap_or("All");
                    self.status_message = format!("Genre: {label}");
                }

            // Favorite
            "f" if !ctrl => self.toggle_favorite(),

            // Screen switching
            "1" => { self.screen = Screen::Browse; self.selected_station = 0; }
            "2" => { self.screen = Screen::Favorites; self.selected_station = 0; }
            "3" => { self.screen = Screen::Recent; self.selected_station = 0; }

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
        let sidebar_w: f32 = 160.0;
        let player_h: f32 = 80.0;
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
            });
            ty += 24.0;
        }

        ty += 8.0;

        // Genre filter (only in Browse)
        if self.screen == Screen::Browse {
            cmds.push(RenderCommand::Text {
                x: x + 12.0, y: ty,
                text: "Genres [Left/Right]".into(), font_size: 10.0,
                color: OVERLAY0, font_weight: FontWeightHint::Bold,
                max_width: Some(w - 24.0),
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
            });
            ty += 20.0;

            for genre in &Genre::ALL {
                if ty + 18.0 > y + h {
                    break;
                }
                let active = self.genre_filter == Some(*genre);
                if active {
                    cmds.push(RenderCommand::FillRect {
                        x: x + 6.0, y: ty, width: w - 12.0, height: 18.0,
                        color: SURFACE0, corner_radii: CornerRadii::all(3.0),
                    });
                }
                // Genre color dot
                cmds.push(RenderCommand::FillRect {
                    x: x + 10.0, y: ty + 5.0, width: 8.0, height: 8.0,
                    color: genre.color(), corner_radii: CornerRadii::all(4.0),
                });
                cmds.push(RenderCommand::Text {
                    x: x + 22.0, y: ty + 3.0,
                    text: genre.label().to_string(), font_size: 10.0,
                    color: if active { TEXT_COLOR } else { SUBTEXT0 },
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(w - 36.0),
                });
                ty += 20.0;
            }
        }

        // Search hint
        cmds.push(RenderCommand::Text {
            x: x + 12.0, y: y + h - 20.0,
            text: "[/] Search".into(), font_size: 9.0,
            color: OVERLAY0, font_weight: FontWeightHint::Regular,
            max_width: Some(w - 24.0),
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
        });

        let start_y = y + 30.0;
        let row_h: f32 = 50.0;
        let visible = ((h - 40.0) / row_h) as usize;

        if filtered.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + 20.0, y: start_y + 10.0,
                text: "No stations".into(), font_size: 12.0,
                color: OVERLAY0, font_weight: FontWeightHint::Regular,
                max_width: Some(w - 40.0),
            });
            return;
        }

        for (vi, &station_idx) in filtered.iter().enumerate().skip(self.station_scroll).take(visible) {
            if let Some(station) = self.stations.get(station_idx) {
                let ry = start_y + ((vi - self.station_scroll) as f32) * row_h;
                let is_sel = vi == self.selected_station;
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
                });

                // Info line
                let fav_mark = if self.favorites.contains(&station_idx) { " *" } else { "" };
                cmds.push(RenderCommand::Text {
                    x: x + 18.0, y: ry + 20.0,
                    text: format!("{}kbps {} | {}{}", station.bitrate_kbps, station.codec.label(), station.country, fav_mark),
                    font_size: 9.0, color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(w - 40.0),
                });

                // Description
                cmds.push(RenderCommand::Text {
                    x: x + 18.0, y: ry + 32.0,
                    text: station.description.clone(), font_size: 9.0,
                    color: SUBTEXT0, font_weight: FontWeightHint::Regular,
                    max_width: Some(w - 40.0),
                });
            }
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
        });

        // Sleep timer
        if let Some(remaining) = self.sleep_remaining_secs {
            cmds.push(RenderCommand::Text {
                x: vol_x, y: y + 48.0,
                text: format!("Sleep: {}", Self::format_time(remaining)),
                font_size: 9.0, color: YELLOW,
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
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
            });
        }

        // Status
        cmds.push(RenderCommand::Text {
            x: x + 12.0, y: y + h - 16.0,
            text: self.status_message.clone(), font_size: 9.0,
            color: SUBTEXT0, font_weight: FontWeightHint::Regular,
            max_width: Some(w - 24.0),
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
        });

        if !self.search_results.is_empty() {
            cmds.push(RenderCommand::Text {
                x: sx + 12.0, y: sy + 28.0,
                text: format!("{} results — Enter to view", self.search_results.len()),
                font_size: 10.0, color: GREEN,
                font_weight: FontWeightHint::Regular,
                max_width: Some(sw - 24.0),
            });
        }
    }
}

fn main() {
    let _app = RadioApp::new();
}

#[cfg(test)]
mod tests {
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
        app.search_query = "".into();
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
        assert_eq!(app.selected_station, 1);
        app.handle_key("Up", false, false);
        assert_eq!(app.selected_station, 0);
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
}
