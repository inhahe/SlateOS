//! Slate OS Music Player
//!
//! Desktop music player application with:
//! - Audio format detection (WAV, MP3, FLAC, OGG)
//! - ID3v2 tag parsing for metadata (title, artist, album, year, genre, track)
//! - Playback controls (play/pause, stop, next/prev, seek, volume)
//! - Repeat modes (Off, One, All) and shuffle
//! - Playlist management (add, remove, reorder, save/load M3U)
//! - Library view with search, album/artist grouping
//! - Now Playing view with album art placeholder and waveform visualization
//! - Dark theme (Catppuccin Mocha)
//! - Full keyboard shortcuts
//!
//! Uses the guitk library for UI rendering.

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::rng::{RandomSource, SeededRng, seeded_from_system};
#[allow(unused_imports)]
use guitk::style::CornerRadii;
use guitk::wheel;

use std::path::PathBuf;

/// Seed used when the system has no entropy to offer.
///
/// Shuffle order and the visualiser are novelty randomness, not secrets, so
/// losing entropy must not stop playback. The constant is per-crate
/// ("MUSICPLR") so that two programs falling back on the same boot do not then
/// agree with each other.
const FALLBACK_SEED: u64 = 0x4D55_5349_4350_4C52;

// ============================================================================
// Catppuccin Mocha Theme Colors
// ============================================================================

const BASE: Color = Color::rgb(30, 30, 46);
const MANTLE: Color = Color::rgb(24, 24, 37);
const CRUST: Color = Color::rgb(17, 17, 27);
const SURFACE0: Color = Color::rgb(49, 50, 68);
const SURFACE1: Color = Color::rgb(69, 71, 90);
const SURFACE2: Color = Color::rgb(88, 91, 112);
const TEXT_COLOR: Color = Color::rgb(205, 214, 244);
const SUBTEXT0: Color = Color::rgb(166, 173, 200);
const SUBTEXT1: Color = Color::rgb(186, 194, 222);
const LAVENDER: Color = Color::rgb(180, 190, 254);
const BLUE: Color = Color::rgb(137, 180, 250);
const SAPPHIRE: Color = Color::rgb(116, 199, 236);
const GREEN: Color = Color::rgb(166, 227, 161);
const PEACH: Color = Color::rgb(250, 179, 135);
const RED: Color = Color::rgb(243, 139, 168);
const MAUVE: Color = Color::rgb(203, 166, 247);
const PINK: Color = Color::rgb(245, 194, 231);
const YELLOW: Color = Color::rgb(249, 226, 175);

// ============================================================================
// Layout Constants
// ============================================================================

const WINDOW_WIDTH: f32 = 1000.0;
const WINDOW_HEIGHT: f32 = 700.0;
const TAB_BAR_HEIGHT: f32 = 44.0;
const CONTROLS_HEIGHT: f32 = 80.0;
const TRACK_ROW_HEIGHT: f32 = 36.0;
const VOLUME_BAR_WIDTH: f32 = 100.0;
const PROGRESS_BAR_HEIGHT: f32 = 6.0;
const BUTTON_SIZE: f32 = 32.0;
const SEEK_SECONDS: f32 = 5.0;
const VISUALIZATION_BARS: usize = 32;

// ============================================================================
// Audio Format Detection
// ============================================================================

/// Supported audio formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioFormat {
    Wav,
    Mp3,
    Flac,
    Ogg,
    Unknown,
}

impl AudioFormat {
    /// Detect audio format from the first bytes of a file.
    pub fn detect(data: &[u8]) -> Self {
        if data.len() < 12 {
            return Self::Unknown;
        }

        // WAV: RIFF....WAVE
        if data.get(..4) == Some(b"RIFF") && data.get(8..12) == Some(b"WAVE") {
            return Self::Wav;
        }

        // FLAC: starts with "fLaC"
        if data.get(..4) == Some(b"fLaC") {
            return Self::Flac;
        }

        // OGG: starts with "OggS"
        if data.get(..4) == Some(b"OggS") {
            return Self::Ogg;
        }

        // MP3: ID3 tag or frame sync bytes
        if data.get(..3) == Some(b"ID3") {
            return Self::Mp3;
        }
        // Frame sync: first 11 bits set (0xFF followed by 0xE0+)
        if data.first() == Some(&0xFF)
            && let Some(&b) = data.get(1)
            && b & 0xE0 == 0xE0
        {
            return Self::Mp3;
        }

        Self::Unknown
    }

    /// Human-readable format name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Wav => "WAV",
            Self::Mp3 => "MP3",
            Self::Flac => "FLAC",
            Self::Ogg => "OGG",
            Self::Unknown => "Unknown",
        }
    }
}

// ============================================================================
// WAV Header Parsing
// ============================================================================

/// Parsed WAV file information.
#[derive(Clone, Debug)]
pub struct WavInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub duration_secs: f32,
}

/// Parse WAV RIFF header and fmt chunk.
pub fn parse_wav_header(data: &[u8]) -> Option<WavInfo> {
    if data.len() < 44 {
        return None;
    }
    // Verify RIFF/WAVE
    if data.get(..4) != Some(b"RIFF") || data.get(8..12) != Some(b"WAVE") {
        return None;
    }

    // Find fmt chunk
    let mut offset = 12;
    let mut sample_rate = 0u32;
    let mut channels = 0u16;
    let mut bits_per_sample = 0u16;
    let mut data_size = 0u32;
    let mut found_fmt = false;

    while offset + 8 <= data.len() {
        let chunk_id = data.get(offset..offset + 4)?;
        let chunk_size = u32::from_le_bytes([
            *data.get(offset + 4)?,
            *data.get(offset + 5)?,
            *data.get(offset + 6)?,
            *data.get(offset + 7)?,
        ]);

        if chunk_id == b"fmt " && chunk_size >= 16 {
            let fmt_start = offset + 8;
            channels = u16::from_le_bytes([*data.get(fmt_start + 2)?, *data.get(fmt_start + 3)?]);
            sample_rate = u32::from_le_bytes([
                *data.get(fmt_start + 4)?,
                *data.get(fmt_start + 5)?,
                *data.get(fmt_start + 6)?,
                *data.get(fmt_start + 7)?,
            ]);
            bits_per_sample =
                u16::from_le_bytes([*data.get(fmt_start + 14)?, *data.get(fmt_start + 15)?]);
            found_fmt = true;
        } else if chunk_id == b"data" {
            data_size = chunk_size;
        }

        offset += 8 + chunk_size as usize;
        // Chunks are word-aligned
        if offset % 2 != 0 {
            offset += 1;
        }
    }

    if !found_fmt || sample_rate == 0 || channels == 0 || bits_per_sample == 0 {
        return None;
    }

    let bytes_per_sample = bits_per_sample as u32 / 8;
    let total_samples = if bytes_per_sample > 0 && channels > 0 {
        data_size / (bytes_per_sample * channels as u32)
    } else {
        0
    };
    let duration_secs = if sample_rate > 0 {
        total_samples as f32 / sample_rate as f32
    } else {
        0.0
    };

    Some(WavInfo {
        sample_rate,
        channels,
        bits_per_sample,
        duration_secs,
    })
}

// ============================================================================
// FLAC Header Parsing
// ============================================================================

/// Parsed FLAC STREAMINFO.
#[derive(Clone, Debug)]
pub struct FlacInfo {
    pub sample_rate: u32,
    pub channels: u8,
    pub bits_per_sample: u8,
    pub total_samples: u64,
    pub duration_secs: f32,
}

/// Parse FLAC STREAMINFO metadata block.
pub fn parse_flac_header(data: &[u8]) -> Option<FlacInfo> {
    // "fLaC" + STREAMINFO block (minimum 42 bytes)
    if data.len() < 42 || data.get(..4) != Some(b"fLaC") {
        return None;
    }

    // First metadata block header at offset 4
    // byte 4: last-block flag (1 bit) + block type (7 bits) — STREAMINFO = 0
    let block_type = data.get(4)? & 0x7F;
    if block_type != 0 {
        return None; // First block must be STREAMINFO
    }

    // Block size: 3 bytes at offset 5
    let block_size =
        ((*data.get(5)? as u32) << 16) | ((*data.get(6)? as u32) << 8) | (*data.get(7)? as u32);
    if block_size < 34 || data.len() < 8 + block_size as usize {
        return None;
    }

    // STREAMINFO starts at offset 8
    let si = &data[8..8 + block_size as usize];
    if si.len() < 34 {
        return None;
    }

    // Bytes 10-13 + bits: sample rate (20 bits), channels (3 bits), bps (5 bits), total samples (36 bits)
    // Offset within STREAMINFO: bytes 10..17
    let sr_hi = (*si.get(10)? as u32) << 12;
    let sr_mid = (*si.get(11)? as u32) << 4;
    let sr_lo = (*si.get(12)? as u32) >> 4;
    let sample_rate = sr_hi | sr_mid | sr_lo;

    let channels = ((*si.get(12)? >> 1) & 0x07) + 1;
    let bps_hi = (*si.get(12)? & 0x01) << 4;
    let bps_lo = *si.get(13)? >> 4;
    let bits_per_sample = bps_hi | (bps_lo + 1);

    let total_hi = ((*si.get(13)? & 0x0F) as u64) << 32;
    let total_lo = ((*si.get(14)? as u64) << 24)
        | ((*si.get(15)? as u64) << 16)
        | ((*si.get(16)? as u64) << 8)
        | (*si.get(17)? as u64);
    let total_samples = total_hi | total_lo;

    let duration_secs = if sample_rate > 0 {
        total_samples as f32 / sample_rate as f32
    } else {
        0.0
    };

    Some(FlacInfo {
        sample_rate,
        channels,
        bits_per_sample,
        total_samples,
        duration_secs,
    })
}

// ============================================================================
// ID3v2 Tag Parsing (MP3)
// ============================================================================

/// Parsed ID3v2 metadata.
#[derive(Clone, Debug, Default)]
pub struct Id3Tags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub year: Option<String>,
    pub genre: Option<String>,
    pub track: Option<u32>,
}

/// Parse ID3v2 tags from MP3 file data.
pub fn parse_id3v2(data: &[u8]) -> Option<Id3Tags> {
    if data.len() < 10 || data.get(..3) != Some(b"ID3") {
        return None;
    }

    let version_major = *data.get(3)?;
    // ID3v2 size: 4 bytes synchsafe integer (7 bits per byte)
    let size = synchsafe_u32(&data[6..10])?;
    let header_end = 10 + size as usize;

    if data.len() < header_end {
        return None;
    }

    let mut tags = Id3Tags::default();
    let mut pos = 10;

    // Skip extended header if present (ID3v2.3+)
    let flags = *data.get(5)?;
    if version_major >= 3 && flags & 0x40 != 0 {
        if pos + 4 > header_end {
            return Some(tags);
        }
        let ext_size = u32::from_be_bytes([
            *data.get(pos)?,
            *data.get(pos + 1)?,
            *data.get(pos + 2)?,
            *data.get(pos + 3)?,
        ]) as usize;
        pos += 4 + ext_size;
    }

    while pos + 10 <= header_end {
        let frame_id = data.get(pos..pos + 4)?;
        if frame_id[0] == 0 {
            break; // Padding
        }

        let frame_size = if version_major >= 4 {
            synchsafe_u32(&data[pos + 4..pos + 8])? as usize
        } else {
            u32::from_be_bytes([
                *data.get(pos + 4)?,
                *data.get(pos + 5)?,
                *data.get(pos + 6)?,
                *data.get(pos + 7)?,
            ]) as usize
        };

        let frame_data_start = pos + 10;
        let frame_data_end = frame_data_start + frame_size;

        if frame_data_end > header_end || frame_size == 0 {
            break;
        }

        let frame_content = data.get(frame_data_start..frame_data_end)?;

        match frame_id {
            b"TIT2" => tags.title = decode_id3_text(frame_content),
            b"TPE1" => tags.artist = decode_id3_text(frame_content),
            b"TALB" => tags.album = decode_id3_text(frame_content),
            b"TDRC" | b"TYER" => tags.year = decode_id3_text(frame_content),
            b"TCON" => tags.genre = decode_id3_text(frame_content),
            b"TRCK" => {
                if let Some(s) = decode_id3_text(frame_content) {
                    // Track can be "3" or "3/12"
                    let num_part = s.split('/').next().unwrap_or(&s);
                    tags.track = num_part.parse().ok();
                }
            }
            _ => {}
        }

        pos = frame_data_end;
    }

    Some(tags)
}

/// Decode synchsafe integer (4 bytes, 7 bits each).
fn synchsafe_u32(bytes: &[u8]) -> Option<u32> {
    if bytes.len() < 4 {
        return None;
    }
    Some(
        ((*bytes.first()? as u32) << 21)
            | ((*bytes.get(1)? as u32) << 14)
            | ((*bytes.get(2)? as u32) << 7)
            | (*bytes.get(3)? as u32),
    )
}

