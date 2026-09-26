//! Video Player application for SlateOS.
//!
//! Full-featured media player with playlist management, subtitle support,
//! audio track selection, playback controls, and a modern UI. Supports
//! common container formats (MP4, MKV, AVI, WebM, MOV) and codecs
//! (H.264, H.265, VP9, AV1, AAC, Opus, FLAC).

use appearance::Edge;
use appearance::Palette;
use appearance::Surface;
// The toolkit's rectangle rather than a private copy: this crate had
// the same four floats under `width`/`height`, with the same half-open
// `contains`. See `known-issues.md`
// `TD-C-TEN-RECTANGLE-TYPES-IN-THREE-SPELLINGS`.
use guitk::color::Color;
use guitk::dialog::{FilePicker, Picked};
use guitk::event::{Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::rng::{RandomSource, SeededRng, seeded_from_system};
use guitk::style::CornerRadii;
use mediaprobe::Codec;
use oswindow::app::{self, App, Response};
use oswindow::{Event, RenderTree};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Seed used when the system has no entropy to offer.
///
/// A shuffled playlist is novelty randomness, not a secret, so losing entropy
/// must not stop playback. The constant is per-crate ("VIDEOPLR") so that two
/// programs falling back on the same boot do not then agree with each other.
const FALLBACK_SEED: u64 = 0x5649_4445_4F50_4C52;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

// Part of the complete Catppuccin Mocha palette; kept for completeness even
// though no widget currently paints with it.

/// How long the controls stay up after the last sign of life.
const CONTROLS_HIDE_MS: u64 = 3000;
/// Height of the control bar along the bottom.
const CONTROLS_HEIGHT: f32 = 80.0;
/// Distance from the top of the control bar to the seek bar inside it.
const SEEK_BAR_OFFSET: f32 = 8.0;
/// How far the seek bar is inset from either window edge.
const SEEK_BAR_INSET: f32 = 16.0;
/// How thick the seek bar is drawn.
const SEEK_BAR_HEIGHT: f32 = 6.0;
/// How thick it is to grab. A 6px line is not a thing a pointer can reliably
/// land on, so the band that answers a click is taller than the one drawn.
const SEEK_BAR_GRAB_HEIGHT: f32 = 18.0;
/// Height of the tab strip along the top.
const TAB_BAR_HEIGHT: f32 = 36.0;
/// Y the player's own content starts at, below the tab strip.
const CONTENT_TOP: f32 = 40.0;
/// Widest a tab is allowed to get.
const TAB_MAX_WIDTH: f32 = 120.0;
/// Air around a tab inside its slot.
const TAB_PADDING: f32 = 4.0;
/// How often the picture and the on-screen message are advanced.
///
/// `std::time::Duration`, not this file's own millisecond `Duration`: the two
/// share a name and only one of them is what the harness's clock speaks.
const FRAME_TICK: std::time::Duration = std::time::Duration::from_millis(100);
/// What the window says instead of a picture, with no file open.
///
/// Three lines. The third is about the playlist, which outlives the window:
/// entries naming `/home/user/Videos/sample.mkv` are a claim that a file is at
/// that path, and a playlist is the kind of thing somebody reads later to find
/// out what they have.
///
/// The second said "it has no filesystem access, so nothing has been read",
/// long after both stopped being true; what is still true is the first half
/// of the first line's old wording -- nothing decodes a picture -- and a file
/// can now be opened for everything but that.
const CANNOT_PLAY_LINES: [&str; 3] = [
    "This player cannot play a file: nothing here decodes video.",
    "Ctrl+O opens one to read what it holds -- its length, its picture's size and codec, its sound and its subtitles.",
    "The playlist is empty because nothing was opened -- it is not a list of files you have.",
];

/// What the window says where the picture would be, with a file open.
const NO_PICTURE_LINES: [&str; 2] = [
    "No picture: nothing here decodes video.",
    "What the file holds is on the Media Info tab (I).",
];

/// What Play says, with a file open and nothing to decode it.
const CANNOT_DECODE: &str = "Cannot play: nothing here decodes video";

const WINDOW_WIDTH: f32 = 1280.0;
const WINDOW_HEIGHT: f32 = 720.0;
/// A window smaller than this has no room left for the controls.
const MIN_WINDOW_WIDTH: f32 = 480.0;
const MIN_WINDOW_HEIGHT: f32 = 320.0;

/// What the Screenshots block says under the options it offers.
///
/// **The shortcut for this was already removed as impossible.** `Shortcuts
/// ::list` used to advertise `Ctrl+S` (take a screenshot) and no longer does,
/// because a help panel that promises what the program cannot do is the same
/// defect one level up. The reason that stands is that no frame is decoded:
/// there is nothing to take a picture of.
///
/// The settings block was left behind, still naming a format, a quality and a
/// subtitle option. `CANNOT_PLAY_LINES` does say nothing decodes -- but it is
/// drawn where the picture would be, and relying on a reader having seen it
/// before reaching a settings panel is what `apps/mediaconvert` got wrong:
/// three lines about the queue did not reach the panel that configured it.
const NO_SCREENSHOTS: &str = "Not applied: this player cannot take a \
screenshot -- no frame is decoded.";

// ============================================================================
// Media container and codec types
// ============================================================================

/// Supported container formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerFormat {
    Mp4,
    Mkv,
    Avi,
    WebM,
    Mov,
    Flv,
    Wmv,
    Ogg,
    Ts,
}

impl ContainerFormat {
    /// The container the file's own bytes name, where `mediaprobe` reads it.
    pub fn from_probe(container: mediaprobe::Container) -> Option<Self> {
        match container {
            mediaprobe::Container::Mp4 => Some(Self::Mp4),
            mediaprobe::Container::QuickTime => Some(Self::Mov),
            mediaprobe::Container::Matroska => Some(Self::Mkv),
            mediaprobe::Container::WebM => Some(Self::WebM),
            mediaprobe::Container::Avi => Some(Self::Avi),
            mediaprobe::Container::Unknown => None,
        }
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "mp4" | "m4v" => Some(Self::Mp4),
            "mkv" => Some(Self::Mkv),
            "avi" => Some(Self::Avi),
            "webm" => Some(Self::WebM),
            "mov" => Some(Self::Mov),
            "flv" => Some(Self::Flv),
            "wmv" => Some(Self::Wmv),
            "ogg" | "ogv" => Some(Self::Ogg),
            "ts" | "mts" | "m2ts" => Some(Self::Ts),
            _ => None,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Mp4 => "MPEG-4 (MP4)",
            Self::Mkv => "Matroska (MKV)",
            Self::Avi => "AVI",
            Self::WebM => "WebM",
            Self::Mov => "QuickTime (MOV)",
            Self::Flv => "Flash Video (FLV)",
            Self::Wmv => "Windows Media Video",
            Self::Ogg => "Ogg",
            Self::Ts => "MPEG Transport Stream",
        }
    }

    pub fn typical_extensions(self) -> &'static [&'static str] {
        match self {
            Self::Mp4 => &["mp4", "m4v"],
            Self::Mkv => &["mkv"],
            Self::Avi => &["avi"],
            Self::WebM => &["webm"],
            Self::Mov => &["mov"],
            Self::Flv => &["flv"],
            Self::Wmv => &["wmv"],
            Self::Ogg => &["ogg", "ogv"],
            Self::Ts => &["ts", "mts", "m2ts"],
        }
    }
}

// The codecs are `mediaprobe::Codec`, which names what a file's header
// names. This file had its own `VideoCodec` and `AudioCodec` -- nine and
// eleven variants, filled only by sample data -- and a `HW Decode:
// Supported` row that said so for H.264 on a system that decodes nothing.

/// A channel count's usual name: 6 is 5.1.
pub fn channel_layout_name(channels: u32) -> &'static str {
    match channels {
        1 => "Mono",
        2 => "Stereo",
        3 => "2.1",
        4 => "Quad",
        5 => "5.0",
        6 => "5.1",
        7 => "6.1",
        8 => "7.1",
        _ => "Unknown",
    }
}

/// Whether sound in `codec` comes back exactly as it went in.
pub fn is_lossless(codec: &Codec) -> bool {
    matches!(
        codec,
        Codec::Flac | Codec::Alac | Codec::Pcm | Codec::TrueHd
    )
}

/// Whether subtitles in `codec` are text, rather than pictures of text.
pub fn is_text_subtitle(codec: &Codec) -> bool {
    matches!(
        codec,
        Codec::Text | Codec::WebVtt | Codec::Ttml | Codec::Ass | Codec::Ssa
    )
}

// ============================================================================
// Duration & timestamp helpers
// ============================================================================

/// Represents a duration in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Duration(pub u64);

impl Duration {
    pub const ZERO: Self = Self(0);

    pub fn from_secs(s: u64) -> Self {
        Self(s.saturating_mul(1000))
    }

    pub fn from_millis(ms: u64) -> Self {
        Self(ms)
    }

    /// Seconds as a float -- a header's length -- to the nearest
    /// millisecond. Nothing, for what is not a length.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "checked finite and non-negative first, and `as` saturates"
    )]
    pub fn from_secs_f64(secs: f64) -> Self {
        if secs.is_finite() && secs > 0.0 {
            Self((secs * 1000.0).round() as u64)
        } else {
            Self::ZERO
        }
    }

    pub fn as_secs(self) -> u64 {
        self.0 / 1000
    }

    pub fn as_millis(self) -> u64 {
        self.0
    }

    pub fn as_secs_f64(self) -> f64 {
        self.0 as f64 / 1000.0
    }

    /// Format as HH:MM:SS or MM:SS depending on length.
    pub fn format(self) -> String {
        let total_secs = self.as_secs();
        let hours = total_secs / 3600;
        let minutes = (total_secs % 3600) / 60;
        let seconds = total_secs % 60;

        if hours > 0 {
            format!("{hours}:{minutes:02}:{seconds:02}")
        } else {
            format!("{minutes}:{seconds:02}")
        }
    }

    /// Format as HH:MM:SS.mmm for subtitle timing.
    pub fn format_precise(self) -> String {
        let total_ms = self.as_millis();
        let hours = total_ms / 3_600_000;
        let minutes = (total_ms % 3_600_000) / 60_000;
        let seconds = (total_ms % 60_000) / 1000;
        let ms = total_ms % 1000;
        format!("{hours:02}:{minutes:02}:{seconds:02}.{ms:03}")
    }

    pub fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    pub fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }

    /// Progress fraction (0.0 to 1.0) of self relative to total.
    pub fn progress_of(self, total: Self) -> f64 {
        if total.0 == 0 {
            0.0
        } else {
            (self.0 as f64) / (total.0 as f64)
        }
    }
}

// ============================================================================
// Media streams
// ============================================================================

/// A video stream within a media file: each part `None` where the file's
/// headers do not say.
#[derive(Debug, Clone)]
pub struct VideoStream {
    pub index: u32,
    pub codec: Codec,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub frame_rate: Option<f64>,
    pub bit_rate: Option<u64>,
    pub pixel_format: Option<String>,
    pub color_space: Option<String>,
    pub hdr: bool,
}

impl VideoStream {
    /// The size's usual name -- "1080p (Full HD)" -- if the height is known.
    pub fn resolution_label(&self) -> Option<&'static str> {
        Some(match self.height? {
            0..=360 => "360p",
            361..=480 => "480p (SD)",
            481..=720 => "720p (HD)",
            721..=1080 => "1080p (Full HD)",
            1081..=1440 => "1440p (2K)",
            1441..=2160 => "2160p (4K UHD)",
            2161..=4320 => "4320p (8K UHD)",
            _ => "Unknown",
        })
    }

    /// `16:9`, `4:3` and the like, or `1.85:1`; `None` without a size.
    pub fn aspect_ratio(&self) -> Option<String> {
        let (w, h) = (self.width?, self.height.filter(|&h| h > 0)?);
        let ratio = f64::from(w) / f64::from(h);
        // Common aspect ratios
        Some(if (ratio - 16.0 / 9.0).abs() < 0.05 {
            "16:9".to_string()
        } else if (ratio - 4.0 / 3.0).abs() < 0.05 {
            "4:3".to_string()
        } else if (ratio - 21.0 / 9.0).abs() < 0.1 {
            "21:9".to_string()
        } else if (ratio - 1.0).abs() < 0.05 {
            "1:1".to_string()
        } else {
            format!("{ratio:.2}:1")
        })
    }

    /// `1920x1080`, as far as the size is known.
    pub fn size_label(&self) -> Option<String> {
        Some(format!("{}x{}", self.width?, self.height?))
    }
}

/// An audio stream within a media file.
#[derive(Debug, Clone)]
pub struct AudioStream {
    pub index: u32,
    pub codec: Codec,
    pub sample_rate: Option<u32>,
    pub channels: Option<u32>,
    pub bit_rate: Option<u64>,
    pub language: Option<String>,
    pub title: Option<String>,
    pub is_default: bool,
}

impl AudioStream {
    pub fn display_label(&self) -> String {
        let codec = self.codec.name();
        let lang = self.language.as_deref().unwrap_or("Unknown");
        let title = self.title.as_deref().unwrap_or("");
        let what = match self.channels {
            Some(channels) => format!("{codec} {}", channel_layout_name(channels)),
            None => codec.to_string(),
        };
        if title.is_empty() {
            format!("{lang} - {what}")
        } else {
            format!("{title} ({lang}) - {what}")
        }
    }
}

/// A subtitle stream.
#[derive(Debug, Clone)]
pub struct SubtitleStream {
    pub index: u32,
    pub codec: Codec,
    pub language: Option<String>,
    pub title: Option<String>,
    pub is_default: bool,
    pub is_forced: bool,
}

impl SubtitleStream {
    pub fn display_label(&self) -> String {
        let lang = self.language.as_deref().unwrap_or("Unknown");
        let fmt = self.codec.name();
        let title = self.title.as_deref().unwrap_or("");
        let forced = if self.is_forced { " [Forced]" } else { "" };
        if title.is_empty() {
            format!("{lang} ({fmt}){forced}")
        } else {
            format!("{title} ({lang}, {fmt}){forced}")
        }
    }
}

// ============================================================================
// Subtitle cue
// ============================================================================

/// A single subtitle cue (one displayed text segment).
#[derive(Debug, Clone)]
pub struct SubtitleCue {
    pub start: Duration,
    pub end: Duration,
    pub text: String,
    pub style: Option<SubtitleStyle>,
}

impl SubtitleCue {
    pub fn is_active_at(&self, position: Duration) -> bool {
        position >= self.start && position < self.end
    }

    /// Duration this cue is displayed.
    pub fn display_duration(&self) -> Duration {
        self.end.saturating_sub(self.start)
    }
}

/// Styling for subtitles.
#[derive(Debug, Clone)]
pub struct SubtitleStyle {
    pub font_size: f32,
    pub bold: bool,
    pub italic: bool,
    pub color: Color,
    pub outline_color: Color,
    pub outline_width: f32,
    pub position: SubtitlePosition,
}

impl Default for SubtitleStyle {
    fn default() -> Self {
        Self {
            font_size: 24.0,
            bold: false,
            italic: false,
            color: Color::rgb(255, 255, 255),
            outline_color: Color::rgb(0, 0, 0),
            outline_width: 1.5,
            position: SubtitlePosition::Bottom,
        }
    }
}

/// Where subtitles are displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitlePosition {
    Top,
    Bottom,
    Custom { x_percent: u32, y_percent: u32 },
}

// ============================================================================
// SRT parser
// ============================================================================

/// Parse SRT subtitle content into cues.
pub fn parse_srt(content: &str) -> Vec<SubtitleCue> {
    let mut cues = Vec::new();
    let mut lines = content.lines().peekable();

    while lines.peek().is_some() {
        // Skip blank lines and cue number
        while let Some(line) = lines.peek() {
            if line.trim().is_empty() || line.trim().chars().all(|c| c.is_ascii_digit()) {
                lines.next();
            } else {
                break;
            }
        }

        // Parse timestamp line: "00:01:23,456 --> 00:01:26,789"
        let ts_line = match lines.next() {
            Some(l) => l.trim(),
            None => break,
        };

        let (start, end) = match parse_srt_timestamp_line(ts_line) {
            Some(pair) => pair,
            None => continue,
        };

        // Collect text lines until blank line
        let mut text_parts = Vec::new();
        while let Some(line) = lines.peek() {
            if line.trim().is_empty() {
                lines.next();
                break;
            }
            text_parts.push(lines.next().unwrap_or_default().trim().to_string());
        }

        if !text_parts.is_empty() {
            cues.push(SubtitleCue {
                start,
                end,
                text: text_parts.join("\n"),
                style: None,
            });
        }
    }

    cues
}

fn parse_srt_timestamp_line(line: &str) -> Option<(Duration, Duration)> {
    let parts: Vec<&str> = line.split("-->").collect();
    if parts.len() != 2 {
        return None;
    }
    let start = parse_srt_time(parts.first()?.trim())?;
    let end = parse_srt_time(parts.get(1)?.trim())?;
    Some((start, end))
}

fn parse_srt_time(s: &str) -> Option<Duration> {
    // Format: HH:MM:SS,mmm or HH:MM:SS.mmm
    let s = s.replace(',', ".");
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    let hours: u64 = parts.first()?.parse().ok()?;
    let minutes: u64 = parts.get(1)?.parse().ok()?;

    let sec_parts: Vec<&str> = parts.get(2)?.split('.').collect();
    let seconds: u64 = sec_parts.first()?.parse().ok()?;
    let millis: u64 = if sec_parts.len() > 1 {
        let ms_str = sec_parts.get(1)?;
        // Pad or truncate to 3 digits.
        //
        // `format!`'s width is counted in *characters* but the slice below
        // indexes *bytes*, and the two disagree on anything non-ASCII. For a
        // fractional part of "ab\u{65e5}" the padding adds nothing — that is
        // already 3 characters — and byte 3 lands inside the kanji, so the
        // slice aborted the player on a subtitle file it had merely been asked
        // to open. Milliseconds are digits; requiring that up front makes the
        // character count and the byte count the same number.
        if !ms_str.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let padded = format!("{ms_str:0<3}");
        padded.get(..3)?.parse().ok()?
    } else {
        0
    };

    let total_ms = hours
        .checked_mul(3_600_000)?
        .checked_add(minutes.checked_mul(60_000)?)?
        .checked_add(seconds.checked_mul(1000)?)?
        .checked_add(millis)?;

    Some(Duration::from_millis(total_ms))
}

// ============================================================================
// WebVTT parser
// ============================================================================

/// Parse WebVTT subtitle content into cues.
pub fn parse_webvtt(content: &str) -> Vec<SubtitleCue> {
    let mut cues = Vec::new();
    let mut lines = content.lines().peekable();

    // Skip WEBVTT header
    if let Some(first) = lines.peek()
        && first.starts_with("WEBVTT")
    {
        lines.next();
        // Skip header lines until blank
        while let Some(line) = lines.peek() {
            if line.trim().is_empty() {
                lines.next();
                break;
            }
            lines.next();
        }
    }

    while lines.peek().is_some() {
        // Skip blank lines and optional cue identifiers
        while let Some(line) = lines.peek() {
            if line.trim().is_empty() {
                lines.next();
            } else if !line.contains("-->") {
                // Could be cue identifier
                lines.next();
            } else {
                break;
            }
        }

        // Parse timestamp line
        let ts_line = match lines.next() {
            Some(l) => l.trim(),
            None => break,
        };

        if !ts_line.contains("-->") {
            continue;
        }

        let (start, end) = match parse_webvtt_timestamp_line(ts_line) {
            Some(pair) => pair,
            None => continue,
        };

        // Collect text until blank line
        let mut text_parts = Vec::new();
        while let Some(line) = lines.peek() {
            if line.trim().is_empty() {
                lines.next();
                break;
            }
            text_parts.push(lines.next().unwrap_or_default().trim().to_string());
        }

        if !text_parts.is_empty() {
            cues.push(SubtitleCue {
                start,
                end,
                text: strip_webvtt_tags(&text_parts.join("\n")),
                style: None,
            });
        }
    }

    cues
}

fn parse_webvtt_timestamp_line(line: &str) -> Option<(Duration, Duration)> {
    let parts: Vec<&str> = line.split("-->").collect();
    if parts.len() != 2 {
        return None;
    }
    let start = parse_webvtt_time(parts.first()?.trim())?;
    let end = parse_webvtt_time(parts.get(1)?.split_whitespace().next()?)?;
    Some((start, end))
}

fn parse_webvtt_time(s: &str) -> Option<Duration> {
    // Format: MM:SS.mmm or HH:MM:SS.mmm
    let parts: Vec<&str> = s.split(':').collect();
    match parts.len() {
        2 => {
            let minutes: u64 = parts.first()?.parse().ok()?;
            let sec_parts: Vec<&str> = parts.get(1)?.split('.').collect();
            let seconds: u64 = sec_parts.first()?.parse().ok()?;
            let millis: u64 = sec_parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            Some(Duration::from_millis(
                minutes
                    .checked_mul(60_000)?
                    .checked_add(seconds.checked_mul(1000)?)?
                    .checked_add(millis)?,
            ))
        }
        3 => {
            let hours: u64 = parts.first()?.parse().ok()?;
            let minutes: u64 = parts.get(1)?.parse().ok()?;
            let sec_parts: Vec<&str> = parts.get(2)?.split('.').collect();
            let seconds: u64 = sec_parts.first()?.parse().ok()?;
            let millis: u64 = sec_parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            Some(Duration::from_millis(
                hours
                    .checked_mul(3_600_000)?
                    .checked_add(minutes.checked_mul(60_000)?)?
                    .checked_add(seconds.checked_mul(1000)?)?
                    .checked_add(millis)?,
            ))
        }
        _ => None,
    }
}

fn strip_webvtt_tags(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut in_tag = false;
    for ch in text.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result
}

// ============================================================================
// Media file info
// ============================================================================

/// Complete information about a media file.
#[derive(Debug, Clone)]
pub struct MediaFile {
    pub path: PathBuf,
    pub file_name: String,
    pub file_size: u64,
    pub container: ContainerFormat,
    pub duration: Duration,
    pub video_streams: Vec<VideoStream>,
    pub audio_streams: Vec<AudioStream>,
    pub subtitle_streams: Vec<SubtitleStream>,
    pub metadata: MediaMetadata,
}

impl MediaFile {
    /// Read the file at `path` for what it holds, or say why it cannot be.
    ///
    /// # Errors
    ///
    /// A message fit to show: the file cannot be read, is a folder, or is not
    /// a video this player knows by its bytes or, failing those, its name.
    pub fn open(path: &Path) -> Result<Self, String> {
        let shown = path.display();
        let md = std::fs::metadata(path).map_err(|e| format!("Could not open {shown}: {e}"))?;
        if md.is_dir() {
            return Err(format!("Could not open {shown}: it is a folder"));
        }
        let probe =
            mediaprobe::probe_path(path).map_err(|e| format!("Could not read {shown}: {e}"))?;
        Self::from_probe(path, md.len(), probe)
            .ok_or_else(|| format!("{shown} is not a video this player knows"))
    }

    /// A file of `size` bytes at `path`, as `probe` read it. `None` when
    /// neither the bytes nor the name say it is a video.
    pub fn from_probe(path: &Path, size: u64, probe: mediaprobe::Probe) -> Option<Self> {
        let container = ContainerFormat::from_probe(probe.container).or_else(|| {
            path.extension()
                .and_then(|e| e.to_str())
                .and_then(ContainerFormat::from_extension)
        })?;
        let mut file = Self {
            path: path.to_path_buf(),
            file_name: shown_name(path),
            file_size: size,
            container,
            duration: probe
                .duration_secs
                .map_or(Duration::ZERO, Duration::from_secs_f64),
            video_streams: Vec::new(),
            audio_streams: Vec::new(),
            subtitle_streams: Vec::new(),
            metadata: MediaMetadata {
                title: probe.title,
                ..MediaMetadata::default()
            },
        };
        for (index, track) in (0_u32..).zip(probe.tracks) {
            match track.kind {
                mediaprobe::Kind::Video => file.video_streams.push(VideoStream {
                    index,
                    codec: track.codec,
                    width: track.width,
                    height: track.height,
                    frame_rate: track.frame_rate,
                    bit_rate: None,
                    pixel_format: None,
                    color_space: None,
                    hdr: false,
                }),
                mediaprobe::Kind::Audio => file.audio_streams.push(AudioStream {
                    index,
                    codec: track.codec,
                    sample_rate: track.sample_rate,
                    channels: track.channels.map(u32::from),
                    bit_rate: None,
                    language: track.language,
                    title: track.name,
                    is_default: track.default,
                }),
                mediaprobe::Kind::Subtitle => file.subtitle_streams.push(SubtitleStream {
                    index,
                    codec: track.codec,
                    language: track.language,
                    title: track.name,
                    is_default: track.default,
                    is_forced: track.forced,
                }),
                mediaprobe::Kind::Other => {}
            }
        }
        Some(file)
    }

    pub fn primary_video(&self) -> Option<&VideoStream> {
        self.video_streams.first()
    }

    pub fn primary_audio(&self) -> Option<&AudioStream> {
        self.audio_streams
            .iter()
            .find(|a| a.is_default)
            .or_else(|| self.audio_streams.first())
    }

    pub fn file_size_display(&self) -> String {
        format_bytes(self.file_size)
    }

    /// Bits per second, or zero for a file whose header says nothing about
    /// how long it is.
    ///
    /// One statement of that rule rather than two: this had an early return
    /// for a zero duration *and* a bare division by it, so the guard was the
    /// only thing keeping the division safe and nothing said so.
    pub fn overall_bitrate(&self) -> u64 {
        self.file_size
            .saturating_mul(8)
            .checked_div(self.duration.as_secs())
            .unwrap_or(0)
    }
}

/// Media file metadata (tags).
#[derive(Debug, Clone, Default)]
pub struct MediaMetadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub year: Option<u32>,
    pub genre: Option<String>,
    pub comment: Option<String>,
    pub encoder: Option<String>,
}

// ============================================================================
// Playback state
// ============================================================================

/// Current playback state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
    Buffering,
    Error,
}

impl PlaybackState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stopped => "Stopped",
            Self::Playing => "Playing",
            Self::Paused => "Paused",
            Self::Buffering => "Buffering...",
            Self::Error => "Error",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Stopped => "[]",
            Self::Playing => ">",
            Self::Paused => "||",
            Self::Buffering => "...",
            Self::Error => "!",
        }
    }
}

/// Playback speed multiplier.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaybackSpeed(f64);

impl PlaybackSpeed {
    pub const QUARTER: Self = Self(0.25);
    pub const HALF: Self = Self(0.5);
    pub const NORMAL: Self = Self(1.0);
    pub const ONE_AND_QUARTER: Self = Self(1.25);
    pub const ONE_AND_HALF: Self = Self(1.5);
    pub const DOUBLE: Self = Self(2.0);
    pub const TRIPLE: Self = Self(3.0);

    pub const PRESETS: &[Self] = &[
        Self::QUARTER,
        Self::HALF,
        Self::NORMAL,
        Self::ONE_AND_QUARTER,
        Self::ONE_AND_HALF,
        Self::DOUBLE,
        Self::TRIPLE,
    ];

    pub fn value(self) -> f64 {
        self.0
    }

    pub fn label(self) -> String {
        if (self.0 - 1.0).abs() < 0.001 {
            "1x".to_string()
        } else {
            format!("{:.2}", self.0)
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string()
                + "x"
        }
    }

    pub fn increase(self) -> Self {
        for preset in Self::PRESETS {
            if preset.0 > self.0 + 0.001 {
                return *preset;
            }
        }
        self
    }

    pub fn decrease(self) -> Self {
        let mut last = Self::QUARTER;
        for preset in Self::PRESETS {
            if preset.0 >= self.0 - 0.001 {
                return last;
            }
            last = *preset;
        }
        last
    }
}

impl Default for PlaybackSpeed {
    fn default() -> Self {
        Self::NORMAL
    }
}

/// Repeat mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatMode {
    Off,
    One,
    All,
}

impl RepeatMode {
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::One,
            Self::One => Self::All,
            Self::All => Self::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Repeat: Off",
            Self::One => "Repeat: One",
            Self::All => "Repeat: All",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Off => "R-",
            Self::One => "R1",
            Self::All => "RA",
        }
    }
}

/// Audio / video synchronization offset.
#[derive(Debug, Clone, Copy)]
pub struct SyncOffset {
    /// Offset in milliseconds. Positive = audio ahead of video.
    pub ms: i64,
}

impl SyncOffset {
    pub const ZERO: Self = Self { ms: 0 };

