//! Video Player application for OurOS.
//!
//! Full-featured media player with playlist management, subtitle support,
//! audio track selection, playback controls, and a modern UI. Supports
//! common container formats (MP4, MKV, AVI, WebM, MOV) and codecs
//! (H.264, H.265, VP9, AV1, AAC, Opus, FLAC).

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
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
// Part of the complete Catppuccin Mocha palette; kept for completeness even
// though no widget currently paints with it.
#[allow(dead_code)]
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);

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

/// Supported video codecs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    H264,
    H265,
    Vp8,
    Vp9,
    Av1,
    Mpeg2,
    Mpeg4,
    Theora,
    WmvV3,
}

impl VideoCodec {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::H264 => "H.264 / AVC",
            Self::H265 => "H.265 / HEVC",
            Self::Vp8 => "VP8",
            Self::Vp9 => "VP9",
            Self::Av1 => "AV1",
            Self::Mpeg2 => "MPEG-2",
            Self::Mpeg4 => "MPEG-4 Part 2",
            Self::Theora => "Theora",
            Self::WmvV3 => "WMV3",
        }
    }

    pub fn is_hardware_decodable(self) -> bool {
        matches!(self, Self::H264 | Self::H265 | Self::Vp9 | Self::Av1)
    }
}

/// Supported audio codecs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioCodec {
    Aac,
    Mp3,
    Opus,
    Vorbis,
    Flac,
    Pcm,
    Ac3,
    Eac3,
    Dts,
    Wma,
    TrueHd,
}

impl AudioCodec {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Aac => "AAC",
            Self::Mp3 => "MP3",
            Self::Opus => "Opus",
            Self::Vorbis => "Vorbis",
            Self::Flac => "FLAC",
            Self::Pcm => "PCM",
            Self::Ac3 => "Dolby Digital (AC-3)",
            Self::Eac3 => "Dolby Digital Plus (E-AC-3)",
            Self::Dts => "DTS",
            Self::Wma => "WMA",
            Self::TrueHd => "Dolby TrueHD",
        }
    }

    pub fn is_lossless(self) -> bool {
        matches!(self, Self::Flac | Self::Pcm | Self::TrueHd)
    }

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

/// A video stream within a media file.
#[derive(Debug, Clone)]
pub struct VideoStream {
    pub index: u32,
    pub codec: VideoCodec,
    pub width: u32,
    pub height: u32,
    pub frame_rate: f64,
    pub bit_rate: u64,
    pub pixel_format: String,
    pub color_space: Option<String>,
    pub hdr: bool,
}

impl VideoStream {
    pub fn resolution_label(&self) -> &'static str {
        match self.height {
            0..=360 => "360p",
            361..=480 => "480p (SD)",
            481..=720 => "720p (HD)",
            721..=1080 => "1080p (Full HD)",
            1081..=1440 => "1440p (2K)",
            1441..=2160 => "2160p (4K UHD)",
            2161..=4320 => "4320p (8K UHD)",
            _ => "Unknown",
        }
    }

    pub fn aspect_ratio(&self) -> String {
        if self.height == 0 {
            return "N/A".to_string();
        }
        let ratio = self.width as f64 / self.height as f64;
        // Common aspect ratios
        if (ratio - 16.0 / 9.0).abs() < 0.05 {
            "16:9".to_string()
        } else if (ratio - 4.0 / 3.0).abs() < 0.05 {
            "4:3".to_string()
        } else if (ratio - 21.0 / 9.0).abs() < 0.1 {
            "21:9".to_string()
        } else if (ratio - 1.0).abs() < 0.05 {
            "1:1".to_string()
        } else {
            format!("{ratio:.2}:1")
        }
    }

    pub fn bitrate_display(&self) -> String {
        format_bitrate(self.bit_rate)
    }
}

/// An audio stream within a media file.
#[derive(Debug, Clone)]
pub struct AudioStream {
    pub index: u32,
    pub codec: AudioCodec,
    pub sample_rate: u32,
    pub channels: u32,
    pub bit_rate: u64,
    pub language: Option<String>,
    pub title: Option<String>,
    pub is_default: bool,
}

impl AudioStream {
    pub fn display_label(&self) -> String {
        let codec = self.codec.display_name();
        let layout = AudioCodec::channel_layout_name(self.channels);
        let lang = self.language.as_deref().unwrap_or("Unknown");
        let title = self.title.as_deref().unwrap_or("");
        if title.is_empty() {
            format!("{lang} - {codec} {layout}")
        } else {
            format!("{title} ({lang}) - {codec} {layout}")
        }
    }
}

/// A subtitle stream.
#[derive(Debug, Clone)]
pub struct SubtitleStream {
    pub index: u32,
    pub format: SubtitleFormat,
    pub language: Option<String>,
    pub title: Option<String>,
    pub is_default: bool,
    pub is_forced: bool,
}

impl SubtitleStream {
    pub fn display_label(&self) -> String {
        let lang = self.language.as_deref().unwrap_or("Unknown");
        let fmt = self.format.display_name();
        let title = self.title.as_deref().unwrap_or("");
        let forced = if self.is_forced { " [Forced]" } else { "" };
        if title.is_empty() {
            format!("{lang} ({fmt}){forced}")
        } else {
            format!("{title} ({lang}, {fmt}){forced}")
        }
    }
}

/// Subtitle format types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitleFormat {
    Srt,
    Ass,
    Ssa,
    VobSub,
    Pgs,
    WebVtt,
    MovText,
}