/// Decode ID3v2 text frame content.
fn decode_id3_text(data: &[u8]) -> Option<String> {
    if data.is_empty() {
        return None;
    }
    let encoding = *data.first()?;
    let text_bytes = data.get(1..)?;
    match encoding {
        0 | 3 => {
            // ISO-8859-1 or UTF-8
            String::from_utf8(text_bytes.to_vec()).ok()
        }
        1 | 2 => {
            // UTF-16 (LE or BE with possible BOM)
            if text_bytes.len() < 2 {
                return None;
            }
            let (start, le) = if text_bytes.get(..2) == Some(&[0xFF, 0xFE]) {
                (2, true)
            } else if text_bytes.get(..2) == Some(&[0xFE, 0xFF]) {
                (2, false)
            } else {
                (0, encoding == 2) // encoding 2 = UTF-16BE
            };
            let pairs = text_bytes.get(start..)?;
            let u16_vec: Vec<u16> = pairs
                .chunks_exact(2)
                .map(|chunk| {
                    if le {
                        u16::from_le_bytes([chunk[0], chunk[1]])
                    } else {
                        u16::from_be_bytes([chunk[0], chunk[1]])
                    }
                })
                .take_while(|&c| c != 0)
                .collect();
            String::from_utf16(&u16_vec).ok()
        }
        _ => None,
    }
}

// ============================================================================
// Track and Playlist Data Structures
// ============================================================================

/// The result of writing a playlist as M3U.
///
/// Carries the tracks that had to be left out alongside the text, so a caller
/// can tell the user rather than handing them a playlist that is quietly
/// shorter than the one they exported.
#[derive(Clone, Debug, Default)]
pub struct M3uExport {
    /// The playlist file's contents.
    pub text: String,
    /// Tracks omitted because their path contains a line break and therefore
    /// has no M3U representation.
    pub skipped: Vec<PathBuf>,
}

/// Make a string safe to place on an `#EXTINF` line.
///
/// M3U offers no escape syntax, so a line break can only be removed. Doing so
/// is lossy, but `#EXTINF` is advisory display metadata -- losing a newline
/// out of a song title costs nothing, whereas keeping it forges a playlist
/// entry.
fn m3u_field(s: &str) -> String {
    s.replace(['\n', '\r'], " ")
}

/// A single music track.
#[derive(Clone, Debug)]
pub struct Track {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub year: String,
    pub genre: String,
    pub track_number: Option<u32>,
    pub duration_secs: f32,
    pub format: AudioFormat,
}

impl Track {
    /// Create a track from a file path with parsed metadata.
    pub fn from_path(path: PathBuf) -> Self {
        let filename = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown")
            .to_string();

        Self {
            path,
            title: filename,
            artist: String::from("Unknown Artist"),
            album: String::from("Unknown Album"),
            year: String::new(),
            genre: String::new(),
            track_number: None,
            duration_secs: 0.0,
            format: AudioFormat::Unknown,
        }
    }

    /// Update track metadata from file header data.
    pub fn update_from_data(&mut self, data: &[u8]) {
        self.format = AudioFormat::detect(data);

        match self.format {
            AudioFormat::Wav => {
                if let Some(info) = parse_wav_header(data) {
                    self.duration_secs = info.duration_secs;
                }
            }
            AudioFormat::Mp3 => {
                if let Some(tags) = parse_id3v2(data) {
                    if let Some(title) = tags.title {
                        self.title = title;
                    }
                    if let Some(artist) = tags.artist {
                        self.artist = artist;
                    }
                    if let Some(album) = tags.album {
                        self.album = album;
                    }
                    if let Some(year) = tags.year {
                        self.year = year;
                    }
                    if let Some(genre_val) = tags.genre {
                        self.genre = genre_val;
                    }
                    self.track_number = tags.track;
                }
            }
            AudioFormat::Flac => {
                if let Some(info) = parse_flac_header(data) {
                    self.duration_secs = info.duration_secs;
                }
            }
            AudioFormat::Ogg | AudioFormat::Unknown => {}
        }
    }

    /// Format duration as mm:ss.
    pub fn duration_display(&self) -> String {
        format_time(self.duration_secs)
    }
}

/// Repeat mode for playback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepeatMode {
    Off,
    One,
    All,
}

impl RepeatMode {
    /// Cycle to next repeat mode.
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::One,
            Self::One => Self::All,
            Self::All => Self::Off,
        }
    }

    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Repeat: Off",
            Self::One => "Repeat: One",
            Self::All => "Repeat: All",
        }
    }
}

/// Sorting criteria for the library.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortBy {
    Title,
    Artist,
    Album,
    Duration,
}

/// Active UI tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    NowPlaying,
    Library,
    Playlists,
}

// ============================================================================
// Player State
// ============================================================================

/// Complete player state.
pub struct PlayerState {
    // Playback
    pub current_track_index: Option<usize>,
    pub position_secs: f32,
    pub playing: bool,
    pub volume: f32,
    pub muted: bool,
    pub repeat_mode: RepeatMode,
    pub shuffle: bool,

    // Playlist
    pub playlist: Vec<Track>,

    // Library
    pub library: Vec<Track>,
    pub library_sort: SortBy,

    // UI
    pub active_tab: Tab,
    pub search_query: String,
    pub searching: bool,
    pub selected_index: Option<usize>,
    pub scroll_offset: f32,

    // Interaction state
    pub dragging_progress: bool,
    pub dragging_volume: bool,
    pub hover_button: Option<&'static str>,

    // Window size
    pub width: f32,
    pub height: f32,

    /// Playlist positions already played in the current shuffle pass.
    ///
    /// Held as "what has been heard" rather than as a precomputed order,
    /// because `playlist` is pushed to from outside this type (the library and
    /// drag-drop handlers append to `state.playlist` directly). A stored
    /// permutation would go stale the moment that happened -- indices past the
    /// new length, or new tracks the order does not mention. A played-set has
    /// no such invariant to keep: entries beyond the end are simply never
    /// candidates, and tracks added mid-pass join the candidates at once.
    shuffle_played: Vec<usize>,

    /// Current heights of the Now Playing visualiser bars, in `0.0..=1.0`.
    ///
    /// State advanced by `tick`, not a function of `position_secs` evaluated
    /// during render -- see `tick` for what the old arithmetic actually drew.
    visualizer_bars: Vec<f32>,

    /// The stream shuffle and the visualiser draw from.
    rng: SeededRng,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self::new()
    }
}

impl PlayerState {
    /// Create a new player with default state.
    pub fn new() -> Self {
        Self::with_rng(seeded_from_system(FALLBACK_SEED))
    }

    /// A player whose shuffle and visualiser come from a known seed.
    #[cfg(test)]
    fn with_seed(seed: u64) -> Self {
        Self::with_rng(SeededRng::new(seed))
    }

    fn with_rng(rng: SeededRng) -> Self {
        Self {
            shuffle_played: Vec::new(),
            visualizer_bars: vec![0.0; VISUALIZATION_BARS],
            rng,
            current_track_index: None,
            position_secs: 0.0,
            playing: false,
            volume: 0.75,
            muted: false,
            repeat_mode: RepeatMode::Off,
            shuffle: false,
            playlist: Vec::new(),
            library: Vec::new(),
            library_sort: SortBy::Title,
            active_tab: Tab::NowPlaying,
            search_query: String::new(),
            searching: false,
            selected_index: None,
            scroll_offset: 0.0,
            dragging_progress: false,
            dragging_volume: false,
            hover_button: None,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        }
    }

    /// Get the currently playing track (if any).
    pub fn current_track(&self) -> Option<&Track> {
        self.current_track_index
            .and_then(|idx| self.playlist.get(idx))
    }

    /// Total duration of current track.
    pub fn current_duration(&self) -> f32 {
        self.current_track().map(|t| t.duration_secs).unwrap_or(0.0)
    }

    /// Toggle play/pause.
    pub fn toggle_play(&mut self) {
        if self.playlist.is_empty() {
            return;
        }
        if self.current_track_index.is_none() {
            self.current_track_index = Some(0);
        }
        self.playing = !self.playing;
    }

    /// Stop playback and reset position.
    pub fn stop(&mut self) {
        self.playing = false;
        self.position_secs = 0.0;
    }

    /// Move to next track.
    pub fn next_track(&mut self) {
        if self.playlist.is_empty() {
            return;
        }
        let len = self.playlist.len();
        match self.current_track_index {
            Some(idx) => {
                if self.shuffle {
                    self.shuffle_played.push(idx);
                    match self.draw_unplayed_track() {
                        Some(next) => self.current_track_index = Some(next),
                        None => {
                            // The pass is over. Repeat-all starts a new one;
                            // anything else stops, which the old code never
                            // did -- its shuffle branch ignored `repeat_mode`
                            // entirely and so could not reach the end of a
                            // playlist.
                            if self.repeat_mode == RepeatMode::All {
                                self.shuffle_played.clear();
                                match self.draw_unplayed_track() {
                                    Some(next) => self.current_track_index = Some(next),
                                    None => return,
                                }
                            } else {
                                self.playing = false;
                                return;
                            }
                        }
                    }
                } else {
                    let next = idx + 1;
                    if next >= len {
                        match self.repeat_mode {
                            RepeatMode::All => self.current_track_index = Some(0),
                            _ => {
                                self.playing = false;
                                return;
                            }
                        }
                    } else {
                        self.current_track_index = Some(next);
                    }
                }
            }
            None => self.current_track_index = Some(0),
        }
        self.position_secs = 0.0;
    }

    /// Redraw the Now Playing visualiser bars.
    ///
    /// The heights used to be computed during render as
    /// `((position_secs * 10) as u32 * 31 + i * 7) % 100`. That is an exact
    /// arithmetic progression in the bar index with a step of 7, so it never
    /// drew a waveform at all -- it drew a straight diagonal ramp that slid
    /// sideways by 31 hundredths per tenth-second and wrapped at 100. Measured
    /// at t = 0 the first sixteen bars were
    /// `0, 7, 14, 21, 28, 35, 42, 49, 56, 63, 70, 77, 84, 91, 98, 5`;
    /// at t = 1, the same ramp shifted: `31, 38, 45, ...`. On screen it was a
    /// moving staircase, identical on every machine and for every track.
    ///
    /// The bars are now state advanced here rather than a function of
    /// `position_secs` evaluated in the renderer, both so that they can be
    /// random at all -- rendering takes `&PlayerState` -- and so that the
    /// animation does not stall whenever the position happens not to change.
    fn advance_visualizer(&mut self) {
        for bar in &mut self.visualizer_bars {
            // Eased towards a new target rather than snapped to it: a bar that
            // jumps to an independent height every frame flickers, which reads
            // as noise rather than as an audio level.
            let target = self.rng.unit_f32();
            *bar = bar.mul_add(0.6, target * 0.4);
        }
    }

    /// Current visualiser bar heights, each in `0.0..=1.0`.
    pub fn visualizer_bars(&self) -> &[f32] {
        &self.visualizer_bars
    }

    /// Pick a playlist position not yet heard in this shuffle pass.
    ///
    /// Returns `None` when every track has been played, which is what tells
    /// `next_track` the pass is over.
    ///
    /// This replaces `next = (idx * 7 + 3) % len`, which was not random and
    /// frequently not even a permutation. It is an affine map, so its orbit is
    /// a fixed cycle whose length depends on `gcd(6, len)` and `gcd(7, len)`;
    /// measured over every playlist length from 2 to 16, the tracks it could
    /// ever reach were:
    ///
    /// ```text
    ///   len:  2  3  4  5  6  7  8  9 10 11 12 13 14 15 16
    ///  seen:  2  1  2  4  2  1  2  3  4 10  2 12  2  4  4
    /// ```
    ///
    /// A twelve-track album on shuffle alternated between exactly two tracks
    /// -- 3, 0, 3, 0 -- forever. A seven-track one played track 3 and nothing
    /// else. A three-track one replayed the track already playing, because for
    /// `len = 3` the map is the identity. Only lengths coprime to both 6 and 7
    /// (11 and 13 in that range) behaved even approximately like a shuffle.
    fn draw_unplayed_track(&mut self) -> Option<usize> {
        let candidates: Vec<usize> = (0..self.playlist.len())
            .filter(|i| !self.shuffle_played.contains(i))
            .collect();
        candidates.get(self.rng.below(candidates.len())).copied()
    }