    pub fn adjust(&mut self, delta_ms: i64) {
        self.ms = self.ms.saturating_add(delta_ms).clamp(-10_000, 10_000);
    }

    pub fn label(&self) -> String {
        match self.ms.cmp(&0) {
            core::cmp::Ordering::Equal => "Sync: 0ms".to_string(),
            core::cmp::Ordering::Greater => format!("Sync: +{}ms", self.ms),
            core::cmp::Ordering::Less => format!("Sync: {}ms", self.ms),
        }
    }
}

impl Default for SyncOffset {
    fn default() -> Self {
        Self::ZERO
    }
}

// ============================================================================
// Volume control
// ============================================================================

/// Volume level (0-150, where 100 is normal and >100 is amplified).
#[derive(Debug, Clone, Copy)]
pub struct Volume {
    level: u32,
    muted: bool,
}

impl Volume {
    pub const MAX: u32 = 150;
    pub const NORMAL: u32 = 100;

    pub fn new(level: u32) -> Self {
        Self {
            level: level.min(Self::MAX),
            muted: false,
        }
    }

    pub fn level(self) -> u32 {
        self.level
    }

    pub fn effective_level(self) -> u32 {
        if self.muted { 0 } else { self.level }
    }

    pub fn set_level(&mut self, level: u32) {
        self.level = level.min(Self::MAX);
    }

    pub fn increase(&mut self, amount: u32) {
        self.level = self.level.saturating_add(amount).min(Self::MAX);
    }

    pub fn decrease(&mut self, amount: u32) {
        self.level = self.level.saturating_sub(amount);
    }

    pub fn toggle_mute(&mut self) {
        self.muted = !self.muted;
    }

    pub fn is_muted(self) -> bool {
        self.muted
    }

    pub fn fraction(self) -> f64 {
        self.level as f64 / Self::MAX as f64
    }

    pub fn icon(self) -> &'static str {
        if self.muted || self.level == 0 {
            "M"
        } else if self.level < 33 {
            "V-"
        } else if self.level < 66 {
            "V="
        } else {
            "V+"
        }
    }

    pub fn label(self) -> String {
        if self.muted {
            "Muted".to_string()
        } else {
            format!("{}%", self.level)
        }
    }
}

impl Default for Volume {
    fn default() -> Self {
        Self::new(Self::NORMAL)
    }
}

// ============================================================================
// Aspect ratio display modes
// ============================================================================

/// How to fit the video in the display area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AspectMode {
    /// Fit video within bounds preserving aspect ratio (letterbox/pillarbox).
    Fit,
    /// Fill the entire area, cropping edges if needed.
    Fill,
    /// Stretch to fill (distorts).
    Stretch,
    /// Original resolution (1:1 pixel mapping).
    Original,
    /// Custom aspect ratio.
    Custom { width: u32, height: u32 },
}

impl AspectMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Fit => "Fit",
            Self::Fill => "Fill (Crop)",
            Self::Stretch => "Stretch",
            Self::Original => "Original",
            Self::Custom { .. } => "Custom",
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            Self::Fit => Self::Fill,
            Self::Fill => Self::Stretch,
            Self::Stretch => Self::Original,
            Self::Original => Self::Fit,
            Self::Custom { .. } => Self::Fit,
        }
    }
}

// ============================================================================
// Playlist
// ============================================================================

/// An entry in the playlist.
#[derive(Debug, Clone)]
pub struct PlaylistEntry {
    pub id: u64,
    pub path: PathBuf,
    pub file_name: String,
    pub duration: Option<Duration>,
    pub title: Option<String>,
}

impl PlaylistEntry {
    pub fn display_name(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.file_name)
    }
}

/// Playlist with shuffle and ordering support.
#[derive(Debug, Clone)]
pub struct Playlist {
    entries: Vec<PlaylistEntry>,
    current_index: Option<usize>,
    next_id: u64,
    shuffle: bool,
    shuffle_order: Vec<usize>,
    shuffle_position: usize,
    /// The stream the shuffle order is drawn from.
    ///
    /// Seeded once when the playlist is created and never reset, so each
    /// rebuild continues the sequence rather than restarting it. That is the
    /// whole point: the previous version derived its seed from the entry IDs,
    /// which do not change, so every rebuild of a given playlist produced the
    /// identical order.
    rng: SeededRng,
}

impl Playlist {
    pub fn new() -> Self {
        Self::with_rng(seeded_from_system(FALLBACK_SEED))
    }

    /// A playlist whose shuffles come from a known seed.
    #[cfg(test)]
    fn with_seed(seed: u64) -> Self {
        Self::with_rng(SeededRng::new(seed))
    }

    fn with_rng(rng: SeededRng) -> Self {
        Self {
            entries: Vec::new(),
            current_index: None,
            next_id: 1,
            shuffle: false,
            shuffle_order: Vec::new(),
            shuffle_position: 0,
            rng,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[PlaylistEntry] {
        &self.entries
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current_index
    }

    pub fn current_entry(&self) -> Option<&PlaylistEntry> {
        self.current_index.and_then(|i| self.entries.get(i))
    }

    pub fn add(
        &mut self,
        path: PathBuf,
        file_name: String,
        duration: Option<Duration>,
        title: Option<String>,
    ) {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.entries.push(PlaylistEntry {
            id,
            path,
            file_name,
            duration,
            title,
        });
        if self.shuffle {
            self.shuffle_order
                .push(self.entries.len().saturating_sub(1));
        }
    }

    pub fn remove(&mut self, index: usize) -> Option<PlaylistEntry> {
        if index >= self.entries.len() {
            return None;
        }
        let entry = self.entries.remove(index);
        // Fix current_index
        match self.current_index {
            Some(ci) if ci == index => {
                if self.entries.is_empty() {
                    self.current_index = None;
                } else {
                    self.current_index = Some(ci.min(self.entries.len().saturating_sub(1)));
                }
            }
            Some(ci) if ci > index => {
                self.current_index = Some(ci.saturating_sub(1));
            }
            _ => {}
        }
        self.rebuild_shuffle_order();
        Some(entry)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.current_index = None;
        self.shuffle_order.clear();
        self.shuffle_position = 0;
    }

    pub fn move_entry(&mut self, from: usize, to: usize) {
        if from >= self.entries.len() || to >= self.entries.len() || from == to {
            return;
        }
        let entry = self.entries.remove(from);
        self.entries.insert(to, entry);
        // Fix current_index
        if let Some(ci) = self.current_index {
            if ci == from {
                self.current_index = Some(to);
            } else if from < ci && ci <= to {
                self.current_index = Some(ci.saturating_sub(1));
            } else if to <= ci && ci < from {
                self.current_index = Some(ci.saturating_add(1));
            }
        }
    }

    pub fn set_current(&mut self, index: usize) {
        if index < self.entries.len() {
            self.current_index = Some(index);
        }
    }

    pub fn next(&mut self, repeat: RepeatMode) -> Option<usize> {
        if self.entries.is_empty() {
            return None;
        }

        if self.shuffle {
            self.shuffle_position = self.shuffle_position.wrapping_add(1);
            if self.shuffle_position >= self.shuffle_order.len() {
                match repeat {
                    RepeatMode::All => {
                        self.rebuild_shuffle_order();
                        self.shuffle_position = 0;
                    }
                    RepeatMode::One => {
                        // Stay at current
                        return self.current_index;
                    }
                    RepeatMode::Off => return None,
                }
            }
            let idx = self
                .shuffle_order
                .get(self.shuffle_position)
                .copied()
                .unwrap_or(0);
            self.current_index = Some(idx);
            return self.current_index;
        }

        match (self.current_index, repeat) {
            (Some(ci), RepeatMode::One) => Some(ci),
            (Some(ci), _) => {
                let next = ci.saturating_add(1);
                if next < self.entries.len() {
                    self.current_index = Some(next);
                    self.current_index
                } else if repeat == RepeatMode::All {
                    self.current_index = Some(0);
                    self.current_index
                } else {
                    None
                }
            }
            (None, _) => {
                if !self.entries.is_empty() {
                    self.current_index = Some(0);
                    self.current_index
                } else {
                    None
                }
            }
        }
    }

    pub fn previous(&mut self) -> Option<usize> {
        if self.entries.is_empty() {
            return None;
        }

        if self.shuffle {
            if self.shuffle_position > 0 {
                self.shuffle_position = self.shuffle_position.saturating_sub(1);
                let idx = self
                    .shuffle_order
                    .get(self.shuffle_position)
                    .copied()
                    .unwrap_or(0);
                self.current_index = Some(idx);
                return self.current_index;
            }
            return self.current_index;
        }

        match self.current_index {
            Some(0) | None => {
                self.current_index = Some(self.entries.len().saturating_sub(1));
                self.current_index
            }
            Some(ci) => {
                self.current_index = Some(ci.saturating_sub(1));
                self.current_index
            }
        }
    }

    pub fn is_shuffle(&self) -> bool {
        self.shuffle
    }

    pub fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        if self.shuffle {
            self.rebuild_shuffle_order();
        }
    }

    pub fn total_duration(&self) -> Duration {
        let mut total = Duration::ZERO;
        for entry in &self.entries {
            if let Some(dur) = entry.duration {
                total = total.saturating_add(dur);
            }
        }
        total
    }

    /// Draw a fresh order for shuffle playback.
    ///
    /// Called both when shuffle is switched on and when a shuffled pass runs
    /// off the end under `RepeatMode::All`, and it has to give a *different*
    /// answer each time or neither call means anything. It did not: the seed
    /// was `12345` mixed with the entry IDs, and the entry IDs do not change,
    /// so ten consecutive rebuilds of one ten-item playlist produced one
    /// distinct order -- measured. Shuffle-with-repeat-all looped the same
    /// permutation forever, which is the exact thing shuffle exists to avoid,
    /// and switching shuffle off and on again replayed the order just heard.
    fn rebuild_shuffle_order(&mut self) {
        self.shuffle_order = (0..self.entries.len()).collect();
        self.rng.shuffle(&mut self.shuffle_order);
        self.shuffle_position = 0;
    }
}

impl Default for Playlist {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Bookmarks (chapter markers / user marks)
// ============================================================================

/// A bookmark/chapter marker in the video.
#[derive(Debug, Clone)]
pub struct Bookmark {
    pub id: u64,
    pub position: Duration,
    pub label: String,
    pub is_chapter: bool,
}

impl Bookmark {
    pub fn display(&self) -> String {
        format!("{} - {}", self.position.format(), self.label)
    }
}

/// Chapter information (from container metadata).
#[derive(Debug, Clone)]
pub struct Chapter {
    pub title: String,
    pub start: Duration,
    pub end: Duration,
}

impl Chapter {
    pub fn duration(&self) -> Duration {
        self.end.saturating_sub(self.start)
    }

    pub fn contains(&self, position: Duration) -> bool {
        position >= self.start && position < self.end
    }
}

// ============================================================================
// Equalizer
// ============================================================================

/// Audio equalizer with predefined and custom presets.
#[derive(Debug, Clone)]
pub struct Equalizer {
    pub enabled: bool,
    pub bands: Vec<EqBand>,
    pub preset: EqPreset,
    pub preamp: f32,
}

/// A single equalizer band.
#[derive(Debug, Clone)]
pub struct EqBand {
    pub frequency: u32,
    pub gain: f32,
    pub label: String,
}

/// Predefined equalizer presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EqPreset {
    Flat,
    Rock,
    Pop,
    Jazz,
    Classical,
    Bass,
    Treble,
    Vocal,
    Movie,
    Custom,
}

impl EqPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Flat => "Flat",
            Self::Rock => "Rock",
            Self::Pop => "Pop",
            Self::Jazz => "Jazz",
            Self::Classical => "Classical",
            Self::Bass => "Bass Boost",
            Self::Treble => "Treble Boost",
            Self::Vocal => "Vocal",
            Self::Movie => "Movie",
            Self::Custom => "Custom",
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::Flat,
            Self::Rock,
            Self::Pop,
            Self::Jazz,
            Self::Classical,
            Self::Bass,
            Self::Treble,
            Self::Vocal,
            Self::Movie,
            Self::Custom,
        ]
    }
}

impl Equalizer {
    pub fn new() -> Self {
        Self {
            enabled: false,
            bands: Self::default_bands(),
            preset: EqPreset::Flat,
            preamp: 0.0,
        }
    }

    fn default_bands() -> Vec<EqBand> {
        let frequencies = [32, 64, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];
        frequencies
            .iter()
            .map(|&f| EqBand {
                frequency: f,
                gain: 0.0,
                label: if f >= 1000 {
                    format!("{}kHz", f / 1000)
                } else {
                    format!("{f}Hz")
                },
            })
            .collect()
    }

    pub fn apply_preset(&mut self, preset: EqPreset) {
        self.preset = preset;
        let gains: &[f32] = match preset {
            EqPreset::Flat => &[0.0; 10],
            EqPreset::Rock => &[5.0, 4.0, 3.0, 1.5, -0.5, -1.0, 1.0, 3.0, 4.0, 5.0],
            EqPreset::Pop => &[-1.0, 2.0, 4.0, 5.0, 3.0, 0.0, -1.0, -1.0, 2.0, 3.0],
            EqPreset::Jazz => &[3.0, 2.0, 1.0, 2.0, -1.0, -1.0, 0.0, 2.0, 3.0, 4.0],
            EqPreset::Classical => &[4.0, 3.0, 2.0, 1.0, -1.0, -1.0, 0.0, 2.0, 3.0, 4.0],
            EqPreset::Bass => &[6.0, 5.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            EqPreset::Treble => &[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 4.0, 5.0, 6.0],
            EqPreset::Vocal => &[-2.0, -1.0, 0.0, 3.0, 5.0, 5.0, 3.0, 1.0, 0.0, -2.0],
            EqPreset::Movie => &[4.0, 3.0, 2.0, 0.0, -2.0, -1.0, 0.0, 2.0, 4.0, 5.0],
            EqPreset::Custom => return,
        };

        for (band, &gain) in self.bands.iter_mut().zip(gains.iter()) {
            band.gain = gain;
        }
    }

    pub fn set_band_gain(&mut self, band_index: usize, gain: f32) {
        if let Some(band) = self.bands.get_mut(band_index) {
            band.gain = gain.clamp(-12.0, 12.0);
            self.preset = EqPreset::Custom;
        }
    }

    pub fn reset(&mut self) {
        self.apply_preset(EqPreset::Flat);
        self.preamp = 0.0;
    }
}

impl Default for Equalizer {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Video filters / adjustments
// ============================================================================

/// Video image adjustments.
#[derive(Debug, Clone)]
pub struct VideoAdjustments {
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub hue: f32,
    pub gamma: f32,
    pub sharpness: f32,
}

impl VideoAdjustments {
    pub fn is_default(&self) -> bool {
        (self.brightness - 0.0).abs() < 0.01
            && (self.contrast - 1.0).abs() < 0.01
            && (self.saturation - 1.0).abs() < 0.01
            && (self.hue - 0.0).abs() < 0.01
            && (self.gamma - 1.0).abs() < 0.01
            && (self.sharpness - 0.0).abs() < 0.01
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

impl Default for VideoAdjustments {
    fn default() -> Self {
        Self {
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            hue: 0.0,
            gamma: 1.0,
            sharpness: 0.0,
        }
    }
}

/// Deinterlace mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeinterlaceMode {
    Off,
    Blend,
    Bob,
    Yadif,
    Auto,
}

impl DeinterlaceMode {
    /// The next mode, wrapping. `Auto` is last so the cycle returns to `Off`.
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Blend,
            Self::Blend => Self::Bob,
            Self::Bob => Self::Yadif,
            Self::Yadif => Self::Auto,
            Self::Auto => Self::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Blend => "Blend",
            Self::Bob => "Bob",
            Self::Yadif => "Yadif",
            Self::Auto => "Auto",
        }
    }
}

// ============================================================================
// Keyboard shortcuts
// ============================================================================

/// All keyboard shortcuts for the video player.
/// What a shortcut does when its keys are pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    TogglePlayPause,
    Stop,
    ToggleFullscreen,
    ToggleMute,
    VolumeUp,
    VolumeDown,
    /// Move by this many milliseconds; negative goes back.
    SeekBy(i64),
    /// Jump to the tenth of the file named by the digit that was pressed.
    SeekToDigit,
    PlaylistNext,
    PlaylistPrevious,
    SpeedUp,
    SpeedDown,
    SpeedReset,
    CycleAspect,
    CycleSubtitleTrack,
    CycleAudioTrack,
    ToggleSubtitles,
    ToggleChapterList,
    TogglePlaylist,
    ToggleInfo,
    ToggleEqualizer,
    /// Shift the subtitles against the picture by this many milliseconds.
    SubtitleDelay(i64),
    /// Shift the sound against the picture by this many milliseconds.
    AudioSync(i64),
    AddBookmark,
    /// Step the repeat mode: off, one, all.
    CycleRepeat,
    /// Turn the equalizer's processing on or off.
    ToggleEqualizerEnabled,
    /// Change the Settings row the cursor is on.
    ChangeSetting,
    /// Put up the file picker to open a video.
    Open,
}

/// Which keystroke runs a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    /// This key with no modifier held.
    Plain(Key),
    /// This key with shift.
    Shift(Key),
    /// This key with control.
    Ctrl(Key),
    /// Any of `Num0`..=`Num9`, the digit being the argument.
    Digit,
}

impl Press {
    /// Does this keystroke run it?
    ///
    /// `Plain` insists the modifiers are *clear* rather than merely ignoring
    /// them, so `Shift+Right` cannot also be `Right`. That makes the table's
    /// order irrelevant, which is what stops a row added at the top from
    /// silently shadowing one below it.
    pub fn matches(self, event: &KeyEvent) -> bool {
        let mods = event.modifiers;
        match self {
            Self::Plain(key) => event.key == key && !mods.shift && !mods.ctrl && !mods.alt,
            Self::Shift(key) => event.key == key && mods.shift && !mods.ctrl && !mods.alt,
            Self::Ctrl(key) => event.key == key && mods.ctrl && !mods.alt,
            Self::Digit => Self::digit_of(event.key).is_some() && !mods.ctrl && !mods.alt,
        }
    }

    /// The digit a key names, if it names one.
    pub fn digit_of(key: Key) -> Option<u32> {
        Some(match key {
            Key::Num0 => 0,
            Key::Num1 => 1,
            Key::Num2 => 2,
            Key::Num3 => 3,
            Key::Num4 => 4,
            Key::Num5 => 5,
            Key::Num6 => 6,
            Key::Num7 => 7,
            Key::Num8 => 8,
            Key::Num9 => 9,
            _ => return None,
        })
    }
}

/// One row of the keymap: what to press, what it says it does, and what it
/// actually does.
///
/// These were three separate things, of which only two existed. `Shortcuts`
/// was a `Vec<(&str, &str)>` -- a keys column and an action column, drawn in
/// its own tab -- and the program had no key handler at all, so every one of
/// the thirty-two rows was a promise nothing kept. Holding the dispatch in the
/// same row as the text is what makes that impossible to repeat: a shortcut
/// cannot be documented without being bound, or bound without being
/// documented.
#[derive(Clone, Copy, Debug)]
pub struct Shortcut {
    /// The keys, as the help panel prints them.
    pub keys: &'static str,
    /// What it does, as the help panel prints it.
    pub action: &'static str,
    /// The keystroke that runs it.
    pub press: Press,
    /// What running it does.
    pub command: Command,
}

pub struct Shortcuts;

impl Shortcuts {
    /// Every shortcut the player has.
    ///
    /// `Ctrl+S` (take a screenshot) was listed here and is not: no frame is
    /// decoded, so there is nothing to take, and a help panel that promises
    /// what the program cannot do is the same defect one level up. `Ctrl+O`
    /// was removed for want of a file chooser and is back: the toolkit has
    /// one, and a file opens to be read for what it holds.
    pub const fn list() -> &'static [Shortcut] {
        const fn sc(
            keys: &'static str,
            action: &'static str,
            press: Press,
            command: Command,
        ) -> Shortcut {
            Shortcut {
                keys,
                action,
                press,
                command,
            }
        }
        static TABLE: &[Shortcut] = &[
            sc("Ctrl+O", "Open a File", Press::Ctrl(Key::O), Command::Open),
            sc(
                "Space",
                "Play / Pause",
                Press::Plain(Key::Space),
                Command::TogglePlayPause,
            ),
            sc("S", "Stop", Press::Plain(Key::S), Command::Stop),
            sc(
                "F",
                "Toggle Fullscreen",
                Press::Plain(Key::F),
                Command::ToggleFullscreen,
            ),
            sc(
                "M",
                "Toggle Mute",
                Press::Plain(Key::M),
                Command::ToggleMute,
            ),
            sc(
                "Up",
                "Volume Up / Settings Row Up",
                Press::Plain(Key::Up),
                Command::VolumeUp,
            ),
            sc(
                "Down",
                "Volume Down / Settings Row Down",
                Press::Plain(Key::Down),
                Command::VolumeDown,
            ),
            sc(
                "Right",
                "Seek Forward 10s",
                Press::Plain(Key::Right),
                Command::SeekBy(10_000),
            ),
            sc(
                "Left",
                "Seek Backward 10s",
                Press::Plain(Key::Left),
                Command::SeekBy(-10_000),
            ),
            sc(
                "Shift+Right",
                "Seek Forward 60s",
                Press::Shift(Key::Right),
                Command::SeekBy(60_000),
            ),
            sc(
                "Shift+Left",
                "Seek Backward 60s",
                Press::Shift(Key::Left),
                Command::SeekBy(-60_000),
            ),
            sc(
                "N",
                "Next in Playlist",
                Press::Plain(Key::N),
                Command::PlaylistNext,
            ),
            sc(
                "P",
                "Previous in Playlist",
                Press::Plain(Key::P),
                Command::PlaylistPrevious,
            ),
            sc(
                "[",
                "Decrease Speed",
                Press::Plain(Key::LeftBracket),
                Command::SpeedDown,
            ),
            sc(
                "]",
                "Increase Speed",
                Press::Plain(Key::RightBracket),
                Command::SpeedUp,
            ),
            sc(
                "\\",
                "Reset Speed",
                Press::Plain(Key::Backslash),
                Command::SpeedReset,
            ),
            sc(
                "A",
                "Cycle Aspect Ratio",
                Press::Plain(Key::A),
                Command::CycleAspect,
            ),
            sc(
                "V",
                "Cycle Subtitles",
                Press::Plain(Key::V),
                Command::CycleSubtitleTrack,
            ),
            sc(
                "B",
                "Cycle Audio Track",
                Press::Plain(Key::B),
                Command::CycleAudioTrack,
            ),
            sc(
                "T",
                "Toggle Subtitle",
                Press::Plain(Key::T),
                Command::ToggleSubtitles,
            ),
            sc(
                "C",
                "Toggle Chapter List",
                Press::Plain(Key::C),
                Command::ToggleChapterList,
            ),
            sc(
                "L",
                "Toggle Playlist",
                Press::Plain(Key::L),
                Command::TogglePlaylist,
            ),
            sc(
                "I",
                "Show Media Info",
                Press::Plain(Key::I),
                Command::ToggleInfo,
            ),
            sc(
                "E",
                "Toggle Equalizer",
                Press::Plain(Key::E),
                Command::ToggleEqualizer,
            ),
            sc(
                "J",
                "Subtitle Delay -100ms",
                Press::Plain(Key::J),
                Command::SubtitleDelay(-100),
            ),
            sc(
                "K",
                "Subtitle Delay +100ms",
                Press::Plain(Key::K),
                Command::SubtitleDelay(100),
            ),
            sc(
                "G",
                "Audio Sync -100ms",
                Press::Plain(Key::G),
                Command::AudioSync(-100),
            ),
            sc(
                "H",
                "Audio Sync +100ms",
                Press::Plain(Key::H),
                Command::AudioSync(100),
            ),
            sc(
                "Ctrl+B",
                "Add Bookmark",
                Press::Ctrl(Key::B),
                Command::AddBookmark,
            ),
            sc(
                "R",
                "Cycle Repeat Mode",
                Press::Plain(Key::R),
                Command::CycleRepeat,
            ),
            sc(
                "Shift+E",
                "Equalizer On / Off",
                Press::Shift(Key::E),
                Command::ToggleEqualizerEnabled,
            ),
            sc(
                "Enter",
                "Change the Selected Setting",
                Press::Plain(Key::Enter),
                Command::ChangeSetting,
            ),
            sc("0-9", "Seek to 0%-90%", Press::Digit, Command::SeekToDigit),
            // The keyboard's own media keys, which a player should honour and
            // which cost nothing to bind now that there is one table to bind
            // them in.
            sc(
                "Play/Pause",
                "Play / Pause",
                Press::Plain(Key::MediaPlayPause),
                Command::TogglePlayPause,
            ),
            sc("Stop", "Stop", Press::Plain(Key::MediaStop), Command::Stop),
            sc(
                "Next Track",
                "Next in Playlist",
                Press::Plain(Key::MediaNextTrack),
                Command::PlaylistNext,
            ),
            sc(
                "Prev Track",
                "Previous in Playlist",
                Press::Plain(Key::MediaPrevTrack),
                Command::PlaylistPrevious,
            ),
            sc(
                "Vol Up",
                "Volume Up",
                Press::Plain(Key::VolumeUp),
                Command::VolumeUp,
            ),
            sc(
                "Vol Down",
                "Volume Down",
                Press::Plain(Key::VolumeDown),
                Command::VolumeDown,
            ),
            sc(
                "Mute",
                "Toggle Mute",
                Press::Plain(Key::VolumeMute),
                Command::ToggleMute,
            ),
        ];
        TABLE
    }

    /// The shortcut a keystroke runs, if any.
    pub fn for_event(event: &KeyEvent) -> Option<&'static Shortcut> {
        Self::list().iter().find(|sc| sc.press.matches(event))
    }
}

// ============================================================================
// Screenshot capture
// ============================================================================

/// Screenshot configuration.
#[derive(Debug, Clone)]
pub struct ScreenshotConfig {
    pub format: ScreenshotFormat,
    pub include_subtitles: bool,
    pub save_directory: String,
    pub quality: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenshotFormat {
    Png,
    Jpeg,
    Bmp,
}

impl ScreenshotFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Bmp => "bmp",
        }
    }
}

impl Default for ScreenshotConfig {
    fn default() -> Self {
        Self {
            format: ScreenshotFormat::Png,
            include_subtitles: true,
            save_directory: "/home/user/Pictures/Screenshots".to_string(),
            quality: 95,
        }
    }
}

/// A file's name as the window shows it: its bytes as text, with every byte
/// that is not UTF-8 written as `\xNN`.
///
/// For showing only -- the path is kept whole beside it, and that is what is
/// opened. A lossy decode would show both of two names that differ only in
/// such a byte as the same `\u{FFFD}`, which is the name a user would then
/// hunt for and not find; this shows each as it is.
fn shown_name(path: &Path) -> String {
    use std::fmt::Write as _;
    let name = path.file_name().unwrap_or(path.as_os_str());
    let mut out = String::new();
    for chunk in name.as_encoded_bytes().utf8_chunks() {
        out.push_str(chunk.valid());
        for byte in chunk.invalid() {
            // Writing to a String cannot fail.
            let _ = write!(out, "\\x{byte:02X}");
        }
    }
    out
}

// ============================================================================
// Recent files
// ============================================================================

/// A recently opened file.
#[derive(Debug, Clone)]
pub struct RecentFile {
    pub path: PathBuf,
    pub file_name: String,
    pub last_position: Duration,
    pub last_opened_timestamp: u64,
    pub duration: Option<Duration>,
}

impl RecentFile {
    pub fn resume_label(&self) -> String {
        if self.last_position.as_secs() > 0 {
            format!("Resume from {}", self.last_position.format())
        } else {
            "Play from start".to_string()
        }
    }

    pub fn progress_fraction(&self) -> f64 {
        match self.duration {
            Some(dur) => self.last_position.progress_of(dur),
            None => 0.0,
        }
    }
}

/// Recent file history (bounded).
#[derive(Debug, Clone)]
pub struct RecentHistory {
    files: Vec<RecentFile>,
    max_entries: usize,
}

impl RecentHistory {
    pub fn new(max_entries: usize) -> Self {
        Self {
            files: Vec::new(),
            max_entries,
        }
    }

    pub fn add(
        &mut self,
        path: PathBuf,
        file_name: String,
        position: Duration,
        timestamp: u64,
        duration: Option<Duration>,
    ) {
        // Remove existing entry for this path
        self.files.retain(|f| f.path != path);

        self.files.insert(
            0,
            RecentFile {
                path,
                file_name,
                last_position: position,
                last_opened_timestamp: timestamp,
                duration,
            },
        );

        if self.files.len() > self.max_entries {
            self.files.truncate(self.max_entries);
        }
    }