impl SubtitleFormat {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Srt => "SRT",
            Self::Ass => "ASS/SSA",
            Self::Ssa => "SSA",
            Self::VobSub => "VobSub (DVD)",
            Self::Pgs => "PGS (Blu-ray)",
            Self::WebVtt => "WebVTT",
            Self::MovText => "mov_text",
        }
    }

    pub fn is_text_based(self) -> bool {
        matches!(
            self,
            Self::Srt | Self::Ass | Self::Ssa | Self::WebVtt | Self::MovText
        )
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
        // Pad or truncate to 3 digits
        let padded = format!("{ms_str:0<3}");
        padded[..3].parse().ok()?
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
    pub path: String,
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

    pub fn overall_bitrate(&self) -> u64 {
        if self.duration.as_secs() == 0 {
            return 0;
        }
        self.file_size.saturating_mul(8) / self.duration.as_secs()
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
        if self.ms == 0 {
            "Sync: 0ms".to_string()
        } else if self.ms > 0 {
            format!("Sync: +{}ms", self.ms)
        } else {
            format!("Sync: {}ms", self.ms)
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
    pub path: String,
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
}

impl Playlist {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            current_index: None,
            next_id: 1,
            shuffle: false,
            shuffle_order: Vec::new(),
            shuffle_position: 0,
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
        path: String,
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
            self.shuffle_order.push(self.entries.len() - 1);
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
                    self.current_index = Some(ci.min(self.entries.len() - 1));
                }
            }
            Some(ci) if ci > index => {
                self.current_index = Some(ci - 1);
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
                self.current_index = Some(ci - 1);
            } else if to <= ci && ci < from {
                self.current_index = Some(ci + 1);
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
                let next = ci + 1;
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
                self.shuffle_position -= 1;
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
                self.current_index = Some(ci - 1);
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

    fn rebuild_shuffle_order(&mut self) {
        self.shuffle_order = (0..self.entries.len()).collect();
        // Simple deterministic shuffle using entry IDs as seed
        let len = self.shuffle_order.len();
        if len > 1 {
            let mut seed: u64 = 12345;
            for entry in &self.entries {
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(entry.id);
            }
            for i in (1..len).rev() {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                let j = (seed >> 33) as usize % (i + 1);
                self.shuffle_order.swap(i, j);
            }
        }
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
pub struct Shortcuts;

impl Shortcuts {
    pub fn list() -> Vec<(&'static str, &'static str)> {
        vec![
            ("Space", "Play / Pause"),
            ("S", "Stop"),
            ("F", "Toggle Fullscreen"),
            ("M", "Toggle Mute"),
            ("Up", "Volume Up"),
            ("Down", "Volume Down"),
            ("Right", "Seek Forward 10s"),
            ("Left", "Seek Backward 10s"),
            ("Shift+Right", "Seek Forward 60s"),
            ("Shift+Left", "Seek Backward 60s"),
            ("N", "Next in Playlist"),
            ("P", "Previous in Playlist"),
            ("[", "Decrease Speed"),
            ("]", "Increase Speed"),
            ("\\", "Reset Speed"),
            ("A", "Cycle Aspect Ratio"),
            ("V", "Cycle Subtitles"),
            ("B", "Cycle Audio Track"),
            ("T", "Toggle Subtitle"),
            ("C", "Toggle Chapter List"),
            ("L", "Toggle Playlist"),
            ("I", "Show Media Info"),
            ("E", "Toggle Equalizer"),
            ("J", "Subtitle Delay -100ms"),
            ("K", "Subtitle Delay +100ms"),
            ("G", "Audio Sync -100ms"),
            ("H", "Audio Sync +100ms"),
            ("Ctrl+O", "Open File"),
            ("Ctrl+S", "Take Screenshot"),
            ("Ctrl+B", "Add Bookmark"),
            ("1-9", "Seek to 10%-90%"),
            ("0", "Seek to Start"),
        ]
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

// ============================================================================
// Recent files
// ============================================================================

/// A recently opened file.
#[derive(Debug, Clone)]
pub struct RecentFile {
    pub path: String,
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
        path: String,
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

    pub fn find_by_path(&self, path: &str) -> Option<&RecentFile> {
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

// ============================================================================
// Utility functions
// ============================================================================

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{} KB", bytes / KB)
    } else {
        format!("{bytes} B")
    }
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

    // OSD messages
    pub osd_message: Option<String>,
    pub osd_timestamp: u64,
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
            osd_message: None,
            osd_timestamp: 0,
        }
    }

    // ========================================================================
    // Playback control
    // ========================================================================

    pub fn play(&mut self) {
        if self.current_file.is_some() {
            self.state = PlaybackState::Playing;
            self.show_osd("Play");
        }
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
            && idx + 1 < self.chapters.len()
        {
            self.seek_to_chapter(idx + 1);
        }
    }

    pub fn previous_chapter(&mut self) {
        if let Some((idx, ch)) = self.current_chapter() {
            // If we're more than 3 seconds into the chapter, go to its start
            if self.position.saturating_sub(ch.start).as_secs() > 3 {
                self.position = ch.start;
            } else if idx > 0 {
                self.seek_to_chapter(idx - 1);
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
                        Some(p) if p + 1 < file.audio_streams.len() => {
                            file.audio_streams.get(p + 1).map(|s| s.index)
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
                        Some(p) if p + 1 < file.subtitle_streams.len() => {
                            self.selected_subtitle_track =
                                file.subtitle_streams.get(p + 1).map(|s| s.index);
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

    fn load_playlist_entry(&mut self, index: usize) {
        if let Some(entry) = self.playlist.entries().get(index) {
            let file_name = entry.file_name.clone();
            self.show_osd(&format!("Now playing: {file_name}"));
            self.position = Duration::ZERO;
            self.state = PlaybackState::Playing;
        }
    }

    // ========================================================================
    // OSD
    // ========================================================================

    fn show_osd(&mut self, message: &str) {
        self.osd_message = Some(message.to_string());
        self.osd_timestamp = 0; // Would be set to current time in real impl
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
                .saturating_sub(Duration::from_millis((-self.subtitle_delay.ms) as u64))
        };
        self.external_subtitles
            .iter()
            .find(|cue| cue.is_active_at(adjusted_pos))
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    pub fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(256);

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

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

        cmds
    }

    fn render_tab_bar(&self, cmds: &mut Vec<RenderCommand>) {
        let tab_bar_h = 36.0;

        // Tab bar background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: tab_bar_h,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Tab items
        let tabs = PlayerTab::all();
        let tab_width = (self.width / tabs.len() as f32).min(120.0);
        let mut tx = 4.0;

        for &tab in tabs {
            let active = tab == self.active_tab;
            let bg = if active { SURFACE0 } else { MANTLE };
            let fg = if active { BLUE } else { SUBTEXT0 };

            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: 4.0,
                width: tab_width - 4.0,
                height: tab_bar_h - 8.0,
                color: bg,
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: tx + (tab_width - 4.0) / 2.0 - 24.0,
                y: 12.0,
                text: tab.label().to_string(),
                font_size: 12.0,
                color: fg,
                font_weight: if active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(tab_width - 12.0),
            });

            if active {
                cmds.push(RenderCommand::FillRect {
                    x: tx + 2.0,
                    y: tab_bar_h - 3.0,
                    width: tab_width - 8.0,
                    height: 2.0,
                    color: BLUE,
                    corner_radii: CornerRadii::all(1.0),
                });
            }

            tx += tab_width;
        }

        // Separator
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: tab_bar_h,
            x2: self.width,
            y2: tab_bar_h,
            color: SURFACE0,
            width: 1.0,
        });
    }

    fn render_player_view(&self, cmds: &mut Vec<RenderCommand>) {
        let top = 40.0;
        let controls_h = 80.0;
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

        // If no file loaded, show placeholder
        if self.current_file.is_none() {
            cmds.push(RenderCommand::Text {
                x: self.width / 2.0 - 80.0,
                y: top + video_h / 2.0 - 20.0,
                text: "No file loaded".to_string(),
                font_size: 18.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(300.0),
            });

            cmds.push(RenderCommand::Text {
                x: self.width / 2.0 - 60.0,
                y: top + video_h / 2.0 + 10.0,
                text: "Ctrl+O to open".to_string(),
                font_size: 13.0,
                color: SURFACE2,
                font_weight: FontWeightHint::Regular,
                max_width: Some(200.0),
            });
        } else {
            // Video frame placeholder
            if let Some(file) = &self.current_file
                && let Some(vs) = file.primary_video()
            {
                let label = format!("{}x{} {}", vs.width, vs.height, vs.codec.display_name());
                cmds.push(RenderCommand::Text {
                    x: self.width / 2.0 - 60.0,
                    y: top + video_h / 2.0 - 8.0,
                    text: label,
                    font_size: 14.0,
                    color: SURFACE2,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(300.0),
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
                });
            }
        }

        // Controls area
        let ctrl_y = top + video_h;
        self.render_controls(cmds, ctrl_y, controls_h);
    }

    fn render_controls(&self, cmds: &mut Vec<RenderCommand>, y: f32, height: f32) {
        // Controls background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.width,
            height,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Seek bar
        let seek_y = y + 8.0;
        let seek_x = 16.0;
        let seek_w = self.width - 32.0;
        let seek_h = 6.0;

        // Seek track background
        cmds.push(RenderCommand::FillRect {
            x: seek_x,
            y: seek_y,
            width: seek_w,
            height: seek_h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(3.0),
        });

        // Buffer progress (slightly ahead of play position)
        let buffer_frac = (self.progress_fraction() + 0.05).min(1.0);
        cmds.push(RenderCommand::FillRect {
            x: seek_x,
            y: seek_y,
            width: seek_w * buffer_frac as f32,
            height: seek_h,
            color: SURFACE1,
            corner_radii: CornerRadii::all(3.0),
        });

        // Play progress
        let progress = self.progress_fraction() as f32;
        cmds.push(RenderCommand::FillRect {
            x: seek_x,
            y: seek_y,
            width: seek_w * progress,
            height: seek_h,
            color: BLUE,
            corner_radii: CornerRadii::all(3.0),
        });

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
                    color: YELLOW,
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
            color: BLUE,
            corner_radii: CornerRadii::all(6.0),
        });

        // Time display
        let time_y = seek_y + seek_h + 8.0;
        cmds.push(RenderCommand::Text {
            x: seek_x,
            y: time_y,
            text: self.time_display(),
            font_size: 11.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
        });

        // Remaining time
        cmds.push(RenderCommand::Text {
            x: self.width - 100.0,
            y: time_y,
            text: format!("-{}", self.remaining_duration().format()),
            font_size: 11.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(100.0),
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
            color: if self.volume.is_muted() { RED } else { TEXT },
            font_weight: FontWeightHint::Bold,
            max_width: None,
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
            color: SURFACE0,
            corner_radii: CornerRadii::all(2.0),
        });

        let vol_frac = self.volume.fraction() as f32;
        let vol_color = if self.volume.effective_level() > Volume::NORMAL {
            PEACH
        } else {
            GREEN
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
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(50.0),
        });

        // Right controls: speed, repeat, shuffle, aspect
        let right_x = self.width - 220.0;

        cmds.push(RenderCommand::Text {
            x: right_x,
            y: btn_y + 6.0,
            text: self.speed.label(),
            font_size: 11.0,
            color: if self.speed.value() != 1.0 {
                PEACH
            } else {
                SUBTEXT0
            },
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: right_x + 40.0,
            y: btn_y + 6.0,
            text: self.repeat.icon().to_string(),
            font_size: 11.0,
            color: if self.repeat != RepeatMode::Off {
                BLUE
            } else {
                SUBTEXT0
            },
            font_weight: FontWeightHint::Bold,
            max_width: None,
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
                GREEN
            } else {
                SUBTEXT0
            },
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: right_x + 100.0,
            y: btn_y + 6.0,
            text: self.aspect_mode.label().to_string(),
            font_size: 11.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(60.0),
        });

        // Subtitle indicator
        if self.subtitle_enabled {
            cmds.push(RenderCommand::Text {
                x: right_x + 160.0,
                y: btn_y + 6.0,
                text: "CC".to_string(),
                font_size: 11.0,
                color: YELLOW,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }

        // Fullscreen toggle
        cmds.push(RenderCommand::Text {
            x: right_x + 190.0,
            y: btn_y + 6.0,
            text: if self.fullscreen { "[-]" } else { "[+]" }.to_string(),
            font_size: 11.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
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
        let bg = if primary { BLUE } else { SURFACE0 };
        let fg = if primary { CRUST } else { TEXT };

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
            color: BASE,
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
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(panel_w - 32.0),
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
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(panel_w - 32.0),
        });

        // Separator
        cmds.push(RenderCommand::Line {
            x1: 16.0,
            y1: top + 52.0,
            x2: panel_w - 16.0,
            y2: top + 52.0,
            color: SURFACE0,
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
                cmds.push(RenderCommand::FillRect {
                    x: 8.0,
                    y: ey,
                    width: panel_w - 16.0,
                    height: item_h - 2.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            // Index
            cmds.push(RenderCommand::Text {
                x: 16.0,
                y: ey + 8.0,
                text: format!("{}", i + 1),
                font_size: 11.0,
                color: if is_current { BLUE } else { OVERLAY0 },
                font_weight: FontWeightHint::Regular,
                max_width: Some(24.0),
            });

            // Playing indicator
            if is_current && self.state == PlaybackState::Playing {
                cmds.push(RenderCommand::Text {
                    x: 40.0,
                    y: ey + 8.0,
                    text: ">".to_string(),
                    font_size: 12.0,
                    color: GREEN,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }

            // Title
            cmds.push(RenderCommand::Text {
                x: 56.0,
                y: ey + 6.0,
                text: entry.display_name().to_string(),
                font_size: 13.0,
                color: if is_current { TEXT } else { SUBTEXT1 },
                font_weight: if is_current {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(panel_w - 160.0),
            });

            // Duration
            if let Some(dur) = entry.duration {
                cmds.push(RenderCommand::Text {
                    x: panel_w - 80.0,
                    y: ey + 8.0,
                    text: dur.format(),
                    font_size: 11.0,
                    color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(60.0),
                });
            }

            // Path (small)
            cmds.push(RenderCommand::Text {
                x: 56.0,
                y: ey + 22.0,
                text: entry.path.clone(),
                font_size: 10.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(panel_w - 100.0),
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
            color: BASE,
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
                    color: BLUE,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(300.0),
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
                        color: SUBTEXT0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(120.0),
                    });
                    cmds.push(RenderCommand::Text {
                        x: value_x,
                        y: *y,
                        text: value.to_string(),
                        font_size: 12.0,
                        color: TEXT,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(400.0),
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
                info_section(cmds, &mut line_y, &format!("Video Stream #{}", i + 1));
                info_row(cmds, &mut line_y, "Codec", vs.codec.display_name());
                info_row(
                    cmds,
                    &mut line_y,
                    "Resolution",
                    &format!("{}x{} ({})", vs.width, vs.height, vs.resolution_label()),
                );
                info_row(cmds, &mut line_y, "Aspect Ratio", &vs.aspect_ratio());
                info_row(
                    cmds,
                    &mut line_y,
                    "Frame Rate",
                    &format!("{:.3} fps", vs.frame_rate),
                );
                info_row(cmds, &mut line_y, "Bit Rate", &vs.bitrate_display());
                info_row(cmds, &mut line_y, "Pixel Format", &vs.pixel_format);
                if vs.hdr {
                    info_row(cmds, &mut line_y, "HDR", "Yes");
                }
                if let Some(cs) = &vs.color_space {
                    info_row(cmds, &mut line_y, "Color Space", cs);
                }
                info_row(
                    cmds,
                    &mut line_y,
                    "HW Decode",
                    if vs.codec.is_hardware_decodable() {
                        "Supported"
                    } else {
                        "Not available"
                    },
                );
            }

            // Audio streams
            for (i, audio) in file.audio_streams.iter().enumerate() {
                info_section(cmds, &mut line_y, &format!("Audio Stream #{}", i + 1));
                info_row(cmds, &mut line_y, "Codec", audio.codec.display_name());
                info_row(
                    cmds,
                    &mut line_y,
                    "Sample Rate",
                    &format!("{} Hz", audio.sample_rate),
                );
                info_row(
                    cmds,
                    &mut line_y,
                    "Channels",
                    AudioCodec::channel_layout_name(audio.channels),
                );
                info_row(
                    cmds,
                    &mut line_y,
                    "Bit Rate",
                    &format_bitrate(audio.bit_rate),
                );
                if let Some(lang) = &audio.language {
                    info_row(cmds, &mut line_y, "Language", lang);
                }
                info_row(
                    cmds,
                    &mut line_y,
                    "Lossless",
                    if audio.codec.is_lossless() {
                        "Yes"
                    } else {
                        "No"
                    },
                );
            }

            // Subtitle streams
            for (i, sub) in file.subtitle_streams.iter().enumerate() {
                info_section(cmds, &mut line_y, &format!("Subtitle Stream #{}", i + 1));
                info_row(cmds, &mut line_y, "Format", sub.format.display_name());
                if let Some(lang) = &sub.language {
                    info_row(cmds, &mut line_y, "Language", lang);
                }
                info_row(
                    cmds,
                    &mut line_y,
                    "Text Based",
                    if sub.format.is_text_based() {
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
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(200.0),
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
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Header
        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: top + 20.0,
            text: format!("Equalizer - {}", self.equalizer.preset.label()),
            font_size: 16.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(300.0),
        });

        // Enabled indicator
        let enabled_color = if self.equalizer.enabled { GREEN } else { RED };
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
            color: CRUST,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Preamp
        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: top + 52.0,
            text: format!("Preamp: {:.1} dB", self.equalizer.preamp),
            font_size: 12.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(150.0),
        });

        // Preset buttons
        let preset_y = top + 50.0;
        let mut px = 180.0;
        for preset in EqPreset::all() {
            let active = *preset == self.equalizer.preset;
            let bg = if active { BLUE } else { SURFACE0 };
            let fg = if active { CRUST } else { SUBTEXT1 };

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
                color: SURFACE1,
                width: 1.0,
            });

            // +/- labels
            cmds.push(RenderCommand::Text {
                x: 4.0,
                y: bands_y + 4.0,
                text: "+12".to_string(),
                font_size: 9.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cmds.push(RenderCommand::Text {
                x: 4.0,
                y: center_y - 6.0,
                text: "0".to_string(),
                font_size: 9.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cmds.push(RenderCommand::Text {
                x: 4.0,
                y: bands_y + bands_h - 12.0,
                text: "-12".to_string(),
                font_size: 9.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            for (i, band) in self.equalizer.bands.iter().enumerate() {
                let bx = 30.0 + i as f32 * band_w;
                let max_travel = bands_h / 2.0;
                let bar_h = (band.gain / 12.0) * max_travel;

                let color = if band.gain > 0.0 {
                    GREEN
                } else if band.gain < 0.0 {
                    RED
                } else {
                    SURFACE2
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
                    color: LAVENDER,
                    corner_radii: CornerRadii::all(3.0),
                });

                // Frequency label
                cmds.push(RenderCommand::Text {
                    x: bx + band_w * 0.1,
                    y: bands_y + bands_h + 4.0,
                    text: band.label.clone(),
                    font_size: 9.0,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(band_w),
                });

                // Gain value
                cmds.push(RenderCommand::Text {
                    x: bx + band_w * 0.15,
                    y: handle_y - 16.0,
                    text: format!("{:.0}", band.gain),
                    font_size: 9.0,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(band_w * 0.7),
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
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: top + 20.0,
            text: "Video Adjustments".to_string(),
            font_size: 16.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(300.0),
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
                color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(110.0),
            });

            // Slider track
            cmds.push(RenderCommand::FillRect {
                x: bar_x,
                y: sy + 8.0,
                width: slider_w,
                height: 6.0,
                color: SURFACE0,
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
                color: BLUE,
                corner_radii: CornerRadii::all(3.0),
            });

            // Slider handle
            let handle_x = bar_x + slider_w * frac;
            cmds.push(RenderCommand::FillRect {
                x: handle_x - 5.0,
                y: sy + 4.0,
                width: 10.0,
                height: 14.0,
                color: LAVENDER,
                corner_radii: CornerRadii::all(5.0),
            });

            // Value
            cmds.push(RenderCommand::Text {
                x: bar_x + slider_w + 12.0,
                y: sy + 4.0,
                text: format!("{value:.2}"),
                font_size: 12.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(50.0),
            });
        }

        // Reset button
        let reset_y = top + 60.0 + sliders.len() as f32 * 50.0 + 10.0;
        let is_default = adj.is_default();
        let reset_bg = if is_default { SURFACE0 } else { PEACH };
        let reset_fg = if is_default { OVERLAY0 } else { CRUST };

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
        });

        // Deinterlace mode
        cmds.push(RenderCommand::Text {
            x: label_x,
            y: reset_y + 50.0,
            text: format!("Deinterlace: {}", self.preferences.deinterlace.label()),
            font_size: 13.0,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(300.0),
        });
    }

    fn render_settings(&self, cmds: &mut Vec<RenderCommand>) {
        let top = 40.0;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: top,
            width: self.width,
            height: self.height - top,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: top + 20.0,
            text: "Player Settings".to_string(),
            font_size: 16.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(300.0),
        });

        let prefs = &self.preferences;
        let settings = [
            (
                "Resume Playback",
                if prefs.resume_playback { "On" } else { "Off" },
            ),
            (
                "Remember Volume",
                if prefs.remember_volume { "On" } else { "Off" },
            ),
            (
                "Hardware Decode",
                if prefs.hardware_decode { "On" } else { "Off" },
            ),
            (
                "Auto-load Subtitles",
                if prefs.subtitle_auto_load {
                    "On"
                } else {
                    "Off"
                },
            ),
            ("On Finish", prefs.on_finish.label()),
            ("Deinterlace", prefs.deinterlace.label()),
        ];

        let label_x = 20.0;
        let value_x = 220.0;

        for (i, (name, value)) in settings.iter().enumerate() {
            let sy = top + 60.0 + i as f32 * 36.0;

            cmds.push(RenderCommand::FillRect {
                x: 12.0,
                y: sy - 2.0,
                width: self.width - 24.0,
                height: 32.0,
                color: if i % 2 == 0 { SURFACE0 } else { BASE },
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: label_x,
                y: sy + 6.0,
                text: name.to_string(),
                font_size: 13.0,
                color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(180.0),
            });

            let value_color = if *value == "On" {
                GREEN
            } else if *value == "Off" {
                RED
            } else {
                SUBTEXT1
            };
            cmds.push(RenderCommand::Text {
                x: value_x,
                y: sy + 6.0,
                text: value.to_string(),
                font_size: 13.0,
                color: value_color,
                font_weight: FontWeightHint::Bold,
                max_width: Some(200.0),
            });
        }

        // Additional settings
        let extra_y = top + 60.0 + settings.len() as f32 * 36.0 + 20.0;

        cmds.push(RenderCommand::Text {
            x: label_x,
            y: extra_y,
            text: "Seek Steps".to_string(),
            font_size: 14.0,
            color: BLUE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
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
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(300.0),
        });

        cmds.push(RenderCommand::Text {
            x: label_x,
            y: extra_y + 52.0,
            text: "Preferred Languages".to_string(),
            font_size: 14.0,
            color: BLUE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
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
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(400.0),
        });

        cmds.push(RenderCommand::Text {
            x: label_x,
            y: extra_y + 104.0,
            text: "Screenshots".to_string(),
            font_size: 14.0,
            color: BLUE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
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
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(500.0),
        });
    }

    fn render_shortcuts(&self, cmds: &mut Vec<RenderCommand>) {
        let top = 40.0;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: top,
            width: self.width,
            height: self.height - top,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: top + 20.0,
            text: "Keyboard Shortcuts".to_string(),
            font_size: 16.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(300.0),
        });

        let shortcuts = Shortcuts::list();
        let col1_x = 20.0;
        let col1_val = 140.0;
        let col2_x = self.width / 2.0 + 10.0;
        let col2_val = self.width / 2.0 + 130.0;

        let half = shortcuts.len().div_ceil(2);

        for (i, (key, action)) in shortcuts.iter().enumerate() {
            let (kx, vx, row) = if i < half {
                (col1_x, col1_val, i)
            } else {
                (col2_x, col2_val, i - half)
            };

            let sy = top + 52.0 + row as f32 * 24.0;

            // Key badge
            cmds.push(RenderCommand::FillRect {
                x: kx,
                y: sy - 1.0,
                width: 80.0,
                height: 20.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(3.0),
            });
            cmds.push(RenderCommand::Text {
                x: kx + 4.0,
                y: sy + 2.0,
                text: key.to_string(),
                font_size: 11.0,
                color: MAUVE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(74.0),
            });

            // Action
            cmds.push(RenderCommand::Text {
                x: vx,
                y: sy + 2.0,
                text: action.to_string(),
                font_size: 11.0,
                color: SUBTEXT1,
                font_weight: FontWeightHint::Regular,
                max_width: Some(180.0),
            });
        }
    }
}

// ============================================================================
// Sample data for testing
// ============================================================================

fn sample_media_file() -> MediaFile {
    MediaFile {
        path: "/home/user/Videos/sample.mkv".to_string(),
        file_name: "sample.mkv".to_string(),
        file_size: 1_500_000_000,
        container: ContainerFormat::Mkv,
        duration: Duration::from_secs(7200),
        video_streams: vec![VideoStream {
            index: 0,
            codec: VideoCodec::H265,
            width: 3840,
            height: 2160,
            frame_rate: 23.976,
            bit_rate: 15_000_000,
            pixel_format: "yuv420p10le".to_string(),
            color_space: Some("bt2020nc".to_string()),
            hdr: true,
        }],
        audio_streams: vec![
            AudioStream {
                index: 1,
                codec: AudioCodec::TrueHd,
                sample_rate: 48000,
                channels: 8,
                bit_rate: 4_500_000,
                language: Some("English".to_string()),
                title: Some("Dolby TrueHD 7.1".to_string()),
                is_default: true,
            },
            AudioStream {
                index: 2,
                codec: AudioCodec::Aac,
                sample_rate: 48000,
                channels: 2,
                bit_rate: 192_000,
                language: Some("English".to_string()),
                title: Some("Stereo Commentary".to_string()),
                is_default: false,
            },
            AudioStream {
                index: 3,
                codec: AudioCodec::Ac3,
                sample_rate: 48000,
                channels: 6,
                bit_rate: 640_000,
                language: Some("Spanish".to_string()),
                title: None,
                is_default: false,
            },
        ],
        subtitle_streams: vec![
            SubtitleStream {
                index: 4,
                format: SubtitleFormat::Pgs,
                language: Some("English".to_string()),
                title: Some("Full".to_string()),
                is_default: true,
                is_forced: false,
            },
            SubtitleStream {
                index: 5,
                format: SubtitleFormat::Srt,
                language: Some("Spanish".to_string()),
                title: None,
                is_default: false,
                is_forced: false,
            },
            SubtitleStream {
                index: 6,
                format: SubtitleFormat::Ass,
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

fn main() {
    let mut app = VideoPlayerApp::new(1280.0, 720.0);

    // Load sample media file
    app.current_file = Some(sample_media_file());
    app.chapters = sample_chapters();
    app.selected_audio_track = Some(1);
    app.selected_subtitle_track = Some(4);

    // Load SRT subtitles
    app.external_subtitles = parse_srt(sample_subtitle_srt());

    // Add to playlist
    app.playlist.add(
        "/home/user/Videos/sample.mkv".to_string(),
        "sample.mkv".to_string(),
        Some(Duration::from_secs(7200)),
        Some("Sample Movie".to_string()),
    );
    app.playlist.add(
        "/home/user/Videos/trailer.mp4".to_string(),
        "trailer.mp4".to_string(),
        Some(Duration::from_secs(120)),
        None,
    );
    app.playlist.add(
        "/home/user/Videos/concert.webm".to_string(),
        "concert.webm".to_string(),
        Some(Duration::from_secs(5400)),
        Some("Live Concert 2024".to_string()),
    );
    app.playlist.set_current(0);

    // Simulate some interaction
    app.play();
    app.position = Duration::from_secs(300);
    app.volume.set_level(80);

    // Add a bookmark
    app.add_bookmark("Favorite scene".to_string());

    // Render
    let commands = app.render();
    let _ = commands.len();

    // Verify all tabs render
    for tab in PlayerTab::all() {
        app.active_tab = *tab;
        let cmds = app.render();
        let _ = cmds.len();
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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

    // Video codec tests
    #[test]
    fn test_video_codec_hw_decode() {
        assert!(VideoCodec::H264.is_hardware_decodable());
        assert!(VideoCodec::Av1.is_hardware_decodable());
        assert!(!VideoCodec::Theora.is_hardware_decodable());
    }

    // Audio codec tests
    #[test]
    fn test_audio_codec_lossless() {
        assert!(AudioCodec::Flac.is_lossless());
        assert!(AudioCodec::TrueHd.is_lossless());
        assert!(!AudioCodec::Aac.is_lossless());
    }

    #[test]
    fn test_channel_layout_name() {
        assert_eq!(AudioCodec::channel_layout_name(2), "Stereo");
        assert_eq!(AudioCodec::channel_layout_name(6), "5.1");
        assert_eq!(AudioCodec::channel_layout_name(8), "7.1");
    }

    // Video stream tests
    #[test]
    fn test_resolution_label() {
        let vs = VideoStream {
            index: 0,
            codec: VideoCodec::H264,
            width: 1920,
            height: 1080,
            frame_rate: 24.0,
            bit_rate: 5_000_000,
            pixel_format: "yuv420p".to_string(),
            color_space: None,
            hdr: false,
        };
        assert_eq!(vs.resolution_label(), "1080p (Full HD)");
    }

    #[test]
    fn test_aspect_ratio() {
        let vs = VideoStream {
            index: 0,
            codec: VideoCodec::H264,
            width: 1920,
            height: 1080,
            frame_rate: 24.0,
            bit_rate: 5_000_000,
            pixel_format: "yuv420p".to_string(),
            color_space: None,
            hdr: false,
        };
        assert_eq!(vs.aspect_ratio(), "16:9");
    }

    #[test]
    fn test_aspect_ratio_4_3() {
        let vs = VideoStream {
            index: 0,
            codec: VideoCodec::Mpeg2,
            width: 640,
            height: 480,
            frame_rate: 30.0,
            bit_rate: 2_000_000,
            pixel_format: "yuv420p".to_string(),
            color_space: None,
            hdr: false,
        };
        assert_eq!(vs.aspect_ratio(), "4:3");
    }

    // Audio stream tests
    #[test]
    fn test_audio_display_label() {
        let stream = AudioStream {
            index: 1,
            codec: AudioCodec::Aac,
            sample_rate: 48000,
            channels: 2,
            bit_rate: 192000,
            language: Some("English".to_string()),
            title: None,
            is_default: true,
        };
        assert_eq!(stream.display_label(), "English - AAC Stereo");
    }

    #[test]
    fn test_audio_display_label_with_title() {
        let stream = AudioStream {
            index: 1,
            codec: AudioCodec::Ac3,
            sample_rate: 48000,
            channels: 6,
            bit_rate: 640000,
            language: Some("English".to_string()),
            title: Some("Commentary".to_string()),
            is_default: false,
        };
        assert_eq!(
            stream.display_label(),
            "Commentary (English) - Dolby Digital (AC-3) 5.1"
        );
    }

    // Subtitle tests
    #[test]
    fn test_subtitle_display_label() {
        let sub = SubtitleStream {
            index: 0,
            format: SubtitleFormat::Srt,
            language: Some("English".to_string()),
            title: None,
            is_default: true,
            is_forced: false,
        };
        assert_eq!(sub.display_label(), "English (SRT)");
    }

    #[test]
    fn test_subtitle_forced_label() {
        let sub = SubtitleStream {
            index: 0,
            format: SubtitleFormat::Pgs,
            language: Some("English".to_string()),
            title: Some("Signs".to_string()),
            is_default: false,
            is_forced: true,
        };
        assert!(sub.display_label().contains("[Forced]"));
    }

    #[test]
    fn test_subtitle_format_text_based() {
        assert!(SubtitleFormat::Srt.is_text_based());
        assert!(SubtitleFormat::WebVtt.is_text_based());
        assert!(!SubtitleFormat::Pgs.is_text_based());
        assert!(!SubtitleFormat::VobSub.is_text_based());
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
        pl.add("a.mp4".to_string(), "a.mp4".to_string(), None, None);
        pl.add("b.mp4".to_string(), "b.mp4".to_string(), None, None);
        assert_eq!(pl.len(), 2);
        pl.remove(0);
        assert_eq!(pl.len(), 1);
        assert_eq!(pl.entries()[0].file_name, "b.mp4");
    }

    #[test]
    fn test_playlist_next_sequential() {
        let mut pl = Playlist::new();
        pl.add("a.mp4".to_string(), "a.mp4".to_string(), None, None);
        pl.add("b.mp4".to_string(), "b.mp4".to_string(), None, None);
        pl.add("c.mp4".to_string(), "c.mp4".to_string(), None, None);
        pl.set_current(0);
        assert_eq!(pl.next(RepeatMode::Off), Some(1));
        assert_eq!(pl.next(RepeatMode::Off), Some(2));
        assert_eq!(pl.next(RepeatMode::Off), None);
    }

    #[test]
    fn test_playlist_next_repeat_all() {
        let mut pl = Playlist::new();
        pl.add("a.mp4".to_string(), "a.mp4".to_string(), None, None);
        pl.add("b.mp4".to_string(), "b.mp4".to_string(), None, None);
        pl.set_current(1);
        assert_eq!(pl.next(RepeatMode::All), Some(0));
    }

    #[test]
    fn test_playlist_next_repeat_one() {
        let mut pl = Playlist::new();
        pl.add("a.mp4".to_string(), "a.mp4".to_string(), None, None);
        pl.add("b.mp4".to_string(), "b.mp4".to_string(), None, None);
        pl.set_current(0);
        assert_eq!(pl.next(RepeatMode::One), Some(0));
    }

    #[test]
    fn test_playlist_previous() {
        let mut pl = Playlist::new();
        pl.add("a.mp4".to_string(), "a.mp4".to_string(), None, None);
        pl.add("b.mp4".to_string(), "b.mp4".to_string(), None, None);
        pl.set_current(1);
        assert_eq!(pl.previous(), Some(0));
    }

    #[test]
    fn test_playlist_clear() {
        let mut pl = Playlist::new();
        pl.add("a.mp4".to_string(), "a.mp4".to_string(), None, None);
        pl.set_current(0);
        pl.clear();
        assert!(pl.is_empty());
        assert!(pl.current_index().is_none());
    }

    #[test]
    fn test_playlist_move_entry() {
        let mut pl = Playlist::new();
        pl.add("a.mp4".to_string(), "a.mp4".to_string(), None, None);
        pl.add("b.mp4".to_string(), "b.mp4".to_string(), None, None);
        pl.add("c.mp4".to_string(), "c.mp4".to_string(), None, None);
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
            pl.add(format!("{i}.mp4"), format!("{i}.mp4"), None, None);
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

    #[test]
    fn test_playlist_total_duration() {
        let mut pl = Playlist::new();
        pl.add(
            "a.mp4".to_string(),
            "a.mp4".to_string(),
            Some(Duration::from_secs(60)),
            None,
        );
        pl.add(
            "b.mp4".to_string(),
            "b.mp4".to_string(),
            Some(Duration::from_secs(120)),
            None,
        );
        pl.add("c.mp4".to_string(), "c.mp4".to_string(), None, None);
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

    // Recent files tests
    #[test]
    fn test_recent_history() {
        let mut recent = RecentHistory::new(3);
        recent.add(
            "a.mp4".to_string(),
            "a.mp4".to_string(),
            Duration::ZERO,
            100,
            None,
        );
        recent.add(
            "b.mp4".to_string(),
            "b.mp4".to_string(),
            Duration::ZERO,
            200,
            None,
        );
        recent.add(
            "c.mp4".to_string(),
            "c.mp4".to_string(),
            Duration::ZERO,
            300,
            None,
        );
        assert_eq!(recent.files().len(), 3);
        recent.add(
            "d.mp4".to_string(),
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
            "a.mp4".to_string(),
            "a.mp4".to_string(),
            Duration::from_secs(10),
            100,
            None,
        );
        recent.add(
            "b.mp4".to_string(),
            "b.mp4".to_string(),
            Duration::ZERO,
            200,
            None,
        );
        recent.add(
            "a.mp4".to_string(),
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
            "a.mp4".to_string(),
            "a.mp4".to_string(),
            Duration::from_secs(30),
            100,
            Some(Duration::from_secs(120)),
        );
        let found = recent.find_by_path("a.mp4").unwrap();
        assert_eq!(found.last_position, Duration::from_secs(30));
        assert!((found.progress_fraction() - 0.25).abs() < 0.01);
    }

    // Media file tests
    #[test]
    fn test_media_file_display() {
        let file = sample_media_file();
        assert!(file.file_size_display().contains("GB"));
        assert!(file.overall_bitrate() > 0);
    }

    #[test]
    fn test_media_file_primary_streams() {
        let file = sample_media_file();
        let video = file.primary_video().unwrap();
        assert_eq!(video.codec, VideoCodec::H265);
        let audio = file.primary_audio().unwrap();
        assert!(audio.is_default);
    }

    // Player app tests
    #[test]
    fn test_player_play_pause() {
        let mut app = VideoPlayerApp::new(800.0, 600.0);
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
            "test.mp4".to_string(),
            "test.mp4".to_string(),
            Some(Duration::from_secs(120)),
            None,
        );
        app.playlist.set_current(0);

        for tab in PlayerTab::all() {
            app.active_tab = *tab;
            let cmds = app.render();
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
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_osd() {
        let mut app = VideoPlayerApp::new(800.0, 600.0);
        app.current_file = Some(sample_media_file());
        app.osd_message = Some("Test OSD".to_string());
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_paused_state() {
        let mut app = VideoPlayerApp::new(800.0, 600.0);
        app.current_file = Some(sample_media_file());
        app.state = PlaybackState::Paused;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    // Format helpers tests
    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(2048), "2 KB");
        assert!(format_bytes(1_500_000).contains("MB"));
        assert!(format_bytes(2_000_000_000).contains("GB"));
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
        assert!(shortcuts.iter().any(|(k, _)| *k == "Space"));
        assert!(shortcuts.iter().any(|(_, a)| *a == "Toggle Fullscreen"));
    }

    // Recent file tests
    #[test]
    fn test_recent_file_resume_label() {
        let rf = RecentFile {
            path: "test.mp4".to_string(),
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
            path: "/home/test.mp4".to_string(),
            file_name: "test.mp4".to_string(),
            duration: None,
            title: Some("My Video".to_string()),
        };
        assert_eq!(entry.display_name(), "My Video");

        let entry2 = PlaylistEntry {
            id: 2,
            path: "/home/other.mp4".to_string(),
            file_name: "other.mp4".to_string(),
            duration: None,
            title: None,
        };
        assert_eq!(entry2.display_name(), "other.mp4");
    }
}