    /// Turn shuffle on or off, starting a fresh pass when it goes on.
    ///
    /// A method rather than a bare `state.shuffle = !state.shuffle` at each of
    /// the keyboard and mouse handlers, so that the played-set is cleared in
    /// both places instead of in whichever one was edited most recently.
    pub fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        self.shuffle_played.clear();
    }

    /// Move to previous track.
    pub fn prev_track(&mut self) {
        if self.playlist.is_empty() {
            return;
        }
        let len = self.playlist.len();
        // If more than 3 seconds in, restart current track
        if self.position_secs > 3.0 {
            self.position_secs = 0.0;
            return;
        }
        match self.current_track_index {
            Some(idx) => {
                if idx == 0 {
                    self.current_track_index = Some(len - 1);
                } else {
                    self.current_track_index = Some(idx - 1);
                }
            }
            None => self.current_track_index = Some(0),
        }
        self.position_secs = 0.0;
    }

    /// Seek forward/backward by seconds.
    pub fn seek_relative(&mut self, delta: f32) {
        let dur = self.current_duration();
        if dur <= 0.0 {
            return;
        }
        self.position_secs = (self.position_secs + delta).clamp(0.0, dur);
    }

    /// Set absolute seek position (0.0 to 1.0 fraction).
    pub fn seek_fraction(&mut self, fraction: f32) {
        let dur = self.current_duration();
        self.position_secs = (fraction.clamp(0.0, 1.0)) * dur;
    }

    /// Adjust volume by delta.
    pub fn adjust_volume(&mut self, delta: f32) {
        self.volume = (self.volume + delta).clamp(0.0, 1.0);
    }

    /// Toggle mute.
    pub fn toggle_mute(&mut self) {
        self.muted = !self.muted;
    }

    /// Advance playback by elapsed seconds (simulated tick).
    pub fn tick(&mut self, elapsed_secs: f32) {
        if !self.playing {
            return;
        }
        self.advance_visualizer();
        let dur = self.current_duration();
        if dur <= 0.0 {
            return;
        }

        self.position_secs += elapsed_secs;
        if self.position_secs >= dur {
            match self.repeat_mode {
                RepeatMode::One => self.position_secs = 0.0,
                _ => self.next_track(),
            }
        }
    }

    /// Add a track to the playlist.
    pub fn add_track(&mut self, track: Track) {
        self.library.push(track.clone());
        self.playlist.push(track);
    }

    /// Remove track at index from playlist.
    pub fn remove_track(&mut self, index: usize) {
        if index >= self.playlist.len() {
            return;
        }
        self.playlist.remove(index);
        // Adjust current index
        if let Some(cur) = self.current_track_index {
            if index == cur {
                self.playing = false;
                self.position_secs = 0.0;
                if self.playlist.is_empty() {
                    self.current_track_index = None;
                } else if cur >= self.playlist.len() {
                    self.current_track_index = Some(self.playlist.len() - 1);
                }
            } else if index < cur {
                self.current_track_index = Some(cur - 1);
            }
        }
    }

    /// Move track up in playlist.
    pub fn move_track_up(&mut self, index: usize) {
        if index == 0 || index >= self.playlist.len() {
            return;
        }
        self.playlist.swap(index, index - 1);
        if let Some(cur) = self.current_track_index {
            if cur == index {
                self.current_track_index = Some(index - 1);
            } else if cur == index - 1 {
                self.current_track_index = Some(index);
            }
        }
    }

    /// Move track down in playlist.
    pub fn move_track_down(&mut self, index: usize) {
        if index + 1 >= self.playlist.len() {
            return;
        }
        self.playlist.swap(index, index + 1);
        if let Some(cur) = self.current_track_index {
            if cur == index {
                self.current_track_index = Some(index + 1);
            } else if cur == index + 1 {
                self.current_track_index = Some(index);
            }
        }
    }

    /// Clear the playlist.
    pub fn clear_playlist(&mut self) {
        self.playlist.clear();
        self.current_track_index = None;
        self.playing = false;
        self.position_secs = 0.0;
    }

    /// Sort library by given criteria.
    pub fn sort_library(&mut self, sort_by: SortBy) {
        self.library_sort = sort_by;
        match sort_by {
            SortBy::Title => self.library.sort_by(|a, b| a.title.cmp(&b.title)),
            SortBy::Artist => self.library.sort_by(|a, b| a.artist.cmp(&b.artist)),
            SortBy::Album => self.library.sort_by(|a, b| a.album.cmp(&b.album)),
            SortBy::Duration => self.library.sort_by(|a, b| {
                a.duration_secs
                    .partial_cmp(&b.duration_secs)
                    .unwrap_or(core::cmp::Ordering::Equal)
            }),
        }
    }

    /// Get filtered library tracks based on search query.
    pub fn filtered_library(&self) -> Vec<&Track> {
        if self.search_query.is_empty() {
            return self.library.iter().collect();
        }
        let query = self.search_query.to_lowercase();
        self.library
            .iter()
            .filter(|t| {
                t.title.to_lowercase().contains(&query)
                    || t.artist.to_lowercase().contains(&query)
                    || t.album.to_lowercase().contains(&query)
                    || t.genre.to_lowercase().contains(&query)
            })
            .collect()
    }

    // ------------------------------------------------------------------
    // Track-list geometry
    //
    // One definition of where the rows are, used by the renderer and by
    // every pointer path. They used to have separate ones that disagreed
    // about two different things at once -- see `row_at`.
    // ------------------------------------------------------------------

    /// The top of the track rows, measured from the top of the content area
    /// -- i.e. the bottom edge of the non-scrolling column header.
    ///
    /// Content-relative, because that is the space the list is drawn in:
    /// `render` pushes a `translate(0, TAB_BAR_HEIGHT)` before calling the
    /// per-tab renderer. `row_at` converts a window coordinate into this
    /// space once, rather than each pointer path doing its own arithmetic.
    const fn rows_top() -> f32 {
        TRACK_ROW_HEIGHT
    }

    /// The height of the content area, between the tab bar and the controls.
    fn content_height(&self) -> f32 {
        (self.height - TAB_BAR_HEIGHT - CONTROLS_HEIGHT).max(0.0)
    }

    /// The height of the scrolling track-row area.
    ///
    /// The column header does not scroll, so it is not part of this.
    fn rows_height(&self) -> f32 {
        (self.content_height() - Self::rows_top()).max(0.0)
    }

    /// How many rows the active tab's list holds.
    fn row_count(&self) -> usize {
        match self.active_tab {
            Tab::Library => self.filtered_library().len(),
            Tab::Playlists => self.playlist.len(),
            // Now Playing is not a list. Clicking it used to set
            // `selected_index` anyway, to a track the tab never showed.
            Tab::NowPlaying => 0,
        }
    }

    /// The track under the *window* y coordinate `y`, or `None` if the
    /// pointer is not over a row.
    ///
    /// The single bound for the click path, the double-click path and the
    /// renderer. Two separate faults met here:
    ///
    /// * The renderer snapped: it drew row `(scroll_offset /
    ///   TRACK_ROW_HEIGHT) as usize` at the top of the pane, discarding the
    ///   fractional part, while this arithmetic kept it. A trackpad scrolls
    ///   by fractions of a row, so after any such scroll the row under the
    ///   pointer was not the row that had been painted there -- by up to a
    ///   whole row. The renderer now draws at the continuous offset and
    ///   clips, which is what the hit test always assumed.
    /// * `selected_index` was assigned unconditionally, so a click below the
    ///   last track selected a track index past the end of the list, and a
    ///   click in the header strip -- or on the Now Playing tab -- selected
    ///   one too.
    fn row_at(&self, y: f32) -> Option<usize> {
        let rel = y - TAB_BAR_HEIGHT - Self::rows_top();
        if !rel.is_finite() || rel < 0.0 || rel >= self.rows_height() {
            return None;
        }
        let from_top = rel + self.scroll_offset;
        if !from_top.is_finite() || from_top < 0.0 {
            return None;
        }
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let idx = (from_top / TRACK_ROW_HEIGHT) as usize;
        if idx < self.row_count() {
            Some(idx)
        } else {
            None
        }
    }

    /// How far the active tab's list may be scrolled before its last row
    /// sits on the bottom edge of the pane.
    fn max_scroll(&self) -> f32 {
        let viewport = self.rows_height();
        if viewport <= 0.0 {
            // No viewport, so "how far can this scroll" has no answer.
            // Reporting the current offset makes the clamp a no-op rather
            // than a decision: a window dragged down to nothing and back
            // does not lose the reader's place, and does not jump to the
            // end of the list either.
            return self.scroll_offset.max(0.0);
        }
        #[allow(clippy::cast_precision_loss)]
        let total = self.row_count() as f32 * TRACK_ROW_HEIGHT;
        (total - viewport).max(0.0)
    }

    /// Generate M3U playlist content.
    ///
    /// M3U is a bare line-oriented format with no quoting or escape mechanism
    /// whatsoever, so a line break in a field cannot be *escaped* -- it can
    /// only be removed or refused. That matters here because neither field is
    /// ours: `artist`/`title` come verbatim from the file's ID3 tags (see
    /// [`Track::update_from_data`]), and this OS permits every byte except
    /// `/` and NUL in a path. Written naively, a track whose ID3 title
    /// contained a newline emitted an extra line that [`Self::load_m3u`] then
    /// read back as a *file path*, so a downloaded file could inject
    /// arbitrary entries into the user's playlist.
    pub fn export_m3u(&self) -> M3uExport {
        let mut text = String::from("#EXTM3U\n");
        let mut skipped = Vec::new();
        for track in &self.playlist {
            let path = track.path.display().to_string();
            // A path containing a line break has no M3U representation at
            // all. Emitting it anyway would silently point the entry at a
            // different file, so the track is omitted and reported instead.
            if path.contains(['\n', '\r']) {
                skipped.push(track.path.clone());
                continue;
            }
            text.push_str(&format!(
                "#EXTINF:{},{} - {}\n",
                track.duration_secs as i32,
                m3u_field(&track.artist),
                m3u_field(&track.title)
            ));
            text.push_str(&path);
            text.push('\n');
        }
        M3uExport { text, skipped }
    }

    /// Load tracks from M3U content.
    ///
    /// Note that this reads every non-`#` line as a file path, which is what
    /// makes an unsanitised [`Self::export_m3u`] a real injection vector
    /// rather than a cosmetic problem.
    pub fn load_m3u(&mut self, content: &str) {
        self.clear_playlist();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let track = Track::from_path(PathBuf::from(trimmed));
            self.playlist.push(track);
        }
    }
}

// ============================================================================
// Rendering
// ============================================================================

/// Render the full player UI.
pub fn render(state: &PlayerState) -> RenderTree {
    let mut tree = RenderTree::new();

    // Background
    tree.fill_rect(0.0, 0.0, state.width, state.height, BASE);

    // Tab bar at top
    render_tab_bar(state, &mut tree);

    // Main content area
    let content_y = TAB_BAR_HEIGHT;
    let content_height = state.height - TAB_BAR_HEIGHT - CONTROLS_HEIGHT;
    tree.clip(0.0, content_y, state.width, content_height);
    tree.translate(0.0, content_y);

    match state.active_tab {
        Tab::NowPlaying => render_now_playing(state, &mut tree, content_height),
        Tab::Library => render_library(state, &mut tree),
        Tab::Playlists => render_playlist_view(state, &mut tree, content_height),
    }

    tree.untranslate();
    tree.unclip();

    // Playback controls at bottom
    render_controls(state, &mut tree);

    tree
}