    pub fn files(&self) -> &[RecentFile] {
        &self.files
    }

    pub fn clear(&mut self) {
        self.files.clear();
    }

    pub fn find_by_path(&self, path: &Path) -> Option<&RecentFile> {
        self.files.iter().find(|f| f.path == path)
    }
}

impl Default for RecentHistory {
    fn default() -> Self {
        Self::new(50)
    }
}

// ============================================================================
// Player settings / preferences
// ============================================================================

/// Player behavior settings.
#[derive(Debug, Clone)]
pub struct PlayerPreferences {
    pub resume_playback: bool,
    pub remember_volume: bool,
    pub default_volume: u32,
    pub hardware_decode: bool,
    pub subtitle_auto_load: bool,
    pub subtitle_preferred_lang: Option<String>,
    pub audio_preferred_lang: Option<String>,
    pub on_finish: OnFinishAction,
    pub osd_duration_ms: u64,
    pub seek_small_step: u64,
    pub seek_large_step: u64,
    pub screenshot_config: ScreenshotConfig,
    pub deinterlace: DeinterlaceMode,
}

/// What to do when playback finishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnFinishAction {
    DoNothing,
    PlayNext,
    RepeatFile,
    ExitFullscreen,
    Quit,
}

impl OnFinishAction {
    /// The next action, wrapping.
    pub fn next(self) -> Self {
        match self {
            Self::DoNothing => Self::PlayNext,
            Self::PlayNext => Self::RepeatFile,
            Self::RepeatFile => Self::ExitFullscreen,
            Self::ExitFullscreen => Self::Quit,
            Self::Quit => Self::DoNothing,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::DoNothing => "Do Nothing",
            Self::PlayNext => "Play Next",
            Self::RepeatFile => "Repeat File",
            Self::ExitFullscreen => "Exit Fullscreen",
            Self::Quit => "Quit",
        }
    }
}

impl Default for PlayerPreferences {
    fn default() -> Self {
        Self {
            resume_playback: true,
            remember_volume: true,
            default_volume: 100,
            hardware_decode: true,
            subtitle_auto_load: true,
            subtitle_preferred_lang: Some("eng".to_string()),
            audio_preferred_lang: None,
            on_finish: OnFinishAction::PlayNext,
            osd_duration_ms: 2000,
            seek_small_step: 10_000,
            seek_large_step: 60_000,
            screenshot_config: ScreenshotConfig::default(),
            deinterlace: DeinterlaceMode::Auto,
        }
    }
}

/// One row of the Settings tab: what it is called, what it reads, what
/// changing it does.
///
/// The panel used to build a `[(&str, &str); 6]` of labels and rendered
/// values inside `render_settings`, and nothing anywhere could change one of
/// them. Six preferences were drawn as "On"/"Off" for a user who had no way
/// to make any of them read the other word -- `hardware_decode` was `true`
/// for everybody, `on_finish` was `PlayNext` for everybody.
///
/// Making the row an enum with a `cycle` is what stops that recurring: the
/// renderer walks this list, the cursor indexes this list, and Enter calls
/// `cycle` on whatever it lands on. A row cannot be drawn without being
/// changeable, because drawing it means being in this list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingRow {
    ResumePlayback,
    RememberVolume,
    HardwareDecode,
    SubtitleAutoLoad,
    OnFinish,
    Deinterlace,
}

impl SettingRow {
    /// Every row, in the order the panel draws them.
    pub const ALL: [SettingRow; 6] = [
        Self::ResumePlayback,
        Self::RememberVolume,
        Self::HardwareDecode,
        Self::SubtitleAutoLoad,
        Self::OnFinish,
        Self::Deinterlace,
    ];

    /// The name in the left column.
    pub fn label(self) -> &'static str {
        match self {
            Self::ResumePlayback => "Resume Playback",
            Self::RememberVolume => "Remember Volume",
            Self::HardwareDecode => "Hardware Decode",
            Self::SubtitleAutoLoad => "Auto-load Subtitles",
            Self::OnFinish => "On Finish",
            Self::Deinterlace => "Deinterlace",
        }
    }

    /// The value in the right column, as the panel prints it.
    pub fn value(self, prefs: &PlayerPreferences) -> &'static str {
        fn on_off(flag: bool) -> &'static str {
            if flag { "On" } else { "Off" }
        }
        match self {
            Self::ResumePlayback => on_off(prefs.resume_playback),
            Self::RememberVolume => on_off(prefs.remember_volume),
            Self::HardwareDecode => on_off(prefs.hardware_decode),
            Self::SubtitleAutoLoad => on_off(prefs.subtitle_auto_load),
            Self::OnFinish => prefs.on_finish.label(),
            Self::Deinterlace => prefs.deinterlace.label(),
        }
    }

    /// Move this row to its next value. Booleans flip; lists step and wrap.
    pub fn cycle(self, prefs: &mut PlayerPreferences) {
        match self {
            Self::ResumePlayback => prefs.resume_playback = !prefs.resume_playback,
            Self::RememberVolume => prefs.remember_volume = !prefs.remember_volume,
            Self::HardwareDecode => prefs.hardware_decode = !prefs.hardware_decode,
            Self::SubtitleAutoLoad => prefs.subtitle_auto_load = !prefs.subtitle_auto_load,
            Self::OnFinish => prefs.on_finish = prefs.on_finish.next(),
            Self::Deinterlace => prefs.deinterlace = prefs.deinterlace.next(),
        }
    }
}

// ============================================================================
// Utility functions
// ============================================================================

fn format_bytes(bytes: u64) -> String {
    guitk::bytes::iec(bytes)
}

fn format_bitrate(bps: u64) -> String {
    const KBPS: u64 = 1000;
    const MBPS: u64 = KBPS * 1000;

    if bps >= MBPS {
        format!("{:.1} Mbps", bps as f64 / MBPS as f64)
    } else if bps >= KBPS {
        format!("{} kbps", bps / KBPS)
    } else {
        format!("{bps} bps")
    }
}

// ============================================================================
// Main application state
// ============================================================================

/// The video player application.
pub struct VideoPlayerApp {
    // Window
    pub width: f32,
    pub height: f32,
    pub fullscreen: bool,
    pub controls_visible: bool,
    pub controls_hide_timer: u64,

    // Playback
    pub state: PlaybackState,
    pub current_file: Option<MediaFile>,
    pub position: Duration,
    pub speed: PlaybackSpeed,
    pub volume: Volume,
    pub repeat: RepeatMode,
    pub aspect_mode: AspectMode,

    // Tracks
    pub selected_audio_track: Option<u32>,
    pub selected_subtitle_track: Option<u32>,
    pub subtitle_enabled: bool,
    pub subtitle_delay: SyncOffset,
    pub audio_sync: SyncOffset,

    // Subtitles (external loaded)
    pub external_subtitles: Vec<SubtitleCue>,

    // Playlist
    pub playlist: Playlist,
    pub playlist_visible: bool,
    pub playlist_scroll: f32,

    // Chapters & bookmarks
    pub chapters: Vec<Chapter>,
    pub bookmarks: Vec<Bookmark>,
    pub next_bookmark_id: u64,
    pub chapter_list_visible: bool,

    // Equalizer
    pub equalizer: Equalizer,
    pub equalizer_visible: bool,

    // Video adjustments
    pub video_adjustments: VideoAdjustments,
    pub adjustments_visible: bool,

    // Media info dialog
    pub info_visible: bool,

    // Seek bar
    pub seeking: bool,
    pub seek_preview_position: Option<Duration>,

    // Recent files
    pub recent: RecentHistory,

    // Preferences
    pub preferences: PlayerPreferences,

    // Active tab in settings
    pub active_tab: PlayerTab,
    /// Which row of the Settings tab the cursor is on.
    pub settings_row: usize,

    // OSD messages
    pub osd_message: Option<String>,
    /// How much longer the message stays up, in milliseconds.
    ///
    /// A countdown rather than a timestamp: this was `osd_timestamp: u64`,
    /// set to `0` with the comment "would be set to current time in real
    /// impl", and nothing ever read it or `osd_duration_ms` beside it. The
    /// message therefore never expired -- turn the volume up once and
    /// "Volume: 80" stayed on the picture for the rest of the film. The player
    /// has no wall clock and does not need one; the tick already knows how
    /// long it has been.
    pub osd_remaining_ms: u64,
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
    /// Whether anything here turns a file into pictures and sound. Nothing
    /// does: there is no decoder. The transport -- play, the clock, the end of
    /// a file, repeat -- is written and tested for the day one exists; until
    /// then `play` says why it will not, rather than running a clock over a
    /// black picture and calling that playing.
    pub decodes: bool,
    /// The picker Ctrl+O puts up.
    pub picker: FilePicker,
}

/// UI tabs/panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerTab {
    Player,
    Playlist,
    MediaInfo,
    Equalizer,
    Adjustments,
    Settings,
    Shortcuts,
}

impl PlayerTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::Player => "Player",
            Self::Playlist => "Playlist",
            Self::MediaInfo => "Media Info",
            Self::Equalizer => "Equalizer",
            Self::Adjustments => "Adjustments",
            Self::Settings => "Settings",
            Self::Shortcuts => "Shortcuts",
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::Player,
            Self::Playlist,
            Self::MediaInfo,
            Self::Equalizer,
            Self::Adjustments,
            Self::Settings,
            Self::Shortcuts,
        ]
    }
}