/// Render tab bar.
fn render_tab_bar(state: &PlayerState, tree: &mut RenderTree) {
    tree.fill_rect(0.0, 0.0, state.width, TAB_BAR_HEIGHT, MANTLE);

    let tabs = [
        (Tab::NowPlaying, "Now Playing"),
        (Tab::Library, "Library"),
        (Tab::Playlists, "Playlists"),
    ];

    let tab_width = 120.0;
    let mut x = 16.0;

    for (tab, label) in &tabs {
        let active = state.active_tab == *tab;
        let bg = if active { SURFACE0 } else { MANTLE };
        let fg = if active { TEXT_COLOR } else { SUBTEXT0 };

        tree.push(RenderCommand::FillRect {
            x,
            y: 6.0,
            width: tab_width,
            height: TAB_BAR_HEIGHT - 8.0,
            color: bg,
            corner_radii: CornerRadii::all(6.0),
        });

        tree.push(RenderCommand::Text {
            x: x + 12.0,
            y: 18.0,
            text: label.to_string(),
            color: fg,
            font_size: 13.0,
            font_weight: if active {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            },
            max_width: Some(tab_width - 24.0),
            overflow: TextOverflow::Ellipsis,
        });

        if active {
            // Active indicator line
            tree.fill_rect(
                x + 10.0,
                TAB_BAR_HEIGHT - 3.0,
                tab_width - 20.0,
                3.0,
                LAVENDER,
            );
        }

        x += tab_width + 8.0;
    }

    // Search indicator (if searching)
    if state.searching {
        let search_text = format!("Search: {}_", state.search_query);
        tree.push(RenderCommand::Text {
            x: state.width - 250.0,
            y: 16.0,
            text: search_text,
            color: YELLOW,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(240.0),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

/// Render Now Playing view.
fn render_now_playing(state: &PlayerState, tree: &mut RenderTree, content_height: f32) {
    let center_x = state.width / 2.0;

    if let Some(track) = state.current_track() {
        // Album art placeholder (colored rectangle from album hash)
        let art_size = content_height.min(300.0);
        let art_x = center_x - art_size / 2.0;
        let art_y = 30.0;
        let art_color = album_color(&track.album);

        tree.push(RenderCommand::FillRect {
            x: art_x,
            y: art_y,
            width: art_size,
            height: art_size,
            color: art_color,
            corner_radii: CornerRadii::all(12.0),
        });

        // Music note icon in center of art
        tree.push(RenderCommand::Text {
            x: center_x - 20.0,
            y: art_y + art_size / 2.0 - 10.0,
            text: String::from("♪"),
            color: Color::rgba(255, 255, 255, 180),
            font_size: 48.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Track title
        let text_y = art_y + art_size + 24.0;
        tree.push(RenderCommand::Text {
            x: center_x - 200.0,
            y: text_y,
            text: track.title.clone(),
            color: TEXT_COLOR,
            font_size: 22.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(400.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Artist
        tree.push(RenderCommand::Text {
            x: center_x - 200.0,
            y: text_y + 32.0,
            text: track.artist.clone(),
            color: SUBTEXT1,
            font_size: 16.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(400.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Album
        tree.push(RenderCommand::Text {
            x: center_x - 200.0,
            y: text_y + 56.0,
            text: track.album.clone(),
            color: SUBTEXT0,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(400.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Waveform visualization
        let viz_y = text_y + 90.0;
        let viz_width = 400.0;
        let viz_x = center_x - viz_width / 2.0;
        let bar_width = viz_width / VISUALIZATION_BARS as f32;
        let max_bar_height = 40.0;

        for i in 0..VISUALIZATION_BARS {
            let amplitude = if state.playing {
                // Read, not computed: the heights are advanced by `tick`. See
                // `PlayerState::advance_visualizer` for what the arithmetic
                // that used to live here actually drew.
                state.visualizer_bars().get(i).copied().unwrap_or(0.0) * max_bar_height
            } else {
                2.0 // Flat line when paused
            };

            let bar_x = viz_x + i as f32 * bar_width + 1.0;
            let bar_h = amplitude.max(2.0);
            let bar_y = viz_y + max_bar_height - bar_h;

            tree.push(RenderCommand::FillRect {
                x: bar_x,
                y: bar_y,
                width: bar_width - 2.0,
                height: bar_h,
                color: LAVENDER,
                corner_radii: CornerRadii::all(1.0),
            });
        }
    } else {
        // No track loaded
        tree.push(RenderCommand::Text {
            x: center_x - 100.0,
            y: content_height / 2.0 - 20.0,
            text: String::from("No Track Playing"),
            color: SUBTEXT0,
            font_size: 20.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        tree.push(RenderCommand::Text {
            x: center_x - 140.0,
            y: content_height / 2.0 + 10.0,
            text: String::from("Add music to your library to get started"),
            color: SURFACE2,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }
}

/// The first track index that is at all visible at `scroll_offset`.
///
/// Rounds *down*, so a list scrolled by half a row starts with that row's
/// bottom half rather than skipping it -- which is what makes the drawn rows
/// line up with the continuous hit test.
fn first_visible_row(scroll_offset: f32) -> usize {
    if !scroll_offset.is_finite() || scroll_offset <= 0.0 {
        return 0;
    }
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let first = (scroll_offset / TRACK_ROW_HEIGHT) as usize;
    first
}

/// Render Library view with track list.
fn render_library(state: &PlayerState, tree: &mut RenderTree) {
    // Column headers
    let header_y = 0.0;
    tree.fill_rect(0.0, header_y, state.width, TRACK_ROW_HEIGHT, MANTLE);

    let col_title_x = 16.0;
    let col_artist_x = state.width * 0.35;
    let col_album_x = state.width * 0.58;
    let col_duration_x = state.width * 0.82;

    let header_text_y = header_y + 11.0;
    tree.push(RenderCommand::Text {
        x: col_title_x,
        y: header_text_y,
        text: String::from("Title"),
        color: SUBTEXT0,
        font_size: 12.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    tree.push(RenderCommand::Text {
        x: col_artist_x,
        y: header_text_y,
        text: String::from("Artist"),
        color: SUBTEXT0,
        font_size: 12.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    tree.push(RenderCommand::Text {
        x: col_album_x,
        y: header_text_y,
        text: String::from("Album"),
        color: SUBTEXT0,
        font_size: 12.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    tree.push(RenderCommand::Text {
        x: col_duration_x,
        y: header_text_y,
        text: String::from("Duration"),
        color: SUBTEXT0,
        font_size: 12.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // Track list. Drawn at the *continuous* scroll offset, under a clip, so
    // a partial row is shown as a partial row. It used to be snapped to a
    // whole row (`(scroll_offset / TRACK_ROW_HEIGHT) as usize`), which threw
    // away the fraction the hit test kept -- so after a trackpad scroll the
    // row under the pointer was not the row painted there.
    let filtered = state.filtered_library();
    let rows_y = PlayerState::rows_top();
    let rows_h = state.rows_height();
    let first = first_visible_row(state.scroll_offset);
    let origin_y = rows_y - state.scroll_offset;

    tree.clip(0.0, rows_y, state.width, rows_h);

    for track_idx in first..filtered.len() {
        #[allow(clippy::cast_precision_loss)]
        let row_y = origin_y + track_idx as f32 * TRACK_ROW_HEIGHT;
        if row_y >= rows_y + rows_h {
            break;
        }
        let track = match filtered.get(track_idx) {
            Some(t) => *t,
            None => break,
        };

        let is_playing = state.current_track().map(|ct| ct.path == track.path) == Some(true);
        let is_selected = state.selected_index == Some(track_idx);

        // Striped by *track* index, not by visible slot: the stripes belong
        // to the rows, so they no longer invert every time the list scrolls
        // past a row boundary.
        let row_bg = if is_playing {
            SURFACE0
        } else if is_selected {
            SURFACE1
        } else if track_idx % 2 == 0 {
            BASE
        } else {
            Color::rgba(49, 50, 68, 80)
        };

        tree.fill_rect(0.0, row_y, state.width, TRACK_ROW_HEIGHT, row_bg);

        let text_color = if is_playing { LAVENDER } else { TEXT_COLOR };
        let text_y = row_y + 10.0;

        // Playing indicator
        if is_playing {
            tree.push(RenderCommand::Text {
                x: 4.0,
                y: text_y,
                text: String::from("▶"),
                color: GREEN,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        tree.push(RenderCommand::Text {
            x: col_title_x,
            y: text_y,
            text: track.title.clone(),
            color: text_color,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(col_artist_x - col_title_x - 16.0),
            overflow: TextOverflow::Ellipsis,
        });

        tree.push(RenderCommand::Text {
            x: col_artist_x,
            y: text_y,
            text: track.artist.clone(),
            color: SUBTEXT1,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(col_album_x - col_artist_x - 16.0),
            overflow: TextOverflow::Ellipsis,
        });

        tree.push(RenderCommand::Text {
            x: col_album_x,
            y: text_y,
            text: track.album.clone(),
            color: SUBTEXT0,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(col_duration_x - col_album_x - 16.0),
            overflow: TextOverflow::Ellipsis,
        });

        tree.push(RenderCommand::Text {
            x: col_duration_x,
            y: text_y,
            text: track.duration_display(),
            color: SUBTEXT0,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    tree.unclip();

    // Sort indicator
    let sort_label = match state.library_sort {
        SortBy::Title => "Sorted: Title",
        SortBy::Artist => "Sorted: Artist",
        SortBy::Album => "Sorted: Album",
        SortBy::Duration => "Sorted: Duration",
    };
    tree.push(RenderCommand::Text {
        x: state.width - 120.0,
        y: header_text_y,
        text: sort_label.to_string(),
        color: SURFACE2,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
}

/// Render Playlists view.
fn render_playlist_view(state: &PlayerState, tree: &mut RenderTree, content_height: f32) {
    // Header
    tree.fill_rect(0.0, 0.0, state.width, TRACK_ROW_HEIGHT, MANTLE);
    tree.push(RenderCommand::Text {
        x: 16.0,
        y: 11.0,
        text: format!("Playlist — {} tracks", state.playlist.len()),
        color: TEXT_COLOR,
        font_size: 14.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // Total duration
    let total_secs: f32 = state.playlist.iter().map(|t| t.duration_secs).sum();
    tree.push(RenderCommand::Text {
        x: state.width - 150.0,
        y: 11.0,
        text: format!("Total: {}", format_time(total_secs)),
        color: SUBTEXT0,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // Playlist tracks. Continuous, clipped -- see `render_library`.
    let rows_y = PlayerState::rows_top();
    let rows_h = state.rows_height();
    let first = first_visible_row(state.scroll_offset);
    let origin_y = rows_y - state.scroll_offset;

    tree.clip(0.0, rows_y, state.width, rows_h);

    for track_idx in first..state.playlist.len() {
        #[allow(clippy::cast_precision_loss)]
        let row_y = origin_y + track_idx as f32 * TRACK_ROW_HEIGHT;
        if row_y >= rows_y + rows_h {
            break;
        }
        let track = match state.playlist.get(track_idx) {
            Some(t) => t,
            None => break,
        };

        let is_current = state.current_track_index == Some(track_idx);
        let is_selected = state.selected_index == Some(track_idx);

        let row_bg = if is_current {
            SURFACE0
        } else if is_selected {
            SURFACE1
        } else if track_idx % 2 == 0 {
            BASE
        } else {
            Color::rgba(49, 50, 68, 80)
        };

        tree.fill_rect(0.0, row_y, state.width, TRACK_ROW_HEIGHT, row_bg);

        // Track number
        let text_y = row_y + 10.0;
        tree.push(RenderCommand::Text {
            x: 16.0,
            y: text_y,
            text: format!("{}.", track_idx + 1),
            color: SURFACE2,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Playing indicator
        let title_color = if is_current { LAVENDER } else { TEXT_COLOR };
        if is_current && state.playing {
            tree.push(RenderCommand::Text {
                x: 40.0,
                y: text_y,
                text: String::from("▶"),
                color: GREEN,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        // Title + Artist
        tree.push(RenderCommand::Text {
            x: 56.0,
            y: text_y,
            text: track.title.clone(),
            color: title_color,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(state.width * 0.4),
            overflow: TextOverflow::Ellipsis,
        });

        tree.push(RenderCommand::Text {
            x: state.width * 0.5,
            y: text_y,
            text: track.artist.clone(),
            color: SUBTEXT0,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(state.width * 0.25),
            overflow: TextOverflow::Ellipsis,
        });

        // Duration
        tree.push(RenderCommand::Text {
            x: state.width - 70.0,
            y: text_y,
            text: track.duration_display(),
            color: SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Format badge
        tree.push(RenderCommand::FillRect {
            x: state.width - 120.0,
            y: row_y + 8.0,
            width: 36.0,
            height: 18.0,
            color: SURFACE1,
            corner_radii: CornerRadii::all(3.0),
        });
        tree.push(RenderCommand::Text {
            x: state.width - 116.0,
            y: text_y,
            text: track.format.name().to_string(),
            color: SUBTEXT0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    tree.unclip();

    // Empty state
    if state.playlist.is_empty() {
        tree.push(RenderCommand::Text {
            x: state.width / 2.0 - 80.0,
            y: content_height / 2.0,
            text: String::from("Playlist is empty"),
            color: SUBTEXT0,
            font_size: 16.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }
}

/// Render playback controls at the bottom.
fn render_controls(state: &PlayerState, tree: &mut RenderTree) {
    let controls_y = state.height - CONTROLS_HEIGHT;

    // Background
    tree.fill_rect(0.0, controls_y, state.width, CONTROLS_HEIGHT, CRUST);

    // Separator line
    tree.fill_rect(0.0, controls_y, state.width, 1.0, SURFACE0);

    // Progress bar
    let progress_y = controls_y + 8.0;
    let progress_x = 16.0;
    let progress_width = state.width - 32.0;
    let duration = state.current_duration();
    let progress_fraction = if duration > 0.0 {
        state.position_secs / duration
    } else {
        0.0
    };

    // Progress track (background)
    tree.push(RenderCommand::FillRect {
        x: progress_x,
        y: progress_y,
        width: progress_width,
        height: PROGRESS_BAR_HEIGHT,
        color: SURFACE1,
        corner_radii: CornerRadii::all(3.0),
    });

    // Progress fill
    let fill_width = progress_width * progress_fraction;
    if fill_width > 0.0 {
        tree.push(RenderCommand::FillRect {
            x: progress_x,
            y: progress_y,
            width: fill_width,
            height: PROGRESS_BAR_HEIGHT,
            color: LAVENDER,
            corner_radii: CornerRadii::all(3.0),
        });
    }

    // Progress handle
    let handle_x = progress_x + fill_width - 4.0;
    tree.push(RenderCommand::FillRect {
        x: handle_x,
        y: progress_y - 2.0,
        width: 8.0,
        height: PROGRESS_BAR_HEIGHT + 4.0,
        color: TEXT_COLOR,
        corner_radii: CornerRadii::all(4.0),
    });

    // Time labels
    let time_y = progress_y + PROGRESS_BAR_HEIGHT + 4.0;
    tree.push(RenderCommand::Text {
        x: progress_x,
        y: time_y,
        text: format_time(state.position_secs),
        color: SUBTEXT0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    tree.push(RenderCommand::Text {
        x: progress_x + progress_width - 40.0,
        y: time_y,
        text: format_time(duration),
        color: SUBTEXT0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // Playback buttons row
    let btn_y = controls_y + 36.0;
    let btn_center_x = state.width / 2.0;

    // Previous button
    render_button(
        tree,
        btn_center_x - 80.0,
        btn_y,
        BUTTON_SIZE,
        "⏮",
        SURFACE0,
        TEXT_COLOR,
    );

    // Play/Pause button (larger)
    let play_icon = if state.playing { "⏸" } else { "▶" };
    let play_size = BUTTON_SIZE + 8.0;
    tree.push(RenderCommand::FillRect {
        x: btn_center_x - play_size / 2.0,
        y: btn_y - 4.0,
        width: play_size,
        height: play_size,
        color: LAVENDER,
        corner_radii: CornerRadii::all(play_size / 2.0),
    });
    tree.push(RenderCommand::Text {
        x: btn_center_x - 8.0,
        y: btn_y + 6.0,
        text: play_icon.to_string(),
        color: CRUST,
        font_size: 16.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // Next button
    render_button(
        tree,
        btn_center_x + 48.0,
        btn_y,
        BUTTON_SIZE,
        "⏭",
        SURFACE0,
        TEXT_COLOR,
    );

    // Stop button
    render_button(
        tree,
        btn_center_x - 130.0,
        btn_y,
        BUTTON_SIZE,
        "⏹",
        SURFACE0,
        TEXT_COLOR,
    );

    // Repeat mode button
    let repeat_color = match state.repeat_mode {
        RepeatMode::Off => SURFACE2,
        RepeatMode::One => PEACH,
        RepeatMode::All => GREEN,
    };
    let repeat_label = match state.repeat_mode {
        RepeatMode::Off => "R",
        RepeatMode::One => "R1",
        RepeatMode::All => "R∞",
    };
    render_button(
        tree,
        btn_center_x + 100.0,
        btn_y,
        BUTTON_SIZE,
        repeat_label,
        SURFACE0,
        repeat_color,
    );

    // Shuffle button
    let shuffle_color = if state.shuffle { GREEN } else { SURFACE2 };
    render_button(
        tree,
        btn_center_x + 140.0,
        btn_y,
        BUTTON_SIZE,
        "⇄",
        SURFACE0,
        shuffle_color,
    );

    // Volume section (right side)
    let vol_x = state.width - VOLUME_BAR_WIDTH - 60.0;
    let vol_y = btn_y + 4.0;

    // Mute icon
    let vol_icon = if state.muted || state.volume == 0.0 {
        "🔇"
    } else if state.volume < 0.5 {
        "🔉"
    } else {
        "🔊"
    };
    tree.push(RenderCommand::Text {
        x: vol_x - 24.0,
        y: vol_y,
        text: vol_icon.to_string(),
        color: if state.muted { RED } else { TEXT_COLOR },
        font_size: 14.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // Volume bar background
    tree.push(RenderCommand::FillRect {
        x: vol_x,
        y: vol_y + 4.0,
        width: VOLUME_BAR_WIDTH,
        height: 4.0,
        color: SURFACE1,
        corner_radii: CornerRadii::all(2.0),
    });

    // Volume fill
    let effective_volume = if state.muted { 0.0 } else { state.volume };
    let vol_fill_width = VOLUME_BAR_WIDTH * effective_volume;
    if vol_fill_width > 0.0 {
        tree.push(RenderCommand::FillRect {
            x: vol_x,
            y: vol_y + 4.0,
            width: vol_fill_width,
            height: 4.0,
            color: SAPPHIRE,
            corner_radii: CornerRadii::all(2.0),
        });
    }

    // Volume percentage
    tree.push(RenderCommand::Text {
        x: vol_x + VOLUME_BAR_WIDTH + 8.0,
        y: vol_y,
        text: format!("{}%", (effective_volume * 100.0) as u32),
        color: SUBTEXT0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // Current track info (left side of controls)
    if let Some(track) = state.current_track() {
        let info_x = 16.0;
        let info_y = btn_y;

        // Mini album art
        let mini_art_color = album_color(&track.album);
        tree.push(RenderCommand::FillRect {
            x: info_x,
            y: info_y - 4.0,
            width: 36.0,
            height: 36.0,
            color: mini_art_color,
            corner_radii: CornerRadii::all(4.0),
        });

        tree.push(RenderCommand::Text {
            x: info_x + 44.0,
            y: info_y,
            text: track.title.clone(),
            color: TEXT_COLOR,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });

        tree.push(RenderCommand::Text {
            x: info_x + 44.0,
            y: info_y + 16.0,
            text: track.artist.clone(),
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

/// Render a small circular button.
fn render_button(
    tree: &mut RenderTree,
    x: f32,
    y: f32,
    size: f32,
    label: &str,
    bg: Color,
    fg: Color,
) {
    tree.push(RenderCommand::FillRect {
        x,
        y,
        width: size,
        height: size,
        color: bg,
        corner_radii: CornerRadii::all(size / 2.0),
    });
    tree.push(RenderCommand::Text {
        x: x + 6.0,
        y: y + 8.0,
        text: label.to_string(),
        color: fg,
        font_size: 12.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(size - 4.0),
        overflow: TextOverflow::Ellipsis,
    });
}

// ============================================================================
// Event Handling
// ============================================================================

/// Handle an input event, returning true if the event was consumed.
pub fn handle_event(state: &mut PlayerState, event: &Event) -> bool {
    let consumed = match event {
        Event::Key(key_event) => handle_key(state, key_event),
        Event::Mouse(mouse_event) => handle_mouse(state, mouse_event),
        Event::Resize { width, height } => {
            #[allow(clippy::cast_precision_loss)]
            {
                state.width = *width as f32;
                state.height = *height as f32;
            }
            true
        }
        Event::Tick { elapsed_ms } => {
            #[allow(clippy::cast_precision_loss)]
            let secs = *elapsed_ms as f32 / 1000.0;
            state.tick(secs);
            true
        }
        _ => false,
    };

    // Pull the offset back inside its bounds *here*, once, rather than at
    // each of the dozen places that can shorten the content under a
    // scrolled pane: a resize, a search keystroke, a tab switch, removing a
    // track. Only the wheel handler clamped, so any of the others could
    // leave the list wound off the end of itself and showing nothing.
    //
    // Guarded on a non-zero offset because `max_scroll` walks the library to
    // count the filtered rows, and an unscrolled list -- which is the usual
    // case, and the case every `Tick` arrives in -- has nothing to clamp.
    if state.scroll_offset > 0.0 || !state.scroll_offset.is_finite() {
        state.scroll_offset = state.scroll_offset.clamp(0.0, state.max_scroll());
    }

    consumed
}

/// Handle keyboard input.
fn handle_key(state: &mut PlayerState, key_event: &KeyEvent) -> bool {
    if !key_event.pressed {
        return false;
    }

    // Handle search input mode
    if state.searching {
        match key_event.key {
            Key::Escape => {
                state.searching = false;
                state.search_query.clear();
                return true;
            }
            Key::Enter => {
                state.searching = false;
                return true;
            }
            Key::Backspace => {
                state.search_query.pop();
                return true;
            }
            _ => {
                if let Some(ch) = key_event.text {
                    state.search_query.push(ch);
                    return true;
                }
            }
        }
        return false;
    }

    // Global keyboard shortcuts
    match key_event.key {
        Key::Space => {
            state.toggle_play();
            true
        }
        Key::N if !key_event.modifiers.ctrl => {
            state.next_track();
            true
        }
        Key::P if !key_event.modifiers.ctrl => {
            state.prev_track();
            true
        }
        Key::M if !key_event.modifiers.ctrl => {
            state.toggle_mute();
            true
        }
        Key::Equals | Key::Num0 => {
            // + key (volume up)
            state.adjust_volume(0.05);
            true
        }
        Key::Minus => {
            state.adjust_volume(-0.05);
            true
        }
        Key::Left => {
            state.seek_relative(-SEEK_SECONDS);
            true
        }
        Key::Right => {
            state.seek_relative(SEEK_SECONDS);
            true
        }
        Key::F if key_event.modifiers.ctrl => {
            state.searching = true;
            state.search_query.clear();
            true
        }
        Key::S if !key_event.modifiers.ctrl => {
            state.toggle_shuffle();
            true
        }
        Key::R if !key_event.modifiers.ctrl => {
            state.repeat_mode = state.repeat_mode.next();
            true
        }
        Key::Num1 => {
            state.active_tab = Tab::NowPlaying;
            true
        }
        Key::Num2 => {
            state.active_tab = Tab::Library;
            true
        }
        Key::Num3 => {
            state.active_tab = Tab::Playlists;
            true
        }
        Key::Up => {
            if let Some(idx) = state.selected_index {
                if idx > 0 {
                    state.selected_index = Some(idx - 1);
                }
            } else if !state.playlist.is_empty() {
                state.selected_index = Some(0);
            }
            true
        }
        Key::Down => {
            let max_idx = match state.active_tab {
                Tab::Library => state.filtered_library().len(),
                Tab::Playlists => state.playlist.len(),
                _ => 0,
            };
            if let Some(idx) = state.selected_index {
                if idx + 1 < max_idx {
                    state.selected_index = Some(idx + 1);
                }
            } else if max_idx > 0 {
                state.selected_index = Some(0);
            }
            true
        }
        Key::Enter => {
            // Play selected track
            if let Some(idx) = state.selected_index {
                match state.active_tab {
                    Tab::Library => {
                        let filtered = state.filtered_library();
                        if let Some(track) = filtered.get(idx) {
                            let path = track.path.clone();
                            // Find matching track in playlist or add it
                            if let Some(pos) = state.playlist.iter().position(|t| t.path == path) {
                                state.current_track_index = Some(pos);
                            } else {
                                let track_clone = (*track).clone();
                                state.playlist.push(track_clone);
                                state.current_track_index = Some(state.playlist.len() - 1);
                            }
                            state.position_secs = 0.0;
                            state.playing = true;
                        }
                    }
                    Tab::Playlists if idx < state.playlist.len() => {
                        state.current_track_index = Some(idx);
                        state.position_secs = 0.0;
                        state.playing = true;
                    }
                    _ => {}
                }
            }
            true
        }
        Key::Delete => {
            // Remove selected track from playlist
            if state.active_tab == Tab::Playlists
                && let Some(idx) = state.selected_index
            {
                state.remove_track(idx);
                if state.playlist.is_empty() {
                    state.selected_index = None;
                } else if idx >= state.playlist.len() {
                    state.selected_index = Some(state.playlist.len() - 1);
                }
            }
            true
        }
        Key::Escape => {
            state.selected_index = None;
            true
        }
        _ => false,
    }
}

/// Handle mouse input.
fn handle_mouse(state: &mut PlayerState, mouse_event: &MouseEvent) -> bool {
    let x = mouse_event.x;
    let y = mouse_event.y;

    match &mouse_event.kind {
        MouseEventKind::Press(MouseButton::Left) => {
            // Tab bar clicks
            if y < TAB_BAR_HEIGHT {
                let tab_x = 16.0;
                let tab_width = 120.0;
                let gap = 8.0;
                if x >= tab_x && x < tab_x + tab_width {
                    state.active_tab = Tab::NowPlaying;
                    return true;
                }
                let x2 = tab_x + tab_width + gap;
                if x >= x2 && x < x2 + tab_width {
                    state.active_tab = Tab::Library;
                    return true;
                }
                let x3 = x2 + tab_width + gap;
                if x >= x3 && x < x3 + tab_width {
                    state.active_tab = Tab::Playlists;
                    return true;
                }
            }

            // Controls area
            let controls_y = state.height - CONTROLS_HEIGHT;
            if y >= controls_y {
                let rel_y = y - controls_y;

                // Progress bar click
                let progress_y_local = 8.0;
                if rel_y >= progress_y_local
                    && rel_y <= progress_y_local + PROGRESS_BAR_HEIGHT + 8.0
                {
                    let progress_x = 16.0;
                    let progress_width = state.width - 32.0;
                    if x >= progress_x && x <= progress_x + progress_width {
                        let fraction = (x - progress_x) / progress_width;
                        state.seek_fraction(fraction);
                        state.dragging_progress = true;
                        return true;
                    }
                }

                // Playback button clicks
                let btn_y = 36.0;
                let btn_center_x = state.width / 2.0;

                // Play/Pause (center)
                if (x - btn_center_x).abs() < 24.0 && (rel_y - btn_y - 16.0).abs() < 24.0 {
                    state.toggle_play();
                    return true;
                }
                // Previous
                if (x - (btn_center_x - 80.0 + 16.0)).abs() < 20.0
                    && (rel_y - btn_y - 16.0).abs() < 20.0
                {
                    state.prev_track();
                    return true;
                }
                // Next
                if (x - (btn_center_x + 48.0 + 16.0)).abs() < 20.0
                    && (rel_y - btn_y - 16.0).abs() < 20.0
                {
                    state.next_track();
                    return true;
                }
                // Stop
                if (x - (btn_center_x - 130.0 + 16.0)).abs() < 20.0
                    && (rel_y - btn_y - 16.0).abs() < 20.0
                {
                    state.stop();
                    return true;
                }
                // Repeat
                if (x - (btn_center_x + 100.0 + 16.0)).abs() < 20.0
                    && (rel_y - btn_y - 16.0).abs() < 20.0
                {
                    state.repeat_mode = state.repeat_mode.next();
                    return true;
                }
                // Shuffle
                if (x - (btn_center_x + 140.0 + 16.0)).abs() < 20.0
                    && (rel_y - btn_y - 16.0).abs() < 20.0
                {
                    state.toggle_shuffle();
                    return true;
                }

                // Volume bar
                let vol_x = state.width - VOLUME_BAR_WIDTH - 60.0;
                let vol_bar_y = controls_y + btn_y + 4.0;
                if x >= vol_x
                    && x <= vol_x + VOLUME_BAR_WIDTH
                    && y >= vol_bar_y
                    && y <= vol_bar_y + 12.0
                {
                    let fraction = (x - vol_x) / VOLUME_BAR_WIDTH;
                    state.volume = fraction.clamp(0.0, 1.0);
                    state.muted = false;
                    state.dragging_volume = true;
                    return true;
                }

                // Mute toggle (volume icon)
                if x >= vol_x - 28.0 && x <= vol_x - 4.0 && y >= vol_bar_y && y <= vol_bar_y + 16.0
                {
                    state.toggle_mute();
                    return true;
                }
            }

            // Content area clicks (track selection). `row_at` is the bound
            // this never had: it used to assign `selected_index`
            // unconditionally, so a click below the last track selected an
            // index past the end of the list.
            if let Some(row_idx) = state.row_at(y) {
                state.selected_index = Some(row_idx);
                return true;
            }

            false
        }

        MouseEventKind::Release(MouseButton::Left) => {
            state.dragging_progress = false;
            state.dragging_volume = false;
            true
        }

        MouseEventKind::Move => {
            // Drag progress bar
            if state.dragging_progress {
                let progress_x = 16.0;
                let progress_width = state.width - 32.0;
                let fraction = (x - progress_x) / progress_width;
                state.seek_fraction(fraction);
                return true;
            }
            // Drag volume
            if state.dragging_volume {
                let vol_x = state.width - VOLUME_BAR_WIDTH - 60.0;
                let fraction = (x - vol_x) / VOLUME_BAR_WIDTH;
                state.volume = fraction.clamp(0.0, 1.0);
                return true;
            }
            false
        }

        MouseEventKind::Scroll { dy, .. } => {
            // Scroll track list
            let content_y = TAB_BAR_HEIGHT;
            let content_height = state.content_height();
            if y >= content_y && y < content_y + content_height {
                let max_scroll = state.max_scroll();
                // A pixel offset, so `wheel::pixels` rather than an
                // accumulator: a trackpad's fifth of a notch can be shown as a
                // fifth of a notch here. The `30.0` it replaces was one of a
                // dozen private guesses at what a notch is worth -- the tree
                // held 1, 20, 30 and 40 px per notch at once, and this list's
                // rows are 36 px tall, so 30 was not even a row.
                state.scroll_offset = (state.scroll_offset + wheel::pixels(*dy, TRACK_ROW_HEIGHT))
                    .clamp(0.0, max_scroll);
                return true;
            }
            false
        }

        MouseEventKind::DoubleClick(MouseButton::Left) => {
            // Double-click to play track
            if let Some(row_idx) = state.row_at(y) {
                match state.active_tab {
                    Tab::Library => {
                        let filtered = state.filtered_library();
                        if let Some(track) = filtered.get(row_idx) {
                            let path = track.path.clone();
                            if let Some(pos) = state.playlist.iter().position(|t| t.path == path) {
                                state.current_track_index = Some(pos);
                            } else {
                                let track_clone = (*track).clone();
                                state.playlist.push(track_clone);
                                state.current_track_index =
                                    Some(state.playlist.len().saturating_sub(1));
                            }
                            state.position_secs = 0.0;
                            state.playing = true;
                        }
                    }
                    Tab::Playlists if row_idx < state.playlist.len() => {
                        state.current_track_index = Some(row_idx);
                        state.position_secs = 0.0;
                        state.playing = true;
                    }
                    _ => {}
                }
                return true;
            }
            false
        }

        _ => false,
    }
}

// ============================================================================
// Utility Functions
// ============================================================================

/// Format seconds as mm:ss.
fn format_time(secs: f32) -> String {
    let total = secs.max(0.0) as u32;
    let minutes = total / 60;
    let seconds = total % 60;
    format!("{:02}:{:02}", minutes, seconds)
}

/// Generate a deterministic color from an album name (for album art placeholder).
fn album_color(album: &str) -> Color {
    let mut hash: u32 = 5381;
    for byte in album.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u32);
    }

    // Use hash to pick from a set of pleasing colors
    let palette = [
        MAUVE, BLUE, SAPPHIRE, GREEN, PEACH, PINK, LAVENDER, YELLOW, RED,
    ];
    let idx = (hash as usize) % palette.len();
    let base_color = palette[idx];

    // Darken slightly for album art background
    Color::rgba(
        (base_color.r as u16 * 180 / 255) as u8,
        (base_color.g as u16 * 180 / 255) as u8,
        (base_color.b as u16 * 180 / 255) as u8,
        255,
    )
}

// ============================================================================
// Application Entry Point
// ============================================================================

fn main() {
    let mut state = PlayerState::new();

    // Add some demo tracks to show the UI populated
    let demo_tracks = [
        (
            "Midnight Drive",
            "Neon Waves",
            "Synthwave Dreams",
            234.0,
            AudioFormat::Mp3,
        ),
        (
            "Crystal Caves",
            "Aurora Borealis",
            "Northern Lights",
            187.0,
            AudioFormat::Flac,
        ),
        (
            "Summer Rain",
            "The Drifters",
            "Coastal Vibes",
            312.0,
            AudioFormat::Wav,
        ),
        (
            "Binary Stars",
            "Quantum Loop",
            "Digital Horizons",
            256.0,
            AudioFormat::Ogg,
        ),
        (
            "Velvet Thunder",
            "Storm Chasers",
            "Electric Sky",
            198.0,
            AudioFormat::Mp3,
        ),
        (
            "Paper Moon",
            "Origami Hearts",
            "Folded Memories",
            275.0,
            AudioFormat::Flac,
        ),
        (
            "Desert Wind",
            "Sandstone",
            "Arid Dreams",
            341.0,
            AudioFormat::Wav,
        ),
        (
            "Neon Pulse",
            "Circuit Breaker",
            "Digital Horizons",
            223.0,
            AudioFormat::Mp3,
        ),
        (
            "Ocean Floor",
            "Deep Blue",
            "Abyssal",
            289.0,
            AudioFormat::Flac,
        ),
        (
            "Starlight",
            "Cosmos",
            "Infinite Void",
            167.0,
            AudioFormat::Ogg,
        ),
    ];

    for (title, artist, album, duration, format) in &demo_tracks {
        let track = Track {
            path: PathBuf::from(format!(
                "/music/{}/{}.{}",
                artist,
                title,
                format.name().to_lowercase()
            )),
            title: title.to_string(),
            artist: artist.to_string(),
            album: album.to_string(),
            year: String::from("2025"),
            genre: String::from("Electronic"),
            track_number: None,
            duration_secs: *duration,
            format: *format,
        };
        state.add_track(track);
    }

    // Set first track as current
    state.current_track_index = Some(0);

    // Main event loop (placeholder — in real Slate OS, this would receive events from compositor)
    let _tree = render(&state);

    // Simulate a tick
    state.tick(1.0);
    let _tree = render(&state);
}

// ============================================================================
// Tests
// ============================================================================

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

    // -- Wheel scrolling --

    /// A player whose library is far longer than the track pane.
    fn player_with_library(len: usize) -> PlayerState {
        let mut state = PlayerState::new();
        for i in 0..len {
            let mut t = Track::from_path(PathBuf::from(format!("{i}.wav")));
            t.title = format!("track {i}");
            state.add_track(t);
        }
        state.active_tab = Tab::Library;
        state
    }

    fn wheel(state: &mut PlayerState, dy: f32) -> bool {
        handle_mouse(
            state,
            &MouseEvent {
                x: 100.0,
                // Inside the track pane: below the tab bar, above the controls.
                y: TAB_BAR_HEIGHT + 10.0,
                kind: MouseEventKind::Scroll { dx: 0.0, dy },
            },
        )
    }

    #[test]
    fn one_wheel_notch_crosses_three_rows() {
        // Not three pixels, and not the flat 30.0 this handler used to add --
        // which was less than one of its own 36 px rows.
        let mut state = player_with_library(400);
        assert!(wheel(&mut state, -1.0));
        assert_eq!(state.scroll_offset, 3.0 * TRACK_ROW_HEIGHT);
        assert!(wheel(&mut state, 1.0));
        assert_eq!(state.scroll_offset, 0.0);
    }

    #[test]
    fn a_fraction_of_a_notch_moves_now_rather_than_being_banked() {
        // The offset is in pixels, so there is nothing an accumulator could
        // buy: it would only sit on movement this list can already show.
        let mut state = player_with_library(400);
        wheel(&mut state, -0.1);
        assert_eq!(state.scroll_offset, 0.3 * TRACK_ROW_HEIGHT);
    }

    #[test]
    fn the_wheel_stops_at_both_ends_of_the_library() {
        let mut state = player_with_library(400);
        for _ in 0..500 {
            wheel(&mut state, -1.0);
        }
        let total = 400.0 * TRACK_ROW_HEIGHT;
        assert_eq!(state.scroll_offset, total - state.rows_height());
        // The last row's bottom edge lands on the pane's bottom edge, which
        // is what "scrolled to the end" is supposed to mean.
        let last_row_bottom = PlayerState::rows_top() + total - state.scroll_offset;
        assert_eq!(
            last_row_bottom,
            PlayerState::rows_top() + state.rows_height()
        );
        for _ in 0..500 {
            wheel(&mut state, 1.0);
        }
        assert_eq!(state.scroll_offset, 0.0);
    }

    #[test]
    fn a_nonfinite_delta_does_not_freeze_the_list() {
        // An infinity added to the offset clamps to the far end and never
        // comes back; `wheel::pixels` turns one into no movement at all.
        let mut state = player_with_library(400);
        wheel(&mut state, f32::NAN);
        wheel(&mut state, f32::NEG_INFINITY);
        assert_eq!(state.scroll_offset, 0.0);
        wheel(&mut state, -1.0);
        assert_eq!(state.scroll_offset, 3.0 * TRACK_ROW_HEIGHT);
    }

    #[test]
    fn the_wheel_is_ignored_outside_the_track_pane() {
        let mut state = player_with_library(400);
        let handled = handle_mouse(
            &mut state,
            &MouseEvent {
                x: 100.0,
                y: 4.0,
                kind: MouseEventKind::Scroll { dx: 0.0, dy: -1.0 },
            },
        );
        assert!(!handled, "the tab bar does not scroll");
        assert_eq!(state.scroll_offset, 0.0);
    }

    // -- The track list's edges --

    fn click(state: &mut PlayerState, y: f32) {
        state.selected_index = None;
        handle_mouse(
            state,
            &MouseEvent {
                x: 100.0,
                y,
                kind: MouseEventKind::Press(MouseButton::Left),
            },
        );
    }

    /// The rectangle the renderer actually clipped the track rows to, in
    /// *window* coordinates.
    ///
    /// Read out of the emitted commands rather than recomputed from the
    /// constants. Recomputing is what makes a layout test worthless: it
    /// re-derives the renderer's arithmetic and then checks the hit test
    /// against *that*, so the two can drift together and the test still
    /// passes. This asks the renderer what it drew, and adds back the
    /// `translate(0, TAB_BAR_HEIGHT)` the list is drawn under.
    fn rows_clip(state: &PlayerState) -> (f32, f32) {
        let tree = render(state);
        let (y, h) = tree
            .commands
            .iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::PushClip { y, height, .. } => Some((*y, *height)),
                _ => None,
            })
            // The first clip is the whole content area, pushed by `render`
            // before the translate. The row clip is the one inside it.
            .nth(1)
            .expect("the track rows are drawn under a clip of their own");
        (y + TAB_BAR_HEIGHT, h)
    }

    /// The y of every row rectangle the renderer painted, in window
    /// coordinates, paired with the track index it painted there.
    ///
    /// The row backgrounds are the only full-width `FillRect`s inside the
    /// row clip, and they are emitted in track order, so walking them in
    /// order recovers the mapping the renderer used.
    fn painted_rows(state: &PlayerState) -> Vec<f32> {
        let tree = render(state);
        let mut seen_clips = 0usize;
        let mut out = Vec::new();
        for cmd in &tree.commands {
            match cmd {
                RenderCommand::PushClip { .. } => seen_clips += 1,
                RenderCommand::PopClip => seen_clips = seen_clips.saturating_sub(1),
                RenderCommand::FillRect { y, width, .. }
                    if seen_clips == 2 && *width == state.width =>
                {
                    out.push(*y + TAB_BAR_HEIGHT);
                }
                _ => {}
            }
        }
        out
    }

    #[test]
    fn the_lists_clip_is_the_region_the_hit_test_accepts() {
        let mut state = player_with_library(400);
        let (clip_y, clip_h) = rows_clip(&state);
        assert_eq!(clip_y, TAB_BAR_HEIGHT + PlayerState::rows_top());
        assert_eq!(clip_h, state.rows_height());

        click(&mut state, clip_y);
        assert_eq!(state.selected_index, Some(0), "the clip's top edge is dead");
        click(&mut state, clip_y + clip_h - 0.5);
        assert!(
            state.selected_index.is_some(),
            "the clip's bottom edge is dead"
        );
        click(&mut state, clip_y + clip_h);
        assert_eq!(
            state.selected_index, None,
            "the hit test runs past the clip the rows are painted in"
        );
    }

    #[test]
    fn a_half_row_scroll_selects_the_row_that_was_drawn_there() {
        // The bug: the renderer snapped to whole rows and the hit test did
        // not, so after any fractional scroll -- which is every trackpad
        // scroll -- the row under the pointer was not the row painted there.
        // Probing row *tops* would not catch it: the snapped renderer and
        // the continuous hit test happened to agree there. The divergence is
        // in the lower part of each painted row, which is why every probe
        // below sweeps the row rather than sampling one edge of it.
        let mut state = player_with_library(400);
        state.scroll_offset = TRACK_ROW_HEIGHT / 2.0;
        let painted = painted_rows(&state);
        let (clip_y, clip_h) = rows_clip(&state);
        assert!(painted.len() > 2, "the pane must fit several rows");
        for (track_idx, row_y) in painted.iter().enumerate() {
            for step in 0..8 {
                #[allow(clippy::cast_precision_loss)]
                let probe = row_y + (step as f32 + 0.5) * TRACK_ROW_HEIGHT / 8.0;
                if probe < clip_y || probe >= clip_y + clip_h {
                    continue;
                }
                click(&mut state, probe);
                assert_eq!(
                    state.selected_index,
                    Some(track_idx),
                    "row painted at {row_y} hit-tests as {:?} at y={probe}",
                    state.selected_index
                );
            }
        }
    }

    #[test]
    fn every_row_edge_selects_the_row_the_renderer_drew_there() {
        let mut state = player_with_library(400);
        let painted = painted_rows(&state);
        let (clip_y, clip_h) = rows_clip(&state);
        for (track_idx, row_y) in painted.iter().enumerate() {
            for probe in [*row_y, row_y + TRACK_ROW_HEIGHT - 0.5] {
                if probe < clip_y || probe >= clip_y + clip_h {
                    continue;
                }
                click(&mut state, probe);
                assert_eq!(state.selected_index, Some(track_idx), "at y={probe}");
            }
        }
    }

    #[test]
    fn the_column_header_does_not_select_a_track() {
        let mut state = player_with_library(400);
        for y in [
            TAB_BAR_HEIGHT,
            TAB_BAR_HEIGHT + TRACK_ROW_HEIGHT / 2.0,
            TAB_BAR_HEIGHT + PlayerState::rows_top() - 0.5,
        ] {
            click(&mut state, y);
            assert_eq!(state.selected_index, None, "the header selected at {y}");
        }
    }

    #[test]
    fn empty_space_below_a_short_list_selects_nothing() {
        // The bug: `selected_index` was assigned unconditionally, so a click
        // in the blank part of the pane selected a track index past the end
        // of the list -- which nothing downstream expected.
        let mut state = player_with_library(3);
        let (clip_y, clip_h) = rows_clip(&state);
        let below_last = clip_y + 3.0 * TRACK_ROW_HEIGHT + 1.0;
        assert!(below_last < clip_y + clip_h, "the pane must fit >3 rows");
        click(&mut state, below_last);
        assert_eq!(state.selected_index, None);
    }

    #[test]
    fn the_controls_strip_does_not_select_a_track() {
        let mut state = player_with_library(400);
        let in_controls = state.height - CONTROLS_HEIGHT / 2.0;
        click(&mut state, in_controls);
        assert_eq!(state.selected_index, None);
    }

    #[test]
    fn the_now_playing_tab_has_no_rows_to_select() {
        // It is not a list, but the click path used to set `selected_index`
        // on it anyway -- to a track that tab never showed.
        let mut state = player_with_library(400);
        state.active_tab = Tab::NowPlaying;
        click(&mut state, TAB_BAR_HEIGHT + PlayerState::rows_top() + 1.0);
        assert_eq!(state.selected_index, None);
    }

    #[test]
    fn a_window_shorter_than_its_own_chrome_has_no_rows() {
        let mut state = player_with_library(400);
        state.height = TAB_BAR_HEIGHT;
        assert_eq!(state.rows_height(), 0.0);
        click(&mut state, TAB_BAR_HEIGHT + 1.0);
        assert_eq!(state.selected_index, None);
    }

    #[test]
    fn collapsing_the_window_and_restoring_it_keeps_the_reader_in_place() {
        // A degenerate pane has no answer to "how far can this scroll", so
        // the clamp must be a no-op there rather than snapping to either
        // end of a list it cannot show.
        let mut state = player_with_library(400);
        for _ in 0..20 {
            wheel(&mut state, -1.0);
        }
        let was = state.scroll_offset;
        assert!(was > 0.0);
        handle_event(
            &mut state,
            &Event::Resize {
                width: 900,
                height: TAB_BAR_HEIGHT as u32,
            },
        );
        assert_eq!(state.scroll_offset, was);
        handle_event(
            &mut state,
            &Event::Resize {
                width: 900,
                height: 700,
            },
        );
        assert_eq!(state.scroll_offset, was);
    }

    #[test]
    fn shrinking_the_window_winds_the_list_back_inside_itself() {
        // Only the wheel handler clamped, so a resize could leave the list
        // scrolled past its own end and showing nothing at all.
        let mut state = player_with_library(30);
        for _ in 0..200 {
            wheel(&mut state, -1.0);
        }
        assert_eq!(state.scroll_offset, state.max_scroll());
        handle_event(
            &mut state,
            &Event::Resize {
                width: 900,
                height: 2000,
            },
        );
        assert_eq!(state.scroll_offset, state.max_scroll());
        assert!(!painted_rows(&state).is_empty(), "the list went blank");
    }

    #[test]
    fn a_search_that_shortens_the_library_winds_the_list_back() {
        let mut state = player_with_library(400);
        for _ in 0..200 {
            wheel(&mut state, -1.0);
        }
        assert!(state.scroll_offset > 0.0);
        // "track 39" matches 39 and 390-399: eleven of the 400 titles, few
        // enough that the pane holds them all and the offset must go to 0.
        state.searching = true;
        for ch in "track 39".chars() {
            handle_event(
                &mut state,
                &Event::Key(KeyEvent {
                    key: Key::A,
                    pressed: true,
                    modifiers: Modifiers::default(),
                    text: Some(ch),
                }),
            );
        }
        assert_eq!(state.filtered_library().len(), 11);
        assert_eq!(state.max_scroll(), 0.0, "eleven rows must fit the pane");
        assert_eq!(state.scroll_offset, 0.0);
        assert!(!painted_rows(&state).is_empty(), "the list went blank");
    }

    #[test]
    fn a_nonfinite_coordinate_selects_nothing() {
        let mut state = player_with_library(400);
        for y in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            click(&mut state, y);
            assert_eq!(state.selected_index, None, "selected on {y}");
        }
    }

    #[test]
    fn a_double_click_plays_the_track_the_pointer_is_over() {
        let mut state = player_with_library(400);
        state.scroll_offset = 10.5 * TRACK_ROW_HEIGHT;
        let painted = painted_rows(&state);
        let (clip_y, _) = rows_clip(&state);
        assert!(!painted.is_empty());
        // The second painted row is fully inside the clip.
        let row_y = painted[1];
        assert!(row_y > clip_y);
        handle_mouse(
            &mut state,
            &MouseEvent {
                x: 100.0,
                y: row_y + 1.0,
                kind: MouseEventKind::DoubleClick(MouseButton::Left),
            },
        );
        assert_eq!(
            state
                .playlist
                .get(state.current_track_index.unwrap())
                .map(|t| t.title.clone()),
            Some(String::from("track 11")),
        );
    }

    /// A player with `len` one-second tracks, shuffle on, sitting on track 0.
    fn shuffling_player(seed: u64, len: usize) -> PlayerState {
        let mut state = PlayerState::with_seed(seed);
        for i in 0..len {
            let mut t = Track::from_path(PathBuf::from(format!("{i}.wav")));
            t.title = format!("track {i}");
            t.duration_secs = 1.0;
            state.playlist.push(t);
        }
        state.current_track_index = Some(0);
        state.repeat_mode = RepeatMode::All;
        state.toggle_shuffle();
        state
    }

    /// Shuffle has to be able to reach every track.
    ///
    /// The regression test for `next = (idx * 7 + 3) % len`, an affine map
    /// whose orbit is a fixed cycle. Measured on the old code, the number of
    /// distinct tracks it could ever reach, by playlist length:
    ///
    /// ```text
    ///   len:  2  3  4  5  6  7  8  9 10 11 12 13 14 15 16
    ///  seen:  2  1  2  4  2  1  2  3  4 10  2 12  2  4  4
    /// ```
    ///
    /// Twelve is the case that matters most -- an album -- and it alternated
    /// between two tracks forever. This test walks every length in that table
    /// so that a future change cannot fix one and break another.
    #[test]
    fn shuffle_reaches_every_track_at_every_playlist_length() {
        for len in 2..=16usize {
            let mut state = shuffling_player(0x5EED_0000_0000_0000 + len as u64, len);
            let mut seen = std::collections::HashSet::new();
            seen.insert(0usize);
            for _ in 0..(len * 12) {
                state.next_track();
                if let Some(idx) = state.current_track_index {
                    seen.insert(idx);
                }
            }
            assert_eq!(
                seen.len(),
                len,
                "playlist of {len}: shuffle only ever reached {} tracks: {seen:?}",
                seen.len()
            );
        }
    }

    /// A shuffle pass plays each track once before repeating any.
    ///
    /// This is the property the played-set exists to provide, and the reason
    /// the fix is not merely "draw a random index each time": drawing
    /// independently would let a track repeat immediately, which listeners
    /// read as a broken shuffle even though it is uniform.
    #[test]
    fn a_shuffle_pass_plays_every_track_once_before_repeating() {
        const LEN: usize = 10;
        let mut state = shuffling_player(0xC0FF_EE00_1234_5678, LEN);
        let mut heard = vec![0usize];
        for _ in 1..LEN {
            state.next_track();
            heard.push(state.current_track_index.unwrap());
        }
        let distinct: std::collections::HashSet<usize> = heard.iter().copied().collect();
        assert_eq!(
            distinct.len(),
            LEN,
            "a track repeated within one pass: {heard:?}"
        );
    }

    /// Two passes over the same playlist must not be the same pass.
    #[test]
    fn consecutive_shuffle_passes_differ() {
        const LEN: usize = 8;
        let mut state = shuffling_player(0xBEEF_1234_5678_9ABC, LEN);
        let pass = |state: &mut PlayerState| -> Vec<usize> {
            (0..LEN)
                .map(|_| {
                    state.next_track();
                    state.current_track_index.unwrap()
                })
                .collect()
        };
        let first = pass(&mut state);
        let second = pass(&mut state);
        assert_ne!(first, second, "the second pass replayed the first");
    }

    /// Shuffle with repeat off has to end.
    ///
    /// The old shuffle branch never consulted `repeat_mode`, so it could not
    /// reach the end of a playlist under any setting.
    #[test]
    fn shuffle_with_repeat_off_stops_after_one_pass() {
        const LEN: usize = 6;
        let mut state = shuffling_player(0x1357_9BDF_2468_ACE0, LEN);
        state.repeat_mode = RepeatMode::Off;
        state.playing = true;
        for _ in 1..LEN {
            state.next_track();
            assert!(state.playing, "stopped before the pass was over");
        }
        state.next_track();
        assert!(!state.playing, "shuffle ran past the end of the playlist");
    }

    /// The visualiser must not draw a straight ramp.
    ///
    /// The old heights were `(t * 31 + i * 7) % 100`, an exact arithmetic
    /// progression in the bar index: bar to bar the height rose by precisely
    /// 0.07, and on the two indices where the modulo wrapped it fell by 0.93.
    ///
    /// So the discriminating property is not "the steps are not all equal" --
    /// the wrap alone defeats that, and the first version of this test passed
    /// on the broken code for exactly that reason. It is how *many* distinct
    /// steps there are: a ramp with a wrap has two, and thirty-one independent
    /// draws have essentially thirty-one.
    #[test]
    fn the_visualizer_is_not_an_arithmetic_ramp() {
        let mut state = shuffling_player(0x0BAD_C0DE_0BAD_C0DE, 3);
        state.playing = true;
        for _ in 0..30 {
            state.advance_visualizer();
        }
        let bars = state.visualizer_bars().to_vec();
        // Bucketed to a hundredth, the resolution the old ramp itself worked
        // at, so its two step values collapse to two buckets rather than being
        // separated by float noise.
        let steps: std::collections::HashSet<i32> = bars
            .windows(2)
            .map(|w| ((w[1] - w[0]) * 100.0).round() as i32)
            .collect();
        assert!(
            steps.len() >= 20,
            "bar heights take only {} distinct steps -- a ramp, not a level: {bars:?}",
            steps.len()
        );
    }

    /// Every visualiser bar has to move.
    #[test]
    fn every_visualizer_bar_moves_over_time() {
        let mut state = shuffling_player(0x2468_ACE0_1357_9BDF, 3);
        state.playing = true;
        let mut history: Vec<Vec<f32>> = Vec::new();
        for _ in 0..40 {
            state.advance_visualizer();
            history.push(state.visualizer_bars().to_vec());
        }
        for i in 0..VISUALIZATION_BARS {
            let distinct = history
                .iter()
                .map(|f| f[i].to_bits())
                .collect::<std::collections::HashSet<u32>>()
                .len();
            assert!(
                distinct >= 20,
                "bar {i} took only {distinct} distinct heights over 40 frames"
            );
        }
    }

    /// A fresh player is seeded by the system, not by a literal.
    ///
    /// Host `cargo test` has no SlateOS entropy source, so `seeded_from_system`
    /// returns the fallback and two fresh players agree -- exactly as a
    /// hardcoded seed would. The test therefore asserts *which* seed. Gated off
    /// Unix, where the host does have entropy.
    #[cfg(not(unix))]
    #[test]
    fn a_fresh_player_is_seeded_by_the_system_and_not_by_a_literal() {
        fn first_pass(mut state: PlayerState) -> Vec<usize> {
            for i in 0..12 {
                let mut t = Track::from_path(PathBuf::from(format!("{i}.wav")));
                t.duration_secs = 1.0;
                state.playlist.push(t);
            }
            state.current_track_index = Some(0);
            state.repeat_mode = RepeatMode::All;
            state.toggle_shuffle();
            (0..12)
                .map(|_| {
                    state.next_track();
                    state.current_track_index.unwrap()
                })
                .collect()
        }
        let fresh = first_pass(PlayerState::new());
        assert_eq!(fresh, first_pass(PlayerState::with_seed(FALLBACK_SEED)));
        assert_ne!(fresh, first_pass(PlayerState::with_seed(7)));
    }

    #[test]
    fn test_audio_format_detection_wav() {
        let mut data = vec![0u8; 44];
        data[0..4].copy_from_slice(b"RIFF");
        data[8..12].copy_from_slice(b"WAVE");
        assert_eq!(AudioFormat::detect(&data), AudioFormat::Wav);
    }

    #[test]
    fn test_audio_format_detection_flac() {
        let mut data = vec![0u8; 12];
        data[0..4].copy_from_slice(b"fLaC");
        assert_eq!(AudioFormat::detect(&data), AudioFormat::Flac);
    }

    #[test]
    fn test_audio_format_detection_ogg() {
        let mut data = vec![0u8; 12];
        data[0..4].copy_from_slice(b"OggS");
        assert_eq!(AudioFormat::detect(&data), AudioFormat::Ogg);
    }

    #[test]
    fn test_audio_format_detection_mp3_id3() {
        let mut data = vec![0u8; 12];
        data[0..3].copy_from_slice(b"ID3");
        assert_eq!(AudioFormat::detect(&data), AudioFormat::Mp3);
    }

    #[test]
    fn test_audio_format_detection_mp3_sync() {
        let data = vec![
            0xFF, 0xFB, 0x90, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        assert_eq!(AudioFormat::detect(&data), AudioFormat::Mp3);
    }

    #[test]
    fn test_audio_format_detection_unknown() {
        let data = vec![0x00; 12];
        assert_eq!(AudioFormat::detect(&data), AudioFormat::Unknown);
    }

    #[test]
    fn test_wav_header_parsing() {
        // Minimal valid WAV: 44100 Hz, 2 channels, 16 bits. 1 second of audio is
        // 44100 * 2 channels * 2 bytes = 176400 bytes of PCM data.
        let mut data = vec![0u8; 44];
        data[0..4].copy_from_slice(b"RIFF");
        data[4..8].copy_from_slice(&(36 + 176400u32).to_le_bytes());
        data[8..12].copy_from_slice(b"WAVE");
        // fmt chunk
        data[12..16].copy_from_slice(b"fmt ");
        data[16..20].copy_from_slice(&16u32.to_le_bytes()); // chunk size
        data[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
        data[22..24].copy_from_slice(&2u16.to_le_bytes()); // channels
        data[24..28].copy_from_slice(&44100u32.to_le_bytes()); // sample rate
        data[28..32].copy_from_slice(&176400u32.to_le_bytes()); // byte rate
        data[32..34].copy_from_slice(&4u16.to_le_bytes()); // block align
        data[34..36].copy_from_slice(&16u16.to_le_bytes()); // bits per sample
        // data chunk
        data[36..40].copy_from_slice(b"data");
        data[40..44].copy_from_slice(&176400u32.to_le_bytes());

        let info = parse_wav_header(&data).expect("Should parse valid WAV header");
        assert_eq!(info.sample_rate, 44100);
        assert_eq!(info.channels, 2);
        assert_eq!(info.bits_per_sample, 16);
        assert!((info.duration_secs - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_format_time() {
        assert_eq!(format_time(0.0), "00:00");
        assert_eq!(format_time(65.0), "01:05");
        assert_eq!(format_time(3661.0), "61:01");
        assert_eq!(format_time(-5.0), "00:00");
    }

    #[test]
    fn test_player_state_play_pause() {
        let mut state = PlayerState::new();
        let track = Track::from_path(PathBuf::from("/test.mp3"));
        state.add_track(track);

        assert!(!state.playing);
        state.toggle_play();
        assert!(state.playing);
        assert_eq!(state.current_track_index, Some(0));
        state.toggle_play();
        assert!(!state.playing);
    }

    #[test]
    fn test_player_state_stop() {
        let mut state = PlayerState::new();
        let track = Track::from_path(PathBuf::from("/test.mp3"));
        state.add_track(track);
        state.toggle_play();
        state.position_secs = 30.0;
        state.stop();
        assert!(!state.playing);
        assert_eq!(state.position_secs, 0.0);
    }

    #[test]
    fn test_player_next_prev() {
        let mut state = PlayerState::new();
        for i in 0..3 {
            state.add_track(Track::from_path(PathBuf::from(format!("/track{}.mp3", i))));
        }
        state.current_track_index = Some(0);
        state.next_track();
        assert_eq!(state.current_track_index, Some(1));
        state.next_track();
        assert_eq!(state.current_track_index, Some(2));

        // Test prev (within first 3 seconds goes to previous)
        state.position_secs = 1.0;
        state.prev_track();
        assert_eq!(state.current_track_index, Some(1));
    }

    #[test]
    fn test_player_volume() {
        let mut state = PlayerState::new();
        state.volume = 0.5;
        state.adjust_volume(0.1);
        assert!((state.volume - 0.6).abs() < 0.001);
        state.adjust_volume(-0.8);
        assert_eq!(state.volume, 0.0); // Clamped to 0
        state.adjust_volume(1.5);
        assert_eq!(state.volume, 1.0); // Clamped to 1
    }

    #[test]
    fn test_player_repeat_mode_cycle() {
        assert_eq!(RepeatMode::Off.next(), RepeatMode::One);
        assert_eq!(RepeatMode::One.next(), RepeatMode::All);
        assert_eq!(RepeatMode::All.next(), RepeatMode::Off);
    }

    #[test]
    fn test_playlist_operations() {
        let mut state = PlayerState::new();
        for i in 0..5 {
            state.add_track(Track::from_path(PathBuf::from(format!("/track{}.mp3", i))));
        }
        assert_eq!(state.playlist.len(), 5);

        state.remove_track(2);
        assert_eq!(state.playlist.len(), 4);

        state.move_track_up(2);
        assert_eq!(state.playlist[1].path, PathBuf::from("/track3.mp3"));

        state.move_track_down(0);
        assert_eq!(state.playlist[1].path, PathBuf::from("/track0.mp3"));

        state.clear_playlist();
        assert_eq!(state.playlist.len(), 0);
        assert_eq!(state.current_track_index, None);
    }

    #[test]
    fn test_m3u_export() {
        let mut state = PlayerState::new();
        let mut track = Track::from_path(PathBuf::from("/music/song.mp3"));
        track.title = String::from("My Song");
        track.artist = String::from("Artist");
        track.duration_secs = 180.0;
        state.playlist.push(track);

        let m3u = state.export_m3u();
        assert!(m3u.text.starts_with("#EXTM3U\n"));
        assert!(m3u.text.contains("#EXTINF:180,Artist - My Song"));
        assert!(m3u.text.contains("/music/song.mp3"));
        assert!(m3u.skipped.is_empty());
    }

    #[test]
    fn a_newline_in_an_id3_title_cannot_forge_a_playlist_entry() {
        // `title` and `artist` are set from the file's own ID3 tags, so for a
        // downloaded file they are chosen by whoever made it.
        let mut state = PlayerState::new();
        let mut track = Track::from_path(PathBuf::from("/music/song.mp3"));
        track.title = String::from("Song\n/etc/shadow\n#EXTINF:1,x");
        track.artist = String::from("Artist\n/evil/payload");
        track.duration_secs = 180.0;
        state.playlist.push(track);

        let m3u = state.export_m3u();
        // One track in, one track out: `load_m3u` reads every non-`#` line as
        // a path, so counting them is exactly counting forged entries.
        let paths: Vec<&str> = m3u
            .text
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .collect();
        assert_eq!(paths, ["/music/song.mp3"], "forged entries: {}", m3u.text);

        // And the round trip through the reader agrees.
        let mut back = PlayerState::new();
        back.load_m3u(&m3u.text);
        assert_eq!(back.playlist.len(), 1);
    }

    #[test]
    fn a_track_whose_path_has_a_newline_is_reported_not_silently_wrong() {
        // This OS allows every byte but `/` and NUL in a path, so such a path
        // is legal -- and unrepresentable in M3U.
        let mut state = PlayerState::new();
        state
            .playlist
            .push(Track::from_path(PathBuf::from("/music/od\nd.mp3")));
        state
            .playlist
            .push(Track::from_path(PathBuf::from("/music/fine.mp3")));

        let m3u = state.export_m3u();
        assert_eq!(m3u.skipped, [PathBuf::from("/music/od\nd.mp3")]);
        let mut back = PlayerState::new();
        back.load_m3u(&m3u.text);
        assert_eq!(back.playlist.len(), 1);
        assert_eq!(back.playlist[0].path, PathBuf::from("/music/fine.mp3"));
    }

    #[test]
    fn test_m3u_load() {
        let mut state = PlayerState::new();
        let content = "#EXTM3U\n#EXTINF:180,Artist - Song\n/music/song.mp3\n/music/song2.flac\n";
        state.load_m3u(content);
        assert_eq!(state.playlist.len(), 2);
        assert_eq!(state.playlist[0].path, PathBuf::from("/music/song.mp3"));
        assert_eq!(state.playlist[1].path, PathBuf::from("/music/song2.flac"));
    }

    #[test]
    fn test_seek() {
        let mut state = PlayerState::new();
        let mut track = Track::from_path(PathBuf::from("/test.mp3"));
        track.duration_secs = 100.0;
        state.add_track(track);
        state.current_track_index = Some(0);

        state.seek_fraction(0.5);
        assert!((state.position_secs - 50.0).abs() < 0.01);

        state.seek_relative(10.0);
        assert!((state.position_secs - 60.0).abs() < 0.01);

        state.seek_relative(-100.0);
        assert_eq!(state.position_secs, 0.0); // Clamped
    }

    #[test]
    fn test_tick_advances_playback() {
        let mut state = PlayerState::new();
        let mut track = Track::from_path(PathBuf::from("/test.mp3"));
        track.duration_secs = 10.0;
        state.add_track(track);
        state.current_track_index = Some(0);
        state.playing = true;

        state.tick(3.0);
        assert!((state.position_secs - 3.0).abs() < 0.01);
    }

    #[test]
    fn test_tick_track_end_repeat_one() {
        let mut state = PlayerState::new();
        let mut track = Track::from_path(PathBuf::from("/test.mp3"));
        track.duration_secs = 5.0;
        state.add_track(track);
        state.current_track_index = Some(0);
        state.playing = true;
        state.repeat_mode = RepeatMode::One;
        state.position_secs = 4.0;

        state.tick(2.0); // Goes past end
        assert_eq!(state.position_secs, 0.0); // Loops back
        assert!(state.playing);
    }

    #[test]
    fn test_album_color_deterministic() {
        let c1 = album_color("Test Album");
        let c2 = album_color("Test Album");
        assert_eq!(c1, c2);

        let c3 = album_color("Different Album");
        // Different albums should (usually) get different colors
        assert_ne!(album_color("A"), album_color("B"));
        let _ = c3; // used
    }

    #[test]
    fn test_render_produces_output() {
        let state = PlayerState::new();
        let tree = render(&state);
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_synchsafe_u32() {
        assert_eq!(synchsafe_u32(&[0x00, 0x00, 0x02, 0x01]), Some(257));
        assert_eq!(synchsafe_u32(&[0x00, 0x00, 0x00, 0x7F]), Some(127));
        assert_eq!(synchsafe_u32(&[0x00, 0x00, 0x01, 0x00]), Some(128));
    }

    #[test]
    fn test_filtered_library_empty_query() {
        let mut state = PlayerState::new();
        state.add_track(Track::from_path(PathBuf::from("/a.mp3")));
        state.add_track(Track::from_path(PathBuf::from("/b.mp3")));
        assert_eq!(state.filtered_library().len(), 2);
    }

    #[test]
    fn test_filtered_library_with_query() {
        let mut state = PlayerState::new();
        let mut t1 = Track::from_path(PathBuf::from("/a.mp3"));
        t1.title = String::from("Hello World");
        let mut t2 = Track::from_path(PathBuf::from("/b.mp3"));
        t2.title = String::from("Goodbye");
        state.library.push(t1);
        state.library.push(t2);
        state.search_query = String::from("hello");
        assert_eq!(state.filtered_library().len(), 1);
    }

    #[test]
    fn test_sort_library() {
        let mut state = PlayerState::new();
        let mut t1 = Track::from_path(PathBuf::from("/a.mp3"));
        t1.title = String::from("Zebra");
        let mut t2 = Track::from_path(PathBuf::from("/b.mp3"));
        t2.title = String::from("Apple");
        state.library.push(t1);
        state.library.push(t2);

        state.sort_library(SortBy::Title);
        assert_eq!(state.library[0].title, "Apple");
        assert_eq!(state.library[1].title, "Zebra");
    }
}