impl VideoPlayerApp {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            width,
            height,
            fullscreen: false,
            controls_visible: true,
            controls_hide_timer: 0,
            state: PlaybackState::Stopped,
            current_file: None,
            position: Duration::ZERO,
            speed: PlaybackSpeed::default(),
            volume: Volume::default(),
            repeat: RepeatMode::Off,
            aspect_mode: AspectMode::Fit,
            selected_audio_track: None,
            selected_subtitle_track: None,
            subtitle_enabled: true,
            subtitle_delay: SyncOffset::default(),
            audio_sync: SyncOffset::default(),
            external_subtitles: Vec::new(),
            playlist: Playlist::new(),
            playlist_visible: false,
            playlist_scroll: 0.0,
            chapters: Vec::new(),
            bookmarks: Vec::new(),
            next_bookmark_id: 1,
            chapter_list_visible: false,
            equalizer: Equalizer::new(),
            equalizer_visible: false,
            video_adjustments: VideoAdjustments::default(),
            adjustments_visible: false,
            info_visible: false,
            seeking: false,
            seek_preview_position: None,
            recent: RecentHistory::default(),
            preferences: PlayerPreferences::default(),
            active_tab: PlayerTab::Player,
            settings_row: 0,
            osd_message: None,
            osd_remaining_ms: 0,
            decodes: false,
            picker: FilePicker::default(),
        }
    }

    // ========================================================================
    // Playback control
    // ========================================================================

    pub fn play(&mut self) {
        if self.current_file.is_none() {
            return;
        }
        if !self.decodes {
            self.show_osd(CANNOT_DECODE);
            return;
        }
        self.state = PlaybackState::Playing;
        self.show_osd("Play");
    }

    pub fn pause(&mut self) {
        if self.state == PlaybackState::Playing {
            self.state = PlaybackState::Paused;
            self.show_osd("Paused");
        }
    }

    pub fn toggle_play_pause(&mut self) {
        match self.state {
            PlaybackState::Playing => self.pause(),
            PlaybackState::Paused => self.play(),
            PlaybackState::Stopped if self.current_file.is_some() => {
                self.play();
            }
            _ => {}
        }
    }

    pub fn stop(&mut self) {
        self.state = PlaybackState::Stopped;
        self.position = Duration::ZERO;
        self.show_osd("Stopped");
    }

    pub fn seek_to(&mut self, position: Duration) {
        if let Some(file) = &self.current_file {
            let max_pos = file.duration;
            self.position = if position > max_pos {
                max_pos
            } else {
                position
            };
        }
    }

    pub fn seek_forward(&mut self, ms: u64) {
        let target = self.position.saturating_add(Duration::from_millis(ms));
        self.seek_to(target);
        let secs = ms / 1000;
        self.show_osd(&format!(">> +{secs}s"));
    }

    pub fn seek_backward(&mut self, ms: u64) {
        let target = self.position.saturating_sub(Duration::from_millis(ms));
        self.seek_to(target);
        let secs = ms / 1000;
        self.show_osd(&format!("<< -{secs}s"));
    }

    pub fn seek_to_fraction(&mut self, fraction: f64) {
        if let Some(file) = &self.current_file {
            let target_ms = (file.duration.as_millis() as f64 * fraction.clamp(0.0, 1.0)) as u64;
            self.position = Duration::from_millis(target_ms);
        }
    }

    pub fn seek_to_chapter(&mut self, index: usize) {
        if let Some(chapter) = self.chapters.get(index) {
            self.position = chapter.start;
            self.show_osd(&format!("Chapter: {}", chapter.title));
        }
    }

    pub fn current_chapter(&self) -> Option<(usize, &Chapter)> {
        self.chapters
            .iter()
            .enumerate()
            .find(|(_, ch)| ch.contains(self.position))
    }

    pub fn next_chapter(&mut self) {
        if let Some((idx, _)) = self.current_chapter()
            && idx.saturating_add(1) < self.chapters.len()
        {
            self.seek_to_chapter(idx.saturating_add(1));
        }
    }

    pub fn previous_chapter(&mut self) {
        if let Some((idx, ch)) = self.current_chapter() {
            // If we're more than 3 seconds into the chapter, go to its start
            if self.position.saturating_sub(ch.start).as_secs() > 3 {
                self.position = ch.start;
            } else if idx > 0 {
                self.seek_to_chapter(idx.saturating_sub(1));
            }
        }
    }

    // ========================================================================
    // Track selection
    // ========================================================================

    pub fn cycle_audio_track(&mut self) {
        if let Some(file) = &self.current_file {
            if file.audio_streams.is_empty() {
                return;
            }
            let next = match self.selected_audio_track {
                None => file.audio_streams.first().map(|s| s.index),
                Some(current) => {
                    let pos = file.audio_streams.iter().position(|s| s.index == current);
                    match pos {
                        Some(p) if p.saturating_add(1) < file.audio_streams.len() => {
                            file.audio_streams.get(p.saturating_add(1)).map(|s| s.index)
                        }
                        _ => file.audio_streams.first().map(|s| s.index),
                    }
                }
            };
            self.selected_audio_track = next;
            if let Some(idx) = next
                && let Some(stream) = file.audio_streams.iter().find(|s| s.index == idx)
            {
                self.show_osd(&format!("Audio: {}", stream.display_label()));
            }
        }
    }

    pub fn cycle_subtitle_track(&mut self) {
        if let Some(file) = &self.current_file {
            if file.subtitle_streams.is_empty() {
                self.subtitle_enabled = false;
                return;
            }

            match self.selected_subtitle_track {
                None => {
                    self.selected_subtitle_track = file.subtitle_streams.first().map(|s| s.index);
                    self.subtitle_enabled = true;
                }
                Some(current) => {
                    let pos = file
                        .subtitle_streams
                        .iter()
                        .position(|s| s.index == current);
                    match pos {
                        Some(p) if p.saturating_add(1) < file.subtitle_streams.len() => {
                            self.selected_subtitle_track = file
                                .subtitle_streams
                                .get(p.saturating_add(1))
                                .map(|s| s.index);
                            self.subtitle_enabled = true;
                        }
                        _ => {
                            // Wrap around to "off"
                            self.selected_subtitle_track = None;
                            self.subtitle_enabled = false;
                        }
                    }
                }
            }

            if self.subtitle_enabled {
                if let Some(idx) = self.selected_subtitle_track
                    && let Some(stream) = file.subtitle_streams.iter().find(|s| s.index == idx)
                {
                    self.show_osd(&format!("Subtitle: {}", stream.display_label()));
                }
            } else {
                self.show_osd("Subtitles: Off");
            }
        }
    }

    pub fn toggle_subtitles(&mut self) {
        self.subtitle_enabled = !self.subtitle_enabled;
        self.show_osd(if self.subtitle_enabled {
            "Subtitles: On"
        } else {
            "Subtitles: Off"
        });
    }

    // ========================================================================
    // Speed control
    // ========================================================================

    pub fn increase_speed(&mut self) {
        self.speed = self.speed.increase();
        self.show_osd(&format!("Speed: {}", self.speed.label()));
    }

    pub fn decrease_speed(&mut self) {
        self.speed = self.speed.decrease();
        self.show_osd(&format!("Speed: {}", self.speed.label()));
    }

    pub fn reset_speed(&mut self) {
        self.speed = PlaybackSpeed::NORMAL;
        self.show_osd("Speed: 1x");
    }

    // ========================================================================
    // Volume
    // ========================================================================

    pub fn volume_up(&mut self) {
        self.volume.increase(5);
        self.show_osd(&format!("Volume: {}", self.volume.label()));
    }

    pub fn volume_down(&mut self) {
        self.volume.decrease(5);
        self.show_osd(&format!("Volume: {}", self.volume.label()));
    }

    pub fn toggle_mute(&mut self) {
        self.volume.toggle_mute();
        let msg = if self.volume.is_muted() {
            "Muted".to_string()
        } else {
            format!("Volume: {}", self.volume.label())
        };
        self.show_osd(&msg);
    }

    // ========================================================================
    // Bookmarks
    // ========================================================================

    pub fn add_bookmark(&mut self, label: String) {
        let id = self.next_bookmark_id;
        self.next_bookmark_id = self.next_bookmark_id.wrapping_add(1);
        self.bookmarks.push(Bookmark {
            id,
            position: self.position,
            label: label.clone(),
            is_chapter: false,
        });
        self.bookmarks.sort_by_key(|b| b.position);
        self.show_osd(&format!("Bookmark added: {label}"));
    }

    pub fn remove_bookmark(&mut self, id: u64) {
        self.bookmarks.retain(|b| b.id != id);
    }

    pub fn seek_to_bookmark(&mut self, id: u64) {
        if let Some(bm) = self.bookmarks.iter().find(|b| b.id == id) {
            let pos = bm.position;
            let label = bm.label.clone();
            self.seek_to(pos);
            self.show_osd(&format!("Bookmark: {label}"));
        }
    }

    // ========================================================================
    // Playlist control
    // ========================================================================

    pub fn playlist_next(&mut self) {
        if let Some(idx) = self.playlist.next(self.repeat) {
            self.load_playlist_entry(idx);
        }
    }

    pub fn playlist_previous(&mut self) {
        if let Some(idx) = self.playlist.previous() {
            self.load_playlist_entry(idx);
        }
    }

    /// Open the playlist's entry `index`: its file is read again, since it
    /// may have changed or gone since it was listed. It said "Now playing"
    /// and set the clock running without opening anything -- the picture,
    /// the Media Info tab and the length all stayed the previous file's.
    fn load_playlist_entry(&mut self, index: usize) {
        let Some(entry) = self.playlist.entries().get(index) else {
            return;
        };
        let path = entry.path.clone();
        match MediaFile::open(&path) {
            Ok(file) => {
                let name = file.file_name.clone();
                self.show_file(file);
                if self.decodes {
                    self.state = PlaybackState::Playing;
                    self.show_osd(&format!("Now playing: {name}"));
                } else {
                    self.show_osd(&format!("Opened {name}"));
                }
            }
            Err(why) => {
                self.current_file = None;
                self.state = PlaybackState::Stopped;
                self.position = Duration::ZERO;
                self.show_osd(&why);
            }
        }
    }

    /// Make `file` the one on screen: at its start, stopped, its own default
    /// sound and subtitles chosen, and nothing of the last file's -- its
    /// chapters or loaded subtitles -- carried over.
    fn show_file(&mut self, file: MediaFile) {
        self.selected_audio_track = file.primary_audio().map(|a| a.index);
        self.selected_subtitle_track = file
            .subtitle_streams
            .iter()
            .find(|s| s.is_forced || s.is_default)
            .map(|s| s.index);
        self.current_file = Some(file);
        self.position = Duration::ZERO;
        self.state = PlaybackState::Stopped;
        self.chapters.clear();
        self.external_subtitles.clear();
    }

    /// Read the video at `path` into the playlist, after what is there.
    ///
    /// # Errors
    ///
    /// Why the file could not be read, fit to show.
    pub fn add_path(&mut self, path: &Path) -> Result<usize, String> {
        let file = MediaFile::open(path)?;
        let title = file.metadata.title.clone();
        let duration = (file.duration > Duration::ZERO).then_some(file.duration);
        self.playlist
            .add(path.to_path_buf(), file.file_name.clone(), duration, title);
        Ok(self.playlist.len().saturating_sub(1))
    }

    /// Open the video at `path`: listed, made the current entry, and shown.
    /// Returns what happened, which is also what the window says.
    pub fn open_path(&mut self, path: &Path) -> String {
        match self.add_path(path) {
            Ok(index) => {
                self.playlist.set_current(index);
                self.load_playlist_entry(index);
                self.active_tab = PlayerTab::Player;
                self.osd_message.clone().unwrap_or_default()
            }
            Err(why) => {
                self.show_osd(&why);
                why
            }
        }
    }

    // ========================================================================
    // OSD
    // ========================================================================

    fn show_osd(&mut self, message: &str) {
        self.osd_message = Some(message.to_string());
        self.osd_remaining_ms = self.preferences.osd_duration_ms;
    }

    // ========================================================================
    // Fullscreen
    // ========================================================================

    pub fn toggle_fullscreen(&mut self) {
        self.fullscreen = !self.fullscreen;
    }

    // ========================================================================
    // Progress display
    // ========================================================================

    pub fn progress_fraction(&self) -> f64 {
        match &self.current_file {
            Some(file) => self.position.progress_of(file.duration),
            None => 0.0,
        }
    }

    pub fn remaining_duration(&self) -> Duration {
        match &self.current_file {
            Some(file) => file.duration.saturating_sub(self.position),
            None => Duration::ZERO,
        }
    }

    pub fn time_display(&self) -> String {
        match &self.current_file {
            Some(file) => format!("{} / {}", self.position.format(), file.duration.format()),
            None => "--:-- / --:--".to_string(),
        }
    }

    // ========================================================================
    // Active subtitle cue lookup
    // ========================================================================

    pub fn active_subtitle_cue(&self) -> Option<&SubtitleCue> {
        let adjusted_pos = if self.subtitle_delay.ms >= 0 {
            self.position
                .saturating_add(Duration::from_millis(self.subtitle_delay.ms as u64))
        } else {
            self.position
                .saturating_sub(Duration::from_millis(self.subtitle_delay.ms.unsigned_abs()))
        };
        self.external_subtitles
            .iter()
            .find(|cue| cue.is_active_at(adjusted_pos))
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    // ------------------------------------------------------------------
    // Input
    // ------------------------------------------------------------------

    /// Handle one input event. Returns whether anything changed.
    pub fn handle_event(&mut self, event: &Event) -> bool {
        // The picker takes everything while it is up.
        match self.picker.handle(event, self.width, self.height) {
            Picked::Chose(path) => {
                self.open_path(&path);
                return true;
            }
            Picked::Handled | Picked::Cancelled => return true,
            Picked::Ignored => {}
        }
        match event {
            Event::Key(key) if key.pressed => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Tick { elapsed_ms } => self.tick(*elapsed_ms),
            _ => false,
        }
    }

    /// Run whatever the keystroke is bound to.
    fn handle_key(&mut self, event: &KeyEvent) -> bool {
        let Some(shortcut) = Shortcuts::for_event(event) else {
            return false;
        };
        // Any keystroke the player acted on is a sign of life, so the controls
        // come back and start their countdown again.
        self.wake_controls();
        self.run(shortcut.command, Press::digit_of(event.key));
        true
    }

    /// Carry out a command. `digit` is read only by `SeekToDigit`.
    pub fn run(&mut self, command: Command, digit: Option<u32>) {
        match command {
            Command::TogglePlayPause => self.toggle_play_pause(),
            Command::Open => self.picker.open_to_read(),
            Command::Stop => self.stop(),
            Command::ToggleFullscreen => self.toggle_fullscreen(),
            Command::ToggleMute => self.toggle_mute(),
            // On the Settings tab the list is what the arrows are for;
            // there is no volume slider drawn there to move.
            Command::VolumeUp => {
                if self.active_tab == PlayerTab::Settings {
                    self.settings_row = self.settings_row.saturating_sub(1);
                } else {
                    self.volume_up();
                }
            }
            Command::VolumeDown => {
                if self.active_tab == PlayerTab::Settings {
                    let last = SettingRow::ALL.len().saturating_sub(1);
                    self.settings_row = self.settings_row.saturating_add(1).min(last);
                } else {
                    self.volume_down();
                }
            }
            Command::SeekBy(ms) => {
                if ms < 0 {
                    self.seek_backward(ms.unsigned_abs());
                } else {
                    self.seek_forward(ms.unsigned_abs());
                }
            }
            Command::SeekToDigit => {
                if let Some(digit) = digit {
                    self.seek_to_fraction(f64::from(digit) / 10.0);
                    self.show_osd(&format!("Seek: {}%", digit.saturating_mul(10)));
                }
            }
            Command::PlaylistNext => self.playlist_next(),
            Command::PlaylistPrevious => self.playlist_previous(),
            Command::SpeedUp => self.increase_speed(),
            Command::SpeedDown => self.decrease_speed(),
            Command::SpeedReset => self.reset_speed(),
            Command::CycleAspect => {
                self.aspect_mode = self.aspect_mode.cycle();
                self.show_osd(&format!("Aspect: {}", self.aspect_mode.label()));
            }
            Command::CycleSubtitleTrack => self.cycle_subtitle_track(),
            Command::CycleAudioTrack => self.cycle_audio_track(),
            Command::ToggleSubtitles => self.toggle_subtitles(),
            Command::ToggleChapterList => {
                self.chapter_list_visible = !self.chapter_list_visible;
            }
            Command::TogglePlaylist => {
                self.playlist_visible = !self.playlist_visible;
                self.active_tab = if self.playlist_visible {
                    PlayerTab::Playlist
                } else {
                    PlayerTab::Player
                };
            }
            Command::ToggleInfo => {
                self.info_visible = !self.info_visible;
                self.active_tab = if self.info_visible {
                    PlayerTab::MediaInfo
                } else {
                    PlayerTab::Player
                };
            }
            Command::ToggleEqualizer => {
                self.equalizer_visible = !self.equalizer_visible;
                self.active_tab = if self.equalizer_visible {
                    PlayerTab::Equalizer
                } else {
                    PlayerTab::Player
                };
            }
            Command::SubtitleDelay(ms) => {
                self.subtitle_delay.adjust(ms);
                self.show_osd(&format!("Subtitle {}", self.subtitle_delay.label()));
            }
            Command::AudioSync(ms) => {
                self.audio_sync.adjust(ms);
                self.show_osd(&format!("Audio {}", self.audio_sync.label()));
            }
            Command::CycleRepeat => {
                self.repeat = self.repeat.cycle();
                self.show_osd(&format!("Repeat: {}", self.repeat.label()));
            }
            Command::ToggleEqualizerEnabled => {
                self.equalizer.enabled = !self.equalizer.enabled;
                self.show_osd(if self.equalizer.enabled {
                    "Equalizer on"
                } else {
                    "Equalizer off"
                });
            }
            Command::ChangeSetting => {
                // Deliberately does nothing outside the Settings tab rather
                // than acting on a row the user cannot see.
                if self.active_tab == PlayerTab::Settings {
                    if let Some(row) = SettingRow::ALL.get(self.settings_row) {
                        row.cycle(&mut self.preferences);
                        self.show_osd(&format!(
                            "{}: {}",
                            row.label(),
                            row.value(&self.preferences)
                        ));
                    }
                }
            }
            Command::AddBookmark => {
                let label = format!("Bookmark at {}", self.position.format());
                self.add_bookmark(label);
                self.show_osd("Bookmark added");
            }
        }
    }

    // ------------------------------------------------------------------
    // The clock
    // ------------------------------------------------------------------

    /// Advance everything that moves on its own. Returns whether anything did.
    ///
    /// Three things keep time and none of them could: the position, which is
    /// what makes a playing film play; the on-screen message, which had no way
    /// to expire; and the controls, which are supposed to fade out of a
    /// fullscreen picture and never did.
    pub fn tick(&mut self, elapsed_ms: u64) -> bool {
        let mut moved = false;

        if self.state == PlaybackState::Playing {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss,
                reason = "a tick is milliseconds and the speed is one of seven small presets"
            )]
            let advanced = (elapsed_ms as f64 * self.speed.value()) as u64;
            self.position = self
                .position
                .saturating_add(Duration::from_millis(advanced));
            if let Some(file) = &self.current_file
                && self.position >= file.duration
            {
                // The end of a file is the start of the next one, or a stop.
                self.position = file.duration;
                self.at_end_of_file();
            }
            moved = true;
        }

        if self.osd_remaining_ms > 0 {
            self.osd_remaining_ms = self.osd_remaining_ms.saturating_sub(elapsed_ms);
            if self.osd_remaining_ms == 0 {
                self.osd_message = None;
            }
            moved = true;
        }

        if self.controls_visible && self.state == PlaybackState::Playing {
            self.controls_hide_timer = self.controls_hide_timer.saturating_sub(elapsed_ms);
            if self.controls_hide_timer == 0 {
                self.controls_visible = false;
                moved = true;
            }
        }

        moved
    }

    /// What happens when the picture runs out.
    fn at_end_of_file(&mut self) {
        match self.repeat {
            RepeatMode::One => self.position = Duration::ZERO,
            RepeatMode::All => self.playlist_next(),
            RepeatMode::Off => {
                if self
                    .playlist
                    .current_index()
                    .is_some_and(|i| i.saturating_add(1) < self.playlist.len())
                {
                    self.playlist_next();
                } else {
                    self.pause();
                }
            }
        }
    }

    /// Show the controls and restart their countdown.
    fn wake_controls(&mut self) {
        self.controls_visible = true;
        self.controls_hide_timer = CONTROLS_HIDE_MS;
    }

    /// Is anything still moving?
    pub fn has_work(&self) -> bool {
        // The controls only count down while playing, so "visible and
        // playing" is already covered by the first clause.
        self.state == PlaybackState::Playing || self.osd_remaining_ms > 0
    }

    // ------------------------------------------------------------------
    // Layout the renderer draws and the mouse reads
    // ------------------------------------------------------------------

    /// Adopt a new window size. Returns whether it changed.
    pub fn set_window_size(&mut self, width: f32, height: f32) -> bool {
        let width = width.max(MIN_WINDOW_WIDTH);
        let height = height.max(MIN_WINDOW_HEIGHT);
        if (self.width - width).abs() < f32::EPSILON && (self.height - height).abs() < f32::EPSILON
        {
            return false;
        }
        self.width = width;
        self.height = height;
        true
    }

    /// The tab strip, one rectangle per tab.
    ///
    /// The width follows the window, so the renderer could not have hard-coded
    /// it and the hit test could not have guessed it -- which is the argument
    /// for there being one of these rather than two.
    pub fn tab_rects(&self) -> Vec<(PlayerTab, Rect)> {
        let tabs = PlayerTab::all();
        #[allow(clippy::cast_precision_loss, reason = "there are seven tabs")]
        let slot = (self.width / tabs.len() as f32).min(TAB_MAX_WIDTH);
        tabs.iter()
            .enumerate()
            .map(|(i, tab)| {
                #[allow(clippy::cast_precision_loss, reason = "there are seven tabs")]
                let x = TAB_PADDING + (i as f32) * slot;
                (
                    *tab,
                    Rect {
                        x,
                        y: TAB_PADDING,
                        w: (slot - TAB_PADDING).max(1.0),
                        h: TAB_BAR_HEIGHT - TAB_PADDING * 2.0,
                    },
                )
            })
            .collect()
    }

    /// Which tab a point is on, if any.
    pub fn tab_at(&self, x: f32, y: f32) -> Option<PlayerTab> {
        self.tab_rects()
            .into_iter()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(tab, _)| tab)
    }

    /// The strip of the window that seeks when clicked or dragged.
    pub fn seek_bar(&self) -> Rect {
        let drawn_y = self.controls_top() + SEEK_BAR_OFFSET;
        Rect {
            x: SEEK_BAR_INSET,
            // Centred on the line that is drawn, so the grab band reaches as
            // far above it as below.
            y: drawn_y - (SEEK_BAR_GRAB_HEIGHT - SEEK_BAR_HEIGHT) / 2.0,
            w: (self.width - SEEK_BAR_INSET * 2.0).max(1.0),
            h: SEEK_BAR_GRAB_HEIGHT,
        }
    }

    /// Y the control bar starts at.
    pub fn controls_top(&self) -> f32 {
        self.height - CONTROLS_HEIGHT
    }

    /// Where along the file a point on the seek bar is.
    fn seek_fraction_at(&self, x: f32) -> f64 {
        let bar = self.seek_bar();
        f64::from(((x - bar.x) / bar.w).clamp(0.0, 1.0))
    }

    fn handle_mouse(&mut self, event: &MouseEvent) -> bool {
        match event.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                self.wake_controls();
                if let Some(tab) = self.tab_at(event.x, event.y) {
                    self.active_tab = tab;
                    return true;
                }
                if self.seek_bar().contains(event.x, event.y) {
                    // A press on the bar starts a drag: the preview follows the
                    // pointer and the picture only moves when it is let go, so
                    // dragging across a film does not seek to every pixel of
                    // the way there.
                    self.seeking = true;
                    self.seek_preview_position = Some(self.preview_at(event.x));
                    return true;
                }
                false
            }
            MouseEventKind::Move => {
                let woke = !self.controls_visible;
                self.wake_controls();
                if self.seeking {
                    self.seek_preview_position = Some(self.preview_at(event.x));
                    return true;
                }
                woke
            }
            MouseEventKind::Release(MouseButton::Left) => {
                if !self.seeking {
                    return false;
                }
                self.seeking = false;
                self.seek_preview_position = None;
                self.seek_to_fraction(self.seek_fraction_at(event.x));
                true
            }
            _ => false,
        }
    }

    /// The position a point on the seek bar names.
    fn preview_at(&self, x: f32) -> Duration {
        let fraction = self.seek_fraction_at(x);
        self.current_file.as_ref().map_or(Duration::ZERO, |file| {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss,
                reason = "the fraction is clamped to 0..=1 and a duration in ms fits f64"
            )]
            let ms = (file.duration.as_millis() as f64 * fraction) as u64;
            Duration::from_millis(ms)
        })
    }

    pub fn render_commands(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(256);

        // Background
        self.palette.push_surface(
            &mut cmds,
            0.0,
            0.0,
            self.width,
            self.height,
            0.0,
            Surface::Card,
        );

        match self.active_tab {
            PlayerTab::Player => self.render_player_view(&mut cmds),
            PlayerTab::Playlist => self.render_playlist_panel(&mut cmds),
            PlayerTab::MediaInfo => self.render_media_info(&mut cmds),
            PlayerTab::Equalizer => self.render_equalizer(&mut cmds),
            PlayerTab::Adjustments => self.render_adjustments(&mut cmds),
            PlayerTab::Settings => self.render_settings(&mut cmds),
            PlayerTab::Shortcuts => self.render_shortcuts(&mut cmds),
        }

        // Tab bar at top
        self.render_tab_bar(&mut cmds);

        // Over the tab bar as well: it is a list you asked for, and a panel
        // under the bar would be half-hidden by it.
        if self.chapter_list_visible {
            self.render_chapter_list(&mut cmds);
        }

        // The picker last, over everything.
        cmds.extend(self.picker.render(&self.palette, self.width, self.height));

        cmds
    }

    /// The chapter list, which `C` opens.
    ///
    /// Until 2026-09-22 `chapter_list_visible` was declared, initialised and
    /// inverted by `C` and read by nothing, while `("C", "Toggle Chapter
    /// List")` sat in the shortcut table this window both draws from and
    /// dispatches on. The key was advertised and moved nothing.
    ///
    /// **It says so when there are none.** `chapters` is empty until a video
    /// with chapter marks is loaded, and a panel that opens blank reads as a
    /// broken control -- the lesson `apps/netscan`'s Send button and
    /// `apps/soundrecorder`'s transport both taught: a press that changes
    /// nothing visible sends someone hunting a fault in the wrong place.
    fn render_chapter_list(&self, cmds: &mut Vec<RenderCommand>) {
        let w = (self.width * 0.6).clamp(220.0, 460.0);
        let h = (self.height * 0.6).clamp(120.0, 420.0);
        let x = (self.width - w) / 2.0;
        let y = (self.height - h) / 2.0;

        self.palette
            .push_surface(cmds, x, y, w, h, 6.0, Surface::Card);
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: y + 10.0,
            text: "Chapters  --  C closes this".into(),
            font_size: 13.0,
            color: self.palette.ink(self.palette.blue),
            font_weight: FontWeightHint::Bold,
            max_width: Some(w - 24.0),
            overflow: TextOverflow::Ellipsis,
        });

        if self.chapters.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: y + 34.0,
                text: "This video has no chapter marks.".into(),
                font_size: 11.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(w - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
            return;
        }

        let here = self.current_chapter().map(|(idx, _)| idx);
        for (i, chapter) in self.chapters.iter().enumerate() {
            #[expect(
                clippy::cast_precision_loss,
                reason = "a video has tens of chapters, not millions"
            )]
            let row_y = y + 34.0 + i as f32 * 18.0;
            if row_y + 18.0 > y + h {
                break;
            }
            let current = here == Some(i);
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: row_y,
                text: format!("{}   {}", chapter.start.format(), chapter.title),
                font_size: 11.0,
                color: if current {
                    self.palette.ink(self.palette.blue)
                } else {
                    self.palette.text
                },
                font_weight: if current {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(w - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    fn render_tab_bar(&self, cmds: &mut Vec<RenderCommand>) {
        // Tab bar background
        self.palette.push_surface(
            cmds,
            0.0,
            0.0,
            self.width,
            TAB_BAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Bottom),
        );

        // Tab items, from the rectangles the hit test reads.
        for (tab, rect) in self.tab_rects() {
            let active = tab == self.active_tab;
            let bg = if active {
                self.palette.surface0
            } else {
                self.palette.mantle
            };
            let fg = if active {
                self.palette.blue
            } else {
                self.palette.subtext0
            };

            cmds.push(RenderCommand::FillRect {
                x: rect.x,
                y: rect.y,
                width: rect.w,
                height: rect.h,
                color: bg,
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: rect.x + rect.w / 2.0 - 24.0,
                y: 12.0,
                text: tab.label().to_string(),
                font_size: 12.0,
                color: fg,
                font_weight: if active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some((rect.w - 8.0).max(1.0)),
                overflow: TextOverflow::Ellipsis,
            });

            if active {
                cmds.push(RenderCommand::FillRect {
                    x: rect.x + 2.0,
                    y: TAB_BAR_HEIGHT - 3.0,
                    width: (rect.w - 4.0).max(1.0),
                    height: 2.0,
                    color: self.palette.blue,
                    corner_radii: CornerRadii::all(1.0),
                });
            }
        }

        // Separator
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: TAB_BAR_HEIGHT,
            x2: self.width,
            y2: TAB_BAR_HEIGHT,
            color: self.palette.surface0,
            width: 1.0,
        });
    }

    fn render_player_view(&self, cmds: &mut Vec<RenderCommand>) {
        let top = CONTENT_TOP;
        let controls_h = CONTROLS_HEIGHT;
        let video_h = self.height - top - controls_h;

        // Video area background (black for letterbox)
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: top,
            width: self.width,
            height: video_h,
            color: Color::rgb(0, 0, 0),
            corner_radii: CornerRadii::ZERO,
        });

        // With no file, what this player cannot do, where a picture would be.
        //
        // It said "No file loaded" and "Ctrl+O to open" here -- and Ctrl+O is
        // bound to nothing -- while the true lines were drawn at the top of the
        // window, under the video area and the tab bar that were then drawn
        // over them. Keyed on there being no file, so they retire themselves
        // the day something can open one.
        if self.current_file.is_none() {
            let avail = (self.width - 32.0).max(0.0);
            for (i, line) in CANNOT_PLAY_LINES.iter().enumerate() {
                #[expect(clippy::cast_precision_loss, reason = "three lines; index is 0..3")]
                let ty = top + video_h / 2.0 - 30.0 + i as f32 * 22.0;
                if avail <= 0.0 || ty < top || ty + 22.0 > top + video_h {
                    continue;
                }
                cmds.push(RenderCommand::Text {
                    x: 16.0,
                    y: ty,
                    text: (*line).to_string(),
                    font_size: if i == 0 { 16.0 } else { 13.0 },
                    color: if i == 0 {
                        self.palette.ink(self.palette.yellow)
                    } else {
                        self.palette.subtext0
                    },
                    font_weight: if i == 0 {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    },
                    max_width: Some(avail),
                    overflow: TextOverflow::Ellipsis,
                });
            }
        } else {
            // Where the picture would be: that there is none, and why; then
            // what the picture is, as far as the file says.
            let avail = (self.width - 32.0).max(0.0);
            let what = self.current_file.as_ref().and_then(|file| {
                let vs = file.primary_video()?;
                Some(match vs.size_label() {
                    Some(size) => format!("{size} {}", vs.codec.name()),
                    None => vs.codec.name().to_string(),
                })
            });
            let lines = NO_PICTURE_LINES
                .iter()
                .map(|l| (*l).to_string())
                .chain(what);
            for (i, line) in lines.enumerate() {
                #[expect(clippy::cast_precision_loss, reason = "three lines; index is 0..3")]
                let ty = top + video_h / 2.0 - 30.0 + i as f32 * 22.0;
                if avail <= 0.0 || ty < top || ty + 22.0 > top + video_h {
                    continue;
                }
                cmds.push(RenderCommand::Text {
                    x: 16.0,
                    y: ty,
                    text: line,
                    font_size: if i == 0 { 16.0 } else { 13.0 },
                    color: if i == 0 {
                        self.palette.ink(self.palette.yellow)
                    } else {
                        self.palette.subtext0
                    },
                    font_weight: if i == 0 {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    },
                    max_width: Some(avail),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            // Subtitle display
            if self.subtitle_enabled
                && let Some(cue) = self.active_subtitle_cue()
            {
                let sub_y = top + video_h - 60.0;
                // Shadow behind subtitle
                cmds.push(RenderCommand::FillRect {
                    x: self.width / 2.0 - 200.0,
                    y: sub_y - 4.0,
                    width: 400.0,
                    height: 36.0,
                    color: Color::rgba(0, 0, 0, 180),
                    corner_radii: CornerRadii::all(4.0),
                });
                cmds.push(RenderCommand::Text {
                    x: self.width / 2.0 - 190.0,
                    y: sub_y + 2.0,
                    text: cue.text.clone(),
                    font_size: 20.0,
                    color: Color::rgb(255, 255, 255),
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(380.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            // OSD message
            if let Some(msg) = &self.osd_message {
                cmds.push(RenderCommand::FillRect {
                    x: 16.0,
                    y: top + 16.0,
                    width: 200.0,
                    height: 32.0,
                    color: Color::rgba(0, 0, 0, 160),
                    corner_radii: CornerRadii::all(6.0),
                });
                cmds.push(RenderCommand::Text {
                    x: 28.0,
                    y: top + 24.0,
                    text: msg.clone(),
                    font_size: 14.0,
                    color: Color::rgb(255, 255, 255),
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(180.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            // Playback state icon (center of video)
            if self.state == PlaybackState::Paused {
                let cx = self.width / 2.0;
                let cy = top + video_h / 2.0;
                cmds.push(RenderCommand::FillRect {
                    x: cx - 24.0,
                    y: cy - 24.0,
                    width: 48.0,
                    height: 48.0,
                    color: Color::rgba(0, 0, 0, 128),
                    corner_radii: CornerRadii::all(24.0),
                });
                cmds.push(RenderCommand::Text {
                    x: cx - 8.0,
                    y: cy - 10.0,
                    text: "||".to_string(),
                    font_size: 20.0,
                    color: Color::rgb(255, 255, 255),
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }
        }

        // Controls area
        let ctrl_y = top + video_h;
        self.render_controls(cmds, ctrl_y, controls_h);
    }

    fn render_controls(&self, cmds: &mut Vec<RenderCommand>, y: f32, height: f32) {
        // Controls background
        self.palette
            .push_surface(cmds, 0.0, y, self.width, height, 0.0, Surface::Card);

        // Seek bar. The x and width come from the same rectangle the mouse
        // is hit-tested against, so a click lands where the line was drawn.
        let bar = self.seek_bar();
        let seek_y = y + SEEK_BAR_OFFSET;
        let seek_x = bar.x;
        let seek_w = bar.w;
        let seek_h = SEEK_BAR_HEIGHT;

        // Seek track background
        self.palette.push_surface(
            cmds,
            seek_x,
            seek_y,
            seek_w,
            seek_h,
            3.0,
            Surface::ControlTrack,
        );

        // Buffer progress (slightly ahead of play position)
        let buffer_frac = (self.progress_fraction() + 0.05).min(1.0);
        self.palette.push_surface(
            cmds,
            seek_x,
            seek_y,
            seek_w * buffer_frac as f32,
            seek_h,
            3.0,
            Surface::ControlTrack,
        );

        // Play progress
        let progress = self.progress_fraction() as f32;
        cmds.push(RenderCommand::FillRect {
            x: seek_x,
            y: seek_y,
            width: seek_w * progress,
            height: seek_h,
            color: self.palette.blue,
            corner_radii: CornerRadii::all(3.0),
        });

        // The timestamp under the pointer.
        //
        // `seek_preview_position` is maintained through a *drag* -- set on
        // press, followed on move, cleared on release -- and nothing drew it.
        // That is not a missing nicety, it is the missing half of a deliberate
        // design: the comment on the press arm says "the preview follows the
        // pointer and the picture only moves when it is let go, so dragging
        // across a film does not seek to every pixel of the way there". The
        // whole point of not seeking continuously is that the preview tells
        // you where you are. Without it the user drags blind and finds out
        // where they landed by arriving there, which is strictly worse than
        // the continuous seeking this was built to avoid.
        //
        // Positioned from the previewed *time* rather than from the pointer's
        // x, so the label sits over the frame that will actually be sought to
        // even if the two ever diverge. Clamped to the bar so it stays legible
        // at either end instead of sliding off the window.
        if let (Some(preview), Some(file)) = (self.seek_preview_position, &self.current_file) {
            let frac = preview.progress_of(file.duration) as f32;
            let label_w = 48.0;
            let x = (seek_x + seek_w * frac - label_w / 2.0)
                .clamp(seek_x, (seek_x + seek_w - label_w).max(seek_x));
            cmds.push(RenderCommand::Text {
                x,
                y: seek_y - 16.0,
                text: preview.format(),
                font_size: 11.0,
                color: self.palette.text,
                font_weight: FontWeightHint::Regular,
                max_width: Some(label_w),
                overflow: TextOverflow::Clip,
            });
        }

        // Chapter markers
        if let Some(file) = &self.current_file {
            for chapter in &self.chapters {
                let frac = chapter.start.progress_of(file.duration) as f32;
                let marker_x = seek_x + seek_w * frac;
                cmds.push(RenderCommand::FillRect {
                    x: marker_x - 1.0,
                    y: seek_y - 1.0,
                    width: 2.0,
                    height: seek_h + 2.0,
                    color: self.palette.yellow,
                    corner_radii: CornerRadii::ZERO,
                });
            }
        }

        // Seek handle
        let handle_x = seek_x + seek_w * progress;
        cmds.push(RenderCommand::FillRect {
            x: handle_x - 6.0,
            y: seek_y - 3.0,
            width: 12.0,
            height: 12.0,
            color: self.palette.blue,
            corner_radii: CornerRadii::all(6.0),
        });

        // Time display
        let time_y = seek_y + seek_h + 8.0;
        cmds.push(RenderCommand::Text {
            x: seek_x,
            y: time_y,
            text: self.time_display(),
            font_size: 11.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Remaining time
        cmds.push(RenderCommand::Text {
            x: self.width - 100.0,
            y: time_y,
            text: format!("-{}", self.remaining_duration().format()),
            font_size: 11.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(100.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Playback buttons row
        let btn_y = time_y + 18.0;
        let btn_w = 36.0;
        let btn_h = 28.0;
        let center_x = self.width / 2.0;

        // Previous
        self.render_button(
            cmds,
            center_x - btn_w * 2.5,
            btn_y,
            btn_w,
            btn_h,
            "|<",
            false,
        );
        // Rewind
        self.render_button(
            cmds,
            center_x - btn_w * 1.5,
            btn_y,
            btn_w,
            btn_h,
            "<<",
            false,
        );
        // Play/Pause
        let play_label = if self.state == PlaybackState::Playing {
            "||"
        } else {
            ">"
        };
        self.render_button(
            cmds,
            center_x - btn_w * 0.5,
            btn_y,
            btn_w,
            btn_h,
            play_label,
            true,
        );
        // Forward
        self.render_button(
            cmds,
            center_x + btn_w * 0.5,
            btn_y,
            btn_w,
            btn_h,
            ">>",
            false,
        );
        // Next
        self.render_button(
            cmds,
            center_x + btn_w * 1.5,
            btn_y,
            btn_w,
            btn_h,
            ">|",
            false,
        );

        // Left controls: volume
        let vol_x = 16.0;
        cmds.push(RenderCommand::Text {
            x: vol_x,
            y: btn_y + 6.0,
            text: self.volume.icon().to_string(),
            font_size: 13.0,
            color: if self.volume.is_muted() {
                self.palette.ink(self.palette.red)
            } else {
                self.palette.text
            },
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Volume bar
        let vol_bar_x = vol_x + 24.0;
        let vol_bar_w = 80.0;
        let vol_bar_h = 4.0;
        let vol_bar_y = btn_y + 12.0;

        cmds.push(RenderCommand::FillRect {
            x: vol_bar_x,
            y: vol_bar_y,
            width: vol_bar_w,
            height: vol_bar_h,
            color: self.palette.surface0,
            corner_radii: CornerRadii::all(2.0),
        });

        let vol_frac = self.volume.fraction() as f32;
        let vol_color = if self.volume.effective_level() > Volume::NORMAL {
            self.palette.peach
        } else {
            self.palette.green
        };
        cmds.push(RenderCommand::FillRect {
            x: vol_bar_x,
            y: vol_bar_y,
            width: vol_bar_w * vol_frac,
            height: vol_bar_h,
            color: vol_color,
            corner_radii: CornerRadii::all(2.0),
        });

        cmds.push(RenderCommand::Text {
            x: vol_bar_x + vol_bar_w + 8.0,
            y: btn_y + 6.0,
            text: self.volume.label(),
            font_size: 11.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(50.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Right controls: speed, repeat, shuffle, aspect
        let right_x = self.width - 220.0;

        cmds.push(RenderCommand::Text {
            x: right_x,
            y: btn_y + 6.0,
            text: self.speed.label(),
            font_size: 11.0,
            color: if self.speed != PlaybackSpeed::NORMAL {
                self.palette.ink(self.palette.peach)
            } else {
                self.palette.subtext0
            },
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        cmds.push(RenderCommand::Text {
            x: right_x + 40.0,
            y: btn_y + 6.0,
            text: self.repeat.icon().to_string(),
            font_size: 11.0,
            color: if self.repeat != RepeatMode::Off {
                self.palette.ink(self.palette.blue)
            } else {
                self.palette.subtext0
            },
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        cmds.push(RenderCommand::Text {
            x: right_x + 70.0,
            y: btn_y + 6.0,
            text: if self.playlist.is_shuffle() {
                "S+"
            } else {
                "S-"
            }
            .to_string(),
            font_size: 11.0,
            color: if self.playlist.is_shuffle() {
                self.palette.ink(self.palette.green)
            } else {
                self.palette.subtext0
            },
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        cmds.push(RenderCommand::Text {
            x: right_x + 100.0,
            y: btn_y + 6.0,
            text: self.aspect_mode.label().to_string(),
            font_size: 11.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(60.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Subtitle indicator
        if self.subtitle_enabled {
            cmds.push(RenderCommand::Text {
                x: right_x + 160.0,
                y: btn_y + 6.0,
                text: "CC".to_string(),
                font_size: 11.0,
                color: self.palette.ink(self.palette.yellow),
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        // Fullscreen toggle
        cmds.push(RenderCommand::Text {
            x: right_x + 190.0,
            y: btn_y + 6.0,
            text: if self.fullscreen { "[-]" } else { "[+]" }.to_string(),
            font_size: 11.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    // A button is described by its rectangle (x/y/w/h), label, and a primary
    // flag; passing these positionally keeps call sites compact and matches the
    // other immediate-mode render helpers in this file.
    #[allow(clippy::too_many_arguments)]
    fn render_button(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        label: &str,
        primary: bool,
    ) {
        let bg = if primary {
            self.palette.blue
        } else {
            self.palette.surface0
        };
        let fg = if primary {
            self.palette.crust
        } else {
            self.palette.text
        };

        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: bg,
            corner_radii: CornerRadii::all(4.0),
        });

        cmds.push(RenderCommand::Text {
            x: x + w / 2.0 - 8.0,
            y: y + h / 2.0 - 6.0,
            text: label.to_string(),
            font_size: 13.0,
            color: fg,
            font_weight: FontWeightHint::Bold,
            max_width: Some(w - 4.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_playlist_panel(&self, cmds: &mut Vec<RenderCommand>) {
        let top = 40.0;
        let panel_w = self.width;
        let panel_h = self.height - top;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: top,
            width: panel_w,
            height: panel_h,
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });

        // Header
        cmds.push(RenderCommand::Text {
            x: 16.0,
            y: top + 16.0,
            text: format!(
                "Playlist ({} items, total: {})",
                self.playlist.len(),
                self.playlist.total_duration().format()
            ),
            font_size: 14.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(panel_w - 32.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Shuffle / repeat status
        cmds.push(RenderCommand::Text {
            x: 16.0,
            y: top + 36.0,
            text: format!(
                "Shuffle: {} | {}",
                if self.playlist.is_shuffle() {
                    "On"
                } else {
                    "Off"
                },
                self.repeat.label()
            ),
            font_size: 11.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(panel_w - 32.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Separator
        cmds.push(RenderCommand::Line {
            x1: 16.0,
            y1: top + 52.0,
            x2: panel_w - 16.0,
            y2: top + 52.0,
            color: self.palette.surface0,
            width: 1.0,
        });

        // Playlist entries
        let item_h = 40.0;
        let list_y = top + 58.0;
        let visible_items = ((panel_h - 58.0) / item_h) as usize;
        let current_idx = self.playlist.current_index();

        cmds.push(RenderCommand::PushClip {
            x: 0.0,
            y: list_y,
            width: panel_w,
            height: panel_h - 58.0,
        });

        for (i, entry) in self
            .playlist
            .entries()
            .iter()
            .enumerate()
            .take(visible_items)
        {
            let ey = list_y + i as f32 * item_h - self.playlist_scroll;
            let is_current = current_idx == Some(i);

            // Highlight current
            if is_current {
                self.palette.push_surface(
                    cmds,
                    8.0,
                    ey,
                    panel_w - 16.0,
                    item_h - 2.0,
                    4.0,
                    Surface::Selected,
                );
            }

            // Index
            cmds.push(RenderCommand::Text {
                x: 16.0,
                y: ey + 8.0,
                text: format!("{}", i.saturating_add(1)),
                font_size: 11.0,
                color: if is_current {
                    self.palette.ink(self.palette.blue)
                } else {
                    self.palette.subtext0
                },
                font_weight: FontWeightHint::Regular,
                max_width: Some(24.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Playing indicator
            if is_current && self.state == PlaybackState::Playing {
                cmds.push(RenderCommand::Text {
                    x: 40.0,
                    y: ey + 8.0,
                    text: ">".to_string(),
                    font_size: 12.0,
                    color: self.palette.ink(self.palette.green),
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }

            // Title
            cmds.push(RenderCommand::Text {
                x: 56.0,
                y: ey + 6.0,
                text: entry.display_name().to_string(),
                font_size: 13.0,
                color: if is_current {
                    self.palette.text
                } else {
                    self.palette.subtext1
                },
                font_weight: if is_current {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(panel_w - 160.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Duration
            if let Some(dur) = entry.duration {
                cmds.push(RenderCommand::Text {
                    x: panel_w - 80.0,
                    y: ey + 8.0,
                    text: dur.format(),
                    font_size: 11.0,
                    color: self.palette.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(60.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            // Path (small)
            cmds.push(RenderCommand::Text {
                x: 56.0,
                y: ey + 22.0,
                text: entry.path.display().to_string(),
                font_size: 10.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(panel_w - 100.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        cmds.push(RenderCommand::PopClip);
    }

    fn render_media_info(&self, cmds: &mut Vec<RenderCommand>) {
        let top = 40.0;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: top,
            width: self.width,
            height: self.height - top,
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });

        if let Some(file) = &self.current_file {
            let mut line_y = top + 20.0;
            let line_h = 22.0;
            let label_x = 20.0;
            let value_x = 160.0;

            let info_section = |cmds: &mut Vec<RenderCommand>, y: &mut f32, title: &str| {
                *y += 8.0;
                cmds.push(RenderCommand::Text {
                    x: label_x,
                    y: *y,
                    text: title.to_string(),
                    font_size: 14.0,
                    color: self.palette.ink(self.palette.blue),
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(300.0),
                    overflow: TextOverflow::Ellipsis,
                });
                *y += line_h + 4.0;
            };

            let info_row =
                |cmds: &mut Vec<RenderCommand>, y: &mut f32, label: &str, value: &str| {
                    cmds.push(RenderCommand::Text {
                        x: label_x + 8.0,
                        y: *y,
                        text: label.to_string(),
                        font_size: 12.0,
                        color: self.palette.subtext0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(120.0),
                        overflow: TextOverflow::Ellipsis,
                    });
                    cmds.push(RenderCommand::Text {
                        x: value_x,
                        y: *y,
                        text: value.to_string(),
                        font_size: 12.0,
                        color: self.palette.text,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(400.0),
                        overflow: TextOverflow::Ellipsis,
                    });
                    *y += line_h;
                };

            // General
            info_section(cmds, &mut line_y, "General");
            info_row(cmds, &mut line_y, "File Name", &file.file_name);
            info_row(cmds, &mut line_y, "Format", file.container.display_name());
            info_row(cmds, &mut line_y, "File Size", &file.file_size_display());
            info_row(cmds, &mut line_y, "Duration", &file.duration.format());
            info_row(
                cmds,
                &mut line_y,
                "Overall Bitrate",
                &format_bitrate(file.overall_bitrate()),
            );

            // Video streams
            for (i, vs) in file.video_streams.iter().enumerate() {
                info_section(
                    cmds,
                    &mut line_y,
                    &format!("Video Stream #{}", i.saturating_add(1)),
                );
                // Each row only where the file says: a blank or a zero
                // where a header is silent would read as the file's answer.
                info_row(cmds, &mut line_y, "Codec", vs.codec.name());
                if let (Some(size), Some(label)) = (vs.size_label(), vs.resolution_label()) {
                    info_row(
                        cmds,
                        &mut line_y,
                        "Resolution",
                        &format!("{size} ({label})"),
                    );
                }
                if let Some(ratio) = vs.aspect_ratio() {
                    info_row(cmds, &mut line_y, "Aspect Ratio", &ratio);
                }
                if let Some(fps) = vs.frame_rate {
                    info_row(cmds, &mut line_y, "Frame Rate", &format!("{fps:.3} fps"));
                }
                if let Some(bps) = vs.bit_rate {
                    info_row(cmds, &mut line_y, "Bit Rate", &format_bitrate(bps));
                }
                if let Some(pf) = &vs.pixel_format {
                    info_row(cmds, &mut line_y, "Pixel Format", pf);
                }
                if vs.hdr {
                    info_row(cmds, &mut line_y, "HDR", "Yes");
                }
                if let Some(cs) = &vs.color_space {
                    info_row(cmds, &mut line_y, "Color Space", cs);
                }
            }

            // Audio streams
            for (i, audio) in file.audio_streams.iter().enumerate() {
                info_section(
                    cmds,
                    &mut line_y,
                    &format!("Audio Stream #{}", i.saturating_add(1)),
                );
                info_row(cmds, &mut line_y, "Codec", audio.codec.name());
                if let Some(hz) = audio.sample_rate {
                    info_row(cmds, &mut line_y, "Sample Rate", &format!("{hz} Hz"));
                }
                if let Some(channels) = audio.channels {
                    info_row(cmds, &mut line_y, "Channels", channel_layout_name(channels));
                }
                if let Some(bps) = audio.bit_rate {
                    info_row(cmds, &mut line_y, "Bit Rate", &format_bitrate(bps));
                }
                if let Some(lang) = &audio.language {
                    info_row(cmds, &mut line_y, "Language", lang);
                }
                if let Some(title) = &audio.title {
                    info_row(cmds, &mut line_y, "Name", title);
                }
                info_row(
                    cmds,
                    &mut line_y,
                    "Lossless",
                    if is_lossless(&audio.codec) {
                        "Yes"
                    } else {
                        "No"
                    },
                );
            }

            // Subtitle streams
            for (i, sub) in file.subtitle_streams.iter().enumerate() {
                info_section(
                    cmds,
                    &mut line_y,
                    &format!("Subtitle Stream #{}", i.saturating_add(1)),
                );
                info_row(cmds, &mut line_y, "Format", sub.codec.name());
                if let Some(lang) = &sub.language {
                    info_row(cmds, &mut line_y, "Language", lang);
                }
                info_row(
                    cmds,
                    &mut line_y,
                    "Text Based",
                    if is_text_subtitle(&sub.codec) {
                        "Yes"
                    } else {
                        "No"
                    },
                );
                if sub.is_forced {
                    info_row(cmds, &mut line_y, "Forced", "Yes");
                }
            }

            // Metadata
            let meta = &file.metadata;
            let has_meta = meta.title.is_some() || meta.artist.is_some() || meta.album.is_some();
            if has_meta {
                info_section(cmds, &mut line_y, "Metadata");
                if let Some(t) = &meta.title {
                    info_row(cmds, &mut line_y, "Title", t);
                }
                if let Some(a) = &meta.artist {
                    info_row(cmds, &mut line_y, "Artist", a);
                }
                if let Some(a) = &meta.album {
                    info_row(cmds, &mut line_y, "Album", a);
                }
                if let Some(y) = meta.year {
                    info_row(cmds, &mut line_y, "Year", &y.to_string());
                }
                if let Some(g) = &meta.genre {
                    info_row(cmds, &mut line_y, "Genre", g);
                }
                if let Some(e) = &meta.encoder {
                    info_row(cmds, &mut line_y, "Encoder", e);
                }
            }
        } else {
            cmds.push(RenderCommand::Text {
                x: self.width / 2.0 - 60.0,
                y: top + 100.0,
                text: "No file loaded".to_string(),
                font_size: 16.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(200.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    fn render_equalizer(&self, cmds: &mut Vec<RenderCommand>) {
        let top = 40.0;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: top,
            width: self.width,
            height: self.height - top,
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });

        // Header
        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: top + 20.0,
            text: format!("Equalizer - {}", self.equalizer.preset.label()),
            font_size: 16.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(300.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Enabled indicator
        let enabled_color = if self.equalizer.enabled {
            self.palette.green
        } else {
            self.palette.red
        };
        cmds.push(RenderCommand::FillRect {
            x: 280.0,
            y: top + 20.0,
            width: 60.0,
            height: 22.0,
            color: enabled_color,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: 288.0,
            y: top + 24.0,
            text: if self.equalizer.enabled { "ON" } else { "OFF" }.to_string(),
            font_size: 12.0,
            color: self.palette.crust,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Preamp
        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: top + 52.0,
            text: format!("Preamp: {:.1} dB", self.equalizer.preamp),
            font_size: 12.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(150.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Preset buttons
        let preset_y = top + 50.0;
        let mut px = 180.0;
        for preset in EqPreset::all() {
            let active = *preset == self.equalizer.preset;
            let bg = if active {
                self.palette.blue
            } else {
                self.palette.surface0
            };
            let fg = if active {
                self.palette.crust
            } else {
                self.palette.subtext1
            };

            cmds.push(RenderCommand::FillRect {
                x: px,
                y: preset_y,
                width: 60.0,
                height: 24.0,
                color: bg,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: px + 4.0,
                y: preset_y + 6.0,
                text: preset.label().to_string(),
                font_size: 10.0,
                color: fg,
                font_weight: if active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(56.0),
                overflow: TextOverflow::Ellipsis,
            });
            px += 64.0;
        }

        // EQ bands visualization
        let bands_y = top + 90.0;
        let bands_h = self.height - top - 120.0;
        let band_count = self.equalizer.bands.len();
        if band_count > 0 {
            let band_w = (self.width - 60.0) / band_count as f32;
            let center_y = bands_y + bands_h / 2.0;

            // Center line (0 dB)
            cmds.push(RenderCommand::Line {
                x1: 20.0,
                y1: center_y,
                x2: self.width - 20.0,
                y2: center_y,
                color: self.palette.surface1,
                width: 1.0,
            });

            // +/- labels
            cmds.push(RenderCommand::Text {
                x: 4.0,
                y: bands_y + 4.0,
                text: "+12".to_string(),
                font_size: 9.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cmds.push(RenderCommand::Text {
                x: 4.0,
                y: center_y - 6.0,
                text: "0".to_string(),
                font_size: 9.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cmds.push(RenderCommand::Text {
                x: 4.0,
                y: bands_y + bands_h - 12.0,
                text: "-12".to_string(),
                font_size: 9.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            for (i, band) in self.equalizer.bands.iter().enumerate() {
                let bx = 30.0 + i as f32 * band_w;
                let max_travel = bands_h / 2.0;
                let bar_h = (band.gain / 12.0) * max_travel;

                let color = if band.gain > 0.0 {
                    self.palette.green
                } else if band.gain < 0.0 {
                    self.palette.red
                } else {
                    self.palette.surface2
                };

                if bar_h.abs() > 1.0 {
                    if bar_h > 0.0 {
                        cmds.push(RenderCommand::FillRect {
                            x: bx + band_w * 0.2,
                            y: center_y - bar_h,
                            width: band_w * 0.6,
                            height: bar_h,
                            color,
                            corner_radii: CornerRadii::all(2.0),
                        });
                    } else {
                        cmds.push(RenderCommand::FillRect {
                            x: bx + band_w * 0.2,
                            y: center_y,
                            width: band_w * 0.6,
                            height: -bar_h,
                            color,
                            corner_radii: CornerRadii::all(2.0),
                        });
                    }
                }

                // Handle
                let handle_y = center_y - bar_h;
                cmds.push(RenderCommand::FillRect {
                    x: bx + band_w * 0.15,
                    y: handle_y - 3.0,
                    width: band_w * 0.7,
                    height: 6.0,
                    color: self.palette.lavender,
                    corner_radii: CornerRadii::all(3.0),
                });

                // Frequency label
                cmds.push(RenderCommand::Text {
                    x: bx + band_w * 0.1,
                    y: bands_y + bands_h + 4.0,
                    text: band.label.clone(),
                    font_size: 9.0,
                    color: self.palette.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(band_w),
                    overflow: TextOverflow::Ellipsis,
                });

                // Gain value
                cmds.push(RenderCommand::Text {
                    x: bx + band_w * 0.15,
                    y: handle_y - 16.0,
                    text: format!("{:.0}", band.gain),
                    font_size: 9.0,
                    color: self.palette.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(band_w * 0.7),
                    overflow: TextOverflow::Ellipsis,
                });
            }
        }
    }

    fn render_adjustments(&self, cmds: &mut Vec<RenderCommand>) {
        let top = 40.0;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: top,
            width: self.width,
            height: self.height - top,
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: top + 20.0,
            text: "Video Adjustments".to_string(),
            font_size: 16.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(300.0),
            overflow: TextOverflow::Ellipsis,
        });

        let adj = &self.video_adjustments;
        let sliders = [
            ("Brightness", adj.brightness, -1.0, 1.0),
            ("Contrast", adj.contrast, 0.0, 2.0),
            ("Saturation", adj.saturation, 0.0, 3.0),
            ("Hue", adj.hue, -180.0, 180.0),
            ("Gamma", adj.gamma, 0.1, 3.0),
            ("Sharpness", adj.sharpness, 0.0, 2.0),
        ];

        let slider_w = self.width - 200.0;
        let label_x = 20.0;
        let bar_x = 140.0;

        for (i, (name, value, min_val, max_val)) in sliders.iter().enumerate() {
            let sy = top + 60.0 + i as f32 * 50.0;

            cmds.push(RenderCommand::Text {
                x: label_x,
                y: sy + 4.0,
                text: name.to_string(),
                font_size: 13.0,
                color: self.palette.text,
                font_weight: FontWeightHint::Regular,
                max_width: Some(110.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Slider track
            cmds.push(RenderCommand::FillRect {
                x: bar_x,
                y: sy + 8.0,
                width: slider_w,
                height: 6.0,
                color: self.palette.surface0,
                corner_radii: CornerRadii::all(3.0),
            });

            // Slider fill
            let range = max_val - min_val;
            let frac = if range > 0.0 {
                (value - min_val) / range
            } else {
                0.0
            };
            cmds.push(RenderCommand::FillRect {
                x: bar_x,
                y: sy + 8.0,
                width: slider_w * frac,
                height: 6.0,
                color: self.palette.blue,
                corner_radii: CornerRadii::all(3.0),
            });

            // Slider handle
            let handle_x = bar_x + slider_w * frac;
            cmds.push(RenderCommand::FillRect {
                x: handle_x - 5.0,
                y: sy + 4.0,
                width: 10.0,
                height: 14.0,
                color: self.palette.lavender,
                corner_radii: CornerRadii::all(5.0),
            });

            // Value
            cmds.push(RenderCommand::Text {
                x: bar_x + slider_w + 12.0,
                y: sy + 4.0,
                text: format!("{value:.2}"),
                font_size: 12.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(50.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Reset button
        let reset_y = top + 60.0 + sliders.len() as f32 * 50.0 + 10.0;
        let is_default = adj.is_default();
        let reset_bg = if is_default {
            self.palette.surface0
        } else {
            self.palette.peach
        };
        let reset_fg = if is_default {
            self.palette.overlay0
        } else {
            self.palette.crust
        };

        cmds.push(RenderCommand::FillRect {
            x: bar_x,
            y: reset_y,
            width: 100.0,
            height: 30.0,
            color: reset_bg,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: bar_x + 20.0,
            y: reset_y + 8.0,
            text: "Reset All".to_string(),
            font_size: 12.0,
            color: reset_fg,
            font_weight: FontWeightHint::Bold,
            max_width: Some(80.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Deinterlace mode
        cmds.push(RenderCommand::Text {
            x: label_x,
            y: reset_y + 50.0,
            text: format!("Deinterlace: {}", self.preferences.deinterlace.label()),
            font_size: 13.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Regular,
            max_width: Some(300.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_settings(&self, cmds: &mut Vec<RenderCommand>) {
        let top = 40.0;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: top,
            width: self.width,
            height: self.height - top,
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: top + 20.0,
            text: "Player Settings".to_string(),
            font_size: 16.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(300.0),
            overflow: TextOverflow::Ellipsis,
        });

        let prefs = &self.preferences;
        let settings = SettingRow::ALL;

        let label_x = 20.0;
        let value_x = 220.0;

        for (i, row) in settings.iter().enumerate() {
            let (name, value) = (row.label(), row.value(prefs));
            let selected = i == self.settings_row;
            let sy = top + 60.0 + i as f32 * 36.0;

            cmds.push(RenderCommand::FillRect {
                x: 12.0,
                y: sy - 2.0,
                width: self.width - 24.0,
                height: 32.0,
                color: if selected {
                    self.palette.surface1
                } else if i % 2 == 0 {
                    self.palette.surface0
                } else {
                    self.palette.base
                },
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: label_x,
                y: sy + 6.0,
                text: name.to_string(),
                font_size: 13.0,
                color: self.palette.text,
                font_weight: FontWeightHint::Regular,
                max_width: Some(180.0),
                overflow: TextOverflow::Ellipsis,
            });

            let value_color = if value == "On" {
                self.palette.green
            } else if value == "Off" {
                self.palette.red
            } else {
                self.palette.subtext1
            };
            cmds.push(RenderCommand::Text {
                x: value_x,
                y: sy + 6.0,
                text: value.to_string(),
                font_size: 13.0,
                color: value_color,
                font_weight: FontWeightHint::Bold,
                max_width: Some(200.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Additional settings
        let extra_y = top + 60.0 + settings.len() as f32 * 36.0 + 20.0;

        cmds.push(RenderCommand::Text {
            x: label_x,
            y: extra_y,
            text: "Seek Steps".to_string(),
            font_size: 14.0,
            color: self.palette.ink(self.palette.blue),
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });

        cmds.push(RenderCommand::Text {
            x: label_x + 8.0,
            y: extra_y + 24.0,
            text: format!(
                "Small: {}s | Large: {}s",
                prefs.seek_small_step / 1000,
                prefs.seek_large_step / 1000
            ),
            font_size: 12.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(300.0),
            overflow: TextOverflow::Ellipsis,
        });

        cmds.push(RenderCommand::Text {
            x: label_x,
            y: extra_y + 52.0,
            text: "Preferred Languages".to_string(),
            font_size: 14.0,
            color: self.palette.ink(self.palette.blue),
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });

        cmds.push(RenderCommand::Text {
            x: label_x + 8.0,
            y: extra_y + 76.0,
            text: format!(
                "Subtitle: {} | Audio: {}",
                prefs.subtitle_preferred_lang.as_deref().unwrap_or("Any"),
                prefs.audio_preferred_lang.as_deref().unwrap_or("Any")
            ),
            font_size: 12.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(400.0),
            overflow: TextOverflow::Ellipsis,
        });

        cmds.push(RenderCommand::Text {
            x: label_x,
            y: extra_y + 104.0,
            text: "Screenshots".to_string(),
            font_size: 14.0,
            color: self.palette.ink(self.palette.blue),
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });

        cmds.push(RenderCommand::Text {
            x: label_x + 8.0,
            y: extra_y + 128.0,
            text: format!(
                "Format: {} | Quality: {}% | Include subs: {}",
                prefs.screenshot_config.format.extension(),
                prefs.screenshot_config.quality,
                if prefs.screenshot_config.include_subtitles {
                    "Yes"
                } else {
                    "No"
                }
            ),
            font_size: 12.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(500.0),
            overflow: TextOverflow::Ellipsis,
        });

        cmds.push(RenderCommand::Text {
            x: label_x + 8.0,
            y: extra_y + 144.0,
            text: NO_SCREENSHOTS.to_owned(),
            font_size: 11.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(500.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_shortcuts(&self, cmds: &mut Vec<RenderCommand>) {
        let top = 40.0;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: top,
            width: self.width,
            height: self.height - top,
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: top + 20.0,
            text: "Keyboard Shortcuts".to_string(),
            font_size: 16.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(300.0),
            overflow: TextOverflow::Ellipsis,
        });

        let shortcuts = Shortcuts::list();
        let col1_x = 20.0;
        let col1_val = 140.0;
        let col2_x = self.width / 2.0 + 10.0;
        let col2_val = self.width / 2.0 + 130.0;

        let half = shortcuts.len().div_ceil(2);

        for (i, shortcut) in shortcuts.iter().enumerate() {
            let (kx, vx, row) = if i < half {
                (col1_x, col1_val, i)
            } else {
                (col2_x, col2_val, i.saturating_sub(half))
            };

            let sy = top + 52.0 + row as f32 * 24.0;

            // Key badge
            self.palette
                .push_surface(cmds, kx, sy - 1.0, 80.0, 20.0, 3.0, Surface::Card);
            cmds.push(RenderCommand::Text {
                x: kx + 4.0,
                y: sy + 2.0,
                text: shortcut.keys.to_string(),
                font_size: 11.0,
                color: self.palette.ink(self.palette.mauve),
                font_weight: FontWeightHint::Bold,
                max_width: Some(74.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Action
            cmds.push(RenderCommand::Text {
                x: vx,
                y: sy + 2.0,
                text: shortcut.action.to_string(),
                font_size: 11.0,
                color: self.palette.subtext1,
                font_weight: FontWeightHint::Regular,
                max_width: Some(180.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }
}

// ============================================================================
// Sample data for testing
// ============================================================================

#[cfg(test)]
fn sample_media_file() -> MediaFile {
    MediaFile {
        path: PathBuf::from("/home/user/Videos/sample.mkv"),
        file_name: "sample.mkv".to_string(),
        file_size: 1_500_000_000,
        container: ContainerFormat::Mkv,
        duration: Duration::from_secs(7200),
        video_streams: vec![VideoStream {
            index: 0,
            codec: Codec::H265,
            width: Some(3840),
            height: Some(2160),
            frame_rate: Some(23.976),
            bit_rate: Some(15_000_000),
            pixel_format: Some("yuv420p10le".to_string()),
            color_space: Some("bt2020nc".to_string()),
            hdr: true,
        }],
        audio_streams: vec![
            AudioStream {
                index: 1,
                codec: Codec::TrueHd,
                sample_rate: Some(48000),
                channels: Some(8),
                bit_rate: Some(4_500_000),
                language: Some("English".to_string()),
                title: Some("Dolby TrueHD 7.1".to_string()),
                is_default: true,
            },
            AudioStream {
                index: 2,
                codec: Codec::Aac,
                sample_rate: Some(48000),
                channels: Some(2),
                bit_rate: Some(192_000),
                language: Some("English".to_string()),
                title: Some("Stereo Commentary".to_string()),
                is_default: false,
            },
            AudioStream {
                index: 3,
                codec: Codec::Ac3,
                sample_rate: Some(48000),
                channels: Some(6),
                bit_rate: Some(640_000),
                language: Some("Spanish".to_string()),
                title: None,
                is_default: false,
            },
        ],
        subtitle_streams: vec![
            SubtitleStream {
                index: 4,
                codec: Codec::Pgs,
                language: Some("English".to_string()),
                title: Some("Full".to_string()),
                is_default: true,
                is_forced: false,
            },
            SubtitleStream {
                index: 5,
                codec: Codec::Text,
                language: Some("Spanish".to_string()),
                title: None,
                is_default: false,
                is_forced: false,
            },
            SubtitleStream {
                index: 6,
                codec: Codec::Ass,
                language: Some("English".to_string()),
                title: Some("Signs/Songs".to_string()),
                is_default: false,
                is_forced: true,
            },
        ],
        metadata: MediaMetadata {
            title: Some("Sample Movie".to_string()),
            artist: None,
            album: None,
            year: Some(2024),
            genre: Some("Sci-Fi".to_string()),
            comment: None,
            encoder: Some("x265".to_string()),
        },
    }
}

#[cfg(test)]
fn sample_chapters() -> Vec<Chapter> {
    vec![
        Chapter {
            title: "Opening Credits".to_string(),
            start: Duration::ZERO,
            end: Duration::from_secs(180),
        },
        Chapter {
            title: "Act I - The Beginning".to_string(),
            start: Duration::from_secs(180),
            end: Duration::from_secs(1800),
        },
        Chapter {
            title: "Act II - Rising Action".to_string(),
            start: Duration::from_secs(1800),
            end: Duration::from_secs(3600),
        },
        Chapter {
            title: "Act III - Climax".to_string(),
            start: Duration::from_secs(3600),
            end: Duration::from_secs(5400),
        },
        Chapter {
            title: "Act IV - Resolution".to_string(),
            start: Duration::from_secs(5400),
            end: Duration::from_secs(6900),
        },
        Chapter {
            title: "End Credits".to_string(),
            start: Duration::from_secs(6900),
            end: Duration::from_secs(7200),
        },
    ]
}

#[cfg(test)]
fn sample_subtitle_srt() -> &'static str {
    "1\n\
     00:00:05,000 --> 00:00:08,000\n\
     Welcome to the movie.\n\
     \n\
     2\n\
     00:00:10,500 --> 00:00:14,200\n\
     This is the second subtitle.\n\
     It spans two lines.\n\
     \n\
     3\n\
     00:01:00,000 --> 00:01:05,000\n\
     A minute into the movie.\n"
}

impl App for VideoPlayerApp {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    fn title(&self) -> String {
        // What is on screen, because that is what the window is. A player
        // behind three other windows is found again by its title.
        match &self.current_file {
            None => "Video Player".to_string(),
            Some(file) => {
                let name = &file.file_name;
                match self.state {
                    PlaybackState::Paused => format!("{name} (paused) - Video Player"),
                    PlaybackState::Buffering => format!("{name} (buffering) - Video Player"),
                    PlaybackState::Error => format!("{name} (error) - Video Player"),
                    PlaybackState::Playing | PlaybackState::Stopped => {
                        format!("{name} - Video Player")
                    }
                }
            }
        }
    }

    fn initial_size(&self) -> (u32, u32) {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "both are positive and well inside u32"
        )]
        {
            (self.width as u32, self.height as u32)
        }
    }

    /// A clock while a film is playing or a message is on its way out.
    ///
    /// A paused player with the controls up has nothing to advance, and waking
    /// the machine to establish that is `known-issues.md` lesson 47.
    fn tick_interval(&self) -> Option<std::time::Duration> {
        self.has_work().then_some(FRAME_TICK)
    }

    fn on_event(&mut self, event: &Event) -> Response {
        match event {
            Event::CloseRequested => Response::Exit,
            Event::Resize { width, height } => {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a window dimension in pixels is exact in f32"
                )]
                let (w, h) = (*width as f32, *height as f32);
                if self.set_window_size(w, h) {
                    Response::Redraw
                } else {
                    Response::Idle
                }
            }
            other => {
                if self.handle_event(other) {
                    Response::Redraw
                } else {
                    Response::Idle
                }
            }
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // The handed size wins over the recorded one: the first frame is drawn
        // before any `Event::Resize` arrives, so a window opened at another
        // size would be laid out for the size that was asked for, and every
        // hit box in it would name the wrong rectangle.
        self.set_window_size(width, height);
        RenderTree {
            commands: self.render_commands(),
        }
    }
}

/// Sample content, for tests.
///
/// `#[cfg(test)]` since 2026-09-15. Its own doc said it existed "so the first
/// window is not an empty black rectangle", which is the fourth time that
/// reasoning has turned up in this sweep -- the torrent client's "looks broken
/// rather than idle", the photo manager's "so the first window is not an empty
/// grid", the file searcher's "until a real index exists this is what there is
/// to search". Every one of them was right that an empty window looks broken,
/// and every one of them drew the wrong conclusion. **The answer to a window
/// that looks broken is to say why it is empty, not to fill it.**
#[cfg(test)]
fn seeded_player() -> VideoPlayerApp {
    let mut app = VideoPlayerApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
    app.decodes = true;
    app.current_file = Some(sample_media_file());
    app.chapters = sample_chapters();
    app.selected_audio_track = Some(1);
    app.selected_subtitle_track = Some(4);
    app.external_subtitles = parse_srt(sample_subtitle_srt());
    app.playlist.add(
        PathBuf::from("/home/user/Videos/sample.mkv"),
        "sample.mkv".to_string(),
        Some(Duration::from_secs(7200)),
        Some("Sample Movie".to_string()),
    );
    app.playlist.add(
        PathBuf::from("/home/user/Videos/trailer.mp4"),
        "trailer.mp4".to_string(),
        Some(Duration::from_secs(120)),
        None,
    );
    app.playlist.add(
        PathBuf::from("/home/user/Videos/concert.webm"),
        "concert.webm".to_string(),
        Some(Duration::from_secs(5400)),
        Some("Live Concert 2024".to_string()),
    );
    app.playlist.set_current(0);
    app
}

fn main() -> ExitCode {
    // Opens empty, or on the files it is given. It used to call
    // `seeded_player`, so every launch began with a two-hour "Sample Movie" at
    // /home/user/Videos/sample.mkv, chapters, external subtitles and a
    // playlist -- and the clock ran when you pressed play.
    //
    // Parsed here rather than by `app::launch`, which refuses every argument
    // but `--display`: the file manager opens a video by naming it, and the
    // refusal ended the player before its window opened.
    let args = match app::Args::from_env() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("videoplayer: {e}");
            return ExitCode::from(2);
        }
    };
    let mut app = VideoPlayerApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
    for why in open_arguments(&mut app, &args.rest) {
        eprintln!("videoplayer: {why}");
    }
    app::launch_with("videoplayer", args.display.as_deref(), &mut app)
}

/// Open the videos named on the command line: the first shown, the rest
/// listed after it. Returns why each that could not be read was not.
fn open_arguments(app: &mut VideoPlayerApp, paths: &[String]) -> Vec<String> {
    let mut failed = Vec::new();
    let mut shown = false;
    for path in paths.iter().map(Path::new) {
        let result = if shown {
            app.add_path(path).map(drop)
        } else {
            match app.add_path(path) {
                Ok(index) => {
                    app.playlist.set_current(index);
                    app.load_playlist_entry(index);
                    shown = app.current_file.is_some();
                    Ok(())
                }
                Err(why) => Err(why),
            }
        };
        if let Err(why) = result {
            failed.push(why);
        }
    }
    if let Some(last) = failed.last() {
        app.show_osd(last);
    }
    failed
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

    /// A fresh player holds no file and no playlist.
    ///
    /// `main` called `seeded_player`, so every launch opened on a two-hour
    /// "Sample Movie" at `/home/user/Videos/sample.mkv`, with chapters,
    /// external subtitles and a playlist -- and the clock ran when you pressed
    /// play, with subtitles appearing on cue over a black rectangle.
    ///
    /// The playlist is the part that outlives the window. Entries naming a
    /// path are a claim that a file is at that path, and a playlist is the
    /// kind of thing somebody reads later to find out what they have.
    ///
    /// Note where the seeding lived: in `main`, not in `new`. This is the
    /// second application of eighteen where that was true, and the second
    /// where removing the fabrication broke no tests at all -- 151 passed
    /// before and after. The other was `apps/email`.
    /// The screenshot options are drawn with the fact that none can be taken.
    ///
    /// **`Ctrl+S` was already removed from `Shortcuts::list` as impossible**:
    /// no frame is decoded, so there is nothing to take. The settings block
    /// was left behind, still naming a format, a quality and a subtitle option.
    ///
    /// `CANNOT_PLAY_LINES` does say nothing decodes -- but it is drawn where
    /// the picture would be. Relying on a reader having passed it before reaching a
    /// settings panel is exactly what `apps/mediaconvert` got wrong: three
    /// lines about the queue did not reach the panel that configured it.
    /// **C opens the chapter list, and it says so when there are none.**
    ///
    /// `chapter_list_visible` was declared, initialised and inverted by `C`
    /// and read by nothing, while `("C", "Toggle Chapter List")` sat in the
    /// shortcut table this window both draws from and dispatches on -- so the
    /// key was advertised and moved nothing. Third of the five dead toggles
    /// where the card is what turned a dormant field into a claim.
    ///
    /// The empty case is asserted, not assumed: `chapters` is empty until a
    /// video with marks is loaded, and a panel that opens blank reads as a
    /// broken control rather than an empty one.
    #[test]
    fn the_chapter_list_opens_and_says_when_there_is_nothing_in_it() {
        let drawn = |app: &mut VideoPlayerApp| -> Vec<String> {
            app.render_commands()
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Text { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect()
        };

        let mut app = VideoPlayerApp::new(1000.0, 700.0);
        assert!(
            !drawn(&mut app).iter().any(|t| t.starts_with("Chapters")),
            "the list is up before anybody asked for it"
        );

        app.chapter_list_visible = true;
        let shown = drawn(&mut app);
        assert!(
            shown.iter().any(|t| t.starts_with("Chapters")),
            "C opened nothing: {shown:?}"
        );
        assert!(
            shown
                .iter()
                .any(|t| t == "This video has no chapter marks."),
            "an empty chapter list drew an empty box and said nothing"
        );

        // With chapters, each one is a row and the one being played is named.
        app.chapters = vec![
            Chapter {
                title: String::from("Opening"),
                start: Duration::from_secs(0),
                end: Duration::from_secs(60),
            },
            Chapter {
                title: String::from("The middle"),
                start: Duration::from_secs(60),
                end: Duration::from_secs(120),
            },
        ];
        let shown = drawn(&mut app);
        assert!(
            shown.iter().any(|t| t.contains("Opening")),
            "a chapter is missing from the list: {shown:?}"
        );
        assert!(
            shown.iter().any(|t| t.contains("The middle")),
            "a chapter is missing from the list: {shown:?}"
        );
        assert!(
            !shown
                .iter()
                .any(|t| t == "This video has no chapter marks."),
            "the list still claims there are none"
        );
    }

    #[test]
    fn the_screenshot_options_say_no_screenshot_can_be_taken() {
        let mut app = VideoPlayerApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.active_tab = PlayerTab::Settings;
        let texts: Vec<String> = app
            .render_commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();

        assert!(
            texts.iter().any(|t| t == "Screenshots"),
            "control: the panel must be drawing the screenshot block for this \
test to be about anything -- it drew {} text command(s)",
            texts.len()
        );
        assert!(
            texts
                .iter()
                .any(|t| t.contains("Not applied") && t.contains("screenshot")),
            "the panel offered screenshot options and did not say none can be taken"
        );
    }

    /// A scratch directory of the test's own, removed when dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!(
                "slateos-videoplayer-{tag}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&dir).expect("scratch dir");
            Self(dir)
        }

        fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, bytes).expect("write");
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            drop(std::fs::remove_dir_all(&self.0));
        }
    }

    fn texts(app: &VideoPlayerApp) -> Vec<String> {
        app.render_commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// A video opened is read for what it holds: its container, length and
    /// tracks come from the file, not from a sample.
    #[test]
    fn a_video_opened_is_read_for_what_it_holds() {
        let dir = Scratch::new("open");
        let path = dir.file("film.mp4", &mediaprobe::testing::mp4(1920, 1080, 90, 25));
        let mut app = VideoPlayerApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let said = app.open_path(&path);
        let file = app
            .current_file
            .as_ref()
            .unwrap_or_else(|| panic!("not opened: {said}"));
        assert_eq!(file.container, ContainerFormat::Mp4);
        assert_eq!(file.duration, Duration::from_secs(90));
        let video = file.primary_video().expect("a picture");
        assert_eq!(video.codec, Codec::H264);
        assert_eq!(video.size_label().as_deref(), Some("1920x1080"));
        assert_eq!(video.frame_rate, Some(25.0));
        let audio = file.primary_audio().expect("sound");
        assert_eq!(
            (audio.codec.clone(), audio.sample_rate, audio.channels),
            (Codec::Aac, Some(48_000), Some(2))
        );
        assert_eq!(app.playlist.len(), 1, "an opened file is listed");
        assert_eq!(
            app.playlist.entries()[0].duration,
            Some(Duration::from_secs(90))
        );
        assert_eq!(app.state, PlaybackState::Stopped);
        assert_eq!(
            app.selected_audio_track,
            Some(audio.index),
            "its own sound is chosen"
        );
        // Where the picture would be: that there is none, and what it is.
        let shown = texts(&app);
        for line in NO_PICTURE_LINES {
            assert!(shown.iter().any(|t| t == line), "never said {line:?}");
        }
        assert!(shown.iter().any(|t| t == "1920x1080 H.264"), "{shown:?}");
        assert!(
            !shown
                .iter()
                .any(|t| CANNOT_PLAY_LINES.contains(&t.as_str()))
        );
    }

    /// Play says why it will not, rather than running a clock over a black
    /// picture: nothing here decodes a frame.
    #[test]
    fn play_says_there_is_no_decoder_rather_than_pretending() {
        let dir = Scratch::new("play");
        let path = dir.file("film.mkv", &mediaprobe::testing::mkv(640, 360, 30, 24));
        let mut app = VideoPlayerApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.open_path(&path);
        app.handle_event(&press(Key::Space));
        assert_eq!(app.state, PlaybackState::Stopped);
        assert_eq!(app.osd_message.as_deref(), Some(CANNOT_DECODE));
        app.tick(5000);
        assert_eq!(
            app.position,
            Duration::ZERO,
            "the clock ran with nothing playing"
        );
    }

    /// Every kind of file this reads opens; what is not a video, a folder or
    /// nothing at all says so, and the player keeps what it had.
    #[test]
    fn what_cannot_be_opened_says_why() {
        let dir = Scratch::new("refuse");
        let mut app = VideoPlayerApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        for (name, bytes) in [
            ("a.webm", mediaprobe::testing::webm(320, 240, 5, 30)),
            ("b.avi", mediaprobe::testing::avi(320, 240, 5, 25)),
        ] {
            let said = app.open_path(&dir.file(name, &bytes));
            assert!(said.starts_with("Opened"), "{name}: {said}");
        }
        assert_eq!(
            app.current_file.as_ref().unwrap().container,
            ContainerFormat::Avi
        );
        let text = dir.file("notes.txt", b"not a video");
        assert!(
            app.open_path(&text)
                .contains("not a video this player knows")
        );
        assert!(app.open_path(&dir.0).contains("it is a folder"));
        assert!(
            app.open_path(&dir.0.join("gone.mp4"))
                .starts_with("Could not open")
        );
        assert_eq!(
            app.playlist.len(),
            2,
            "nothing was listed for a file that did not open"
        );
        assert_eq!(
            app.current_file.as_ref().unwrap().container,
            ContainerFormat::Avi,
            "a failed open kept the file that was open"
        );
        // A file whose bytes this does not read, named as a video it knows,
        // is listed by its name with nothing read: FLV is a container this
        // player names and nothing here parses.
        let flv = dir.file("old.flv", b"FLV\x01 not parsed here");
        assert!(app.open_path(&flv).starts_with("Opened"));
        let file = app.current_file.as_ref().unwrap();
        assert_eq!(file.container, ContainerFormat::Flv);
        assert!(file.video_streams.is_empty());
    }

    /// Ctrl+O puts up the picker; a file chosen there opens.
    #[test]
    fn ctrl_o_opens_the_picker() {
        let mut app = VideoPlayerApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(app.handle_event(&press_with(Key::O, ctrl())));
        assert!(app.picker.is_open());
        // It takes the keys while it is up: Space is not Play.
        app.handle_event(&press(Key::Escape));
        assert!(!app.picker.is_open());
        assert!(
            Shortcuts::list().iter().any(|s| s.keys == "Ctrl+O"),
            "the help panel does not say how to open a file"
        );
    }

    /// Stepping through the playlist opens each entry's file: it said "Now
    /// playing" and changed nothing but the clock.
    #[test]
    fn the_next_entry_is_its_own_file() {
        let dir = Scratch::new("next");
        let first = dir.file("one.mp4", &mediaprobe::testing::mp4(640, 480, 10, 25));
        let second = dir.file("two.mkv", &mediaprobe::testing::mkv(1280, 720, 20, 24));
        let mut app = VideoPlayerApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let failed = open_arguments(
            &mut app,
            &[first.display().to_string(), second.display().to_string()],
        );
        assert!(failed.is_empty(), "{failed:?}");
        assert_eq!(app.playlist.len(), 2);
        assert_eq!(
            app.current_file.as_ref().unwrap().file_name,
            "one.mp4",
            "the first is shown"
        );
        app.playlist_next();
        let file = app.current_file.as_ref().expect("the next file");
        assert_eq!(file.file_name, "two.mkv");
        assert_eq!(file.duration, Duration::from_secs(20));
        assert_eq!(
            file.primary_video().unwrap().size_label().as_deref(),
            Some("1280x720")
        );
        // An entry whose file has gone says so, and shows nothing.
        std::fs::remove_file(&first).expect("remove");
        app.playlist_previous();
        assert!(app.current_file.is_none());
        assert!(
            app.osd_message
                .as_deref()
                .is_some_and(|m| m.starts_with("Could not open"))
        );
    }

    /// A command line naming files that cannot be read says so for each, and
    /// opens the ones that can.
    #[test]
    fn the_command_line_opens_what_it_can_and_names_what_it_cannot() {
        let dir = Scratch::new("args");
        let good = dir.file("good.avi", &mediaprobe::testing::avi(320, 240, 4, 25));
        let mut app = VideoPlayerApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let failed = open_arguments(
            &mut app,
            &[
                dir.0.join("missing.mp4").display().to_string(),
                good.display().to_string(),
            ],
        );
        assert_eq!(failed.len(), 1);
        assert!(failed[0].contains("missing.mp4"), "{failed:?}");
        assert_eq!(app.current_file.as_ref().unwrap().file_name, "good.avi");
        assert!(open_arguments(&mut VideoPlayerApp::new(1.0, 1.0), &[]).is_empty());
    }

    #[test]
    fn a_fresh_player_holds_no_file_and_no_playlist() {
        let app = VideoPlayerApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(
            app.current_file.is_none(),
            "a media file appeared from nowhere"
        );
        assert!(app.chapters.is_empty(), "chapters appeared from nowhere");
        assert!(
            app.external_subtitles.is_empty(),
            "subtitles appeared from nowhere"
        );
        assert!(app.playlist.is_empty(), "a playlist appeared from nowhere");
    }

    /// And the window says why, rather than showing a black rectangle.
    ///
    /// A black rectangle is what the sample content existed to avoid, and the
    /// reasoning was right about the symptom: an empty window does look
    /// broken. It was wrong about the remedy. This is the same shape as the
    /// torrent client's "looks broken rather than idle" and the photo
    /// manager's "so the first window is not an empty grid".
    #[test]
    fn the_window_says_it_cannot_play() {
        let app = VideoPlayerApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let texts: Vec<String> = app
            .render_commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        for line in CANNOT_PLAY_LINES {
            assert!(
                texts.iter().any(|t| t == line),
                "the window never said {line:?}"
            );
        }
        assert!(
            CANNOT_PLAY_LINES
                .iter()
                .any(|l| l.contains("not a list of files you have")),
            "nothing forecloses reading the empty playlist as a library",
        );
    }

    // Duration tests
    #[test]
    fn test_duration_from_secs() {
        assert_eq!(Duration::from_secs(5).as_millis(), 5000);
    }

    #[test]
    fn test_duration_format_short() {
        assert_eq!(Duration::from_secs(65).format(), "1:05");
    }

    #[test]
    fn test_duration_format_long() {
        assert_eq!(Duration::from_secs(3661).format(), "1:01:01");
    }

    #[test]
    fn test_duration_format_precise() {
        assert_eq!(
            Duration::from_millis(3661500).format_precise(),
            "01:01:01.500"
        );
    }

    #[test]
    fn test_duration_progress_of() {
        let pos = Duration::from_secs(50);
        let total = Duration::from_secs(100);
        assert!((pos.progress_of(total) - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_duration_progress_of_zero_total() {
        assert_eq!(Duration::from_secs(50).progress_of(Duration::ZERO), 0.0);
    }

    #[test]
    fn test_duration_saturating_ops() {
        assert_eq!(
            Duration::from_secs(5).saturating_sub(Duration::from_secs(10)),
            Duration::ZERO
        );
        assert_eq!(
            Duration::from_secs(5).saturating_add(Duration::from_secs(3)),
            Duration::from_secs(8)
        );
    }

    // Container format tests
    #[test]
    fn test_container_from_extension() {
        assert_eq!(
            ContainerFormat::from_extension("mp4"),
            Some(ContainerFormat::Mp4)
        );
        assert_eq!(
            ContainerFormat::from_extension("MKV"),
            Some(ContainerFormat::Mkv)
        );
        assert_eq!(ContainerFormat::from_extension("xyz"), None);
    }

    #[test]
    fn test_container_extensions() {
        let exts = ContainerFormat::Mp4.typical_extensions();
        assert!(exts.contains(&"mp4"));
        assert!(exts.contains(&"m4v"));
    }

    fn video_stream(width: u32, height: u32) -> VideoStream {
        VideoStream {
            index: 0,
            codec: Codec::H264,
            width: Some(width),
            height: Some(height),
            frame_rate: Some(24.0),
            bit_rate: None,
            pixel_format: None,
            color_space: None,
            hdr: false,
        }
    }

    #[test]
    fn a_codec_is_lossless_and_a_subtitle_is_text_by_its_codec() {
        assert!(is_lossless(&Codec::Flac));
        assert!(is_lossless(&Codec::TrueHd));
        assert!(is_lossless(&Codec::Alac));
        assert!(!is_lossless(&Codec::Aac));
        assert!(is_text_subtitle(&Codec::Text));
        assert!(is_text_subtitle(&Codec::WebVtt));
        assert!(!is_text_subtitle(&Codec::Pgs));
        assert!(!is_text_subtitle(&Codec::VobSub));
    }

    #[test]
    fn test_channel_layout_name() {
        assert_eq!(channel_layout_name(2), "Stereo");
        assert_eq!(channel_layout_name(6), "5.1");
        assert_eq!(channel_layout_name(8), "7.1");
    }

    // Video stream tests
    #[test]
    fn test_resolution_label() {
        assert_eq!(
            video_stream(1920, 1080).resolution_label(),
            Some("1080p (Full HD)")
        );
        let mut unknown = video_stream(1920, 1080);
        unknown.height = None;
        assert_eq!(unknown.resolution_label(), None, "no height, no label");
    }

    #[test]
    fn test_aspect_ratio() {
        assert_eq!(
            video_stream(1920, 1080).aspect_ratio().as_deref(),
            Some("16:9")
        );
        assert_eq!(
            video_stream(640, 480).aspect_ratio().as_deref(),
            Some("4:3")
        );
        assert_eq!(video_stream(640, 0).aspect_ratio(), None);
        assert_eq!(
            video_stream(1920, 1080).size_label().as_deref(),
            Some("1920x1080")
        );
    }

    // Audio stream tests
    #[test]
    fn test_audio_display_label() {
        let stream = AudioStream {
            index: 1,
            codec: Codec::Aac,
            sample_rate: Some(48000),
            channels: Some(2),
            bit_rate: None,
            language: Some("eng".to_string()),
            title: None,
            is_default: true,
        };
        assert_eq!(stream.display_label(), "eng - AAC Stereo");
        let no_channels = AudioStream {
            channels: None,
            ..stream
        };
        assert_eq!(no_channels.display_label(), "eng - AAC");
    }

    #[test]
    fn test_audio_display_label_with_title() {
        let stream = AudioStream {
            index: 1,
            codec: Codec::Ac3,
            sample_rate: Some(48000),
            channels: Some(6),
            bit_rate: None,
            language: Some("eng".to_string()),
            title: Some("Commentary".to_string()),
            is_default: false,
        };
        assert_eq!(stream.display_label(), "Commentary (eng) - AC-3 5.1");
    }

    // Subtitle tests
    #[test]
    fn test_subtitle_display_label() {
        let sub = SubtitleStream {
            index: 0,
            codec: Codec::Text,
            language: Some("eng".to_string()),
            title: None,
            is_default: true,
            is_forced: false,
        };
        assert_eq!(sub.display_label(), "eng (Text)");
    }

    #[test]
    fn test_subtitle_forced_label() {
        let sub = SubtitleStream {
            index: 0,
            codec: Codec::Pgs,
            language: Some("eng".to_string()),
            title: Some("Signs".to_string()),
            is_default: false,
            is_forced: true,
        };
        assert!(sub.display_label().contains("[Forced]"));
    }

    // SRT parsing tests
    #[test]
    fn test_parse_srt() {
        let srt = "1\n00:00:05,000 --> 00:00:08,000\nHello world\n\n2\n00:00:10,500 --> 00:00:14,200\nSecond line\n";
        let cues = parse_srt(srt);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start, Duration::from_millis(5000));
        assert_eq!(cues[0].end, Duration::from_millis(8000));
        assert_eq!(cues[0].text, "Hello world");
        assert_eq!(cues[1].start, Duration::from_millis(10500));
        assert_eq!(cues[1].text, "Second line");
    }

    #[test]
    fn test_parse_srt_multiline() {
        let srt = "1\n00:01:00,000 --> 00:01:05,000\nLine one\nLine two\n\n";
        let cues = parse_srt(srt);
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "Line one\nLine two");
    }

    #[test]
    fn test_parse_srt_time() {
        let time = parse_srt_time("01:30:45,678").unwrap();
        assert_eq!(time.as_millis(), 5445678);
    }

    // WebVTT parsing tests
    #[test]
    fn test_parse_webvtt() {
        let vtt = "WEBVTT\n\n00:00:05.000 --> 00:00:08.000\nHello\n\n00:00:10.000 --> 00:00:15.000\nWorld\n";
        let cues = parse_webvtt(vtt);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "Hello");
        assert_eq!(cues[1].start, Duration::from_millis(10000));
    }

    #[test]
    fn test_parse_webvtt_strips_tags() {
        let vtt = "WEBVTT\n\n00:00:01.000 --> 00:00:05.000\n<b>Bold</b> and <i>italic</i>\n";
        let cues = parse_webvtt(vtt);
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "Bold and italic");
    }

    // Subtitle cue tests
    #[test]
    fn test_subtitle_cue_active() {
        let cue = SubtitleCue {
            start: Duration::from_secs(10),
            end: Duration::from_secs(15),
            text: "Test".to_string(),
            style: None,
        };
        assert!(cue.is_active_at(Duration::from_secs(12)));
        assert!(!cue.is_active_at(Duration::from_secs(5)));
        assert!(!cue.is_active_at(Duration::from_secs(15)));
    }

    #[test]
    fn test_subtitle_cue_duration() {
        let cue = SubtitleCue {
            start: Duration::from_secs(10),
            end: Duration::from_secs(15),
            text: "Test".to_string(),
            style: None,
        };
        assert_eq!(cue.display_duration(), Duration::from_secs(5));
    }

    // Volume tests
    #[test]
    fn test_volume_default() {
        let vol = Volume::default();
        assert_eq!(vol.level(), 100);
        assert!(!vol.is_muted());
    }

    #[test]
    fn test_volume_increase_decrease() {
        let mut vol = Volume::new(50);
        vol.increase(10);
        assert_eq!(vol.level(), 60);
        vol.decrease(20);
        assert_eq!(vol.level(), 40);
    }

    #[test]
    fn test_volume_clamp() {
        let mut vol = Volume::new(145);
        vol.increase(20);
        assert_eq!(vol.level(), Volume::MAX);
    }

    #[test]
    fn test_volume_mute() {
        let mut vol = Volume::new(80);
        assert_eq!(vol.effective_level(), 80);
        vol.toggle_mute();
        assert_eq!(vol.effective_level(), 0);
        assert!(vol.is_muted());
        vol.toggle_mute();
        assert_eq!(vol.effective_level(), 80);
    }

    // Playback speed tests
    #[test]
    fn test_speed_increase() {
        let speed = PlaybackSpeed::NORMAL.increase();
        assert!((speed.value() - 1.25).abs() < 0.01);
    }

    #[test]
    fn test_speed_decrease() {
        let speed = PlaybackSpeed::NORMAL.decrease();
        assert!((speed.value() - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_speed_label() {
        assert_eq!(PlaybackSpeed::NORMAL.label(), "1x");
        assert_eq!(PlaybackSpeed::DOUBLE.label(), "2x");
    }

    // Repeat mode tests
    #[test]
    fn test_repeat_cycle() {
        assert_eq!(RepeatMode::Off.cycle(), RepeatMode::One);
        assert_eq!(RepeatMode::One.cycle(), RepeatMode::All);
        assert_eq!(RepeatMode::All.cycle(), RepeatMode::Off);
    }

    // Sync offset tests
    #[test]
    fn test_sync_offset() {
        let mut sync = SyncOffset::ZERO;
        sync.adjust(500);
        assert_eq!(sync.ms, 500);
        sync.adjust(-1000);
        assert_eq!(sync.ms, -500);
    }

    #[test]
    fn test_sync_offset_clamp() {
        let mut sync = SyncOffset::ZERO;
        sync.adjust(20_000);
        assert_eq!(sync.ms, 10_000);
    }

    // Aspect mode tests
    #[test]
    fn test_aspect_cycle() {
        assert_eq!(AspectMode::Fit.cycle(), AspectMode::Fill);
        assert_eq!(AspectMode::Fill.cycle(), AspectMode::Stretch);
        assert_eq!(AspectMode::Original.cycle(), AspectMode::Fit);
    }

    // Playlist tests
    #[test]
    fn test_playlist_add_remove() {
        let mut pl = Playlist::new();
        pl.add(PathBuf::from("a.mp4"), "a.mp4".to_string(), None, None);
        pl.add(PathBuf::from("b.mp4"), "b.mp4".to_string(), None, None);
        assert_eq!(pl.len(), 2);
        pl.remove(0);
        assert_eq!(pl.len(), 1);
        assert_eq!(pl.entries()[0].file_name, "b.mp4");
    }

    #[test]
    fn test_playlist_next_sequential() {
        let mut pl = Playlist::new();
        pl.add(PathBuf::from("a.mp4"), "a.mp4".to_string(), None, None);
        pl.add(PathBuf::from("b.mp4"), "b.mp4".to_string(), None, None);
        pl.add(PathBuf::from("c.mp4"), "c.mp4".to_string(), None, None);
        pl.set_current(0);
        assert_eq!(pl.next(RepeatMode::Off), Some(1));
        assert_eq!(pl.next(RepeatMode::Off), Some(2));
        assert_eq!(pl.next(RepeatMode::Off), None);
    }

    #[test]
    fn test_playlist_next_repeat_all() {
        let mut pl = Playlist::new();
        pl.add(PathBuf::from("a.mp4"), "a.mp4".to_string(), None, None);
        pl.add(PathBuf::from("b.mp4"), "b.mp4".to_string(), None, None);
        pl.set_current(1);
        assert_eq!(pl.next(RepeatMode::All), Some(0));
    }

    #[test]
    fn test_playlist_next_repeat_one() {
        let mut pl = Playlist::new();
        pl.add(PathBuf::from("a.mp4"), "a.mp4".to_string(), None, None);
        pl.add(PathBuf::from("b.mp4"), "b.mp4".to_string(), None, None);
        pl.set_current(0);
        assert_eq!(pl.next(RepeatMode::One), Some(0));
    }

    #[test]
    fn test_playlist_previous() {
        let mut pl = Playlist::new();
        pl.add(PathBuf::from("a.mp4"), "a.mp4".to_string(), None, None);
        pl.add(PathBuf::from("b.mp4"), "b.mp4".to_string(), None, None);
        pl.set_current(1);
        assert_eq!(pl.previous(), Some(0));
    }

    #[test]
    fn test_playlist_clear() {
        let mut pl = Playlist::new();
        pl.add(PathBuf::from("a.mp4"), "a.mp4".to_string(), None, None);
        pl.set_current(0);
        pl.clear();
        assert!(pl.is_empty());
        assert!(pl.current_index().is_none());
    }

    #[test]
    fn test_playlist_move_entry() {
        let mut pl = Playlist::new();
        pl.add(PathBuf::from("a.mp4"), "a.mp4".to_string(), None, None);
        pl.add(PathBuf::from("b.mp4"), "b.mp4".to_string(), None, None);
        pl.add(PathBuf::from("c.mp4"), "c.mp4".to_string(), None, None);
        pl.set_current(0);
        pl.move_entry(0, 2);
        assert_eq!(pl.entries()[0].file_name, "b.mp4");
        assert_eq!(pl.entries()[2].file_name, "a.mp4");
        assert_eq!(pl.current_index(), Some(2));
    }

    #[test]
    fn test_playlist_shuffle() {
        let mut pl = Playlist::new();
        for i in 0..10 {
            pl.add(
                PathBuf::from(format!("{i}.mp4")),
                format!("{i}.mp4"),
                None,
                None,
            );
        }
        pl.set_current(0);
        pl.toggle_shuffle();
        assert!(pl.is_shuffle());
        // Shuffle should still return valid indices
        for _ in 0..9 {
            let next = pl.next(RepeatMode::All);
            assert!(next.is_some());
            assert!(next.unwrap() < 10);
        }
    }

    /// Build a playlist of `len` items and take `rebuilds` shuffle orders from
    /// it by switching shuffle off and on again.
    fn repeated_shuffle_orders(seed: u64, len: usize, rebuilds: usize) -> Vec<Vec<usize>> {
        let mut pl = Playlist::with_seed(seed);
        for i in 0..len {
            pl.add(
                PathBuf::from(format!("{i}.mp4")),
                format!("{i}.mp4"),
                None,
                None,
            );
        }
        (0..rebuilds)
            .map(|_| {
                pl.toggle_shuffle();
                if !pl.is_shuffle() {
                    pl.toggle_shuffle();
                }
                let order = pl.shuffle_order.clone();
                pl.toggle_shuffle();
                order
            })
            .collect()
    }

    /// Reshuffling has to reshuffle.
    ///
    /// The regression test for the entry-ID seed: because the IDs of a
    /// playlist do not change, `rebuild_shuffle_order` was a pure function of
    /// the playlist and ten consecutive rebuilds gave one distinct order --
    /// measured, on a ten-item list. Both callers depend on getting a new
    /// answer: `toggle_shuffle` off-and-on, and the wrap under
    /// `RepeatMode::All`, which otherwise loops one permutation forever.
    #[test]
    fn reshuffling_a_playlist_gives_a_different_order() {
        const REBUILDS: usize = 10;
        let orders = repeated_shuffle_orders(0x5EED_5EED_5EED_5EED, 10, REBUILDS);
        let distinct: std::collections::HashSet<&Vec<usize>> = orders.iter().collect();
        assert!(
            distinct.len() >= REBUILDS - 1,
            "{} distinct orders over {REBUILDS} rebuilds of the same playlist",
            distinct.len()
        );
    }

    /// Every item must be able to come first.
    ///
    /// A shuffle that is a fixed permutation always starts on the same track;
    /// this asks for the opposite over a long enough run. Measured on the old
    /// code it saw exactly one first-track (index 2) in any number of trials.
    #[test]
    fn shuffle_can_start_on_any_track() {
        let orders = repeated_shuffle_orders(0xF1D0_1234_ABCD_5678, 6, 60);
        let firsts: std::collections::HashSet<usize> =
            orders.iter().filter_map(|o| o.first().copied()).collect();
        assert_eq!(
            firsts.len(),
            6,
            "only these tracks ever came first: {firsts:?}"
        );
    }

    /// A fresh playlist is seeded by the system, not by a literal.
    ///
    /// Host `cargo test` has no SlateOS entropy source, so `seeded_from_system`
    /// returns the fallback and two fresh playlists agree -- exactly as a
    /// hardcoded seed would. The test therefore asserts *which* seed. Gated off
    /// Unix, where the host does have entropy and a fresh playlist is genuinely
    /// unpredictable.
    #[cfg(not(unix))]
    #[test]
    fn a_fresh_playlist_is_seeded_by_the_system_and_not_by_a_literal() {
        fn first_order(mut pl: Playlist) -> Vec<usize> {
            for i in 0..12 {
                pl.add(
                    PathBuf::from(format!("{i}.mp4")),
                    format!("{i}.mp4"),
                    None,
                    None,
                );
            }
            pl.toggle_shuffle();
            pl.shuffle_order.clone()
        }
        let fresh = first_order(Playlist::new());
        assert_eq!(fresh, first_order(Playlist::with_seed(FALLBACK_SEED)));
        assert_ne!(fresh, first_order(Playlist::with_seed(12345)));
    }

    #[test]
    fn test_playlist_total_duration() {
        let mut pl = Playlist::new();
        pl.add(
            PathBuf::from("a.mp4"),
            "a.mp4".to_string(),
            Some(Duration::from_secs(60)),
            None,
        );
        pl.add(
            PathBuf::from("b.mp4"),
            "b.mp4".to_string(),
            Some(Duration::from_secs(120)),
            None,
        );
        pl.add(PathBuf::from("c.mp4"), "c.mp4".to_string(), None, None);
        assert_eq!(pl.total_duration(), Duration::from_secs(180));
    }

    // Chapter tests
    #[test]
    fn test_chapter_contains() {
        let ch = Chapter {
            title: "Test".to_string(),
            start: Duration::from_secs(10),
            end: Duration::from_secs(20),
        };
        assert!(ch.contains(Duration::from_secs(15)));
        assert!(!ch.contains(Duration::from_secs(5)));
        assert!(!ch.contains(Duration::from_secs(20)));
    }

    #[test]
    fn test_chapter_duration() {
        let ch = Chapter {
            title: "Test".to_string(),
            start: Duration::from_secs(100),
            end: Duration::from_secs(200),
        };
        assert_eq!(ch.duration(), Duration::from_secs(100));
    }

    // Bookmark tests
    #[test]
    fn test_bookmark_display() {
        let bm = Bookmark {
            id: 1,
            position: Duration::from_secs(65),
            label: "Cool scene".to_string(),
            is_chapter: false,
        };
        assert_eq!(bm.display(), "1:05 - Cool scene");
    }

    // Equalizer tests
    #[test]
    fn test_equalizer_default() {
        let eq = Equalizer::new();
        assert!(!eq.enabled);
        assert_eq!(eq.bands.len(), 10);
        assert_eq!(eq.preset, EqPreset::Flat);
        assert!(eq.bands.iter().all(|b| b.gain == 0.0));
    }

    #[test]
    fn test_equalizer_apply_preset() {
        let mut eq = Equalizer::new();
        eq.apply_preset(EqPreset::Rock);
        assert_eq!(eq.preset, EqPreset::Rock);
        assert!(eq.bands[0].gain > 0.0); // Bass boost for rock
    }

    #[test]
    fn test_equalizer_custom_band() {
        let mut eq = Equalizer::new();
        eq.set_band_gain(0, 8.5);
        assert!((eq.bands[0].gain - 8.5).abs() < 0.01);
        assert_eq!(eq.preset, EqPreset::Custom);
    }

    #[test]
    fn test_equalizer_band_clamp() {
        let mut eq = Equalizer::new();
        eq.set_band_gain(0, 20.0);
        assert_eq!(eq.bands[0].gain, 12.0);
        eq.set_band_gain(0, -20.0);
        assert_eq!(eq.bands[0].gain, -12.0);
    }

    #[test]
    fn test_equalizer_reset() {
        let mut eq = Equalizer::new();
        eq.apply_preset(EqPreset::Bass);
        eq.preamp = 5.0;
        eq.reset();
        assert_eq!(eq.preset, EqPreset::Flat);
        assert_eq!(eq.preamp, 0.0);
        assert!(eq.bands.iter().all(|b| b.gain == 0.0));
    }

    // Video adjustments tests
    #[test]
    fn test_video_adjustments_default() {
        let adj = VideoAdjustments::default();
        assert!(adj.is_default());
    }

    #[test]
    fn test_video_adjustments_non_default() {
        let adj = VideoAdjustments {
            brightness: 0.5,
            ..VideoAdjustments::default()
        };
        assert!(!adj.is_default());
    }

    #[test]
    fn test_video_adjustments_reset() {
        let mut adj = VideoAdjustments {
            brightness: 0.5,
            contrast: 1.5,
            ..VideoAdjustments::default()
        };
        adj.reset();
        assert!(adj.is_default());
    }

    /// A name is shown as its bytes: what is not UTF-8 is written `\xNN`,
    /// not replaced, so two names differing only there look different.
    #[cfg(unix)]
    #[test]
    fn a_name_that_is_not_utf8_is_shown_as_its_bytes() {
        use std::os::unix::ffi::OsStrExt;
        let a = Path::new(std::ffi::OsStr::from_bytes(b"/v/clip\xFF.mp4"));
        let b = Path::new(std::ffi::OsStr::from_bytes(b"/v/clip\xFE.mp4"));
        assert_eq!(shown_name(a), "clip\\xFF.mp4");
        assert_ne!(shown_name(a), shown_name(b));
    }

    #[test]
    fn a_name_is_shown_without_its_folder() {
        assert_eq!(shown_name(Path::new("videos/holiday.mkv")), "holiday.mkv");
        assert_eq!(shown_name(Path::new("caf\u{e9}.mp4")), "caf\u{e9}.mp4");
        // A path with no last part is shown whole.
        assert_eq!(shown_name(Path::new("..")), "..");
    }

    // Recent files tests
    #[test]
    fn test_recent_history() {
        let mut recent = RecentHistory::new(3);
        recent.add(
            PathBuf::from("a.mp4"),
            "a.mp4".to_string(),
            Duration::ZERO,
            100,
            None,
        );
        recent.add(
            PathBuf::from("b.mp4"),
            "b.mp4".to_string(),
            Duration::ZERO,
            200,
            None,
        );
        recent.add(
            PathBuf::from("c.mp4"),
            "c.mp4".to_string(),
            Duration::ZERO,
            300,
            None,
        );
        assert_eq!(recent.files().len(), 3);
        recent.add(
            PathBuf::from("d.mp4"),
            "d.mp4".to_string(),
            Duration::ZERO,
            400,
            None,
        );
        assert_eq!(recent.files().len(), 3);
        assert_eq!(recent.files()[0].file_name, "d.mp4");
    }

    #[test]
    fn test_recent_dedup() {
        let mut recent = RecentHistory::new(10);
        recent.add(
            PathBuf::from("a.mp4"),
            "a.mp4".to_string(),
            Duration::from_secs(10),
            100,
            None,
        );
        recent.add(
            PathBuf::from("b.mp4"),
            "b.mp4".to_string(),
            Duration::ZERO,
            200,
            None,
        );
        recent.add(
            PathBuf::from("a.mp4"),
            "a.mp4".to_string(),
            Duration::from_secs(50),
            300,
            None,
        );
        assert_eq!(recent.files().len(), 2);
        assert_eq!(recent.files()[0].last_position, Duration::from_secs(50));
    }

    #[test]
    fn test_recent_find_by_path() {
        let mut recent = RecentHistory::new(10);
        recent.add(
            PathBuf::from("a.mp4"),
            "a.mp4".to_string(),
            Duration::from_secs(30),
            100,
            Some(Duration::from_secs(120)),
        );
        let found = recent.find_by_path(Path::new("a.mp4")).unwrap();
        assert_eq!(found.last_position, Duration::from_secs(30));
        assert!((found.progress_fraction() - 0.25).abs() < 0.01);
    }

    // Media file tests
    #[test]
    fn test_media_file_display() {
        let file = sample_media_file();
        assert!(file.file_size_display().contains("GiB"));
        assert!(file.overall_bitrate() > 0);
    }

    #[test]
    fn test_media_file_primary_streams() {
        let file = sample_media_file();
        let video = file.primary_video().unwrap();
        assert_eq!(video.codec, Codec::H265);
        let audio = file.primary_audio().unwrap();
        assert!(audio.is_default);
    }

    // Player app tests
    #[test]
    fn test_player_play_pause() {
        let mut app = VideoPlayerApp::new(800.0, 600.0);
        app.decodes = true;
        app.current_file = Some(sample_media_file());
        app.play();
        assert_eq!(app.state, PlaybackState::Playing);
        app.pause();
        assert_eq!(app.state, PlaybackState::Paused);
        app.toggle_play_pause();
        assert_eq!(app.state, PlaybackState::Playing);
    }

    #[test]
    fn test_player_stop() {
        let mut app = VideoPlayerApp::new(800.0, 600.0);
        app.current_file = Some(sample_media_file());
        app.play();
        app.position = Duration::from_secs(100);
        app.stop();
        assert_eq!(app.state, PlaybackState::Stopped);
        assert_eq!(app.position, Duration::ZERO);
    }

    #[test]
    fn test_player_seek() {
        let mut app = VideoPlayerApp::new(800.0, 600.0);
        app.current_file = Some(sample_media_file());
        app.seek_to(Duration::from_secs(300));
        assert_eq!(app.position, Duration::from_secs(300));
    }

    #[test]
    fn test_player_seek_clamp() {
        let mut app = VideoPlayerApp::new(800.0, 600.0);
        app.current_file = Some(sample_media_file());
        app.seek_to(Duration::from_secs(999999));
        assert_eq!(app.position, app.current_file.as_ref().unwrap().duration);
    }

    #[test]
    fn test_player_seek_forward_backward() {
        let mut app = VideoPlayerApp::new(800.0, 600.0);
        app.current_file = Some(sample_media_file());
        app.position = Duration::from_secs(100);
        app.seek_forward(10000);
        assert_eq!(app.position, Duration::from_secs(110));
        app.seek_backward(5000);
        assert_eq!(app.position, Duration::from_secs(105));
    }

    #[test]
    fn test_player_seek_to_fraction() {
        let mut app = VideoPlayerApp::new(800.0, 600.0);
        app.current_file = Some(sample_media_file());
        app.seek_to_fraction(0.5);
        assert_eq!(app.position.as_secs(), 3600);
    }

    #[test]
    fn test_player_volume_controls() {
        let mut app = VideoPlayerApp::new(800.0, 600.0);
        app.volume_up();
        assert_eq!(app.volume.level(), 105);
        app.volume_down();
        assert_eq!(app.volume.level(), 100);
        app.toggle_mute();
        assert!(app.volume.is_muted());
    }

    #[test]
    fn test_player_speed_controls() {
        let mut app = VideoPlayerApp::new(800.0, 600.0);
        app.increase_speed();
        assert!((app.speed.value() - 1.25).abs() < 0.01);
        app.reset_speed();
        assert!((app.speed.value() - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_player_cycle_audio() {
        let mut app = VideoPlayerApp::new(800.0, 600.0);
        app.current_file = Some(sample_media_file());
        app.selected_audio_track = Some(1);
        app.cycle_audio_track();
        assert_eq!(app.selected_audio_track, Some(2));
        app.cycle_audio_track();
        assert_eq!(app.selected_audio_track, Some(3));
        app.cycle_audio_track();
        assert_eq!(app.selected_audio_track, Some(1)); // Wrap around
    }

    #[test]
    fn test_player_toggle_subtitles() {
        let mut app = VideoPlayerApp::new(800.0, 600.0);
        assert!(app.subtitle_enabled);
        app.toggle_subtitles();
        assert!(!app.subtitle_enabled);
    }

    #[test]
    fn test_player_bookmarks() {
        let mut app = VideoPlayerApp::new(800.0, 600.0);
        app.current_file = Some(sample_media_file());
        app.position = Duration::from_secs(60);
        app.add_bookmark("Test".to_string());
        assert_eq!(app.bookmarks.len(), 1);
        assert_eq!(app.bookmarks[0].position, Duration::from_secs(60));

        let id = app.bookmarks[0].id;
        app.position = Duration::from_secs(0);
        app.seek_to_bookmark(id);
        assert_eq!(app.position, Duration::from_secs(60));

        app.remove_bookmark(id);
        assert!(app.bookmarks.is_empty());
    }

    #[test]
    fn test_player_chapters() {
        let mut app = VideoPlayerApp::new(800.0, 600.0);
        app.current_file = Some(sample_media_file());
        app.chapters = sample_chapters();
        app.position = Duration::from_secs(200);

        let (idx, ch) = app.current_chapter().unwrap();
        assert_eq!(idx, 1);
        assert_eq!(ch.title, "Act I - The Beginning");

        app.next_chapter();
        assert_eq!(app.position, Duration::from_secs(1800));
    }

    #[test]
    fn test_player_time_display() {
        let mut app = VideoPlayerApp::new(800.0, 600.0);
        assert_eq!(app.time_display(), "--:-- / --:--");
        app.current_file = Some(sample_media_file());
        app.position = Duration::from_secs(60);
        let display = app.time_display();
        assert!(display.contains("1:00"));
        assert!(display.contains("2:00:00"));
    }

    #[test]
    fn test_player_progress() {
        let mut app = VideoPlayerApp::new(800.0, 600.0);
        assert_eq!(app.progress_fraction(), 0.0);
        app.current_file = Some(sample_media_file());
        app.position = Duration::from_secs(3600);
        assert!((app.progress_fraction() - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_player_fullscreen() {
        let mut app = VideoPlayerApp::new(800.0, 600.0);
        assert!(!app.fullscreen);
        app.toggle_fullscreen();
        assert!(app.fullscreen);
    }

    #[test]
    fn test_player_active_subtitle() {
        let mut app = VideoPlayerApp::new(800.0, 600.0);
        app.external_subtitles = parse_srt(sample_subtitle_srt());
        app.subtitle_enabled = true;

        app.position = Duration::from_secs(6);
        let cue = app.active_subtitle_cue();
        assert!(cue.is_some());
        assert_eq!(cue.unwrap().text, "Welcome to the movie.");

        app.position = Duration::from_secs(9);
        assert!(app.active_subtitle_cue().is_none());
    }

    // Render tests
    #[test]
    fn test_render_all_tabs() {
        let mut app = VideoPlayerApp::new(1280.0, 720.0);
        app.current_file = Some(sample_media_file());
        app.chapters = sample_chapters();
        app.external_subtitles = parse_srt(sample_subtitle_srt());
        app.playlist.add(
            PathBuf::from("test.mp4"),
            "test.mp4".to_string(),
            Some(Duration::from_secs(120)),
            None,
        );
        app.playlist.set_current(0);

        for tab in PlayerTab::all() {
            app.active_tab = *tab;
            let cmds = app.render_commands();
            assert!(
                !cmds.is_empty(),
                "Tab {:?} produced no render commands",
                tab
            );
        }
    }

    #[test]
    fn test_render_empty_state() {
        let app = VideoPlayerApp::new(800.0, 600.0);
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_osd() {
        let mut app = VideoPlayerApp::new(800.0, 600.0);
        app.current_file = Some(sample_media_file());
        app.osd_message = Some("Test OSD".to_string());
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_paused_state() {
        let mut app = VideoPlayerApp::new(800.0, 600.0);
        app.current_file = Some(sample_media_file());
        app.state = PlaybackState::Paused;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    // Format helpers tests
    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(2048), "2.0 KiB");
        assert!(format_bytes(1_500_000).contains("MiB"));
        assert!(format_bytes(2_000_000_000).contains("GiB"));
    }

    #[test]
    fn test_format_bitrate() {
        assert_eq!(format_bitrate(500), "500 bps");
        assert_eq!(format_bitrate(192000), "192 kbps");
        assert!(format_bitrate(5_000_000).contains("Mbps"));
    }

    // Screenshot config tests
    #[test]
    fn test_screenshot_format_extension() {
        assert_eq!(ScreenshotFormat::Png.extension(), "png");
        assert_eq!(ScreenshotFormat::Jpeg.extension(), "jpg");
        assert_eq!(ScreenshotFormat::Bmp.extension(), "bmp");
    }

    // Shortcuts test
    #[test]
    fn test_shortcuts_list() {
        let shortcuts = Shortcuts::list();
        assert!(shortcuts.len() > 20);
        assert!(shortcuts.iter().any(|sc| sc.keys == "Space"));
        assert!(shortcuts.iter().any(|sc| sc.action == "Toggle Fullscreen"));
    }

    // Recent file tests
    #[test]
    fn test_recent_file_resume_label() {
        let rf = RecentFile {
            path: PathBuf::from("test.mp4"),
            file_name: "test.mp4".to_string(),
            last_position: Duration::from_secs(300),
            last_opened_timestamp: 0,
            duration: Some(Duration::from_secs(600)),
        };
        assert!(rf.resume_label().contains("5:00"));
        assert!((rf.progress_fraction() - 0.5).abs() < 0.01);
    }

    // Playback state tests
    #[test]
    fn test_playback_state_labels() {
        assert_eq!(PlaybackState::Playing.label(), "Playing");
        assert_eq!(PlaybackState::Paused.icon(), "||");
    }

    // On finish action test
    #[test]
    fn test_on_finish_labels() {
        assert_eq!(OnFinishAction::PlayNext.label(), "Play Next");
        assert_eq!(OnFinishAction::Quit.label(), "Quit");
    }

    // Deinterlace mode test
    #[test]
    fn test_deinterlace_labels() {
        assert_eq!(DeinterlaceMode::Auto.label(), "Auto");
        assert_eq!(DeinterlaceMode::Yadif.label(), "Yadif");
    }

    // Eq preset test
    #[test]
    fn test_eq_presets() {
        let presets = EqPreset::all();
        assert_eq!(presets.len(), 10);
        assert_eq!(presets[0].label(), "Flat");
    }

    // Playlist entry display name
    #[test]
    fn test_playlist_entry_display() {
        let entry = PlaylistEntry {
            id: 1,
            path: PathBuf::from("/home/test.mp4"),
            file_name: "test.mp4".to_string(),
            duration: None,
            title: Some("My Video".to_string()),
        };
        assert_eq!(entry.display_name(), "My Video");

        let entry2 = PlaylistEntry {
            id: 2,
            path: PathBuf::from("/home/other.mp4"),
            file_name: "other.mp4".to_string(),
            duration: None,
            title: None,
        };
        assert_eq!(entry2.display_name(), "other.mp4");
    }

    // -- A subtitle file may not abort the player -----------------------------

    /// Fractional-second fields whose character count and byte count disagree.
    ///
    /// `format!("{ms_str:0<3}")` pads to a width counted in *characters*, and
    /// the slice that followed indexed *bytes*. Any non-ASCII fractional part
    /// makes the two disagree, and a subtitle file is opened, not authored, by
    /// the user — so this was reachable by playing a video with a bad `.srt`
    /// beside it.
    fn adversarial_fractions() -> Vec<&'static str> {
        vec![
            // 3 characters, 5 bytes: no padding is added and byte 3 lands
            // inside the kanji.
            "ab\u{65e5}",
            "a\u{65e5}b",
            "\u{65e5}\u{672c}\u{8a9e}",
            // 2 characters, 3 bytes: padded to 3 characters / 4 bytes, and
            // byte 3 lands inside nothing here — but it must still not parse.
            "a\u{e9}",
            "\u{e9}",
            // 4 characters, 7 bytes.
            "12\u{65e5}3",
            // A 4-byte character on its own.
            "\u{1f4cc}",
            // Not digits, but ASCII, so the byte and character counts agree —
            // these were always safe and must stay rejected.
            "abc",
            "1a2",
        ]
    }

    #[test]
    fn a_non_ascii_subtitle_timestamp_does_not_abort_the_player() {
        for frac in adversarial_fractions() {
            assert_eq!(
                parse_srt_time(&format!("00:00:01.{frac}")),
                None,
                "accepted fractional part {frac:?}"
            );
            // The comma form goes through the same path after a replace.
            assert_eq!(
                parse_srt_time(&format!("00:00:01,{frac}")),
                None,
                "accepted fractional part {frac:?} in comma form"
            );
        }
    }

    /// A whole file's worth of them, since that is how the parser is reached.
    #[test]
    fn a_non_ascii_subtitle_file_does_not_abort_the_player() {
        let mut srt = String::new();
        for (i, frac) in adversarial_fractions().iter().enumerate() {
            srt.push_str(&format!(
                "{}\n00:00:0{}.{} --> 00:00:0{}.{}\n\u{5b57}\u{5e55}\n\n",
                i + 1,
                i % 10,
                frac,
                (i + 1) % 10,
                frac
            ));
        }
        // The bad cues are dropped, not fatal, and a good one still parses.
        srt.push_str("99\n00:00:05,250 --> 00:00:06,500\nok\n\n");
        let cues = parse_srt(&srt);
        assert_eq!(cues.len(), 1, "expected only the well-formed cue: {cues:?}");
    }

    /// The rejections must not have cost any well-formed timestamp.
    #[test]
    fn every_valid_subtitle_timestamp_still_parses() {
        assert_eq!(
            parse_srt_time("01:30:45,678"),
            Some(Duration::from_millis(5_445_678))
        );
        assert_eq!(
            parse_srt_time("01:30:45.678"),
            Some(Duration::from_millis(5_445_678))
        );
        // Fewer than three digits are right-padded, per the original intent.
        assert_eq!(
            parse_srt_time("00:00:00.5"),
            Some(Duration::from_millis(500))
        );
        assert_eq!(
            parse_srt_time("00:00:00.25"),
            Some(Duration::from_millis(250))
        );
        // More than three are truncated, likewise.
        assert_eq!(
            parse_srt_time("00:00:00.1234"),
            Some(Duration::from_millis(123))
        );
        // No fractional part at all.
        assert_eq!(
            parse_srt_time("00:00:07"),
            Some(Duration::from_millis(7_000))
        );
    }

    // ======================================================================
    // The keymap, the clock and the window
    // ======================================================================

    use guitk::event::Modifiers;

    /// Every row of the Settings tab can be changed from the keyboard, and
    /// the panel draws the new value.
    ///
    /// This asserts the *drawn* value rather than the field, because the
    /// defect being guarded is precisely a panel that shows a value nothing
    /// can move: a test that only checked `app.preferences` would still pass
    /// if the renderer went back to its own hardcoded copy of the list.
    #[test]
    fn every_settings_row_can_be_changed_and_the_panel_shows_it() {
        for (i, row) in SettingRow::ALL.iter().enumerate() {
            let mut app = VideoPlayerApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
            app.active_tab = PlayerTab::Settings;
            app.settings_row = i;
            let before = row.value(&app.preferences).to_string();

            let before_count = drawn_texts(&app).iter().filter(|t| **t == before).count();
            assert!(
                before_count > 0,
                "control: the panel must draw {}'s value {before:?} before \
this test can mean anything",
                row.label()
            );

            assert!(
                app.handle_event(&press(Key::Enter)),
                "Enter on row {} of the settings list was not handled",
                row.label()
            );
            let after = row.value(&app.preferences).to_string();
            assert_ne!(
                before,
                after,
                "Enter on {} left it reading {before:?}",
                row.label()
            );
            // Counted rather than merely present: five of the six rows read
            // "On" or "Off", so "the panel draws {after} somewhere" is
            // satisfied by a *different* row and would pass against a
            // renderer that ignored the change entirely. One fewer of the old
            // word and one more of the new is a statement about this row.
            let texts = drawn_texts(&app);
            assert_eq!(
                texts.iter().filter(|t| **t == before).count(),
                before_count - 1,
                "{} changed to {after:?} and the panel still drew {before:?} \
as many times as before",
                row.label()
            );
            assert!(
                texts.contains(&after),
                "{} now reads {after:?} and the panel does not draw it",
                row.label()
            );
        }
    }

    /// Each row's `cycle` returns to where it started, so no value is a
    /// one-way door the user cannot come back from.
    #[test]
    fn cycling_a_settings_row_comes_back_round() {
        for row in SettingRow::ALL {
            let mut prefs = PlayerPreferences::default();
            let start = row.value(&prefs).to_string();
            // Eight presses passes the longest list here (five) and lands
            // back only if the cycle wraps.
            let mut seen_other = false;
            for _ in 0..40 {
                row.cycle(&mut prefs);
                if row.value(&prefs) != start {
                    seen_other = true;
                } else if seen_other {
                    break;
                }
            }
            assert!(seen_other, "{} never took another value", row.label());
            assert_eq!(
                row.value(&prefs),
                start,
                "{} does not cycle back to {start:?}",
                row.label()
            );
        }
    }

    /// The arrows move the settings cursor only where the settings are.
    #[test]
    fn the_arrows_move_the_cursor_on_the_settings_tab_and_the_volume_elsewhere() {
        let mut app = VideoPlayerApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.active_tab = PlayerTab::Settings;
        app.handle_event(&press(Key::Down));
        assert_eq!(app.settings_row, 1, "Down did not move the settings cursor");
        app.handle_event(&press(Key::Up));
        assert_eq!(app.settings_row, 0, "Up did not move it back");

        // At the ends it stays put rather than wrapping into a row the eye
        // has to hunt for at the other end of the list.
        app.handle_event(&press(Key::Up));
        assert_eq!(app.settings_row, 0, "Up at the top wrapped or ran off");
        app.settings_row = SettingRow::ALL.len() - 1;
        app.handle_event(&press(Key::Down));
        assert_eq!(
            app.settings_row,
            SettingRow::ALL.len() - 1,
            "Down at the bottom ran past the last row"
        );

        // And the volume still answers the same keys on the player itself.
        let mut player = VideoPlayerApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        player.active_tab = PlayerTab::Player;
        let before = player.volume.level();
        player.handle_event(&press(Key::Up));
        assert_ne!(
            player.volume.level(),
            before,
            "Up on the player tab stopped changing the volume"
        );
        assert_eq!(player.settings_row, 0, "it moved the settings cursor too");
    }

    /// `Enter` outside the Settings tab must not change a setting the user
    /// cannot see.
    #[test]
    fn enter_on_the_player_changes_no_setting() {
        let mut app = VideoPlayerApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.active_tab = PlayerTab::Player;
        let before = app.preferences.resume_playback;
        app.handle_event(&press(Key::Enter));
        assert_eq!(
            app.preferences.resume_playback, before,
            "Enter changed a setting from a tab that does not show it"
        );
    }

    /// The repeat mode and the equalizer switch were both read and never
    /// written: the playlist consulted `repeat` to decide what came next, and
    /// the equalizer drew its own on/off state, with nothing able to set
    /// either.
    #[test]
    fn repeat_and_the_equalizer_switch_answer_their_keys() {
        let mut app = VideoPlayerApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let before = app.repeat;
        app.handle_event(&press(Key::R));
        assert_ne!(app.repeat, before, "R did not move the repeat mode");

        let eq = app.equalizer.enabled;
        app.handle_event(&press_with(Key::E, shift()));
        assert_ne!(
            app.equalizer.enabled, eq,
            "Shift+E did not switch the equalizer"
        );
        app.handle_event(&press_with(Key::E, shift()));
        assert_eq!(
            app.equalizer.enabled, eq,
            "Shift+E is a switch, not a one-way door"
        );
    }

    /// The shortcut panel and the dispatcher are one table; this checks the
    /// keys it names are distinct, since two rows claiming one keystroke
    /// means the second is printed and unreachable.
    #[test]
    fn no_two_shortcuts_claim_the_same_keystroke() {
        let list = Shortcuts::list();
        for (i, a) in list.iter().enumerate() {
            for b in list.iter().skip(i + 1) {
                assert_ne!(
                    a.press, b.press,
                    "{} and {} both claim the same keystroke",
                    a.keys, b.keys
                );
            }
        }
    }

    fn drawn_texts(app: &VideoPlayerApp) -> Vec<String> {
        app.render_commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn press(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        })
    }

    fn press_with(k: Key, modifiers: Modifiers) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers,
            text: String::new(),
        })
    }

    fn shift() -> Modifiers {
        Modifiers {
            shift: true,
            ..Modifiers::NONE
        }
    }

    fn ctrl() -> Modifiers {
        Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        }
    }

    fn mouse(x: f32, y: f32, kind: MouseEventKind) -> Event {
        Event::Mouse(MouseEvent { x, y, kind })
    }

    /// A player with a file open, and a decoder: what the transport's tests
    /// are about -- play, the clock, the end of a file -- is what a decoder
    /// will drive, and there is none yet.
    fn loaded() -> VideoPlayerApp {
        let mut app = VideoPlayerApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.decodes = true;
        app.current_file = Some(sample_media_file());
        app.chapters = sample_chapters();
        app
    }

    // --- the table is the keymap ---

    #[test]
    fn every_documented_shortcut_is_bound_to_something() {
        // The point of holding the text and the dispatch in one row: the help
        // panel cannot list a key that does nothing, because there is nowhere
        // to write the row without also writing what it does.
        for shortcut in Shortcuts::list() {
            assert!(
                !shortcut.keys.is_empty() && !shortcut.action.is_empty(),
                "a row with no text is a row nobody can read: {shortcut:?}"
            );
        }
        assert!(
            Shortcuts::list().len() > 30,
            "the panel used to list 32 rows and must not have lost them"
        );
    }

    #[test]
    fn no_two_shortcuts_answer_the_same_keystroke() {
        let list = Shortcuts::list();
        for (i, a) in list.iter().enumerate() {
            for b in list.iter().skip(i + 1) {
                assert_ne!(
                    a.press, b.press,
                    "{} and {} both answer the same keystroke, so one of them \
                     is drawn in the help panel and never runs",
                    a.keys, b.keys
                );
            }
        }
    }

    #[test]
    fn a_modifier_shortcut_is_not_also_the_plain_one() {
        // Shift+Right seeks a minute and Right seeks ten seconds. If `Plain`
        // merely ignored the modifiers, whichever came first in the table
        // would answer both.
        let mut app = loaded();
        app.play();
        app.seek_to(Duration::from_secs(600));
        app.handle_event(&press(Key::Right));
        assert_eq!(app.position, Duration::from_secs(610));
        app.handle_event(&press_with(Key::Right, shift()));
        assert_eq!(
            app.position,
            Duration::from_secs(670),
            "Shift+Right must be the sixty-second seek, not the ten-second one"
        );
    }

    #[test]
    fn space_plays_and_pauses() {
        let mut app = loaded();
        assert!(app.handle_event(&press(Key::Space)));
        assert_eq!(app.state, PlaybackState::Playing);
        app.handle_event(&press(Key::Space));
        assert_eq!(app.state, PlaybackState::Paused);
    }

    #[test]
    fn the_media_keys_work_as_well_as_the_letters() {
        let mut app = loaded();
        app.handle_event(&press(Key::MediaPlayPause));
        assert_eq!(app.state, PlaybackState::Playing);
        app.handle_event(&press(Key::MediaStop));
        assert_eq!(app.state, PlaybackState::Stopped);
    }

    #[test]
    fn the_volume_keys_move_the_volume() {
        let mut app = loaded();
        let before = app.volume.level();
        app.handle_event(&press(Key::Up));
        assert!(app.volume.level() > before);
        app.handle_event(&press(Key::Down));
        assert_eq!(app.volume.level(), before);
        app.handle_event(&press(Key::M));
        assert!(app.volume.is_muted());
    }

    #[test]
    fn the_bracket_keys_change_the_speed() {
        let mut app = loaded();
        app.handle_event(&press(Key::RightBracket));
        assert!(app.speed.value() > 1.0);
        app.handle_event(&press(Key::Backslash));
        assert_eq!(app.speed, PlaybackSpeed::NORMAL);
        app.handle_event(&press(Key::LeftBracket));
        assert!(app.speed.value() < 1.0);
    }

    #[test]
    fn a_digit_seeks_to_its_tenth_of_the_file() {
        let mut app = loaded();
        let duration = app.current_file.as_ref().expect("a file").duration;
        app.handle_event(&press(Key::Num5));
        assert_eq!(
            app.position.as_millis(),
            duration.as_millis() / 2,
            "5 is halfway through"
        );
        app.handle_event(&press(Key::Num0));
        assert_eq!(app.position, Duration::ZERO, "0 is the start");
    }

    #[test]
    fn a_shortcut_that_is_not_bound_leaves_the_player_alone() {
        let mut app = loaded();
        assert!(!app.handle_event(&press(Key::F9)));
        assert!(!app.handle_event(&press_with(Key::Q, ctrl())));
    }

    #[test]
    fn ctrl_b_adds_a_bookmark_where_the_film_is() {
        let mut app = loaded();
        app.play();
        app.seek_to(Duration::from_secs(125));
        assert!(app.handle_event(&press_with(Key::B, ctrl())));
        assert_eq!(app.bookmarks.len(), 1);
        assert_eq!(app.bookmarks[0].position, Duration::from_secs(125));
        assert!(
            app.bookmarks[0].label.contains("2:05"),
            "the label should say where it is: {:?}",
            app.bookmarks[0].label
        );
    }

    #[test]
    fn plain_b_cycles_the_audio_track_and_ctrl_b_does_not() {
        // The two share a key and differ only by the modifier.
        let mut app = loaded();
        app.selected_audio_track = Some(1);
        app.handle_event(&press(Key::B));
        assert!(app.bookmarks.is_empty(), "plain B is not a bookmark");
        app.handle_event(&press_with(Key::B, ctrl()));
        assert_eq!(app.bookmarks.len(), 1);
    }

    #[test]
    fn the_sync_keys_shift_the_subtitles_and_the_sound() {
        let mut app = loaded();
        app.handle_event(&press(Key::K));
        assert_eq!(app.subtitle_delay.ms, 100);
        app.handle_event(&press(Key::J));
        assert_eq!(app.subtitle_delay.ms, 0);
        app.handle_event(&press(Key::H));
        assert_eq!(app.audio_sync.ms, 100);
        app.handle_event(&press(Key::G));
        assert_eq!(app.audio_sync.ms, 0);
    }

    #[test]
    fn a_cycles_the_aspect_ratio() {
        let mut app = loaded();
        assert_eq!(app.aspect_mode, AspectMode::Fit);
        app.handle_event(&press(Key::A));
        assert_eq!(app.aspect_mode, AspectMode::Fill);
    }

    #[test]
    fn the_panel_keys_open_and_close_their_tabs() {
        let mut app = loaded();
        for (key, tab) in [
            (Key::L, PlayerTab::Playlist),
            (Key::I, PlayerTab::MediaInfo),
            (Key::E, PlayerTab::Equalizer),
        ] {
            app.active_tab = PlayerTab::Player;
            app.handle_event(&press(key));
            assert_eq!(app.active_tab, tab, "{key:?} must open {tab:?}");
            app.handle_event(&press(key));
            assert_eq!(
                app.active_tab,
                PlayerTab::Player,
                "{key:?} must close it again"
            );
        }
    }

    // --- the on-screen display expires ---

    #[test]
    fn an_on_screen_message_goes_away_by_itself() {
        let mut app = loaded();
        app.handle_event(&press(Key::Up));
        assert!(
            app.osd_message.is_some(),
            "turning the volume up says so on screen"
        );
        let life = app.preferences.osd_duration_ms;
        app.tick(life / 2);
        assert!(app.osd_message.is_some(), "it has not been long enough yet");
        app.tick(life);
        assert_eq!(
            app.osd_message, None,
            "the message had no way to expire before there was a clock: \
             'Volume: 80' stayed on the picture for the rest of the film"
        );
    }

    #[test]
    fn a_new_message_gets_the_full_time_again() {
        let mut app = loaded();
        app.handle_event(&press(Key::Up));
        app.tick(app.preferences.osd_duration_ms - 1);
        app.handle_event(&press(Key::Down));
        assert_eq!(app.osd_remaining_ms, app.preferences.osd_duration_ms);
    }

    // --- the picture advances ---

    #[test]
    fn a_playing_film_advances_with_the_clock() {
        let mut app = loaded();
        app.play();
        app.tick(1000);
        assert_eq!(app.position, Duration::from_millis(1000));
        app.tick(500);
        assert_eq!(app.position, Duration::from_millis(1500));
    }

    #[test]
    fn a_paused_film_does_not() {
        let mut app = loaded();
        app.play();
        app.tick(1000);
        app.pause();
        app.tick(5000);
        assert_eq!(app.position, Duration::from_millis(1000));
    }

    #[test]
    fn double_speed_advances_twice_as_fast() {
        let mut app = loaded();
        app.play();
        app.speed = PlaybackSpeed::DOUBLE;
        app.tick(1000);
        assert_eq!(
            app.position,
            Duration::from_millis(2000),
            "the speed is what the speed control is for"
        );
    }

    #[test]
    fn the_end_of_a_file_stops_rather_than_running_past_it() {
        let mut app = loaded();
        let duration = app.current_file.as_ref().expect("a file").duration;
        app.play();
        app.seek_to(Duration::from_millis(duration.as_millis() - 100));
        app.tick(5000);
        assert_eq!(app.position, duration, "the film cannot get longer");
        assert_ne!(
            app.state,
            PlaybackState::Playing,
            "with nothing queued behind it, the end of the file is a stop"
        );
    }

    #[test]
    fn repeat_one_starts_the_same_file_again() {
        let mut app = loaded();
        let duration = app.current_file.as_ref().expect("a file").duration;
        app.repeat = RepeatMode::One;
        app.play();
        app.seek_to(Duration::from_millis(duration.as_millis() - 10));
        app.tick(1000);
        assert_eq!(app.position, Duration::ZERO);
        assert_eq!(app.state, PlaybackState::Playing);
    }

    // --- the controls fade ---

    #[test]
    fn the_controls_fade_out_of_a_playing_picture() {
        let mut app = loaded();
        app.play();
        assert!(app.controls_visible);
        app.tick(CONTROLS_HIDE_MS + 1);
        assert!(
            !app.controls_visible,
            "the controls have a hide timer and nothing ever counted it down"
        );
    }

    #[test]
    fn any_sign_of_life_brings_the_controls_back() {
        let mut app = loaded();
        app.play();
        app.tick(CONTROLS_HIDE_MS + 1);
        assert!(!app.controls_visible);
        app.handle_event(&mouse(400.0, 300.0, MouseEventKind::Move));
        assert!(app.controls_visible);
    }

    #[test]
    fn the_controls_stay_up_while_the_film_is_paused() {
        let mut app = loaded();
        app.play();
        app.pause();
        app.tick(CONTROLS_HIDE_MS * 4);
        assert!(
            app.controls_visible,
            "nothing is happening behind them, so there is nothing to get out \
             of the way of"
        );
    }

    // --- the seek bar and the tabs ---

    #[test]
    fn clicking_a_tab_opens_it() {
        let mut app = loaded();
        let rects = app.tab_rects();
        let (tab, rect) = rects[3];
        let (x, y) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
        assert_eq!(app.tab_at(x, y), Some(tab));
        assert!(app.handle_event(&mouse(x, y, MouseEventKind::Press(MouseButton::Left))));
        assert_eq!(app.active_tab, tab);
    }

    #[test]
    fn every_tab_is_reachable_where_it_is_drawn() {
        let app = loaded();
        for (tab, rect) in app.tab_rects() {
            assert_eq!(
                app.tab_at(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0),
                Some(tab),
                "{tab:?} is drawn at {rect:?} and must be clickable there"
            );
        }
    }

    #[test]
    fn a_click_below_the_tab_strip_is_not_a_tab() {
        let app = loaded();
        assert_eq!(app.tab_at(20.0, TAB_BAR_HEIGHT + 4.0), None);
    }

    /// Every string the player draws, with the y it is drawn at.
    fn drawn(app: &VideoPlayerApp) -> Vec<(String, f32)> {
        app.render_commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, y, .. } => Some((text.clone(), *y)),
                _ => None,
            })
            .collect()
    }

    /// Just the seek preview label: the row 16px above the bar.
    ///
    /// Keyed by position rather than by content, because after a release the
    /// clock legitimately reads the very time the label was showing -- the
    /// player has seeked there. A test that searched for the string alone
    /// could not tell "the label is gone" from "the label is gone and the
    /// clock caught up", which is the whole assertion.
    fn preview_labels(app: &VideoPlayerApp) -> Vec<String> {
        // `controls_top() + SEEK_BAR_OFFSET` is where the bar is *drawn*, which
        // `seek_bar()` deliberately is not -- that rect is thicker to grab than
        // the bar is to look at. The first version of this helper used
        // `seek_bar().y` and found no labels at all.
        let row = app.controls_top() + SEEK_BAR_OFFSET - 16.0;
        drawn(app)
            .into_iter()
            .filter(|(_, y)| (*y - row).abs() < 0.5)
            .map(|(t, _)| t)
            .collect()
    }

    /// **Dragging the scrubber shows the time you are dragging to.**
    ///
    /// The press arm deliberately does not seek until release, "so dragging
    /// across a film does not seek to every pixel of the way there" -- which
    /// is only tolerable because a preview says where you are. The preview was
    /// computed on every move and drawn nowhere, so the design's other half
    /// was missing and dragging was blind.
    #[test]
    fn dragging_the_seek_bar_shows_the_time_it_will_seek_to() {
        let mut app = loaded();
        app.play();
        let bar = app.seek_bar();
        let y = bar.y + bar.h / 2.0;

        app.handle_event(&mouse(
            bar.x + bar.w / 2.0,
            y,
            MouseEventKind::Press(MouseButton::Left),
        ));
        let preview = app.seek_preview_position.expect("no preview was computed");

        let labels = preview_labels(&app);
        assert!(
            labels.contains(&preview.format()),
            "the previewed time {:?} is computed and not drawn; labels: {labels:?}",
            preview.format()
        );
    }

    /// The label follows the pointer rather than sitting at the start.
    #[test]
    fn the_preview_label_moves_with_the_drag() {
        let mut app = loaded();
        app.play();
        let bar = app.seek_bar();
        let y = bar.y + bar.h / 2.0;

        app.handle_event(&mouse(
            bar.x + bar.w * 0.25,
            y,
            MouseEventKind::Press(MouseButton::Left),
        ));
        let early = app.seek_preview_position.expect("no preview").format();
        app.handle_event(&mouse(bar.x + bar.w * 0.75, y, MouseEventKind::Move));
        let late = app.seek_preview_position.expect("no preview").format();
        assert_ne!(early, late, "the preview did not follow the pointer");

        let labels = preview_labels(&app);
        assert!(labels.contains(&late), "{labels:?}");
        assert!(
            !labels.contains(&early),
            "the label still shows where the drag started: {labels:?}"
        );
    }

    /// Letting go seeks and takes the label away.
    #[test]
    fn releasing_removes_the_preview_label() {
        let mut app = loaded();
        app.play();
        let bar = app.seek_bar();
        let y = bar.y + bar.h / 2.0;

        app.handle_event(&mouse(
            bar.x + bar.w / 2.0,
            y,
            MouseEventKind::Press(MouseButton::Left),
        ));
        let preview = app.seek_preview_position.expect("no preview").format();
        app.handle_event(&mouse(
            bar.x + bar.w / 2.0,
            y,
            MouseEventKind::Release(MouseButton::Left),
        ));

        assert!(app.seek_preview_position.is_none());
        assert!(
            preview_labels(&app).is_empty(),
            "a label outlived the drag it belonged to; it read {preview:?}"
        );
    }

    #[test]
    fn dragging_the_seek_bar_previews_and_only_seeks_on_release() {
        let mut app = loaded();
        app.play();
        let bar = app.seek_bar();
        let y = bar.y + bar.h / 2.0;
        let quarter = bar.x + bar.w / 4.0;

        app.handle_event(&mouse(quarter, y, MouseEventKind::Press(MouseButton::Left)));
        assert!(app.seeking);
        assert!(app.seek_preview_position.is_some());
        assert_eq!(
            app.position,
            Duration::ZERO,
            "dragging across a film must not seek to every pixel of the way"
        );

        let three_quarters = bar.x + bar.w * 0.75;
        app.handle_event(&mouse(three_quarters, y, MouseEventKind::Move));
        assert_eq!(app.position, Duration::ZERO);

        app.handle_event(&mouse(
            three_quarters,
            y,
            MouseEventKind::Release(MouseButton::Left),
        ));
        assert!(!app.seeking);
        assert_eq!(app.seek_preview_position, None);
        let duration = app.current_file.as_ref().expect("a file").duration;
        let want = duration.as_millis() * 3 / 4;
        assert!(
            app.position.as_millis().abs_diff(want) < duration.as_millis() / 100,
            "released three quarters along: {} of {}",
            app.position.as_millis(),
            duration.as_millis()
        );
    }

    #[test]
    fn the_seek_bar_is_thicker_to_grab_than_it_is_to_look_at() {
        let app = loaded();
        let bar = app.seek_bar();
        assert!(
            bar.h > SEEK_BAR_HEIGHT,
            "a six-pixel line is not a thing a pointer can land on"
        );
        let drawn_top = app.controls_top() + SEEK_BAR_OFFSET;
        assert!(
            bar.y < drawn_top && bar.y + bar.h > drawn_top + SEEK_BAR_HEIGHT,
            "the grab band must reach above and below the line it is for"
        );
    }

    #[test]
    fn a_release_without_a_drag_does_not_seek() {
        let mut app = loaded();
        app.play();
        app.seek_to(Duration::from_secs(300));
        let bar = app.seek_bar();
        assert!(!app.handle_event(&mouse(
            bar.x + 10.0,
            bar.y + 2.0,
            MouseEventKind::Release(MouseButton::Left)
        )));
        assert_eq!(app.position, Duration::from_secs(300));
    }

    #[test]
    fn the_seek_bar_moves_with_the_window() {
        let mut app = loaded();
        let narrow = app.seek_bar();
        app.set_window_size(1920.0, 1080.0);
        let wide = app.seek_bar();
        assert!(wide.w > narrow.w, "the bar spans the window");
        assert!(wide.y > narrow.y, "the controls sit on the bottom edge");
    }

    // --- the strap ---

    #[test]
    fn the_title_names_the_file_and_says_when_it_is_paused() {
        let mut app = VideoPlayerApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(app.title(), "Video Player");
        app.decodes = true;
        app.current_file = Some(sample_media_file());
        let name = app.current_file.as_ref().expect("a file").file_name.clone();
        app.play();
        assert_eq!(app.title(), format!("{name} - Video Player"));
        app.pause();
        assert_eq!(app.title(), format!("{name} (paused) - Video Player"));
    }

    #[test]
    fn the_clock_runs_only_while_there_is_something_to_advance() {
        let mut app = loaded();
        assert_eq!(app.tick_interval(), None);
        app.play();
        assert_eq!(app.tick_interval(), Some(FRAME_TICK));
        app.pause();
        assert_eq!(
            app.tick_interval(),
            Some(FRAME_TICK),
            "pausing says 'Paused' on screen, and that message still has to be taken off it"
        );
        app.tick(app.preferences.osd_duration_ms);
        assert_eq!(
            app.tick_interval(),
            None,
            "once the message is gone a paused player has nothing left to advance"
        );
        app.handle_event(&press(Key::Up));
        assert_eq!(app.tick_interval(), Some(FRAME_TICK));
    }

    #[test]
    fn a_tick_with_nothing_to_do_asks_for_no_frame() {
        let mut app = loaded();
        assert_eq!(
            app.on_event(&Event::Tick { elapsed_ms: 100 }),
            Response::Idle
        );
    }

    #[test]
    fn a_resize_relays_out_and_a_repeat_of_it_does_not() {
        let mut app = loaded();
        let resize = Event::Resize {
            width: 800,
            height: 600,
        };
        assert_eq!(app.on_event(&resize), Response::Redraw);
        assert_eq!(app.width, 800.0);
        assert_eq!(app.on_event(&resize), Response::Idle);
    }

    #[test]
    fn a_window_dragged_tiny_keeps_a_layout() {
        let mut app = loaded();
        app.set_window_size(1.0, 1.0);
        assert!(app.width >= MIN_WINDOW_WIDTH);
        assert!(app.height >= MIN_WINDOW_HEIGHT);
        assert!(app.seek_bar().w >= 1.0);
    }

    #[test]
    fn the_first_frame_uses_the_size_the_compositor_gave() {
        let mut app = loaded();
        let tree = app.render(1600.0, 900.0);
        assert_eq!(app.width, 1600.0);
        assert_eq!(app.height, 900.0);
        assert!(!tree.commands.is_empty());
    }

    #[test]
    fn the_close_button_exits() {
        let mut app = loaded();
        assert_eq!(app.on_event(&Event::CloseRequested), Response::Exit);
    }

    #[test]
    fn the_seeded_player_opens_on_something_to_watch() {
        let app = seeded_player();
        assert!(app.current_file.is_some());
        assert!(!app.chapters.is_empty());
        assert!(!app.playlist.is_empty());
        assert!(!app.external_subtitles.is_empty());
    }

    #[test]
    fn a_modifier_a_shortcut_does_not_ask_for_runs_nothing() {
        // `Plain` insisting the modifiers are clear is only half of it: each
        // arm has to insist on its own modifier too, or Ctrl+Right becomes the
        // shift binding and Shift+B becomes the control one -- keystrokes
        // nobody documented, doing things nobody asked for.
        let mut app = loaded();
        app.play();
        app.seek_to(Duration::from_secs(600));
        assert!(!app.handle_event(&press_with(Key::Right, ctrl())));
        assert_eq!(app.position, Duration::from_secs(600));

        assert!(!app.handle_event(&press_with(Key::B, shift())));
        assert!(app.bookmarks.is_empty());

        assert!(
            !app.handle_event(&press_with(Key::Num5, ctrl())),
            "Ctrl+5 is not the seek-to-halfway shortcut"
        );
        assert_eq!(app.position, Duration::from_secs(600));
    }

    #[test]
    fn the_left_arrow_seeks_backward() {
        let mut app = loaded();
        app.play();
        app.seek_to(Duration::from_secs(600));
        app.handle_event(&press(Key::Left));
        assert_eq!(app.position, Duration::from_secs(590));
        app.handle_event(&press_with(Key::Left, shift()));
        assert_eq!(app.position, Duration::from_secs(530));
    }

    #[test]
    fn a_keystroke_brings_the_controls_back_and_keeps_them_up() {
        let mut app = loaded();
        app.play();
        app.tick(CONTROLS_HIDE_MS + 1);
        assert!(!app.controls_visible);

        app.handle_event(&press(Key::Up));
        assert!(app.controls_visible, "a keystroke is a sign of life");
        app.tick(CONTROLS_HIDE_MS / 2);
        assert!(
            app.controls_visible,
            "waking them without restarting the countdown puts them straight \
             back down on the next tick"
        );
    }

    #[test]
    fn the_tab_strip_does_not_stretch_across_a_wide_window() {
        let mut app = loaded();
        app.set_window_size(2560.0, 1440.0);
        for (tab, rect) in app.tab_rects() {
            assert!(
                rect.w <= TAB_MAX_WIDTH,
                "{tab:?} is {} wide on a 2560px window; a tab strip that \
                 grows without limit is a row of seven enormous buttons",
                rect.w
            );
        }
    }

    #[test]
    fn dragging_off_the_end_of_the_bar_stops_at_the_end_of_the_film() {
        // A drag is not bounded by the bar the way a click is: the pointer
        // leaves it and the moves keep coming.
        let mut app = loaded();
        app.play();
        let bar = app.seek_bar();
        let y = bar.y + bar.h / 2.0;
        app.handle_event(&mouse(
            bar.x + 10.0,
            y,
            MouseEventKind::Press(MouseButton::Left),
        ));
        app.handle_event(&mouse(bar.x + bar.w * 3.0, y, MouseEventKind::Move));
        app.handle_event(&mouse(
            bar.x + bar.w * 3.0,
            y,
            MouseEventKind::Release(MouseButton::Left),
        ));
        let duration = app.current_file.as_ref().expect("a file").duration;
        assert_eq!(app.position, duration, "the film cannot be seeked past");

        app.handle_event(&mouse(
            bar.x + 10.0,
            y,
            MouseEventKind::Press(MouseButton::Left),
        ));
        app.handle_event(&mouse(
            bar.x - bar.w,
            y,
            MouseEventKind::Release(MouseButton::Left),
        ));
        assert_eq!(app.position, Duration::ZERO, "nor before its start");
    }

    #[test]
    fn the_preview_follows_the_pointer_during_a_drag() {
        let mut app = loaded();
        app.play();
        let bar = app.seek_bar();
        let y = bar.y + bar.h / 2.0;
        app.handle_event(&mouse(
            bar.x + bar.w * 0.1,
            y,
            MouseEventKind::Press(MouseButton::Left),
        ));
        let first = app.seek_preview_position.expect("a preview");
        app.handle_event(&mouse(bar.x + bar.w * 0.9, y, MouseEventKind::Move));
        let second = app.seek_preview_position.expect("a preview");
        assert!(
            second > first,
            "the preview is what a drag is for: {first:?} then {second:?}"
        );
    }

    #[test]
    fn a_file_with_no_duration_has_no_bitrate_rather_than_dividing_by_zero() {
        // The division is guarded once, by `checked_div`, rather than by an
        // early return standing in front of a bare `/`.
        let mut file = sample_media_file();
        file.duration = Duration::ZERO;
        assert_eq!(file.overall_bitrate(), 0);
    }

    #[test]
    fn each_binding_insists_on_its_own_modifier() {
        // Asserted on `Press` directly rather than through the table, because
        // through the table it is masked: the plain binding for a key sits
        // above the shifted one and answers first. That masking is the thing
        // being ruled out -- a row moved up the table must not change what
        // any other row answers.
        let plain_right = KeyEvent {
            key: Key::Right,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        assert!(Press::Plain(Key::Right).matches(&plain_right));
        assert!(
            !Press::Shift(Key::Right).matches(&plain_right),
            "the sixty-second seek must not answer an unshifted arrow, whatever order the table happens to be in"
        );
        let shifted = KeyEvent {
            modifiers: shift(),
            ..plain_right.clone()
        };
        assert!(Press::Shift(Key::Right).matches(&shifted));
        assert!(!Press::Plain(Key::Right).matches(&shifted));
    }

    #[test]
    fn a_preview_dragged_off_the_end_still_names_a_point_in_the_film() {
        let mut app = loaded();
        app.play();
        let bar = app.seek_bar();
        let y = bar.y + bar.h / 2.0;
        let duration = app.current_file.as_ref().expect("a file").duration;
        app.handle_event(&mouse(
            bar.x + 10.0,
            y,
            MouseEventKind::Press(MouseButton::Left),
        ));
        app.handle_event(&mouse(bar.x + bar.w * 5.0, y, MouseEventKind::Move));
        assert_eq!(
            app.seek_preview_position,
            Some(duration),
            "the time shown under a pointer dragged off the right of the bar must be the end of the film, not a time past it"
        );
        app.handle_event(&mouse(bar.x - bar.w, y, MouseEventKind::Move));
        assert_eq!(app.seek_preview_position, Some(Duration::ZERO));
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

        fn fills(app: &mut VideoPlayerApp) -> Vec<Color> {
            app.render(1000.0, 700.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let mut app = VideoPlayerApp::new(1000.0, 700.0);

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

    /// The warning lines are where they can be seen: nothing drawn after a
    /// line fills the point it is drawn at. The sweep that added them drew
    /// them "after the background, or it would be painted over" -- and in
    /// several apps a bar was then drawn over the same pixels, while a test
    /// that read the frame's texts said they were there. known-issues.md,
    /// `[E] Warnings drawn where the next thing drawn covers them`.
    #[test]
    fn the_warning_lines_are_not_painted_over() {
        let app = VideoPlayerApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let commands: Vec<RenderCommand> = app.render_commands();
        for line in CANNOT_PLAY_LINES {
            let (at, x, y, reach) = commands
                .iter()
                .enumerate()
                .find_map(|(i, c)| match c {
                    RenderCommand::Text {
                        text,
                        x,
                        y,
                        max_width,
                        ..
                    } if text == line => Some((i, *x, *y, x + max_width.unwrap_or(f32::INFINITY))),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{line:?} is not drawn"));
            let covered = commands.iter().skip(at + 1).any(|c| {
                matches!(c, RenderCommand::FillRect { x: rx, y: ry, width, height, .. }
                    if x >= *rx && x < rx + width && y >= *ry && y < ry + height)
            });
            assert!(!covered, "{line:?} is painted over");
            // Nor drawn on the same row as other text: a header's title over
            // a warning is as unreadable as a fill over it.
            let crowded = commands.iter().any(|c| {
                matches!(c, RenderCommand::Text { text, x: tx, y: ty, max_width: tw, .. }
                    if !CANNOT_PLAY_LINES.contains(&text.as_str())
                        && (ty - y).abs() < 10.0
                        && *tx < reach
                        && tx + tw.unwrap_or(f32::INFINITY) > x)
            });
            assert!(!crowded, "{line:?} shares its row with other text");
        }
    }
}
