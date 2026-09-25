//! `Slate OS` Media Converter
//!
//! A batch media converter: a list of sources, the settings of one output
//! profile, and a queue that converts one file at a time on a worker thread.
//!
//! **What it converts** (`engine`): WAV to WAV at any sample rate, channel
//! count and sample format, and PNG or JPEG to BMP at their own size or fitted
//! inside one. Every other pairing is refused before it is queued, and says
//! which side -- a decoder or an encoder -- this build lacks. Until 2026-09-25
//! it converted nothing: no file could be added, and no job could start.
//!
//! **What its model describes is wider**, and is the shape the converter grows
//! into as codecs arrive: audio (WAV, MP3, FLAC, AAC, OGG, WMA, AIFF, ALAC,
//! Opus), video (MP4, MKV, AVI, `WebM`, MOV, WMV, FLV, in H.264, H.265, VP9 or
//! AV1) and pictures (JPEG, PNG, BMP, GIF, TIFF, WebP, HEIC, ICO, SVG), with
//! quality presets, preset profiles, naming rules and a history of what
//! finished. The profiles this build cannot carry out are listed all the same,
//! marked "Not available" with the reason, so that what is missing is visible
//! rather than absent.
//!
//! **Nothing is written over.** An output whose name is already on disk, or
//! already planned by another job in the queue, or is the source itself, gets
//! " (2)", " (3)" and so on; and the bytes go to a temporary file renamed into
//! place, so a cancelled or failed job leaves nothing behind.

// Lint policy is inherited from the workspace (`[lints] workspace = true`):
// `clippy::all` denied, `clippy::pedantic` at warn, with the curated allow
// list documented in the root Cargo.toml (keeps the discipline centralised).
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::similar_names)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::unreadable_literal)]

use appearance::Edge;
use appearance::Palette;
use appearance::Surface;
use guitk::Color;
use guitk::dialog::{FileDialog, FilePicker, Picked};
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::render::RenderTree;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use guitk::wheel;
use oswindow::app::{self, App, Response};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod engine;

// ============================================================================
// Catppuccin Mocha theme
// ============================================================================

// ============================================================================
// Layout constants
// ============================================================================

/// What this build can convert, above the settings that say how.
///
/// It said "This program cannot read or convert media files", which was true
/// -- and was drawn at the top of the window, under the toolbar that painted
/// over it.
const CAN_CONVERT_LINES: [&str; 3] = [
    "Converts WAV to WAV, and PNG or JPEG to BMP.",
    "Anything else needs a decoder or an encoder this",
    "build lacks, and is refused before it is queued.",
];

const SIDEBAR_WIDTH: f32 = 300.0;
const SETTINGS_PANEL_WIDTH: f32 = 280.0;
const TOOLBAR_HEIGHT: f32 = 40.0;
const STATUS_BAR_HEIGHT: f32 = 24.0;
const ITEM_HEIGHT: f32 = 32.0;
const CORNER_RADIUS: f32 = 4.0;
/// A queue row's height.
const QUEUE_ROW_H: f32 = 46.0;

// ============================================================================
// Unique ID generation
// ============================================================================

#[derive(Debug)]
struct IdGen {
    next: u64,
}

impl IdGen {
    const fn new(start: u64) -> Self {
        Self { next: start }
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next;
        self.next = self.next.saturating_add(1);
        id
    }
}

// ============================================================================
// Media categories
// ============================================================================

/// Top-level category of a media file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MediaCategory {
    Audio,
    Video,
    Image,
}

impl MediaCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Audio => "Audio",
            Self::Video => "Video",
            Self::Image => "Image",
        }
    }
}

// ============================================================================
// Audio formats
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AudioFormat {
    Wav,
    Mp3,
    Flac,
    Aac,
    Ogg,
    Wma,
    Aiff,
    Alac,
    Opus,
}

impl AudioFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Wav => "WAV",
            Self::Mp3 => "MP3",
            Self::Flac => "FLAC",
            Self::Aac => "AAC",
            Self::Ogg => "OGG Vorbis",
            Self::Wma => "WMA",
            Self::Aiff => "AIFF",
            Self::Alac => "ALAC",
            Self::Opus => "Opus",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Mp3 => "mp3",
            Self::Flac => "flac",
            Self::Aac => "aac",
            Self::Ogg => "ogg",
            Self::Wma => "wma",
            Self::Aiff => "aiff",
            Self::Alac => "m4a",
            Self::Opus => "opus",
        }
    }

    pub fn is_lossless(self) -> bool {
        matches!(self, Self::Wav | Self::Flac | Self::Aiff | Self::Alac)
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "wav" => Some(Self::Wav),
            "mp3" => Some(Self::Mp3),
            "flac" => Some(Self::Flac),
            "aac" | "m4a" => Some(Self::Aac),
            "ogg" => Some(Self::Ogg),
            "wma" => Some(Self::Wma),
            "aiff" | "aif" => Some(Self::Aiff),
            "alac" => Some(Self::Alac),
            "opus" => Some(Self::Opus),
            _ => None,
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::Wav,
            Self::Mp3,
            Self::Flac,
            Self::Aac,
            Self::Ogg,
            Self::Wma,
            Self::Aiff,
            Self::Alac,
            Self::Opus,
        ]
    }
}

// ============================================================================
// Video formats
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VideoFormat {
    Mp4,
    Mkv,
    Avi,
    WebM,
    Mov,
    Wmv,
    Flv,
}

impl VideoFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Mp4 => "MP4",
            Self::Mkv => "MKV",
            Self::Avi => "AVI",
            Self::WebM => "WebM",
            Self::Mov => "MOV",
            Self::Wmv => "WMV",
            Self::Flv => "FLV",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Mkv => "mkv",
            Self::Avi => "avi",
            Self::WebM => "webm",
            Self::Mov => "mov",
            Self::Wmv => "wmv",
            Self::Flv => "flv",
        }
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "mp4" => Some(Self::Mp4),
            "mkv" => Some(Self::Mkv),
            "avi" => Some(Self::Avi),
            "webm" => Some(Self::WebM),
            "mov" => Some(Self::Mov),
            "wmv" => Some(Self::Wmv),
            "flv" => Some(Self::Flv),
            _ => None,
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::Mp4,
            Self::Mkv,
            Self::Avi,
            Self::WebM,
            Self::Mov,
            Self::Wmv,
            Self::Flv,
        ]
    }
}

// ============================================================================
// Image formats
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    Jpeg,
    Png,
    Bmp,
    Gif,
    Tiff,
    WebP,
    Heic,
    Ico,
}

impl ImageFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Jpeg => "JPEG",
            Self::Png => "PNG",
            Self::Bmp => "BMP",
            Self::Gif => "GIF",
            Self::Tiff => "TIFF",
            Self::WebP => "WebP",
            Self::Heic => "HEIC",
            Self::Ico => "ICO",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Bmp => "bmp",
            Self::Gif => "gif",
            Self::Tiff => "tiff",
            Self::WebP => "webp",
            Self::Heic => "heic",
            Self::Ico => "ico",
        }
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "png" => Some(Self::Png),
            "bmp" => Some(Self::Bmp),
            "gif" => Some(Self::Gif),
            "tif" | "tiff" => Some(Self::Tiff),
            "webp" => Some(Self::WebP),
            "heic" | "heif" => Some(Self::Heic),
            "ico" => Some(Self::Ico),
            _ => None,
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::Jpeg,
            Self::Png,
            Self::Bmp,
            Self::Gif,
            Self::Tiff,
            Self::WebP,
            Self::Heic,
            Self::Ico,
        ]
    }

    pub fn supports_quality(self) -> bool {
        matches!(self, Self::Jpeg | Self::WebP | Self::Heic)
    }
}

// ============================================================================
// Video codecs
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoCodec {
    H264,
    H265,
    Vp9,
    Av1,
    Mpeg4,
    Copy,
}

impl VideoCodec {
    pub fn label(self) -> &'static str {
        match self {
            Self::H264 => "H.264 (AVC)",
            Self::H265 => "H.265 (HEVC)",
            Self::Vp9 => "VP9",
            Self::Av1 => "AV1",
            Self::Mpeg4 => "MPEG-4",
            Self::Copy => "Copy (no re-encode)",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::H264 => "H.264",
            Self::H265 => "H.265",
            Self::Vp9 => "VP9",
            Self::Av1 => "AV1",
            Self::Mpeg4 => "MPEG4",
            Self::Copy => "Copy",
        }
    }
}

// ============================================================================
// Audio codecs
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioCodec {
    AacLc,
    Mp3Lame,
    OpusEnc,
    FlacEnc,
    Vorbis,
    Pcm,
    Copy,
}

impl AudioCodec {
    pub fn label(self) -> &'static str {
        match self {
            Self::AacLc => "AAC-LC",
            Self::Mp3Lame => "MP3 (LAME)",
            Self::OpusEnc => "Opus",
            Self::FlacEnc => "FLAC",
            Self::Vorbis => "Vorbis",
            Self::Pcm => "PCM (uncompressed)",
            Self::Copy => "Copy (no re-encode)",
        }
    }
}

// ============================================================================
// Quality presets
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QualityPreset {
    Low,
    Medium,
    High,
    VeryHigh,
    Lossless,
}

impl QualityPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::VeryHigh => "Very High",
            Self::Lossless => "Lossless",
        }
    }

    /// Get approximate audio bitrate in kbps.
    pub fn audio_bitrate_kbps(self) -> u32 {
        match self {
            Self::Low => 96,
            Self::Medium => 192,
            Self::High => 320,
            Self::VeryHigh => 320,
            Self::Lossless => 0,
        }
    }

    /// Get approximate video bitrate in kbps.
    pub fn video_bitrate_kbps(self, resolution: &VideoResolution) -> u32 {
        let base = match self {
            Self::Low => 1_000,
            Self::Medium => 4_000,
            Self::High => 8_000,
            Self::VeryHigh => 20_000,
            Self::Lossless => 50_000,
        };
        // Scale by resolution relative to 1080p
        let pixels = u64::from(resolution.width).saturating_mul(u64::from(resolution.height));
        let ref_pixels: u64 = 1920 * 1080;
        if ref_pixels == 0 {
            return base;
        }
        let scale = pixels as f64 / ref_pixels as f64;
        (f64::from(base) * scale.max(0.25)) as u32
    }

    /// Image quality percentage (for JPEG, WebP).
    pub fn image_quality(self) -> u8 {
        match self {
            Self::Low => 60,
            Self::Medium => 80,
            Self::High => 90,
            Self::VeryHigh => 95,
            Self::Lossless => 100,
        }
    }
}

// ============================================================================
// Video resolution
// ============================================================================

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoResolution {
    pub width: u32,
    pub height: u32,
}

impl VideoResolution {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    pub fn label(&self) -> String {
        let name = match (self.width, self.height) {
            (3840, 2160) => "4K UHD",
            (2560, 1440) => "1440p QHD",
            (1920, 1080) => "1080p FHD",
            (1280, 720) => "720p HD",
            (854, 480) => "480p SD",
            (640, 360) => "360p",
            _ => "",
        };
        if name.is_empty() {
            format!("{}x{}", self.width, self.height)
        } else {
            format!("{} ({}x{})", name, self.width, self.height)
        }
    }

    pub fn common_resolutions() -> Vec<Self> {
        vec![
            Self::new(3840, 2160),
            Self::new(2560, 1440),
            Self::new(1920, 1080),
            Self::new(1280, 720),
            Self::new(854, 480),
            Self::new(640, 360),
        ]
    }

    pub fn pixel_count(&self) -> u64 {
        u64::from(self.width).saturating_mul(u64::from(self.height))
    }

    pub fn aspect_ratio(&self) -> String {
        if self.height == 0 {
            return "N/A".to_owned();
        }
        let gcd = gcd_u32(self.width, self.height);
        if gcd == 0 {
            return "N/A".to_owned();
        }
        let w = self.width.checked_div(gcd).unwrap_or(0);
        let h = self.height.checked_div(gcd).unwrap_or(0);
        format!("{w}:{h}")
    }
}

fn gcd_u32(a: u32, b: u32) -> u32 {
    let mut x = a;
    let mut y = b;
    while y != 0 {
        let temp = y;
        y = x.checked_rem(y).unwrap_or(0);
        x = temp;
    }
    x
}

// ============================================================================
// Audio settings
// ============================================================================

#[derive(Clone, Debug)]
pub struct AudioSettings {
    pub codec: AudioCodec,
    pub bitrate_kbps: u32,
    pub sample_rate: u32,
    pub channels: u8,
    pub bit_depth: u8,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            codec: AudioCodec::AacLc,
            bitrate_kbps: 192,
            sample_rate: 44100,
            channels: 2,
            bit_depth: 16,
        }
    }
}

impl AudioSettings {
    pub fn from_preset(preset: QualityPreset) -> Self {
        Self {
            codec: match preset {
                QualityPreset::Low => AudioCodec::Mp3Lame,
                QualityPreset::Medium => AudioCodec::AacLc,
                QualityPreset::High => AudioCodec::OpusEnc,
                QualityPreset::VeryHigh => AudioCodec::OpusEnc,
                QualityPreset::Lossless => AudioCodec::FlacEnc,
            },
            bitrate_kbps: preset.audio_bitrate_kbps(),
            sample_rate: if preset == QualityPreset::Low {
                22050
            } else {
                44100
            },
            channels: 2,
            bit_depth: if preset == QualityPreset::Lossless {
                24
            } else {
                16
            },
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "{}, {} kbps, {} Hz, {}ch, {}bit",
            self.codec.label(),
            self.bitrate_kbps,
            self.sample_rate,
            self.channels,
            self.bit_depth,
        )
    }
}

// ============================================================================
// Video settings
// ============================================================================

#[derive(Clone, Debug)]
pub struct VideoSettings {
    pub video_codec: VideoCodec,
    pub audio_codec: AudioCodec,
    pub resolution: Option<VideoResolution>,
    pub framerate: Option<f32>,
    pub video_bitrate_kbps: u32,
    pub audio_bitrate_kbps: u32,
    pub two_pass: bool,
    pub crop: Option<CropSettings>,
}

impl Default for VideoSettings {
    fn default() -> Self {
        Self {
            video_codec: VideoCodec::H264,
            audio_codec: AudioCodec::AacLc,
            resolution: None,
            framerate: None,
            video_bitrate_kbps: 4000,
            audio_bitrate_kbps: 192,
            two_pass: false,
            crop: None,
        }
    }
}

impl VideoSettings {
    pub fn from_preset(preset: QualityPreset) -> Self {
        let res = VideoResolution::new(1920, 1080);
        Self {
            video_codec: match preset {
                QualityPreset::Low => VideoCodec::H264,
                QualityPreset::Medium => VideoCodec::H264,
                QualityPreset::High => VideoCodec::H265,
                QualityPreset::VeryHigh => VideoCodec::H265,
                QualityPreset::Lossless => VideoCodec::H265,
            },
            audio_codec: AudioCodec::AacLc,
            resolution: Some(match preset {
                QualityPreset::Low => VideoResolution::new(854, 480),
                QualityPreset::Medium => VideoResolution::new(1280, 720),
                _ => VideoResolution::new(1920, 1080),
            }),
            framerate: Some(if preset == QualityPreset::Low {
                24.0
            } else {
                30.0
            }),
            video_bitrate_kbps: preset.video_bitrate_kbps(&res),
            audio_bitrate_kbps: preset.audio_bitrate_kbps(),
            two_pass: preset == QualityPreset::VeryHigh || preset == QualityPreset::Lossless,
            crop: None,
        }
    }

    pub fn summary(&self) -> String {
        let res_str = self
            .resolution
            .as_ref()
            .map_or("Original".to_owned(), VideoResolution::label);
        let fps_str = self
            .framerate
            .map_or("Original".to_owned(), |f| format!("{f:.0} fps"));
        format!(
            "{} | {} | {} | {} kbps",
            self.video_codec.short_label(),
            res_str,
            fps_str,
            self.video_bitrate_kbps,
        )
    }
}

/// Crop rectangle for video.
#[derive(Clone, Debug)]
pub struct CropSettings {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

impl CropSettings {
    pub fn new(left: u32, top: u32, right: u32, bottom: u32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }
}

// ============================================================================
// Image settings
// ============================================================================

#[derive(Clone, Debug)]
pub struct ImageSettings {
    pub quality: u8,
    pub max_width: Option<u32>,
    pub max_height: Option<u32>,
    pub strip_metadata: bool,
    pub preserve_aspect: bool,
}

impl Default for ImageSettings {
    fn default() -> Self {
        Self {
            quality: 90,
            max_width: None,
            max_height: None,
            strip_metadata: false,
            preserve_aspect: true,
        }
    }
}

impl ImageSettings {
    pub fn from_preset(preset: QualityPreset) -> Self {
        Self {
            quality: preset.image_quality(),
            max_width: match preset {
                QualityPreset::Low => Some(1280),
                QualityPreset::Medium => Some(1920),
                _ => None,
            },
            max_height: None,
            strip_metadata: preset == QualityPreset::Low,
            preserve_aspect: true,
        }
    }

    pub fn summary(&self) -> String {
        let size_str = match (self.max_width, self.max_height) {
            (Some(w), Some(h)) => format!("max {w}x{h}"),
            (Some(w), None) => format!("max width {w}"),
            (None, Some(h)) => format!("max height {h}"),
            (None, None) => "Original size".to_owned(),
        };
        format!("Quality {}%, {}", self.quality, size_str)
    }
}

// ============================================================================
// Output format selection
// ============================================================================

/// Target output format, which determines category-specific settings.
#[derive(Clone, Debug, PartialEq)]
pub enum OutputFormat {
    Audio(AudioFormat),
    Video(VideoFormat),
    Image(ImageFormat),
}

impl OutputFormat {
    pub fn label(&self) -> String {
        match self {
            Self::Audio(f) => f.label().to_owned(),
            Self::Video(f) => f.label().to_owned(),
            Self::Image(f) => f.label().to_owned(),
        }
    }

    pub fn extension(&self) -> &str {
        match self {
            Self::Audio(f) => f.extension(),
            Self::Video(f) => f.extension(),
            Self::Image(f) => f.extension(),
        }
    }

    pub fn category(&self) -> MediaCategory {
        match self {
            Self::Audio(_) => MediaCategory::Audio,
            Self::Video(_) => MediaCategory::Video,
            Self::Image(_) => MediaCategory::Image,
        }
    }
}

// ============================================================================
// Conversion profile/preset
// ============================================================================

/// A named conversion preset.
#[derive(Clone, Debug)]
pub struct ConversionProfile {
    pub name: String,
    pub description: String,
    pub output_format: OutputFormat,
    pub quality_preset: QualityPreset,
    pub audio_settings: AudioSettings,
    pub video_settings: VideoSettings,
    pub image_settings: ImageSettings,
}

impl ConversionProfile {
    pub fn web_optimized_video() -> Self {
        Self {
            name: "Web Optimized (Video)".to_owned(),
            description: "H.264 720p for web streaming".to_owned(),
            output_format: OutputFormat::Video(VideoFormat::Mp4),
            quality_preset: QualityPreset::Medium,
            audio_settings: AudioSettings::from_preset(QualityPreset::Medium),
            video_settings: VideoSettings::from_preset(QualityPreset::Medium),
            image_settings: ImageSettings::default(),
        }
    }

    pub fn archive_video() -> Self {
        Self {
            name: "Archive (Video)".to_owned(),
            description: "H.265 high quality for archival".to_owned(),
            output_format: OutputFormat::Video(VideoFormat::Mkv),
            quality_preset: QualityPreset::VeryHigh,
            audio_settings: AudioSettings::from_preset(QualityPreset::VeryHigh),
            video_settings: VideoSettings::from_preset(QualityPreset::VeryHigh),
            image_settings: ImageSettings::default(),
        }
    }

    pub fn mobile_video() -> Self {
        Self {
            name: "Mobile (Video)".to_owned(),
            description: "H.264 480p for mobile devices".to_owned(),
            output_format: OutputFormat::Video(VideoFormat::Mp4),
            quality_preset: QualityPreset::Low,
            audio_settings: AudioSettings::from_preset(QualityPreset::Low),
            video_settings: VideoSettings::from_preset(QualityPreset::Low),
            image_settings: ImageSettings::default(),
        }
    }

    pub fn high_quality_audio() -> Self {
        Self {
            name: "High Quality Audio".to_owned(),
            description: "FLAC lossless audio".to_owned(),
            output_format: OutputFormat::Audio(AudioFormat::Flac),
            quality_preset: QualityPreset::Lossless,
            audio_settings: AudioSettings::from_preset(QualityPreset::Lossless),
            video_settings: VideoSettings::default(),
            image_settings: ImageSettings::default(),
        }
    }

    pub fn web_audio() -> Self {
        Self {
            name: "Web Audio".to_owned(),
            description: "Opus for web streaming".to_owned(),
            output_format: OutputFormat::Audio(AudioFormat::Opus),
            quality_preset: QualityPreset::Medium,
            audio_settings: AudioSettings::from_preset(QualityPreset::Medium),
            video_settings: VideoSettings::default(),
            image_settings: ImageSettings::default(),
        }
    }

    pub fn web_image() -> Self {
        Self {
            name: "Web Image".to_owned(),
            description: "WebP optimized for web".to_owned(),
            output_format: OutputFormat::Image(ImageFormat::WebP),
            quality_preset: QualityPreset::Medium,
            audio_settings: AudioSettings::default(),
            video_settings: VideoSettings::default(),
            image_settings: ImageSettings::from_preset(QualityPreset::Medium),
        }
    }

    /// A WAV profile.
    fn wav(name: &str, description: &str, sample_rate: u32, channels: u8, bit_depth: u8) -> Self {
        Self {
            name: name.to_owned(),
            description: description.to_owned(),
            output_format: OutputFormat::Audio(AudioFormat::Wav),
            quality_preset: QualityPreset::High,
            audio_settings: AudioSettings {
                codec: AudioCodec::AacLc,
                bitrate_kbps: 0,
                sample_rate,
                channels,
                bit_depth,
            },
            video_settings: VideoSettings::default(),
            image_settings: ImageSettings::default(),
        }
    }

    /// A BMP profile.
    fn bmp(name: &str, description: &str, max_width: Option<u32>, max_height: Option<u32>) -> Self {
        Self {
            name: name.to_owned(),
            description: description.to_owned(),
            output_format: OutputFormat::Image(ImageFormat::Bmp),
            quality_preset: QualityPreset::High,
            audio_settings: AudioSettings::default(),
            video_settings: VideoSettings::default(),
            image_settings: ImageSettings {
                max_width,
                max_height,
                ..ImageSettings::default()
            },
        }
    }

    /// Whether this build can make what the profile makes, and if not, why.
    ///
    /// # Errors
    ///
    /// The encoder or decoder this build lacks.
    pub fn availability(&self) -> Result<(), String> {
        // Against a source of the one kind that could feed it, so the answer
        // is about the output side alone.
        let probe = match &self.output_format {
            OutputFormat::Audio(_) => "probe.wav",
            OutputFormat::Image(_) => "probe.png",
            OutputFormat::Video(_) => "probe.mkv",
        };
        engine::recipe(
            OsStr::new(probe),
            &self.output_format,
            &self.audio_settings,
            &self.image_settings,
        )
        .map(|_| ())
    }

    /// All built-in profiles: those this build can carry out first, then the
    /// rest, which say what they are missing.
    pub fn builtin_profiles() -> Vec<Self> {
        vec![
            Self::wav("WAV, CD quality", "44.1 kHz, stereo, 16-bit", 44_100, 2, 16),
            Self::wav("WAV, studio", "48 kHz, stereo, 24-bit", 48_000, 2, 24),
            Self::wav(
                "WAV, speech",
                "16 kHz, mono, 16-bit: small, for voices",
                16_000,
                1,
                16,
            ),
            Self::bmp("BMP picture", "The picture at its own size", None, None),
            Self::bmp(
                "BMP, screen size",
                "Fitted inside 1920x1080",
                Some(1920),
                Some(1080),
            ),
            Self::web_optimized_video(),
            Self::archive_video(),
            Self::mobile_video(),
            Self::high_quality_audio(),
            Self::web_audio(),
            Self::web_image(),
        ]
    }
}

// ============================================================================
// Output naming
// ============================================================================

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputNaming {
    /// Keep original name, change extension.
    KeepOriginal,
    /// Add suffix before extension: `file_converted.ext`.
    Suffix(String),
    /// Add prefix: `converted_file.ext`.
    Prefix(String),
    /// Custom pattern with `{name}`, `{ext}`, `{date}`, `{index}` placeholders.
    Pattern(String),
}

impl OutputNaming {
    pub fn apply(&self, original: &str, new_ext: &str, index: usize) -> String {
        let stem = original
            .rfind('.')
            .and_then(|pos| original.get(..pos))
            .unwrap_or(original);

        match self {
            Self::KeepOriginal => format!("{stem}.{new_ext}"),
            Self::Suffix(suf) => format!("{stem}{suf}.{new_ext}"),
            Self::Prefix(pre) => format!("{pre}{stem}.{new_ext}"),
            Self::Pattern(pat) => pat
                .replace("{name}", stem)
                .replace("{ext}", new_ext)
                .replace("{index}", &index.to_string())
                .replace("{date}", &today_yyyymmdd()),
        }
    }

    /// As [`Self::apply`], for a file name that may not be text: the stem is
    /// kept byte for byte.
    #[must_use]
    pub fn apply_os(&self, original: &OsStr, new_ext: &str, index: usize) -> OsString {
        let stem = Path::new(original).file_stem().unwrap_or(original);
        let mut out = OsString::new();
        match self {
            Self::KeepOriginal => out.push(stem),
            Self::Suffix(suf) => {
                out.push(stem);
                out.push(suf);
            }
            Self::Prefix(pre) => {
                out.push(pre);
                out.push(stem);
            }
            Self::Pattern(pat) => {
                let filled = pat
                    .replace("{ext}", new_ext)
                    .replace("{index}", &index.to_string())
                    .replace("{date}", &today_yyyymmdd());
                let mut parts = filled.split("{name}");
                if let Some(first) = parts.next() {
                    out.push(first);
                }
                for part in parts {
                    out.push(stem);
                    out.push(part);
                }
                return out;
            }
        }
        out.push(".");
        out.push(new_ext);
        out
    }

    pub fn label(&self) -> &str {
        match self {
            Self::KeepOriginal => "Keep Original",
            Self::Suffix(_) => "Add Suffix",
            Self::Prefix(_) => "Add Prefix",
            Self::Pattern(_) => "Custom Pattern",
        }
    }
}

// ============================================================================
// Source file
// ============================================================================

/// A source media file to be converted.
#[derive(Clone, Debug)]
pub struct SourceFile {
    pub id: u64,
    /// Where it is -- a path, not text: a file name may hold any byte.
    pub path: PathBuf,
    pub file_name: String,
    pub file_size: u64,
    pub category: MediaCategory,
    pub duration_secs: Option<f64>,
    pub source_format: String,
}

impl SourceFile {
    pub fn new(id: u64, path: &str, name: &str, size: u64, category: MediaCategory) -> Self {
        Self {
            id,
            path: PathBuf::from(path),
            file_name: name.to_owned(),
            file_size: size,
            category,
            duration_secs: None,
            source_format: String::new(),
        }
    }

    pub fn with_duration(mut self, secs: f64) -> Self {
        self.duration_secs = Some(secs);
        self
    }

    pub fn with_format(mut self, fmt: &str) -> Self {
        self.source_format = fmt.to_owned();
        self
    }

    /// The file at `path`, as it really is: its size from the file system,
    /// and for a WAV its length and format, for a picture its size in pixels,
    /// read from the file's own header.
    ///
    /// # Errors
    ///
    /// When it cannot be read, or its name says it is not media.
    pub fn from_file(id: u64, path: &Path) -> Result<Self, String> {
        let name = path.file_name().unwrap_or(path.as_os_str());
        let category = MediaConvertApp::detect_category_os(name)
            .ok_or_else(|| format!("{} is not a media file by its name", path.display()))?;
        let meta = std::fs::metadata(path)
            .map_err(|err| format!("could not read {}: {err}", path.display()))?;
        let mut source = Self {
            id,
            path: path.to_path_buf(),
            file_name: Path::new(name).display().to_string(),
            file_size: meta.len(),
            category,
            duration_secs: None,
            source_format: String::new(),
        };
        // The first part of the file is enough for a header: a WAV's chunks
        // before its samples, a picture's dimensions.
        if let Ok(head) = safeio::read_capped(path, 1 << 20) {
            let ext = Path::new(name)
                .extension()
                .and_then(OsStr::to_str)
                .map(str::to_ascii_lowercase)
                .unwrap_or_default();
            if ext == "wav" {
                // The file's own length, not the part read: a data chunk
                // running past the part looks like a streaming file's.
                match wavpcm::parse_header_prefix(&head.bytes, meta.len()) {
                    Ok(info) => {
                        source.duration_secs = Some(info.seconds());
                        source.source_format = format!(
                            "WAV {} Hz {}ch {}",
                            info.sample_rate,
                            info.channels,
                            info.format.label()
                        );
                    }
                    Err(err) => source.source_format = format!("WAV ({err})"),
                }
            } else if let Ok((w, h)) = imagecodec::dimensions(&head.bytes) {
                source.source_format = format!("{w}x{h}");
            }
        }
        Ok(source)
    }

    /// Human-readable file size.
    pub fn human_size(&self) -> String {
        human_file_size(self.file_size)
    }

    /// Duration formatted as HH:MM:SS.
    pub fn duration_str(&self) -> String {
        match self.duration_secs {
            Some(secs) => format_duration(secs),
            None => "N/A".to_owned(),
        }
    }
}

fn human_file_size(bytes: u64) -> String {
    guitk::bytes::iec(bytes)
}

fn format_duration(secs: f64) -> String {
    let total = secs as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

// ============================================================================
// Conversion job
// ============================================================================

/// Status of a conversion job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Running => "Running",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }

    pub fn color(self, pal: &Palette) -> Color {
        match self {
            Self::Queued => pal.overlay0,
            Self::Running => pal.blue,
            Self::Completed => pal.green,
            Self::Failed => pal.red,
            Self::Cancelled => pal.yellow,
        }
    }
}

/// A single conversion job in the queue.
#[derive(Clone, Debug)]
pub struct ConversionJob {
    pub id: u64,
    pub source: SourceFile,
    pub output_format: OutputFormat,
    pub output_name: String,
    pub output_path: PathBuf,
    /// What the job does, fixed when it was queued.
    pub recipe: engine::Recipe,
    /// Asked to stop, and not yet stopped.
    pub stopping: bool,
    pub status: JobStatus,
    pub progress: f32,
    pub estimated_size: Option<u64>,
    pub actual_size: Option<u64>,
    pub error_message: Option<String>,
    pub started_at: Option<u64>,
    pub completed_at: Option<u64>,
}

impl ConversionJob {
    pub fn new(
        id: u64,
        source: SourceFile,
        output_format: OutputFormat,
        output_name: String,
        output_path: PathBuf,
        recipe: engine::Recipe,
    ) -> Self {
        Self {
            id,
            source,
            output_format,
            output_name,
            output_path,
            recipe,
            stopping: false,
            status: JobStatus::Queued,
            progress: 0.0,
            estimated_size: None,
            actual_size: None,
            error_message: None,
            started_at: None,
            completed_at: None,
        }
    }

    pub fn start(&mut self, timestamp: u64) {
        self.status = JobStatus::Running;
        self.started_at = Some(timestamp);
    }

    /// Move a running job's progress on, and say whether it has finished.
    ///
    /// `progress` was set to 0 when the job was made and 100 when it completed,
    /// and to nothing in between -- while the queue panel draws a bar from it
    /// (`fill_w = bar_w * progress / 100`). So the bar this program is built to
    /// show was empty or full and never anything else. The module doc's "batch
    /// conversion queue with progress tracking" was the queue without the
    /// tracking.
    pub fn advance(&mut self, percent: f32) -> bool {
        if self.status != JobStatus::Running {
            return false;
        }
        self.progress = (self.progress + percent).clamp(0.0, 100.0);
        self.progress >= 100.0
    }

    pub fn complete(&mut self, timestamp: u64, actual_size: u64) {
        self.status = JobStatus::Completed;
        self.progress = 100.0;
        self.completed_at = Some(timestamp);
        self.actual_size = Some(actual_size);
    }

    pub fn fail(&mut self, timestamp: u64, error: &str) {
        self.status = JobStatus::Failed;
        self.completed_at = Some(timestamp);
        self.error_message = Some(error.to_owned());
    }

    pub fn cancel(&mut self) {
        self.status = JobStatus::Cancelled;
        self.stopping = false;
    }

    /// Format conversion direction.
    pub fn conversion_label(&self) -> String {
        format!(
            "{} -> {}",
            if self.source.source_format.is_empty() {
                self.source.category.label()
            } else {
                &self.source.source_format
            },
            self.output_format.label(),
        )
    }
}

// ============================================================================
// Conversion history entry
// ============================================================================

#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub source_path: PathBuf,
    pub output_path: PathBuf,
    pub conversion_type: String,
    pub source_size: u64,
    pub output_size: u64,
    pub timestamp: u64,
    pub duration_secs: f64,
    pub success: bool,
}

impl HistoryEntry {
    /// Compression ratio (output/source).
    pub fn compression_ratio(&self) -> f64 {
        if self.source_size == 0 {
            return 0.0;
        }
        self.output_size as f64 / self.source_size as f64
    }

    /// Space saved in bytes.
    pub fn space_saved(&self) -> i64 {
        (self.source_size as i64).saturating_sub(self.output_size as i64)
    }
}

// ============================================================================
// Active panel
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivePanel {
    SourceList,
    Settings,
    Queue,
}

// ============================================================================
// Main application
// ============================================================================

/// How often the window looks at a running job: its progress bar's pace.
const JOB_STEP: Duration = Duration::from_millis(120);

/// Today as `YYYYMMDD`, for an output name's `{date}`. It was always
/// `20260518`.
fn today_yyyymmdd() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = i32::try_from(secs / 86_400).unwrap_or(0);
    let (y, m, d) = guitk::date::Date::from_days_since_epoch(days).ymd();
    format!("{y:04}{m:02}{d:02}")
}

/// The keys this window answers, as a reader sees them.
///
/// Every row is checked by `every_advertised_key_does_something`.
const SHORTCUTS: &[(&str, &str)] = &[
    ("F1 / ?", "This list"),
    ("Ctrl+O", "Add files"),
    ("Ctrl+Shift+O", "Add every media file in a folder"),
    ("Tab", "Next panel: sources, settings, queue"),
    ("Up / Down", "Move through sources, settings or queue"),
    ("Left / Right", "Change the chosen setting"),
    ("O", "Choose the folder outputs go to"),
    ("B", "Write each output beside its source"),
    ("Enter", "Queue the selected source"),
    ("Ctrl+Enter", "Queue every source the profile can convert"),
    ("Ctrl+C", "Cancel everything waiting"),
    ("Ctrl+L", "Clear the jobs that finished"),
    ("Delete", "Drop a source, or cancel or clear a job"),
    ("1-4", "Quality: low, medium, high, lossless"),
];

/// What a file picker is up for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerFor {
    Files,
    Folder,
    OutputFolder,
}

/// A row of the settings panel, which Up and Down walk and Left and Right
/// change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingRow {
    Profile,
    Quality,
    SampleRate,
    Channels,
    SampleFormat,
    MaxWidth,
    MaxHeight,
    Naming,
}

/// Sample rates a WAV can be converted to.
const SAMPLE_RATES: [u32; 9] = [
    8_000, 11_025, 16_000, 22_050, 32_000, 44_100, 48_000, 88_200, 96_000,
];

/// Bit depths a WAV can be written at: 32 is float.
const BIT_DEPTHS: [u8; 4] = [8, 16, 24, 32];

/// The largest widths and heights a picture can be fitted inside; `None` is
/// its own size.
const MAX_WIDTHS: [Option<u32>; 5] = [None, Some(640), Some(1280), Some(1920), Some(3840)];
const MAX_HEIGHTS: [Option<u32>; 5] = [None, Some(480), Some(720), Some(1080), Some(2160)];

/// The naming rules the Naming row steps through.
fn naming_rules() -> [OutputNaming; 3] {
    [
        OutputNaming::KeepOriginal,
        OutputNaming::Suffix(String::from("-converted")),
        OutputNaming::Prefix(String::from("converted-")),
    ]
}

/// Everything in the window a pointer can press, as the renderer records it.
///
/// Nothing answered the pointer: the profile and quality boxes, the Convert
/// All button and every row were drawn as controls and were pictures of them
/// (`known-issues.md` -> `TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    AddFiles,
    AddFolder,
    ConvertAll,
    CancelWaiting,
    ClearFinished,
    Help,
    Panel(ActivePanel),
    SourceList,
    SourceRow(u64),
    RemoveSource(u64),
    /// A settings row: a press chooses it.
    Setting(SettingRow),
    /// The arrows either side of a settings row's value.
    StepBack(SettingRow),
    StepForward(SettingRow),
    ChooseOutput,
    OutputBeside,
    QueueList,
    /// A job's row: a press chooses it.
    QueueRow(u64),
    CancelJob(u64),
    HelpCard,
}

/// The rows the settings panel shows for the profile chosen.
fn setting_rows(output: Option<&OutputFormat>) -> Vec<SettingRow> {
    let mut rows = vec![SettingRow::Profile, SettingRow::Quality];
    match output {
        Some(OutputFormat::Audio(AudioFormat::Wav)) => {
            rows.extend([
                SettingRow::SampleRate,
                SettingRow::Channels,
                SettingRow::SampleFormat,
            ]);
        }
        Some(OutputFormat::Image(ImageFormat::Bmp)) => {
            rows.extend([SettingRow::MaxWidth, SettingRow::MaxHeight]);
        }
        _ => {}
    }
    rows.push(SettingRow::Naming);
    rows
}

/// The scroll that shows row `at` in a pane of `rows` rows, moving `scroll`
/// as little as it can.
fn scrolled_to(scroll: usize, at: usize, rows: usize) -> usize {
    if at < scroll {
        at
    } else if at >= scroll.saturating_add(rows) {
        at.saturating_add(1).saturating_sub(rows)
    } else {
        scroll
    }
}

pub struct MediaConvertApp {
    pub sources: Vec<SourceFile>,
    pub jobs: Vec<ConversionJob>,
    pub history: Vec<HistoryEntry>,
    pub profiles: Vec<ConversionProfile>,
    pub selected_source: Option<u64>,
    /// The job the keys are on, by id: a finished job leaving the list takes
    /// its selection with it rather than handing it to a neighbour.
    pub selected_job: Option<u64>,
    pub selected_profile_idx: usize,
    pub output_naming: OutputNaming,
    /// Where outputs go: `None` is beside each source. It was
    /// `/home/converted`, a folder nobody had made.
    pub output_dir: Option<PathBuf>,
    pub quality_preset: QualityPreset,
    pub active_panel: ActivePanel,
    /// Whether the shortcut card is up.
    pub show_help: bool,
    pub audio_settings: AudioSettings,
    pub video_settings: VideoSettings,
    pub image_settings: ImageSettings,
    /// What the last action said: why a file was refused, where a job went.
    pub status_line: String,
    pub window_width: f32,
    pub window_height: f32,
    /// The settings row the keys are on.
    pub setting_row: SettingRow,
    /// The job running, on its thread.
    worker: Option<engine::Worker>,
    /// The file and folder picker, and what it is up for.
    pub picker: FilePicker,
    pub picker_for: PickerFor,
    /// How far the source list and the queue are scrolled, in rows.
    pub source_scroll: usize,
    pub queue_scroll: usize,
    hover: Option<Target>,
    last_hits: Vec<(Target, Rect)>,
    wheel: wheel::Accumulator,
    id_gen: IdGen,
    timestamp: u64,
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
}

impl Default for MediaConvertApp {
    fn default() -> Self {
        Self::new()
    }
}

impl MediaConvertApp {
    pub fn new() -> Self {
        let profiles = ConversionProfile::builtin_profiles();
        let first = profiles.first().cloned();
        let mut app = Self {
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            sources: Vec::new(),
            jobs: Vec::new(),
            history: Vec::new(),
            profiles,
            selected_source: None,
            selected_job: None,
            selected_profile_idx: 0,
            output_naming: OutputNaming::KeepOriginal,
            output_dir: None,
            quality_preset: QualityPreset::Medium,
            active_panel: ActivePanel::SourceList,
            show_help: false,
            audio_settings: AudioSettings::default(),
            video_settings: VideoSettings::default(),
            image_settings: ImageSettings::default(),
            status_line: String::new(),
            window_width: 1280.0,
            window_height: 800.0,
            setting_row: SettingRow::Profile,
            worker: None,
            picker: FilePicker::default(),
            picker_for: PickerFor::Files,
            source_scroll: 0,
            queue_scroll: 0,
            hover: None,
            last_hits: Vec::new(),
            wheel: wheel::Accumulator::default(),
            id_gen: IdGen::new(1),
            timestamp: 1000,
        };
        if first.is_some() {
            app.select_profile(0);
        }
        app
    }

    fn tick(&mut self) -> u64 {
        self.timestamp = self.timestamp.saturating_add(1);
        self.timestamp
    }

    // -----------------------------------------------------------------------
    // Source management
    // -----------------------------------------------------------------------

    /// Add a source file.
    pub fn add_source(
        &mut self,
        path: &str,
        name: &str,
        size: u64,
        category: MediaCategory,
    ) -> u64 {
        let id = self.id_gen.next_id();
        self.sources
            .push(SourceFile::new(id, path, name, size, category));
        id
    }

    /// Add a source with duration (audio/video).
    pub fn add_source_with_duration(
        &mut self,
        path: &str,
        name: &str,
        size: u64,
        category: MediaCategory,
        duration: f64,
        fmt: &str,
    ) -> u64 {
        let id = self.id_gen.next_id();
        let src = SourceFile::new(id, path, name, size, category)
            .with_duration(duration)
            .with_format(fmt);
        self.sources.push(src);
        id
    }

    /// Add the file at `path`, read for what it really is: its size, and for
    /// a WAV its length and format, for a picture its size in pixels.
    ///
    /// Nothing could add a file at all: the list said "Drop files here" and
    /// nothing took a drop, a pick or a path.
    ///
    /// # Errors
    ///
    /// Why the file was not added: it cannot be read, or it is already in the
    /// list, or it is not media by its name.
    pub fn add_file(&mut self, path: &Path) -> Result<u64, String> {
        if self.sources.iter().any(|s| s.path == path) {
            return Err(format!("{} is already in the list", path.display()));
        }
        let id = self.id_gen.next_id();
        let source = SourceFile::from_file(id, path)?;
        self.sources.push(source);
        self.selected_source = Some(id);
        Ok(id)
    }

    /// Add every media file directly in `folder`, by name order; answers how
    /// many were added.
    pub fn add_folder(&mut self, folder: &Path) -> Result<usize, String> {
        let entries = std::fs::read_dir(folder)
            .map_err(|err| format!("could not read {}: {err}", folder.display()))?;
        let mut paths: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            // `add_file` refuses what is not media by its name.
            .filter(|p| p.is_file())
            .collect();
        paths.sort();
        let mut added = 0_usize;
        for path in paths {
            if self.add_file(&path).is_ok() {
                added = added.saturating_add(1);
            }
        }
        Ok(added)
    }

    /// Remove a source.
    pub fn remove_source(&mut self, id: u64) -> bool {
        let len = self.sources.len();
        self.sources.retain(|s| s.id != id);
        if self.selected_source == Some(id) {
            self.selected_source = None;
        }
        self.sources.len() < len
    }

    /// Clear all sources.
    pub fn clear_sources(&mut self) {
        self.sources.clear();
        self.selected_source = None;
    }

    /// Find a source by ID.
    pub fn find_source(&self, id: u64) -> Option<&SourceFile> {
        self.sources.iter().find(|s| s.id == id)
    }

    /// Detect media category from file extension.
    pub fn detect_category(filename: &str) -> Option<MediaCategory> {
        let ext = filename.rsplit('.').next()?;
        let lower = ext.to_lowercase();
        if AudioFormat::from_extension(&lower).is_some() {
            Some(MediaCategory::Audio)
        } else if VideoFormat::from_extension(&lower).is_some() {
            Some(MediaCategory::Video)
        } else if ImageFormat::from_extension(&lower).is_some() {
            Some(MediaCategory::Image)
        } else {
            None
        }
    }

    /// As [`Self::detect_category`], for a name that may not be text.
    pub fn detect_category_os(name: &OsStr) -> Option<MediaCategory> {
        let ext = Path::new(name).extension()?.to_str()?;
        Self::detect_category(&format!("x.{ext}"))
    }

    // -----------------------------------------------------------------------
    // Profile management
    // -----------------------------------------------------------------------

    /// Select a profile by index.
    pub fn select_profile(&mut self, idx: usize) {
        if idx < self.profiles.len() {
            self.selected_profile_idx = idx;
            let profile = self.profiles.get(idx).cloned();
            if let Some(p) = profile {
                self.quality_preset = p.quality_preset;
                self.audio_settings = p.audio_settings;
                self.video_settings = p.video_settings;
                self.image_settings = p.image_settings;
            }
            // The rows below the profile are the profile's: a row the new
            // one does not have is not where the keys can stay.
            if !setting_rows(self.output_format().as_ref()).contains(&self.setting_row) {
                self.setting_row = SettingRow::Profile;
            }
        }
    }

    /// Set quality preset and update settings.
    pub fn set_quality_preset(&mut self, preset: QualityPreset) {
        self.quality_preset = preset;
        self.audio_settings = AudioSettings::from_preset(preset);
        self.video_settings = VideoSettings::from_preset(preset);
        self.image_settings = ImageSettings::from_preset(preset);
    }

    /// The output format of the chosen profile.
    fn output_format(&self) -> Option<OutputFormat> {
        self.profiles
            .get(self.selected_profile_idx)
            .map(|p| p.output_format.clone())
    }

    /// Step the value of settings row `row`: forward, or back.
    fn step_setting(&mut self, row: SettingRow, forward: bool) {
        fn step<T: PartialEq + Copy>(list: &[T], now: T, forward: bool) -> T {
            let at = list.iter().position(|v| *v == now).unwrap_or(0);
            let len = list.len().max(1);
            let next = if forward {
                at.saturating_add(1).checked_rem(len).unwrap_or(0)
            } else {
                at.checked_sub(1).unwrap_or(len.saturating_sub(1))
            };
            list.get(next).copied().unwrap_or(now)
        }
        match row {
            SettingRow::Profile => {
                let count = self.profiles.len().max(1);
                let next = if forward {
                    self.selected_profile_idx
                        .saturating_add(1)
                        .checked_rem(count)
                        .unwrap_or(0)
                } else {
                    self.selected_profile_idx
                        .checked_sub(1)
                        .unwrap_or(count.saturating_sub(1))
                };
                self.select_profile(next);
            }
            SettingRow::Quality => {
                let presets = [
                    QualityPreset::Low,
                    QualityPreset::Medium,
                    QualityPreset::High,
                    QualityPreset::Lossless,
                ];
                let next = step(&presets, self.quality_preset, forward);
                self.set_quality_preset(next);
            }
            SettingRow::SampleRate => {
                self.audio_settings.sample_rate =
                    step(&SAMPLE_RATES, self.audio_settings.sample_rate, forward);
            }
            SettingRow::Channels => {
                self.audio_settings.channels = if self.audio_settings.channels == 1 {
                    2
                } else {
                    1
                };
            }
            SettingRow::SampleFormat => {
                self.audio_settings.bit_depth =
                    step(&BIT_DEPTHS, self.audio_settings.bit_depth, forward);
            }
            SettingRow::MaxWidth => {
                self.image_settings.max_width =
                    step(&MAX_WIDTHS, self.image_settings.max_width, forward);
            }
            SettingRow::MaxHeight => {
                self.image_settings.max_height =
                    step(&MAX_HEIGHTS, self.image_settings.max_height, forward);
            }
            SettingRow::Naming => {
                let rules = naming_rules();
                let at = rules
                    .iter()
                    .position(|r| *r == self.output_naming)
                    .unwrap_or(0);
                let next = if forward {
                    at.saturating_add(1).checked_rem(rules.len()).unwrap_or(0)
                } else {
                    at.checked_sub(1).unwrap_or(rules.len().saturating_sub(1))
                };
                if let Some(rule) = rules.get(next) {
                    self.output_naming = rule.clone();
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Events
    // -----------------------------------------------------------------------

    /// Whether the queue has anything left to do.
    fn has_work(&self) -> bool {
        self.jobs
            .iter()
            .any(|j| matches!(j.status, JobStatus::Queued | JobStatus::Running))
    }

    /// Handle one event from the window.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        // The picker takes input first while it is up -- and a tick falls
        // through it, so a running conversion is not stalled by an open
        // dialog.
        match self
            .picker
            .handle(event, self.window_width, self.window_height)
        {
            Picked::Chose(path) => {
                self.picked(&path);
                return EventResult::Consumed;
            }
            Picked::Handled | Picked::Cancelled => return EventResult::Consumed,
            Picked::Ignored => {}
        }
        match event {
            Event::Key(key) if key.pressed => {
                let before = (self.selected_source, self.selected_job);
                let walked = matches!(key.key, Key::Up | Key::Down);
                let result = self.handle_key(key);
                // An arrow that moved nothing -- at the end of a list the
                // wheel scrolled away -- still brings the chosen row back.
                self.follow_selection(
                    self.selected_source != before.0
                        || (walked && self.active_panel == ActivePanel::SourceList),
                    self.selected_job != before.1
                        || (walked && self.active_panel == ActivePanel::Queue),
                );
                result
            }
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Resize { width, height } => {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a window wider than 16 million pixels does not exist"
                )]
                {
                    self.window_width = *width as f32;
                    self.window_height = *height as f32;
                }
                EventResult::Consumed
            }
            Event::Tick { .. } => self.handle_tick(),
            _ => EventResult::Ignored,
        }
    }

    /// What the picker chose, done with.
    fn picked(&mut self, path: &Path) {
        match self.picker_for {
            PickerFor::Files => {
                self.status_line = match self.add_file(path) {
                    Ok(_) => format!("Added {}", path.display()),
                    Err(why) => why,
                };
            }
            PickerFor::Folder => {
                self.status_line = match self.add_folder(path) {
                    Ok(0) => format!("No media files in {}", path.display()),
                    Ok(n) => format!("Added {n} file(s) from {}", path.display()),
                    Err(why) => why,
                };
            }
            PickerFor::OutputFolder => {
                self.output_dir = Some(path.to_path_buf());
                self.status_line = format!("Outputs go to {}", path.display());
            }
        }
    }

    fn open_picker(&mut self, what: PickerFor) {
        self.picker_for = what;
        match what {
            PickerFor::Files => self.picker.open_to_read(),
            PickerFor::Folder | PickerFor::OutputFolder => self.picker.put_up(
                FileDialog::select_folder().with_initial_path(FilePicker::default_start()),
                false,
            ),
        }
    }

    /// One step of the queue: the running job's progress, its result when it
    /// has one, and the next job when it is done.
    ///
    /// One job at a time, which is what a converter on one machine does: four
    /// conversions at a quarter speed each finish no sooner and make the
    /// progress bars useless.
    fn handle_tick(&mut self) -> EventResult {
        let Some(worker) = self.worker.as_mut() else {
            return if self.start_next_job() {
                EventResult::Consumed
            } else {
                EventResult::Ignored
            };
        };
        let id = worker.job_id;
        let progress = worker.progress();
        let result = worker.poll();
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == id) {
            job.progress = (progress * 100.0).clamp(0.0, 100.0);
        }
        if let Some(result) = result {
            self.worker = None;
            match result {
                Ok(bytes) => {
                    self.complete_job(id, bytes);
                }
                Err(why) if why == "cancelled" => {
                    if let Some(job) = self.jobs.iter_mut().find(|j| j.id == id) {
                        job.cancel();
                    }
                }
                Err(why) => {
                    self.fail_job(id, &why);
                }
            }
            self.start_next_job();
        }
        EventResult::Consumed
    }

    /// Handle a key press.
    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        // Above the Ctrl branch, which returns for every Ctrl chord. Placed
        // after it, Ctrl+C would cancel the queue from behind the card.
        if key.key == Key::F1 || (key.key == Key::Slash && key.modifiers.shift) {
            self.show_help = !self.show_help;
            return EventResult::Consumed;
        }
        if self.show_help {
            if matches!(key.key, Key::Escape | Key::Enter | Key::F1) {
                self.show_help = false;
            }
            return EventResult::Consumed;
        }
        if key.modifiers.ctrl {
            return match key.key {
                Key::O => {
                    self.open_picker(if key.modifiers.shift {
                        PickerFor::Folder
                    } else {
                        PickerFor::Files
                    });
                    EventResult::Consumed
                }
                // Queue everything and start.
                Key::Enter => {
                    self.queue_all();
                    self.start_next_job();
                    self.active_panel = ActivePanel::Queue;
                    EventResult::Consumed
                }
                // Abandon what has not started.
                Key::C => {
                    self.cancel_all_queued();
                    EventResult::Consumed
                }
                // Sweep up what has finished.
                Key::L => {
                    self.clear_finished_jobs();
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            };
        }

        match key.key {
            Key::Tab => {
                self.active_panel = match self.active_panel {
                    ActivePanel::SourceList => ActivePanel::Settings,
                    ActivePanel::Settings => ActivePanel::Queue,
                    ActivePanel::Queue => ActivePanel::SourceList,
                };
                EventResult::Consumed
            }
            Key::Up | Key::Down if self.active_panel == ActivePanel::SourceList => {
                self.move_source_selection(if key.key == Key::Down { 1 } else { -1 });
                EventResult::Consumed
            }
            Key::Up | Key::Down if self.active_panel == ActivePanel::Queue => {
                self.move_job_selection(if key.key == Key::Down { 1 } else { -1 });
                EventResult::Consumed
            }
            // Where outputs go, which the panel's two buttons also set.
            Key::O => {
                self.open_picker(PickerFor::OutputFolder);
                EventResult::Consumed
            }
            Key::B => {
                self.output_beside();
                EventResult::Consumed
            }
            // The settings rows.
            Key::Up | Key::Down if self.active_panel == ActivePanel::Settings => {
                let rows = setting_rows(self.output_format().as_ref());
                let at = rows
                    .iter()
                    .position(|r| *r == self.setting_row)
                    .unwrap_or(0);
                let next = if key.key == Key::Down {
                    at.saturating_add(1).min(rows.len().saturating_sub(1))
                } else {
                    at.saturating_sub(1)
                };
                self.setting_row = rows.get(next).copied().unwrap_or(SettingRow::Profile);
                EventResult::Consumed
            }
            // Queue just the selected file.
            Key::Enter if self.active_panel == ActivePanel::SourceList => {
                if let Some(id) = self.selected_source {
                    self.queue_source(id);
                    self.start_next_job();
                }
                EventResult::Consumed
            }
            Key::Delete => match self.active_panel {
                // Take a file off the list, or the whole list with nothing
                // selected.
                ActivePanel::SourceList => {
                    if let Some(id) = self.selected_source {
                        self.remove_source(id);
                        self.selected_source = None;
                    } else {
                        self.clear_sources();
                    }
                    EventResult::Consumed
                }
                // The chosen job: cancelled if it is still to run, taken off
                // the list if it has finished. With none chosen, the first
                // that can be cancelled -- or, with none left, every
                // finished one.
                ActivePanel::Queue => {
                    let chosen = self
                        .selected_job
                        .and_then(|id| self.jobs.iter().find(|j| j.id == id))
                        .map(|j| (j.id, j.status));
                    match chosen {
                        Some((id, JobStatus::Queued | JobStatus::Running)) => {
                            self.cancel_job(id);
                        }
                        Some((id, _)) => {
                            self.jobs.retain(|j| j.id != id);
                            self.selected_job = None;
                        }
                        None => {
                            if let Some(id) = self.first_cancellable_job() {
                                self.cancel_job(id);
                            } else {
                                self.clear_finished_jobs();
                            }
                        }
                    }
                    EventResult::Consumed
                }
                ActivePanel::Settings => EventResult::Ignored,
            },
            // The chosen setting's value.
            Key::Left | Key::Right if self.active_panel == ActivePanel::Settings => {
                self.step_setting(self.setting_row, key.key == Key::Right);
                EventResult::Consumed
            }
            // Quality, low to lossless.
            Key::Num1 | Key::Num2 | Key::Num3 | Key::Num4 => {
                let preset = match key.key {
                    Key::Num1 => QualityPreset::Low,
                    Key::Num2 => QualityPreset::Medium,
                    Key::Num3 => QualityPreset::High,
                    _ => QualityPreset::Lossless,
                };
                self.set_quality_preset(preset);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Where the selected source sits in the list.
    fn selected_source_index(&self) -> Option<usize> {
        let id = self.selected_source?;
        self.sources.iter().position(|s| s.id == id)
    }

    /// The first job that can still be called off.
    fn first_cancellable_job(&self) -> Option<u64> {
        self.jobs
            .iter()
            .find(|j| matches!(j.status, JobStatus::Queued | JobStatus::Running) && !j.stopping)
            .map(|j| j.id)
    }

    /// Move the source selection by `delta` files.
    ///
    /// Held as an id rather than an index -- `selected_source` is `Option<u64>`
    /// -- so removing a file above the selected one does not silently select
    /// its neighbour. Stops at the ends rather than wrapping.
    fn move_source_selection(&mut self, delta: isize) {
        if self.sources.is_empty() {
            self.selected_source = None;
            return;
        }
        let last = (self.sources.len() as isize).saturating_sub(1);
        let next = match self.selected_source_index() {
            Some(index) => (index as isize).saturating_add(delta).clamp(0, last),
            None if delta < 0 => last,
            None => 0,
        };
        self.selected_source = self.sources.get(next.unsigned_abs()).map(|s| s.id);
    }

    /// Where the chosen job sits in the queue.
    fn selected_job_index(&self) -> Option<usize> {
        let id = self.selected_job?;
        self.jobs.iter().position(|j| j.id == id)
    }

    /// Move the job selection by `delta` rows, stopping at the ends.
    fn move_job_selection(&mut self, delta: isize) {
        if self.jobs.is_empty() {
            self.selected_job = None;
            return;
        }
        let last = (self.jobs.len() as isize).saturating_sub(1);
        let next = match self.selected_job_index() {
            Some(index) => (index as isize).saturating_add(delta).clamp(0, last),
            None if delta < 0 => last,
            None => 0,
        };
        self.selected_job = self.jobs.get(next.unsigned_abs()).map(|j| j.id);
    }

    /// Send outputs beside their sources again.
    fn output_beside(&mut self) {
        self.output_dir = None;
        self.status_line = String::from("Outputs go beside each source");
    }

    /// How to convert `source` with the chosen profile and settings, or why
    /// it cannot be.
    fn recipe_for(&self, source: &SourceFile) -> Result<engine::Recipe, String> {
        let output = self
            .output_format()
            .ok_or_else(|| String::from("no profile is chosen"))?;
        let name = source
            .path
            .file_name()
            .unwrap_or_else(|| source.path.as_os_str());
        engine::recipe(name, &output, &self.audio_settings, &self.image_settings)
    }

    /// Queue all sources for conversion with current settings; answers how
    /// many were queued. Each one the profile cannot convert is left out, and
    /// the status line says how many and why the first was.
    pub fn queue_all(&mut self) -> usize {
        let ids: Vec<u64> = self.sources.iter().map(|s| s.id).collect();
        let mut queued = 0_usize;
        let mut refused = 0_usize;
        let mut first_reason = None;
        for id in ids {
            if self.queue_source(id).is_some() {
                queued = queued.saturating_add(1);
            } else {
                refused = refused.saturating_add(1);
                if first_reason.is_none() {
                    first_reason = Some(self.status_line.clone());
                }
            }
        }
        self.status_line = match (queued, refused) {
            (_, 0) => format!("Queued {queued} file(s)"),
            (0, _) => first_reason.unwrap_or_default(),
            _ => format!(
                "Queued {queued}; left out {refused}: {}",
                first_reason.unwrap_or_default()
            ),
        };
        queued
    }

    /// Queue a single source, if the chosen profile can convert it; if not,
    /// the status line says why and nothing is queued.
    pub fn queue_source(&mut self, source_id: u64) -> Option<u64> {
        let src = self.find_source(source_id)?.clone();
        let recipe = match self.recipe_for(&src) {
            Ok(recipe) => recipe,
            Err(why) => {
                self.status_line = format!("{}: {why}", src.file_name);
                return None;
            }
        };
        let output_format = self.output_format()?;
        let stem = src.path.file_name().unwrap_or_else(|| src.path.as_os_str());
        let name = self
            .output_naming
            .apply_os(stem, output_format.extension(), self.jobs.len());
        // Nothing is written over: not the source, not a file already there,
        // and not another job's output.
        let planned: Vec<PathBuf> = self
            .jobs
            .iter()
            .filter(|j| !matches!(j.status, JobStatus::Failed | JobStatus::Cancelled))
            .map(|j| j.output_path.clone())
            .collect();
        let output_path = engine::output_path(&src.path, self.output_dir.as_deref(), &name, &|p| {
            p.exists() || planned.iter().any(|q| q == p)
        });
        let id = self.id_gen.next_id();
        let output_name = output_path
            .file_name()
            .map_or_else(String::new, |n| Path::new(n).display().to_string());
        self.jobs.push(ConversionJob::new(
            id,
            src,
            output_format,
            output_name,
            output_path,
            recipe,
        ));
        Some(id)
    }

    /// Start the next queued job on a thread of its own, unless one is
    /// running; answers whether one started.
    pub fn start_next_job(&mut self) -> bool {
        if self.worker.is_some() {
            return false;
        }
        let ts = self.tick();
        let Some(job) = self.jobs.iter_mut().find(|j| j.status == JobStatus::Queued) else {
            return false;
        };
        job.start(ts);
        self.worker = Some(engine::Worker::start(
            job.id,
            job.recipe.clone(),
            job.source.path.clone(),
            job.output_path.clone(),
        ));
        // The status line keeps what queueing said -- which files were left
        // out and why; the queue shows the job running.
        true
    }

    /// Start the next queued job with no thread behind it, for the tests of
    /// the queue's bookkeeping.
    #[cfg(test)]
    pub fn start_next_job_fixture(&mut self) -> bool {
        let ts = self.tick();
        if let Some(job) = self.jobs.iter_mut().find(|j| j.status == JobStatus::Queued) {
            job.start(ts);
            true
        } else {
            false
        }
    }

    /// Complete a running job, and record it.
    pub fn complete_job(&mut self, job_id: u64, output_size: u64) -> bool {
        let ts = self.tick();
        if let Some(job) = self
            .jobs
            .iter_mut()
            .find(|j| j.id == job_id && j.status == JobStatus::Running)
        {
            job.complete(ts, output_size);
            self.history.push(HistoryEntry {
                source_path: job.source.path.clone(),
                output_path: job.output_path.clone(),
                conversion_type: job.conversion_label(),
                source_size: job.source.file_size,
                output_size,
                timestamp: ts,
                duration_secs: 0.0,
                success: true,
            });
            self.status_line = format!("Wrote {}", job.output_path.display());
            true
        } else {
            false
        }
    }

    /// Fail a running job.
    pub fn fail_job(&mut self, job_id: u64, error: &str) -> bool {
        let ts = self.tick();
        if let Some(job) = self
            .jobs
            .iter_mut()
            .find(|j| j.id == job_id && j.status == JobStatus::Running)
        {
            job.fail(ts, error);
            self.status_line = format!("{} failed: {error}", job.source.file_name);
            true
        } else {
            false
        }
    }

    /// Cancel a queued or running job. A running one is asked to stop, and
    /// stops before it writes anything; it shows as stopping until it has.
    pub fn cancel_job(&mut self, job_id: u64) -> bool {
        if let Some(worker) = &self.worker
            && worker.job_id == job_id
        {
            worker.cancel();
            if let Some(job) = self.jobs.iter_mut().find(|j| j.id == job_id) {
                job.stopping = true;
            }
            return true;
        }
        if let Some(job) = self.jobs.iter_mut().find(|j| {
            j.id == job_id && (j.status == JobStatus::Queued || j.status == JobStatus::Running)
        }) {
            job.cancel();
            true
        } else {
            false
        }
    }

    /// Cancel all queued jobs.
    pub fn cancel_all_queued(&mut self) -> usize {
        let mut count = 0usize;
        for job in &mut self.jobs {
            if job.status == JobStatus::Queued {
                job.cancel();
                count = count.saturating_add(1);
            }
        }
        count
    }

    /// Clear completed/failed/cancelled jobs from the list.
    pub fn clear_finished_jobs(&mut self) -> usize {
        let len = self.jobs.len();
        self.jobs
            .retain(|j| j.status == JobStatus::Queued || j.status == JobStatus::Running);
        len.saturating_sub(self.jobs.len())
    }

    // -----------------------------------------------------------------------
    // Statistics
    // -----------------------------------------------------------------------

    pub fn queue_stats(&self) -> QueueStats {
        let count = |status: JobStatus| self.jobs.iter().filter(|j| j.status == status).count();
        let total_source: u64 = self.jobs.iter().map(|j| j.source.file_size).sum();
        let total_output: u64 = self.jobs.iter().filter_map(|j| j.actual_size).sum();
        QueueStats {
            queued: count(JobStatus::Queued),
            running: count(JobStatus::Running),
            completed: count(JobStatus::Completed),
            failed: count(JobStatus::Failed),
            total_jobs: self.jobs.len(),
            total_source_size: total_source,
            total_output_size: total_output,
        }
    }

    pub fn history_stats(&self) -> HistoryStats {
        let total = self.history.len();
        let successful = self.history.iter().filter(|h| h.success).count();
        let total_source: u64 = self.history.iter().map(|h| h.source_size).sum();
        let total_output: u64 = self.history.iter().map(|h| h.output_size).sum();
        let total_saved: i64 = self.history.iter().map(HistoryEntry::space_saved).sum();

        HistoryStats {
            total,
            successful,
            total_source_size: total_source,
            total_output_size: total_output,
            total_space_saved: total_saved,
        }
    }

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

    /// For the tests: the window draws `frame`, whose boxes it keeps.
    ///
    /// Not `render`: [`App::render`] is the one the window calls, and this one
    /// takes the same two arguments -- so at equal arity an inherent method
    /// wins method lookup outright and the trait's is never called.
    #[cfg(test)]
    pub fn render_commands(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        self.frame(width, height).into_tree().commands
    }

    /// Draw the window, recording every control where it is drawn.
    pub fn frame(&self, width: f32, height: f32) -> Frame<Target> {
        let mut f = Frame::new(width, height);
        f.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });
        self.render_toolbar(&mut f, width);
        self.render_status_bar(&mut f, width, height);

        let content_y = TOOLBAR_HEIGHT;
        let content_h = (height - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT).max(0.0);
        f.clip(Rect::new(0.0, content_y, SIDEBAR_WIDTH, content_h));
        self.render_source_list(&mut f, content_y, content_h);
        f.unclip();
        f.clip(Rect::new(
            SIDEBAR_WIDTH,
            content_y,
            SETTINGS_PANEL_WIDTH,
            content_h,
        ));
        self.render_settings_panel(
            &mut f,
            SIDEBAR_WIDTH,
            content_y,
            SETTINGS_PANEL_WIDTH,
            content_h,
        );
        f.unclip();
        // The queue, always: it vanished whenever another panel had the keys.
        let queue_x = SIDEBAR_WIDTH + SETTINGS_PANEL_WIDTH;
        let queue_w = (width - queue_x).max(0.0);
        f.clip(Rect::new(queue_x, content_y, queue_w, content_h));
        self.render_queue_panel(&mut f, queue_x, content_y, queue_w, content_h);
        f.unclip();

        if self.show_help {
            guitk::shortcut::render_card(
                &mut f,
                &self.palette,
                (width, height),
                0.0,
                SHORTCUTS,
                "F1 or ? closes this",
            );
            f.hit(Target::HelpCard, Rect::new(0.0, 0.0, width, height));
        }
        f.extend(self.picker.render(&self.palette, width, height));
        f
    }

    /// A button, lit while the pointer is on it; one with nothing to do is
    /// drawn dim and records no box.
    fn button(
        &self,
        f: &mut Frame<Target>,
        rect: Rect,
        label: &str,
        target: Target,
        enabled: bool,
    ) {
        let lit = enabled && self.hover == Some(target);
        f.push(RenderCommand::FillRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: if lit {
                self.palette.surface2
            } else {
                self.palette.surface1
            },
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        f.push(RenderCommand::Text {
            x: rect.x + 8.0,
            y: rect.y + (rect.h - 11.0) / 2.0,
            text: label.to_owned(),
            color: if enabled {
                self.palette.text
            } else {
                self.palette.overlay0
            },
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some((rect.w - 12.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        if enabled {
            f.hit(target, rect);
        }
    }

    fn heading(&self, f: &mut Frame<Target>, x: f32, y: f32, text: String, width: f32) {
        f.push(RenderCommand::Text {
            x,
            y,
            text,
            color: self.palette.subtext0,
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width.max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn note(&self, f: &mut Frame<Target>, x: f32, y: f32, text: String, width: f32) {
        f.push(RenderCommand::Text {
            x,
            y,
            text,
            color: self.palette.subtext0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width.max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_toolbar(&self, f: &mut Frame<Target>, width: f32) {
        self.palette.push_surface(
            f,
            0.0,
            0.0,
            width,
            TOOLBAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Bottom),
        );
        f.push(RenderCommand::Text {
            x: 12.0,
            y: 12.0,
            text: "Media Converter".to_owned(),
            color: self.palette.ink(self.palette.blue),
            font_size: 15.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(150.0),
            overflow: TextOverflow::Ellipsis,
        });
        let stats = self.queue_stats();
        let finished = self
            .jobs
            .iter()
            .any(|j| !matches!(j.status, JobStatus::Queued | JobStatus::Running));
        let mut bx = 170.0;
        for (label, target, enabled) in [
            ("Add files\u{2026}", Target::AddFiles, true),
            ("Add folder\u{2026}", Target::AddFolder, true),
            ("Convert all", Target::ConvertAll, !self.sources.is_empty()),
            ("Cancel waiting", Target::CancelWaiting, stats.queued > 0),
            ("Clear finished", Target::ClearFinished, finished),
        ] {
            let bw = text::padded_width(label, 10.0, 11.0, FontWeightHint::Regular);
            self.button(f, Rect::new(bx, 8.0, bw, 24.0), label, target, enabled);
            bx += bw + 6.0;
        }
        self.button(
            f,
            Rect::new(width - 12.0 - 80.0, 8.0, 80.0, 24.0),
            "Keys (F1)",
            Target::Help,
            true,
        );
        f.push(RenderCommand::Line {
            x1: 0.0,
            y1: TOOLBAR_HEIGHT,
            x2: width,
            y2: TOOLBAR_HEIGHT,
            color: self.palette.surface0,
            width: 1.0,
        });
    }

    fn render_status_bar(&self, f: &mut Frame<Target>, width: f32, height: f32) {
        let bar_y = height - STATUS_BAR_HEIGHT;
        self.palette.push_surface(
            f,
            0.0,
            bar_y,
            width,
            STATUS_BAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Top),
        );
        let stats = self.queue_stats();
        let mut status = format!(
            "{} sources  |  {} queued  |  {} running  |  {} completed  |  {} failed",
            self.sources.len(),
            stats.queued,
            stats.running,
            stats.completed,
            stats.failed,
        );
        if !self.status_line.is_empty() {
            status.push_str("  |  ");
            status.push_str(&self.status_line);
        }
        f.push(RenderCommand::Text {
            x: 12.0,
            y: bar_y + 6.0,
            text: status,
            color: self.palette.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some((width - 24.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// The source rows' pane, and how many whole rows it holds.
    fn source_pane(&self) -> (Rect, usize) {
        let top = TOOLBAR_HEIGHT + 32.0;
        let bottom = self.window_height - STATUS_BAR_HEIGHT;
        let pane = Rect::new(0.0, top, SIDEBAR_WIDTH, (bottom - top).max(0.0));
        (pane, ((pane.h / ITEM_HEIGHT).floor().max(1.0)) as usize)
    }

    /// The queue rows' pane, and how many whole rows it holds.
    fn queue_pane(&self) -> (Rect, usize) {
        let x = SIDEBAR_WIDTH + SETTINGS_PANEL_WIDTH;
        let top = TOOLBAR_HEIGHT + 32.0;
        let bottom = self.window_height - STATUS_BAR_HEIGHT;
        let pane = Rect::new(
            x,
            top,
            (self.window_width - x).max(0.0),
            (bottom - top).max(0.0),
        );
        (pane, ((pane.h / QUEUE_ROW_H).floor().max(1.0)) as usize)
    }

    /// Scroll the lists whose selection the keys just moved, so that it is on
    /// screen -- and only those. Following the selection on every frame, as
    /// this once did, pinned a list to its chosen row: the wheel scrolled it
    /// away and the next frame scrolled it straight back.
    fn follow_selection(&mut self, source_moved: bool, job_moved: bool) {
        if source_moved && let Some(at) = self.selected_source_index() {
            let (_, rows) = self.source_pane();
            self.source_scroll = scrolled_to(self.source_scroll, at, rows);
        }
        if job_moved && let Some(at) = self.selected_job_index() {
            let (_, rows) = self.queue_pane();
            self.queue_scroll = scrolled_to(self.queue_scroll, at, rows);
        }
        self.clamp_scrolls();
    }

    /// Keep each list's scroll inside the list: never past its end, which a
    /// shorter list or a taller window would otherwise leave it.
    fn clamp_scrolls(&mut self) {
        let (_, rows) = self.source_pane();
        self.source_scroll = self
            .source_scroll
            .min(self.sources.len().saturating_sub(rows));
        let (_, rows) = self.queue_pane();
        self.queue_scroll = self.queue_scroll.min(self.jobs.len().saturating_sub(rows));
    }

    /// The border that says which panel has the keys.
    fn panel_focus(&self, f: &mut Frame<Target>, panel: ActivePanel, rect: Rect) {
        f.hit(Target::Panel(panel), rect);
        if self.active_panel == panel {
            f.push(RenderCommand::StrokeRect {
                x: rect.x + 1.0,
                y: rect.y + 1.0,
                width: (rect.w - 2.0).max(0.0),
                height: (rect.h - 2.0).max(0.0),
                color: self.palette.blue,
                line_width: 1.0,
                corner_radii: CornerRadii::ZERO,
            });
        }
    }

    fn render_source_list(&self, f: &mut Frame<Target>, y: f32, height: f32) {
        f.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: SIDEBAR_WIDTH,
            height,
            color: self.palette.mantle,
            corner_radii: CornerRadii::ZERO,
        });
        self.panel_focus(
            f,
            ActivePanel::SourceList,
            Rect::new(0.0, y, SIDEBAR_WIDTH, height),
        );
        self.heading(
            f,
            12.0,
            y + 10.0,
            format!("SOURCE FILES ({})", self.sources.len()),
            SIDEBAR_WIDTH - 24.0,
        );

        let (pane, rows) = self.source_pane();
        f.hit(Target::SourceList, pane);
        if self.sources.is_empty() {
            for (i, line) in [
                "No files yet.",
                "Add files\u{2026} (Ctrl+O), or a whole",
                "folder (Ctrl+Shift+O).",
                "",
                "WAV becomes WAV; PNG and JPEG",
                "become BMP. Nothing else can",
                "be converted in this build.",
            ]
            .iter()
            .enumerate()
            {
                self.note(
                    f,
                    16.0,
                    pane.y + 12.0 + i as f32 * 14.0,
                    (*line).to_owned(),
                    SIDEBAR_WIDTH - 32.0,
                );
            }
        }
        for (shown, src) in self
            .sources
            .iter()
            .skip(self.source_scroll)
            .take(rows.saturating_add(1))
            .enumerate()
        {
            let cy = pane.y + shown as f32 * ITEM_HEIGHT;
            let row = Rect::new(4.0, cy, SIDEBAR_WIDTH - 8.0, ITEM_HEIGHT);
            let target = Target::SourceRow(src.id);
            let is_selected = self.selected_source == Some(src.id);
            if is_selected || self.hover == Some(target) {
                f.push(RenderCommand::FillRect {
                    x: row.x,
                    y: row.y,
                    width: row.w,
                    height: row.h,
                    color: if is_selected {
                        self.palette.surface0
                    } else {
                        self.palette.crust
                    },
                    corner_radii: CornerRadii::all(CORNER_RADIUS),
                });
            }
            f.hit(target, row);
            f.push(RenderCommand::FillRect {
                x: 12.0,
                y: cy + 11.0,
                width: 8.0,
                height: 8.0,
                color: match src.category {
                    MediaCategory::Audio => self.palette.teal,
                    MediaCategory::Video => self.palette.mauve,
                    MediaCategory::Image => self.palette.peach,
                },
                corner_radii: CornerRadii::all(4.0),
            });
            f.push(RenderCommand::Text {
                x: 26.0,
                y: cy + 5.0,
                text: src.file_name.clone(),
                color: if is_selected {
                    self.palette.text
                } else {
                    self.palette.subtext1
                },
                font_size: 11.0,
                font_weight: if is_selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(SIDEBAR_WIDTH - 64.0),
                overflow: TextOverflow::Ellipsis,
            });
            let mut info = src.human_size();
            if let Some(dur) = src.duration_secs {
                info.push_str(&format!(" | {}", format_duration(dur)));
            }
            if !src.source_format.is_empty() {
                info.push_str(&format!(" | {}", src.source_format));
            }
            f.push(RenderCommand::Text {
                x: 26.0,
                y: cy + 19.0,
                text: info,
                color: self.palette.subtext0,
                font_size: 9.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - 64.0),
                overflow: TextOverflow::Ellipsis,
            });
            // Off the list -- the file itself is not touched.
            self.button(
                f,
                Rect::new(SIDEBAR_WIDTH - 32.0, cy + 6.0, 22.0, 20.0),
                "\u{00D7}",
                Target::RemoveSource(src.id),
                true,
            );
        }
    }

    /// A settings row: its label, and its value between arrows.
    fn render_setting(
        &self,
        f: &mut Frame<Target>,
        row: SettingRow,
        x: f32,
        y: f32,
        w: f32,
        label: &str,
        value: String,
    ) {
        let chosen = self.active_panel == ActivePanel::Settings && self.setting_row == row;
        let rect = Rect::new(x - 4.0, y - 2.0, w + 8.0, 44.0);
        if chosen {
            f.push(RenderCommand::FillRect {
                x: rect.x,
                y: rect.y,
                width: rect.w,
                height: rect.h,
                color: self.palette.surface0,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });
        }
        f.hit(Target::Setting(row), rect);
        self.heading(f, x, y, label.to_owned(), w);
        let box_y = y + 14.0;
        self.button(
            f,
            Rect::new(x, box_y, 24.0, 24.0),
            "\u{25C0}",
            Target::StepBack(row),
            true,
        );
        self.button(
            f,
            Rect::new(x + w - 24.0, box_y, 24.0, 24.0),
            "\u{25B6}",
            Target::StepForward(row),
            true,
        );
        self.palette.push_surface(
            f,
            x + 28.0,
            box_y,
            (w - 56.0).max(0.0),
            24.0,
            CORNER_RADIUS,
            Surface::Card,
        );
        f.push(RenderCommand::Text {
            x: x + 36.0,
            y: box_y + 6.0,
            text: value,
            color: self.palette.text,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some((w - 72.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_settings_panel(
        &self,
        f: &mut Frame<Target>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        f.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });
        f.push(RenderCommand::Line {
            x1: x + width,
            y1: y,
            x2: x + width,
            y2: y + height,
            color: self.palette.surface0,
            width: 1.0,
        });
        self.panel_focus(f, ActivePanel::Settings, Rect::new(x, y, width, height));

        let lx = x + 12.0;
        let max_w = width - 24.0;
        let mut cy = y + 10.0;
        // What this build can do, above everything it can be told to do.
        for (i, line) in CAN_CONVERT_LINES.iter().enumerate() {
            f.push(RenderCommand::Text {
                x: lx,
                y: cy,
                text: (*line).to_owned(),
                color: if i == 0 {
                    self.palette.text
                } else {
                    self.palette.subtext0
                },
                font_size: 10.0,
                font_weight: if i == 0 {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(max_w),
                overflow: TextOverflow::Ellipsis,
            });
            cy += 13.0;
        }
        cy += 8.0;

        let profile = self.profiles.get(self.selected_profile_idx);
        let output = profile.map(|p| p.output_format.clone());
        for row in setting_rows(output.as_ref()) {
            let (label, value) = match row {
                SettingRow::Profile => (
                    "PROFILE",
                    profile.map_or_else(String::new, |p| p.name.clone()),
                ),
                SettingRow::Quality => ("QUALITY (1-4)", self.quality_preset.label().to_owned()),
                SettingRow::SampleRate => (
                    "SAMPLE RATE",
                    format!("{} Hz", self.audio_settings.sample_rate),
                ),
                SettingRow::Channels => (
                    "CHANNELS",
                    String::from(if self.audio_settings.channels == 1 {
                        "Mono"
                    } else {
                        "Stereo"
                    }),
                ),
                SettingRow::SampleFormat => (
                    "SAMPLES",
                    match self.audio_settings.bit_depth {
                        32 => String::from("32-bit float"),
                        bits => format!("{bits}-bit"),
                    },
                ),
                SettingRow::MaxWidth => (
                    "LARGEST WIDTH",
                    self.image_settings
                        .max_width
                        .map_or_else(|| String::from("Its own"), |w| format!("{w} px")),
                ),
                SettingRow::MaxHeight => (
                    "LARGEST HEIGHT",
                    self.image_settings
                        .max_height
                        .map_or_else(|| String::from("Its own"), |h| format!("{h} px")),
                ),
                SettingRow::Naming => (
                    "OUTPUT NAME",
                    match &self.output_naming {
                        OutputNaming::KeepOriginal => String::from("Same name"),
                        OutputNaming::Suffix(s) => format!("Name{s}"),
                        OutputNaming::Prefix(p) => format!("{p}Name"),
                        OutputNaming::Pattern(p) => p.clone(),
                    },
                ),
            };
            self.render_setting(f, row, lx, cy, max_w, label, value);
            cy += 48.0;
            if row == SettingRow::Profile {
                // What the profile makes, or why this build cannot.
                if let Some(profile) = profile {
                    match profile.availability() {
                        Ok(()) => self.note(f, lx, cy - 2.0, profile.description.clone(), max_w),
                        Err(why) => f.push(RenderCommand::Text {
                            x: lx,
                            y: cy - 2.0,
                            text: format!("Not available: {why}"),
                            color: self.palette.ink(self.palette.red),
                            font_size: 10.0,
                            font_weight: FontWeightHint::Bold,
                            max_width: Some(max_w),
                            overflow: TextOverflow::Ellipsis,
                        }),
                    }
                }
                cy += 16.0;
            }
        }

        // Where outputs go.
        self.heading(f, lx, cy, String::from("OUTPUT FOLDER"), max_w);
        cy += 14.0;
        self.note(
            f,
            lx,
            cy,
            self.output_dir.as_ref().map_or_else(
                || String::from("Beside each source"),
                |d| d.display().to_string(),
            ),
            max_w,
        );
        cy += 16.0;
        let half = (max_w - 6.0) / 2.0;
        self.button(
            f,
            Rect::new(lx, cy, half, 24.0),
            "Choose\u{2026}",
            Target::ChooseOutput,
            true,
        );
        self.button(
            f,
            Rect::new(lx + half + 6.0, cy, half, 24.0),
            "Beside sources",
            Target::OutputBeside,
            self.output_dir.is_some(),
        );
    }

    fn render_queue_panel(&self, f: &mut Frame<Target>, x: f32, y: f32, width: f32, height: f32) {
        self.palette
            .push_surface(f, x, y, width, height, 0.0, Surface::Card);
        self.panel_focus(f, ActivePanel::Queue, Rect::new(x, y, width, height));
        let stats = self.queue_stats();
        self.heading(
            f,
            x + 12.0,
            y + 10.0,
            format!("CONVERSION QUEUE ({})", stats.total_jobs),
            width - 24.0,
        );
        let (pane, rows) = self.queue_pane();
        f.hit(Target::QueueList, pane);
        if self.jobs.is_empty() {
            self.note(
                f,
                x + 16.0,
                pane.y + 12.0,
                String::from("Nothing queued. Enter queues the chosen file; Convert all, every file the profile can convert."),
                width - 32.0,
            );
        }
        for (shown, job) in self
            .jobs
            .iter()
            .skip(self.queue_scroll)
            .take(rows.saturating_add(1))
            .enumerate()
        {
            let cy = pane.y + shown as f32 * QUEUE_ROW_H;
            let chosen = self.selected_job == Some(job.id);
            self.palette.push_surface(
                f,
                x + 4.0,
                cy,
                width - 8.0,
                QUEUE_ROW_H - 4.0,
                CORNER_RADIUS,
                if chosen {
                    Surface::Selected
                } else {
                    Surface::Card
                },
            );
            // Under the row's Cancel button, which is pushed after it and so
            // takes the press over its own rectangle.
            f.hit(
                Target::QueueRow(job.id),
                Rect::new(x + 4.0, cy, width - 8.0, QUEUE_ROW_H - 4.0),
            );
            f.push(RenderCommand::FillRect {
                x: x + 10.0,
                y: cy + 12.0,
                width: 8.0,
                height: 8.0,
                color: job.status.color(&self.palette),
                corner_radii: CornerRadii::all(4.0),
            });
            f.push(RenderCommand::Text {
                x: x + 24.0,
                y: cy + 5.0,
                text: format!("{}  \u{2192}  {}", job.source.file_name, job.output_name),
                color: self.palette.text,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some((width - 170.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            let detail = match (&job.status, &job.error_message) {
                (JobStatus::Failed, Some(why)) => why.clone(),
                _ => job.recipe.describe(),
            };
            f.push(RenderCommand::Text {
                x: x + 24.0,
                y: cy + 20.0,
                text: detail,
                color: if job.status == JobStatus::Failed {
                    self.palette.ink(self.palette.red)
                } else {
                    self.palette.subtext0
                },
                font_size: 9.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some((width - 170.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            f.push(RenderCommand::Text {
                x: x + width - 140.0,
                y: cy + 6.0,
                text: if job.stopping {
                    String::from("Stopping")
                } else {
                    job.status.label().to_owned()
                },
                color: self.palette.ink(job.status.color(&self.palette)),
                font_size: 10.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(70.0),
                overflow: TextOverflow::Ellipsis,
            });
            if matches!(job.status, JobStatus::Queued | JobStatus::Running) && !job.stopping {
                self.button(
                    f,
                    Rect::new(x + width - 66.0, cy + 6.0, 56.0, 22.0),
                    "Cancel",
                    Target::CancelJob(job.id),
                    true,
                );
            }
            if job.status == JobStatus::Running {
                let bar_x = x + 24.0;
                let bar_w = (width - 170.0).max(0.0);
                let bar_y = cy + 34.0;
                f.push(RenderCommand::FillRect {
                    x: bar_x,
                    y: bar_y,
                    width: bar_w,
                    height: 4.0,
                    color: self.palette.surface1,
                    corner_radii: CornerRadii::all(2.0),
                });
                f.push(RenderCommand::FillRect {
                    x: bar_x,
                    y: bar_y,
                    width: bar_w * (job.progress / 100.0).clamp(0.0, 1.0),
                    height: 4.0,
                    color: self.palette.blue,
                    corner_radii: CornerRadii::all(2.0),
                });
            }
        }
    }

    // -----------------------------------------------------------------------
    // The pointer
    // -----------------------------------------------------------------------

    /// What is under `(x, y)` in the frame last shown.
    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        if self.last_hits.is_empty() {
            return self
                .frame(self.window_width, self.window_height)
                .hit_test(x, y);
        }
        self.last_hits
            .iter()
            .rev()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(target, _)| *target)
    }

    fn handle_mouse(&mut self, event: &MouseEvent) -> EventResult {
        match event.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                let Some(target) = self
                    .frame(self.window_width, self.window_height)
                    .hit_test(event.x, event.y)
                else {
                    return EventResult::Ignored;
                };
                let result = self.press(target);
                self.clamp_scrolls();
                result
            }
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
            MouseEventKind::Scroll { dy, .. } => self.wheel_at(event.x, event.y, dy),
            _ => EventResult::Ignored,
        }
    }

    /// A left press on `target`.
    fn press(&mut self, target: Target) -> EventResult {
        match target {
            Target::HelpCard => self.show_help = false,
            Target::Help => self.show_help = true,
            Target::AddFiles => self.open_picker(PickerFor::Files),
            Target::AddFolder => self.open_picker(PickerFor::Folder),
            Target::ConvertAll => {
                self.queue_all();
                self.start_next_job();
                self.active_panel = ActivePanel::Queue;
            }
            Target::CancelWaiting => {
                self.cancel_all_queued();
            }
            Target::ClearFinished => {
                self.clear_finished_jobs();
            }
            Target::Panel(panel) => {
                if self.active_panel == panel {
                    return EventResult::Ignored;
                }
                self.active_panel = panel;
            }
            // A press chooses a file; a second press queues it.
            Target::SourceRow(id) => {
                self.active_panel = ActivePanel::SourceList;
                if self.selected_source == Some(id) {
                    self.queue_source(id);
                    self.start_next_job();
                } else {
                    self.selected_source = Some(id);
                }
            }
            Target::RemoveSource(id) => {
                self.remove_source(id);
            }
            Target::Setting(row) => {
                self.active_panel = ActivePanel::Settings;
                self.setting_row = row;
            }
            Target::StepBack(row) | Target::StepForward(row) => {
                self.active_panel = ActivePanel::Settings;
                self.setting_row = row;
                self.step_setting(row, matches!(target, Target::StepForward(_)));
            }
            Target::ChooseOutput => self.open_picker(PickerFor::OutputFolder),
            Target::OutputBeside => self.output_beside(),
            Target::QueueRow(id) => {
                if self.active_panel == ActivePanel::Queue && self.selected_job == Some(id) {
                    return EventResult::Ignored;
                }
                self.active_panel = ActivePanel::Queue;
                self.selected_job = Some(id);
            }
            Target::CancelJob(id) => {
                self.cancel_job(id);
            }
            // A press in a list's empty space gives it the keys.
            Target::SourceList | Target::QueueList => {
                let panel = if target == Target::SourceList {
                    ActivePanel::SourceList
                } else {
                    ActivePanel::Queue
                };
                if self.active_panel == panel {
                    return EventResult::Ignored;
                }
                self.active_panel = panel;
            }
        }
        EventResult::Consumed
    }

    /// The wheel over the source list or the queue.
    fn wheel_at(&mut self, x: f32, y: f32, dy: f32) -> EventResult {
        let sources = match self.target_at(x, y) {
            Some(Target::SourceList | Target::SourceRow(_) | Target::RemoveSource(_)) => true,
            Some(Target::QueueList | Target::QueueRow(_) | Target::CancelJob(_)) => false,
            _ => return EventResult::Ignored,
        };
        let rows = self.wheel.rows(dy);
        let (now, count, visible) = if sources {
            (self.source_scroll, self.sources.len(), self.source_pane().1)
        } else {
            (self.queue_scroll, self.jobs.len(), self.queue_pane().1)
        };
        let next = if rows < 0 {
            now.saturating_sub(rows.unsigned_abs())
        } else {
            now.saturating_add(rows.unsigned_abs())
        }
        .min(count.saturating_sub(visible));
        if next == now {
            return EventResult::Ignored;
        }
        if sources {
            self.source_scroll = next;
        } else {
            self.queue_scroll = next;
        }
        EventResult::Consumed
    }
}

pub struct QueueStats {
    pub queued: usize,
    pub running: usize,
    pub completed: usize,
    pub failed: usize,
    pub total_jobs: usize,
    pub total_source_size: u64,
    pub total_output_size: u64,
}

pub struct HistoryStats {
    pub total: usize,
    pub successful: usize,
    pub total_source_size: u64,
    pub total_output_size: u64,
    pub total_space_saved: i64,
}

// ============================================================================
// Main
// ============================================================================

impl App for MediaConvertApp {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    fn title(&self) -> String {
        // What the queue is doing, because a batch conversion is something you
        // start and then look away from.
        let left = self
            .jobs
            .iter()
            .filter(|j| matches!(j.status, JobStatus::Queued | JobStatus::Running))
            .count();
        if left == 0 {
            return "Media Converter".to_string();
        }
        format!("Converting - {left} left - Media Converter")
    }

    fn initial_size(&self) -> (u32, u32) {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "both are positive constants well inside u32"
        )]
        {
            (self.window_width as u32, self.window_height as u32)
        }
    }

    /// A clock only while a job is running or waiting to.
    ///
    /// It asked for one whenever anything was queued, and nothing could run,
    /// so a queued file woke the machine eight times a second for good.
    fn tick_interval(&self) -> Option<Duration> {
        self.has_work().then_some(JOB_STEP)
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
        self.window_width = width;
        self.window_height = height;
        self.clamp_scrolls();
        let frame = self.frame(width, height);
        self.last_hits = frame.hits().to_vec();
        frame.into_tree()
    }
}

fn main() -> ExitCode {
    app::launch("mediaconvert", &mut MediaConvertApp::new())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    /// A fresh window holds no sources, and says what it can do and how to
    /// begin.
    ///
    /// It opened on `/music/song.flac` at 50 MB and `/videos/clip.mkv` at
    /// 1.5 GB -- paths with sizes and durations, none of which had been read --
    /// and then on a notice, painted over by the toolbar, that it could not
    /// convert anything.
    #[test]
    fn a_fresh_window_holds_no_sources_and_says_what_it_can_do() {
        let app = MediaConvertApp::new();
        assert!(app.sources.is_empty(), "sources appeared from nowhere");
        assert!(app.jobs.is_empty(), "a queue appeared from nowhere");
        assert!(app.history.is_empty(), "conversions appeared from nowhere");
        let drawn = texts(&app);
        for line in CAN_CONVERT_LINES {
            assert!(
                drawn.iter().any(|t| t == line),
                "the window never said {line:?}"
            );
        }
        assert!(drawn.iter().any(|t| t == "No files yet."));
        assert_eq!(app.tick_interval(), None, "an empty window keeps a clock");
    }

    fn key_ev(key: Key, ctrl: bool) -> Event {
        let mut modifiers = guitk::event::Modifiers::NONE;
        modifiers.ctrl = ctrl;
        Event::Key(KeyEvent {
            key,
            pressed: true,
            modifiers,
            text: String::new(),
        })
    }

    /// **Every key the card advertises is answered by this window**, in one
    /// panel or another.
    #[test]
    fn every_advertised_key_does_something() {
        let panels = [
            ActivePanel::SourceList,
            ActivePanel::Settings,
            ActivePanel::Queue,
        ];
        for (label, what) in SHORTCUTS {
            for stroke in guitk::shortcut::keystrokes(label).unwrap_or_else(|e| panic!("{e}")) {
                let answered = panels.into_iter().any(|panel| {
                    let mut app = MediaConvertApp::new();
                    app.active_panel = panel;
                    app.handle_event(&Event::Key(stroke.clone())) == EventResult::Consumed
                });
                assert!(
                    answered,
                    "the card advertises {label:?} for {what:?}, and nothing answers {:?}",
                    stroke.key
                );
            }
        }
    }

    #[test]
    fn the_shortcut_list_reaches_the_window() {
        let drawn = |app: &MediaConvertApp| -> String { texts(app).join(" | ") };
        let mut app = MediaConvertApp::new();
        assert!(
            !drawn(&app).contains("F1 or ? closes this"),
            "the card is up before anybody asked for it"
        );
        app.handle_event(&press(Key::F1));
        let shown = drawn(&app);
        for (keys, what) in SHORTCUTS {
            assert!(shown.contains(keys), "{keys:?} never reached the window");
            assert!(shown.contains(what), "{what:?} never reached the window");
        }
        let panel = app.active_panel;
        app.handle_event(&press(Key::Tab));
        assert_eq!(
            app.active_panel, panel,
            "Tab changed panel through the shortcut card"
        );
        app.handle_event(&press(Key::F1));
        app.handle_event(&press(Key::Tab));
        assert_ne!(
            app.active_panel, panel,
            "control: Tab does nothing even with the card down"
        );
    }

    fn press(k: Key) -> Event {
        key_ev(k, false)
    }

    fn tick() -> Event {
        Event::Tick { elapsed_ms: 120 }
    }

    fn texts(app: &MediaConvertApp) -> Vec<String> {
        app.render_commands(app.window_width, app.window_height)
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// One source of each kind, named but not on disk: for the bookkeeping,
    /// which reads no file.
    fn seeded() -> MediaConvertApp {
        let mut app = MediaConvertApp::new();
        app.add_source_with_duration(
            "/music/song.wav",
            "song.wav",
            50_000_000,
            MediaCategory::Audio,
            243.5,
            "WAV",
        );
        app.add_source_with_duration(
            "/videos/clip.mkv",
            "clip.mkv",
            1_500_000_000,
            MediaCategory::Video,
            3600.0,
            "MKV",
        );
        app.add_source(
            "/pictures/photo.png",
            "photo.png",
            8_000_000,
            MediaCategory::Image,
        );
        app
    }

    /// Three WAVs, named but not on disk.
    fn three_wavs() -> MediaConvertApp {
        let mut app = MediaConvertApp::new();
        for name in ["a.wav", "b.wav", "c.wav"] {
            app.add_source(&format!("/music/{name}"), name, 1000, MediaCategory::Audio);
        }
        app
    }

    /// A directory for one test, removed when it ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("mediaconvert-app-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        /// A WAV of `seconds` of a tone, written here.
        fn wav(&self, name: &str, rate: u32, channels: u16, seconds: f32) -> PathBuf {
            let frames = (rate as f32 * seconds) as usize;
            let mut samples = Vec::with_capacity(frames.saturating_mul(usize::from(channels)));
            for i in 0..frames {
                let v = 0.4 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin();
                for _ in 0..channels {
                    samples.push(v);
                }
            }
            let bytes = wavpcm::encode(
                &wavpcm::Audio {
                    sample_rate: rate,
                    channels,
                    samples,
                },
                wavpcm::SampleFormat::I16,
                1,
            )
            .unwrap();
            let path = self.0.join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            // Best effort: a leftover temp directory is harmless.
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Tick until nothing is queued or running, the way the window's clock
    /// does, with a real pause between ticks for the worker thread.
    fn run_queue(app: &mut MediaConvertApp) -> Vec<f32> {
        let mut seen = Vec::new();
        for _ in 0..20_000 {
            app.handle_event(&tick());
            if let Some(job) = app.jobs.iter().find(|j| j.status == JobStatus::Running) {
                seen.push(job.progress);
            }
            if !app
                .jobs
                .iter()
                .any(|j| matches!(j.status, JobStatus::Queued | JobStatus::Running))
            {
                return seen;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("the queue never finished");
    }

    fn profile(app: &MediaConvertApp, name: &str) -> usize {
        app.profiles
            .iter()
            .position(|p| p.name == name)
            .unwrap_or_else(|| panic!("no profile {name:?}"))
    }

    /// **A WAV is converted, end to end**: a real file in, a real file out,
    /// at the rate, channels and format asked for, the source untouched.
    #[test]
    fn a_wav_becomes_the_wav_the_profile_names() {
        let dir = Scratch::new("end-to-end");
        let source = dir.wav("voice.wav", 44_100, 2, 0.25);
        let before = std::fs::read(&source).unwrap();
        let mut app = MediaConvertApp::new();
        app.add_file(&source).unwrap();
        app.select_profile(profile(&app, "WAV, speech"));
        probe::click(&mut app, Target::ConvertAll);
        assert!(
            app.tick_interval().is_some(),
            "a running job keeps no clock"
        );
        run_queue(&mut app);
        let job = &app.jobs[0];
        assert_eq!(job.status, JobStatus::Completed, "{:?}", job.error_message);
        assert_eq!(
            job.output_path,
            dir.0.join("voice (2).wav"),
            "the source was not avoided"
        );
        let out = std::fs::read(&job.output_path).unwrap();
        let info = wavpcm::parse_header(&out).unwrap();
        assert_eq!((info.sample_rate, info.channels), (16_000, 1));
        assert_eq!(job.actual_size, Some(out.len() as u64));
        assert_eq!(
            std::fs::read(&source).unwrap(),
            before,
            "the source was written over"
        );
        assert_eq!(app.history.len(), 1);
        assert_eq!(app.tick_interval(), None, "a finished queue keeps a clock");
    }

    #[test]
    fn a_picture_becomes_a_bmp_in_the_folder_chosen() {
        let dir = Scratch::new("picture");
        let source = dir.0.join("photo.png");
        std::fs::write(
            &source,
            imagecodec::testing::png_rgba(8, 4, |x, y| [x as u8, y as u8, 7, 255]),
        )
        .unwrap();
        let out_dir = dir.0.join("out");
        std::fs::create_dir_all(&out_dir).unwrap();
        let mut app = MediaConvertApp::new();
        app.add_file(&source).unwrap();
        assert_eq!(
            app.sources[0].source_format, "8x4",
            "the picture's size was not read"
        );
        app.select_profile(profile(&app, "BMP picture"));
        app.picker_for = PickerFor::OutputFolder;
        app.picked(&out_dir);
        assert_eq!(app.output_dir.as_deref(), Some(out_dir.as_path()));
        app.handle_event(&key_ev(Key::Enter, true));
        run_queue(&mut app);
        let job = &app.jobs[0];
        assert_eq!(job.status, JobStatus::Completed, "{:?}", job.error_message);
        assert_eq!(job.output_path, out_dir.join("photo.bmp"));
        assert_eq!(&std::fs::read(&job.output_path).unwrap()[..2], b"BM");
    }

    /// Everything the profile cannot convert is refused before it is queued,
    /// and the reason is on screen.
    #[test]
    fn what_cannot_be_converted_is_never_queued() {
        let mut app = seeded();
        assert_eq!(
            app.profiles[app.selected_profile_idx].name,
            "WAV, CD quality"
        );
        app.handle_event(&key_ev(Key::Enter, true));
        let queued: Vec<&str> = app
            .jobs
            .iter()
            .map(|j| j.source.file_name.as_str())
            .collect();
        assert_eq!(queued, ["song.wav"]);
        assert!(
            app.status_line.contains("left out 2"),
            "{}",
            app.status_line
        );
        assert!(texts(&app).iter().any(|t| t.contains("left out 2")));
        // A video profile says why, and queues nothing.
        let video = app
            .profiles
            .iter()
            .position(|p| p.availability().is_err())
            .unwrap();
        app.select_profile(video);
        assert!(
            texts(&app).iter().any(|t| t.starts_with("Not available: ")),
            "an unavailable profile did not say so"
        );
        let before = app.jobs.len();
        app.queue_all();
        assert_eq!(app.jobs.len(), before);
    }

    /// The job that runs moves its bar, forwards only, and ends full.
    #[test]
    fn a_running_job_moves_its_progress_bar() {
        let dir = Scratch::new("progress");
        let source = dir.wav("long.wav", 48_000, 2, 6.0);
        let mut app = MediaConvertApp::new();
        app.add_file(&source).unwrap();
        app.select_profile(profile(&app, "WAV, speech"));
        app.handle_event(&key_ev(Key::Enter, true));
        let seen = run_queue(&mut app);
        assert!(
            seen.windows(2).all(|w| w[1] >= w[0]),
            "the bar went backwards: {seen:?}"
        );
        assert!(
            seen.iter().any(|p| *p > 0.0 && *p < 100.0),
            "there was never a frame with the bar part-way across"
        );
        assert!(
            (app.jobs[0].progress - 100.0).abs() < f32::EPSILON,
            "{}",
            app.jobs[0].progress
        );
    }

    /// A cancelled job stops before it writes anything.
    #[test]
    fn a_cancelled_job_writes_nothing() {
        let dir = Scratch::new("cancel");
        let source = dir.wav("long.wav", 48_000, 2, 6.0);
        let mut app = MediaConvertApp::new();
        app.add_file(&source).unwrap();
        app.select_profile(profile(&app, "WAV, speech"));
        app.handle_event(&key_ev(Key::Enter, true));
        let id = app.jobs[0].id;
        probe::click(&mut app, Target::CancelJob(id));
        assert!(
            app.jobs[0].stopping,
            "a running job did not say it is stopping"
        );
        run_queue(&mut app);
        assert_eq!(app.jobs[0].status, JobStatus::Cancelled);
        assert!(
            !app.jobs[0].output_path.exists(),
            "a cancelled job left a file"
        );
        assert!(app.history.is_empty());
    }

    /// One job at a time, and the next one starts when the last finishes.
    #[test]
    fn only_one_job_runs_at_a_time() {
        let dir = Scratch::new("one-at-a-time");
        let mut app = MediaConvertApp::new();
        for name in ["a.wav", "b.wav", "c.wav"] {
            app.add_file(&dir.wav(name, 22_050, 1, 0.1)).unwrap();
        }
        app.select_profile(profile(&app, "WAV, CD quality"));
        app.handle_event(&key_ev(Key::Enter, true));
        for _ in 0..20_000 {
            let running = app
                .jobs
                .iter()
                .filter(|j| j.status == JobStatus::Running)
                .count();
            assert!(running <= 1, "{running} jobs were running at once");
            app.handle_event(&tick());
            if app.jobs.iter().all(|j| j.status == JobStatus::Completed) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(app.jobs.iter().all(|j| j.status == JobStatus::Completed));
        let outputs: std::collections::BTreeSet<&PathBuf> =
            app.jobs.iter().map(|j| &j.output_path).collect();
        assert_eq!(outputs.len(), 3, "two jobs were given one output");
    }

    /// Two files of one name, from two folders, into one: two outputs, not
    /// one written over by the other.
    #[test]
    fn two_files_of_one_name_get_two_outputs() {
        let mut app = MediaConvertApp::new();
        app.add_source("/x/song.wav", "song.wav", 1, MediaCategory::Audio);
        app.add_source("/y/song.wav", "song.wav", 1, MediaCategory::Audio);
        app.output_dir = Some(PathBuf::from("/out"));
        app.queue_all();
        let outputs: Vec<&Path> = app.jobs.iter().map(|j| j.output_path.as_path()).collect();
        assert_eq!(
            outputs,
            [Path::new("/out/song.wav"), Path::new("/out/song (2).wav")]
        );
    }

    #[test]
    fn a_file_that_is_not_what_its_name_says_fails_and_says_why() {
        let dir = Scratch::new("liar");
        let fake = dir.0.join("fake.wav");
        std::fs::write(&fake, b"RIFF not really").unwrap();
        let mut app = MediaConvertApp::new();
        app.add_file(&fake).unwrap();
        app.handle_event(&key_ev(Key::Enter, true));
        run_queue(&mut app);
        let job = &app.jobs[0];
        assert_eq!(job.status, JobStatus::Failed);
        assert!(
            texts(&app)
                .iter()
                .any(|t| Some(t) == job.error_message.as_ref()),
            "the reason is not on screen"
        );
    }

    #[test]
    fn a_file_is_added_as_it_really_is() {
        let dir = Scratch::new("add");
        let wav = dir.wav("tone.wav", 8000, 1, 2.0);
        let mut app = MediaConvertApp::new();
        let id = app.add_file(&wav).unwrap();
        let src = app.find_source(id).unwrap();
        assert_eq!(src.file_size, std::fs::metadata(&wav).unwrap().len());
        assert!((src.duration_secs.unwrap() - 2.0).abs() < 1e-6);
        assert_eq!(src.source_format, "WAV 8000 Hz 1ch 16-bit");
        // Past the megabyte that is read for the header, the length is still
        // the file's: it read as 65 seconds.
        let long = dir.wav("long.wav", 8000, 1, 70.0);
        let id = app.add_file(&long).unwrap();
        let src = app.find_source(id).unwrap();
        assert!((src.duration_secs.unwrap() - 70.0).abs() < 1e-6);
        assert!(app.add_file(&wav).is_err(), "the same file was added twice");
        assert!(app.add_file(&dir.0.join("missing.wav")).is_err());
        std::fs::write(dir.0.join("notes.txt"), b"hi").unwrap();
        assert!(
            app.add_file(&dir.0.join("notes.txt")).is_err(),
            "a text file was taken for media"
        );
        std::fs::write(
            dir.0.join("b.png"),
            imagecodec::testing::png_rgba(2, 2, |_, _| [0, 0, 0, 255]),
        )
        .unwrap();
        let mut folder = MediaConvertApp::new();
        assert_eq!(
            folder.add_folder(&dir.0),
            Ok(3),
            "the folder's three media files were not all added"
        );
    }

    #[test]
    fn enter_queues_only_the_selected_file() {
        let mut app = three_wavs();
        app.handle_event(&press(Key::Down));
        assert!(app.selected_source.is_some());
        app.handle_event(&press(Key::Enter));
        assert_eq!(app.jobs.len(), 1, "one file selected, one job");
    }

    /// A queue with no cancel is a queue you have to wait out.
    #[test]
    fn ctrl_c_abandons_what_has_not_started() {
        let mut app = three_wavs();
        app.queue_all();
        app.start_next_job_fixture();
        app.handle_event(&key_ev(Key::C, true));
        let count = |s: JobStatus| app.jobs.iter().filter(|j| j.status == s).count();
        assert_eq!(
            count(JobStatus::Queued),
            0,
            "nothing should be left waiting"
        );
        assert_eq!(
            count(JobStatus::Running),
            1,
            "the one already running is not abandoned"
        );
        assert_eq!(count(JobStatus::Cancelled), 2);
    }

    #[test]
    fn ctrl_l_sweeps_up_the_finished_jobs() {
        let mut app = three_wavs();
        app.queue_all();
        for _ in 0..3 {
            app.start_next_job_fixture();
            let id = app
                .jobs
                .iter()
                .find(|j| j.status == JobStatus::Running)
                .unwrap()
                .id;
            app.complete_job(id, 10);
        }
        app.handle_event(&key_ev(Key::L, true));
        assert!(app.jobs.is_empty(), "the finished jobs should have gone");
        assert_eq!(app.history.len(), 3, "but the history keeps them");
    }

    #[test]
    fn the_arrows_walk_the_file_list_and_stop_at_the_ends() {
        let mut app = seeded();
        app.handle_event(&press(Key::Down));
        let first = app.selected_source;
        assert_eq!(first, app.sources.first().map(|s| s.id));
        app.handle_event(&press(Key::Up));
        assert_eq!(app.selected_source, first, "stopping, not wrapping");
        for _ in 0..10 {
            app.handle_event(&press(Key::Down));
        }
        assert_eq!(app.selected_source, app.sources.last().map(|s| s.id));
    }

    #[test]
    fn delete_removes_the_selected_file_or_all_of_them() {
        let mut app = seeded();
        let total = app.sources.len();
        app.handle_event(&press(Key::Down));
        app.handle_event(&press(Key::Delete));
        assert_eq!(app.sources.len(), total - 1);
        app.handle_event(&press(Key::Delete));
        assert!(app.sources.is_empty());
    }

    #[test]
    fn the_arrows_cycle_the_conversion_profile() {
        let mut app = seeded();
        app.active_panel = ActivePanel::Settings;
        assert!(app.profiles.len() > 1);
        let mut seen = vec![app.selected_profile_idx];
        for _ in 0..app.profiles.len() {
            app.handle_event(&press(Key::Right));
            seen.push(app.selected_profile_idx);
        }
        assert_eq!(seen.first(), seen.last(), "a full cycle should come round");
        let mut distinct = seen.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(
            distinct.len(),
            app.profiles.len(),
            "and visit each: {seen:?}"
        );
        app.handle_event(&press(Key::Left));
        assert_ne!(
            app.selected_profile_idx, seen[0],
            "Left should go the other way"
        );
    }

    /// Up and Down walk the settings; Left and Right change the one chosen.
    #[test]
    fn the_settings_are_walked_and_changed_by_the_keys() {
        let mut app = MediaConvertApp::new();
        app.active_panel = ActivePanel::Settings;
        app.handle_event(&press(Key::Down));
        app.handle_event(&press(Key::Down));
        assert_eq!(app.setting_row, SettingRow::SampleRate);
        let rate = app.audio_settings.sample_rate;
        app.handle_event(&press(Key::Right));
        assert!(app.audio_settings.sample_rate > rate);
        app.handle_event(&press(Key::Down));
        app.handle_event(&press(Key::Right));
        assert_eq!(app.audio_settings.channels, 1);
        for _ in 0..10 {
            app.handle_event(&press(Key::Down));
        }
        assert_eq!(
            app.setting_row,
            SettingRow::Naming,
            "Down ran past the last row"
        );
        app.handle_event(&press(Key::Right));
        assert_eq!(
            app.output_naming,
            OutputNaming::Suffix(String::from("-converted"))
        );
        // A profile without a row the keys are on moves them to the top.
        app.setting_row = SettingRow::SampleRate;
        app.select_profile(profile(&app, "BMP picture"));
        assert_eq!(app.setting_row, SettingRow::Profile);
    }

    #[test]
    fn the_number_keys_choose_the_quality_preset() {
        let mut app = seeded();
        app.handle_event(&press(Key::Num1));
        assert_eq!(app.quality_preset, QualityPreset::Low);
        app.handle_event(&press(Key::Num4));
        assert_eq!(app.quality_preset, QualityPreset::Lossless);
    }

    #[test]
    fn tab_moves_through_the_three_panels() {
        let mut app = seeded();
        let mut seen = vec![app.active_panel];
        for _ in 0..3 {
            app.handle_event(&press(Key::Tab));
            seen.push(app.active_panel);
        }
        assert_eq!(seen.first(), seen.last());
        for panel in [
            ActivePanel::SourceList,
            ActivePanel::Settings,
            ActivePanel::Queue,
        ] {
            assert!(seen.contains(&panel), "{panel:?} was skipped: {seen:?}");
        }
    }

    /// The queue stays on screen whichever panel has the keys: it vanished
    /// when Tab moved off it.
    #[test]
    fn the_queue_is_drawn_whichever_panel_has_the_keys() {
        let mut app = three_wavs();
        app.queue_all();
        app.active_panel = ActivePanel::Settings;
        assert!(probe::rect_of(&app, Target::CancelJob(app.jobs[0].id)).is_some());
    }

    #[test]
    fn the_layout_follows_the_window_it_is_given() {
        let mut app = seeded();
        let _ = App::render(&mut app, 1600.0, 900.0);
        assert!((app.window_width - 1600.0).abs() < 0.01);
        assert!((app.window_height - 900.0).abs() < 0.01);
    }

    #[test]
    fn the_title_says_how_much_is_left() {
        let mut app = three_wavs();
        assert_eq!(app.title(), "Media Converter");
        app.queue_all();
        let title = app.title();
        assert!(title.contains("3 left"), "got {title:?}");
        for _ in 0..3 {
            app.start_next_job_fixture();
            let id = app
                .jobs
                .iter()
                .find(|j| j.status == JobStatus::Running)
                .unwrap()
                .id;
            app.complete_job(id, 1);
        }
        assert_eq!(app.title(), "Media Converter", "and back when it is done");
    }

    // ── The pointer ─────────────────────────────────────────────────

    use guitk::probe::{self, Probe};

    impl Probe for MediaConvertApp {
        type Target = Target;
        type Outcome = EventResult;
        const SIZE: (f32, f32) = (1280.0, 800.0);

        fn draw(&self, _size: (f32, f32)) -> Frame<Target> {
            self.frame(self.window_width, self.window_height)
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

    #[test]
    fn every_control_answers_the_pointer() {
        let mut app = three_wavs();
        probe::click(&mut app, Target::AddFiles);
        assert!(app.picker.is_open() && app.picker_for == PickerFor::Files);
        app.picker.close();
        probe::click(&mut app, Target::AddFolder);
        assert_eq!(app.picker_for, PickerFor::Folder);
        app.picker.close();
        probe::click(&mut app, Target::ChooseOutput);
        assert_eq!(app.picker_for, PickerFor::OutputFolder);
        app.picker.close();
        assert!(
            probe::rect_of(&app, Target::OutputBeside).is_none(),
            "Beside sources is offered when outputs already go there"
        );
        app.output_dir = Some(PathBuf::from("/out"));
        probe::click(&mut app, Target::OutputBeside);
        assert_eq!(app.output_dir, None);
        let id = app.sources[1].id;
        probe::click(&mut app, Target::SourceRow(id));
        assert_eq!(app.selected_source, Some(id));
        probe::click(&mut app, Target::QueueList);
        assert_eq!(app.active_panel, ActivePanel::Queue);
        probe::click(&mut app, Target::Panel(ActivePanel::SourceList));
        assert_eq!(
            app.active_panel,
            ActivePanel::SourceList,
            "a press on the panel's heading"
        );
        probe::click(&mut app, Target::StepForward(SettingRow::Profile));
        assert_eq!(app.selected_profile_idx, 1);
        assert_eq!(app.active_panel, ActivePanel::Settings);
        probe::click(&mut app, Target::StepBack(SettingRow::Profile));
        assert_eq!(app.selected_profile_idx, 0);
        probe::click(&mut app, Target::Setting(SettingRow::Quality));
        assert_eq!(app.setting_row, SettingRow::Quality);
        probe::click(&mut app, Target::RemoveSource(id));
        assert!(app.find_source(id).is_none());
        app.queue_all();
        probe::click(&mut app, Target::CancelWaiting);
        assert!(app.jobs.iter().all(|j| j.status == JobStatus::Cancelled));
        probe::click(&mut app, Target::ClearFinished);
        assert!(app.jobs.is_empty());
        probe::click(&mut app, Target::Help);
        assert!(app.show_help);
        probe::click(&mut app, Target::HelpCard);
        assert!(!app.show_help);
    }

    /// A second press on a chosen file queues it.
    #[test]
    fn a_second_press_on_a_file_queues_it() {
        let mut app = three_wavs();
        let id = app.sources[0].id;
        probe::click(&mut app, Target::SourceRow(id));
        assert!(app.jobs.is_empty());
        probe::click(&mut app, Target::SourceRow(id));
        assert_eq!(app.jobs.len(), 1);
    }

    #[test]
    fn the_lists_scroll() {
        let mut app = MediaConvertApp::new();
        for i in 0..60 {
            app.add_source(
                &format!("/m/{i}.wav"),
                &format!("{i}.wav"),
                1,
                MediaCategory::Audio,
            );
        }
        let (_, rows) = app.source_pane();
        assert!(rows < 60);
        assert_eq!(
            probe::scroll_at_point(&mut app, Target::SourceList, -3.0),
            EventResult::Consumed
        );
        assert!(app.source_scroll > 0);
        for _ in 0..40 {
            probe::scroll_at_point(&mut app, Target::SourceList, -3.0);
        }
        assert_eq!(app.source_scroll, 60 - rows);
        app.queue_all();
        let (_, queue_rows) = app.queue_pane();
        for _ in 0..40 {
            probe::scroll_at_point(&mut app, Target::QueueList, -3.0);
        }
        assert_eq!(app.queue_scroll, 60 - queue_rows);
        // The chosen file follows the keys onto the screen.
        app.active_panel = ActivePanel::SourceList;
        app.selected_source = app.sources.first().map(|s| s.id);
        app.handle_event(&press(Key::Down));
        assert!(app.source_scroll <= 1);
    }

    /// The wheel can leave the chosen row behind, and a frame does not undo
    /// it; the arrows bring the row back.
    ///
    /// Every frame scrolled each list back to its chosen row, so with a file
    /// chosen the source list could not be scrolled at all.
    #[test]
    fn the_wheel_can_leave_the_chosen_row_behind() {
        let mut app = MediaConvertApp::new();
        for i in 0..60 {
            app.add_source(
                &format!("/m/{i}.wav"),
                &format!("{i}.wav"),
                1,
                MediaCategory::Audio,
            );
        }
        app.active_panel = ActivePanel::SourceList;
        app.handle_event(&press(Key::Down));
        assert_eq!(app.selected_source, app.sources.first().map(|s| s.id));
        for _ in 0..10 {
            probe::scroll_at_point(&mut app, Target::SourceList, -3.0);
        }
        let scrolled = app.source_scroll;
        assert!(scrolled > 1);
        app.render(app.window_width, app.window_height);
        assert_eq!(
            app.source_scroll, scrolled,
            "a frame scrolled back to the chosen row"
        );
        app.handle_event(&press(Key::Num2));
        assert_eq!(
            app.source_scroll, scrolled,
            "a key that chose nothing scrolled back"
        );
        app.handle_event(&press(Key::Down));
        assert!(
            app.source_scroll <= 1,
            "the arrow brings the chosen row back"
        );
        // At the end of the list an arrow moves nothing, and still brings the
        // chosen row back.
        app.selected_source = app.sources.last().map(|s| s.id);
        for _ in 0..40 {
            probe::scroll_at_point(&mut app, Target::SourceList, 3.0);
        }
        assert_eq!(app.source_scroll, 0);
        app.handle_event(&press(Key::Down));
        assert_eq!(app.selected_source, app.sources.last().map(|s| s.id));
        let (_, rows) = app.source_pane();
        assert_eq!(app.source_scroll, 60 - rows);
    }

    /// A job is chosen by the arrows or a press, and Delete acts on that one:
    /// cancelled while it waits, taken off the list once it has finished.
    ///
    /// Delete cancelled the first job that could be cancelled, whichever was
    /// meant, and nothing chose a job at all.
    #[test]
    fn a_job_is_chosen_and_delete_acts_on_it() {
        let mut app = three_wavs();
        app.queue_all();
        let ids: Vec<u64> = app.jobs.iter().map(|j| j.id).collect();
        assert_eq!(ids.len(), 3);
        app.active_panel = ActivePanel::Queue;
        app.handle_event(&press(Key::Down));
        app.handle_event(&press(Key::Down));
        assert_eq!(app.selected_job, Some(ids[1]));
        app.handle_event(&press(Key::Delete));
        assert_eq!(app.jobs[1].status, JobStatus::Cancelled, "the chosen job");
        assert_eq!(app.jobs[0].status, JobStatus::Queued, "not the first");
        app.handle_event(&press(Key::Delete));
        assert_eq!(app.jobs.len(), 2, "a finished job, chosen, leaves the list");
        assert!(app.jobs.iter().all(|j| j.id != ids[1]));
        assert_eq!(app.selected_job, None);
        // A press chooses a row, and gives the queue the keys.
        app.active_panel = ActivePanel::SourceList;
        probe::click(&mut app, Target::QueueRow(ids[2]));
        assert_eq!(app.selected_job, Some(ids[2]));
        assert_eq!(app.active_panel, ActivePanel::Queue);
        // The row's own Cancel button still takes its press.
        probe::click(&mut app, Target::CancelJob(ids[2]));
        let third = app.jobs.iter().find(|j| j.id == ids[2]).unwrap();
        assert_eq!(third.status, JobStatus::Cancelled);
    }

    /// Where outputs go is chosen by key as well as by the panel's buttons,
    /// which were the only route.
    #[test]
    fn where_outputs_go_is_chosen_by_key_too() {
        let mut app = MediaConvertApp::new();
        app.handle_event(&press(Key::O));
        assert!(app.picker.is_open());
        assert_eq!(app.picker_for, PickerFor::OutputFolder);
        app.picker.close();
        app.output_dir = Some(PathBuf::from("/out"));
        app.handle_event(&press(Key::B));
        assert_eq!(app.output_dir, None);
        assert_eq!(app.status_line, "Outputs go beside each source");
    }

    #[test]
    fn the_list_of_keys_is_modal_to_the_pointer() {
        let mut app = three_wavs();
        let id = app.sources[0].id;
        let row = probe::rect_of(&app, Target::SourceRow(id)).unwrap();
        app.show_help = true;
        app.handle_event(&Event::Mouse(MouseEvent {
            x: row.x + 4.0,
            y: row.y + 4.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        }));
        assert!(!app.show_help);
        assert_eq!(app.selected_source, None, "the press went through the list");
    }

    #[test]
    fn a_pattern_name_takes_todays_date_and_keeps_the_stem() {
        let naming = OutputNaming::Pattern(String::from("{name}-{date}-{name}"));
        let out = naming.apply_os(OsStr::new("take.wav"), "wav", 0);
        let today = today_yyyymmdd();
        assert_eq!(out, OsString::from(format!("take-{today}-take")));
        assert_ne!(today, "20260518", "the date is the old constant");
        assert_eq!(
            OutputNaming::Suffix(String::from("-x")).apply_os(OsStr::new("a.b.png"), "bmp", 0),
            OsString::from("a.b-x.bmp")
        );
    }

    // --- Format detection ---

    #[test]
    fn test_audio_format_from_extension() {
        assert_eq!(AudioFormat::from_extension("mp3"), Some(AudioFormat::Mp3));
        assert_eq!(AudioFormat::from_extension("FLAC"), Some(AudioFormat::Flac));
        assert_eq!(AudioFormat::from_extension("xyz"), None);
    }

    #[test]
    fn test_video_format_from_extension() {
        assert_eq!(VideoFormat::from_extension("mp4"), Some(VideoFormat::Mp4));
        assert_eq!(VideoFormat::from_extension("webm"), Some(VideoFormat::WebM));
        assert_eq!(VideoFormat::from_extension("xyz"), None);
    }

    #[test]
    fn test_image_format_from_extension() {
        assert_eq!(ImageFormat::from_extension("jpg"), Some(ImageFormat::Jpeg));
        assert_eq!(ImageFormat::from_extension("JPEG"), Some(ImageFormat::Jpeg));
        assert_eq!(ImageFormat::from_extension("webp"), Some(ImageFormat::WebP));
    }

    #[test]
    fn test_audio_lossless() {
        assert!(AudioFormat::Flac.is_lossless());
        assert!(AudioFormat::Wav.is_lossless());
        assert!(!AudioFormat::Mp3.is_lossless());
    }

    #[test]
    fn test_image_supports_quality() {
        assert!(ImageFormat::Jpeg.supports_quality());
        assert!(ImageFormat::WebP.supports_quality());
        assert!(!ImageFormat::Png.supports_quality());
    }

    // --- Quality presets ---

    #[test]
    fn test_quality_preset_audio_bitrate() {
        assert_eq!(QualityPreset::Low.audio_bitrate_kbps(), 96);
        assert_eq!(QualityPreset::High.audio_bitrate_kbps(), 320);
        assert_eq!(QualityPreset::Lossless.audio_bitrate_kbps(), 0);
    }

    #[test]
    fn test_quality_preset_image_quality() {
        assert_eq!(QualityPreset::Low.image_quality(), 60);
        assert_eq!(QualityPreset::Lossless.image_quality(), 100);
    }

    #[test]
    fn test_quality_preset_video_bitrate() {
        let res = VideoResolution::new(1920, 1080);
        let low = QualityPreset::Low.video_bitrate_kbps(&res);
        let high = QualityPreset::High.video_bitrate_kbps(&res);
        assert!(low < high);
    }

    // --- VideoResolution ---

    #[test]
    fn test_video_resolution_label() {
        let r = VideoResolution::new(1920, 1080);
        assert!(r.label().contains("1080p"));
    }

    #[test]
    fn test_video_resolution_aspect_ratio() {
        let r = VideoResolution::new(1920, 1080);
        assert_eq!(r.aspect_ratio(), "16:9");
    }

    #[test]
    fn test_gcd() {
        assert_eq!(gcd_u32(1920, 1080), 120);
        assert_eq!(gcd_u32(0, 5), 5);
    }

    // --- Settings ---

    #[test]
    fn test_audio_settings_from_preset() {
        let s = AudioSettings::from_preset(QualityPreset::Lossless);
        assert_eq!(s.bit_depth, 24);
        assert_eq!(s.sample_rate, 44100);
    }

    #[test]
    fn test_video_settings_from_preset() {
        let s = VideoSettings::from_preset(QualityPreset::Low);
        let Some(res) = s.resolution else {
            panic!("Low preset must have a resolution");
        };
        assert_eq!(res.width, 854);
    }

    #[test]
    fn test_image_settings_from_preset() {
        let s = ImageSettings::from_preset(QualityPreset::Low);
        assert_eq!(s.quality, 60);
        assert!(s.strip_metadata);
    }

    #[test]
    fn test_settings_summary() {
        let audio = AudioSettings::default();
        assert!(audio.summary().contains("AAC"));

        let video = VideoSettings::default();
        assert!(video.summary().contains("H.264"));

        let image = ImageSettings::default();
        assert!(image.summary().contains("90%"));
    }

    // --- Output naming ---

    #[test]
    fn test_output_naming_keep_original() {
        let n = OutputNaming::KeepOriginal;
        assert_eq!(n.apply("video.mkv", "mp4", 0), "video.mp4");
    }

    #[test]
    fn test_output_naming_suffix() {
        let n = OutputNaming::Suffix("_converted".to_owned());
        assert_eq!(n.apply("song.flac", "mp3", 0), "song_converted.mp3");
    }

    #[test]
    fn test_output_naming_prefix() {
        let n = OutputNaming::Prefix("out_".to_owned());
        assert_eq!(n.apply("photo.png", "jpg", 0), "out_photo.jpg");
    }

    #[test]
    fn test_output_naming_pattern() {
        let n = OutputNaming::Pattern("{name}_{index}.{ext}".to_owned());
        assert_eq!(n.apply("file.avi", "mp4", 3), "file_3.mp4");
    }

    // --- SourceFile ---

    #[test]
    fn test_source_file_human_size() {
        let s = SourceFile::new(1, "/a", "a.mp3", 5_242_880, MediaCategory::Audio);
        assert_eq!(s.human_size(), "5.0 MiB");
    }

    #[test]
    fn test_source_file_duration() {
        let s = SourceFile::new(1, "/a", "a.mp3", 1000, MediaCategory::Audio).with_duration(185.0);
        assert_eq!(s.duration_str(), "3:05");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(61.0), "1:01");
        assert_eq!(format_duration(3661.0), "1:01:01");
        assert_eq!(format_duration(0.0), "0:00");
    }

    // --- Detect category ---

    #[test]
    fn test_detect_category() {
        assert_eq!(
            MediaConvertApp::detect_category("song.mp3"),
            Some(MediaCategory::Audio)
        );
        assert_eq!(
            MediaConvertApp::detect_category("clip.mp4"),
            Some(MediaCategory::Video)
        );
        assert_eq!(
            MediaConvertApp::detect_category("photo.jpg"),
            Some(MediaCategory::Image)
        );
        assert_eq!(MediaConvertApp::detect_category("readme.txt"), None);
    }

    // --- ConversionJob ---

    #[test]
    fn test_job_lifecycle() {
        let src =
            SourceFile::new(1, "/a", "test.flac", 1000, MediaCategory::Audio).with_format("FLAC");
        let mut job = ConversionJob::new(
            1,
            src,
            OutputFormat::Audio(AudioFormat::Mp3),
            "test.mp3".to_owned(),
            PathBuf::from("/out/x"),
            engine::Recipe::Bmp {
                max_width: None,
                max_height: None,
            },
        );
        assert_eq!(job.status, JobStatus::Queued);

        job.start(100);
        assert_eq!(job.status, JobStatus::Running);
        assert_eq!(job.started_at, Some(100));

        job.complete(200, 500);
        assert_eq!(job.status, JobStatus::Completed);
        assert_eq!(job.actual_size, Some(500));
    }

    #[test]
    fn test_job_fail() {
        let src = SourceFile::new(1, "/a", "test.avi", 1000, MediaCategory::Video);
        let mut job = ConversionJob::new(
            1,
            src,
            OutputFormat::Video(VideoFormat::Mp4),
            "test.mp4".to_owned(),
            PathBuf::from("/out/x"),
            engine::Recipe::Bmp {
                max_width: None,
                max_height: None,
            },
        );
        job.start(100);
        job.fail(200, "Codec not supported");
        assert_eq!(job.status, JobStatus::Failed);
        assert!(job.error_message.is_some());
    }

    #[test]
    fn test_job_cancel() {
        let src = SourceFile::new(1, "/a", "test.wav", 1000, MediaCategory::Audio);
        let mut job = ConversionJob::new(
            1,
            src,
            OutputFormat::Audio(AudioFormat::Ogg),
            "test.ogg".to_owned(),
            PathBuf::from("/out/x"),
            engine::Recipe::Bmp {
                max_width: None,
                max_height: None,
            },
        );
        job.cancel();
        assert_eq!(job.status, JobStatus::Cancelled);
    }

    #[test]
    fn test_job_conversion_label() {
        let src =
            SourceFile::new(1, "/a", "test.flac", 1000, MediaCategory::Audio).with_format("FLAC");
        let job = ConversionJob::new(
            1,
            src,
            OutputFormat::Audio(AudioFormat::Mp3),
            "test.mp3".to_owned(),
            PathBuf::from("/out/x"),
            engine::Recipe::Bmp {
                max_width: None,
                max_height: None,
            },
        );
        assert_eq!(job.conversion_label(), "FLAC -> MP3");
    }

    // --- History ---

    #[test]
    fn test_history_compression_ratio() {
        let entry = HistoryEntry {
            source_path: PathBuf::from("/a"),
            output_path: PathBuf::from("/b"),
            conversion_type: "test".to_owned(),
            source_size: 1000,
            output_size: 250,
            timestamp: 1,
            duration_secs: 1.0,
            success: true,
        };
        assert!((entry.compression_ratio() - 0.25).abs() < f64::EPSILON);
        assert_eq!(entry.space_saved(), 750);
    }

    // --- App tests ---

    #[test]
    fn test_app_add_source() {
        let mut app = MediaConvertApp::new();
        let id = app.add_source("/a", "song.mp3", 5000, MediaCategory::Audio);
        assert_eq!(app.sources.len(), 1);
        assert!(app.find_source(id).is_some());
    }

    #[test]
    fn test_app_remove_source() {
        let mut app = MediaConvertApp::new();
        let id = app.add_source("/a", "song.mp3", 5000, MediaCategory::Audio);
        assert!(app.remove_source(id));
        assert!(app.sources.is_empty());
    }

    #[test]
    fn test_app_clear_sources() {
        let mut app = MediaConvertApp::new();
        app.add_source("/a", "a", 100, MediaCategory::Audio);
        app.add_source("/b", "b", 200, MediaCategory::Video);
        app.clear_sources();
        assert!(app.sources.is_empty());
    }

    #[test]
    fn test_app_queue_all() {
        let mut app = MediaConvertApp::new();
        app.add_source("/a/song.wav", "song.wav", 1000, MediaCategory::Audio);
        app.add_source("/b/clip.avi", "clip.avi", 2000, MediaCategory::Video);
        let queued = app.queue_all();
        assert_eq!(queued, 1, "the video was queued for a WAV profile");
        assert_eq!(app.jobs.len(), 1);
    }

    #[test]
    fn test_app_queue_single() {
        let mut app = MediaConvertApp::new();
        let id = app.add_source("/a/song.wav", "song.wav", 1000, MediaCategory::Audio);
        let job_id = app.queue_source(id);
        assert!(job_id.is_some());
        assert_eq!(app.jobs.len(), 1);
        let flac = app.add_source("/a/song.flac", "song.flac", 1000, MediaCategory::Audio);
        assert!(
            app.queue_source(flac).is_none(),
            "a FLAC was queued with no FLAC decoder"
        );
        assert!(app.status_line.contains("song.flac"), "{}", app.status_line);
    }

    #[test]
    fn test_app_start_and_complete() {
        let mut app = MediaConvertApp::new();
        app.add_source("/a/test.wav", "test.wav", 10000, MediaCategory::Audio);
        app.queue_all();
        assert!(app.start_next_job_fixture());
        let Some(job_id) = app.jobs.first().map(|j| j.id) else {
            panic!("expected at least one job");
        };
        assert!(app.complete_job(job_id, 5000));
        assert_eq!(app.history.len(), 1);
    }

    #[test]
    fn test_app_fail_job() {
        let mut app = MediaConvertApp::new();
        app.add_source("/a/test.wav", "test.wav", 10000, MediaCategory::Audio);
        app.queue_all();
        app.start_next_job_fixture();
        let Some(job_id) = app.jobs.first().map(|j| j.id) else {
            panic!("expected at least one job");
        };
        assert!(app.fail_job(job_id, "Test error"));
    }

    #[test]
    fn test_app_cancel_all_queued() {
        let mut app = three_wavs();
        app.queue_all();
        let cancelled = app.cancel_all_queued();
        assert_eq!(cancelled, 3);
    }

    #[test]
    fn test_app_clear_finished() {
        let mut app = MediaConvertApp::new();
        app.add_source("/a/a.wav", "a.wav", 100, MediaCategory::Audio);
        app.queue_all();
        app.start_next_job_fixture();
        let Some(job_id) = app.jobs.first().map(|j| j.id) else {
            panic!("expected at least one job");
        };
        app.complete_job(job_id, 50);
        let cleared = app.clear_finished_jobs();
        assert_eq!(cleared, 1);
        assert!(app.jobs.is_empty());
    }

    #[test]
    fn test_app_select_profile() {
        let mut app = MediaConvertApp::new();
        app.select_profile(0);
        assert_eq!(app.selected_profile_idx, 0);
    }

    #[test]
    fn test_app_set_quality_preset() {
        let mut app = MediaConvertApp::new();
        app.set_quality_preset(QualityPreset::High);
        assert_eq!(app.quality_preset, QualityPreset::High);
    }

    #[test]
    fn test_app_queue_stats() {
        let mut app = MediaConvertApp::new();
        app.add_source("/a/a.wav", "a.wav", 100, MediaCategory::Audio);
        app.add_source("/b/b.wav", "b.wav", 200, MediaCategory::Audio);
        app.queue_all();
        app.start_next_job_fixture();
        let stats = app.queue_stats();
        assert_eq!(stats.total_jobs, 2);
        assert_eq!(stats.running, 1);
        assert_eq!(stats.queued, 1);
    }

    #[test]
    fn test_app_history_stats() {
        let mut app = MediaConvertApp::new();
        app.history.push(HistoryEntry {
            source_path: PathBuf::from("/a"),
            output_path: PathBuf::from("/b"),
            conversion_type: "test".to_owned(),
            source_size: 1000,
            output_size: 500,
            timestamp: 1,
            duration_secs: 1.0,
            success: true,
        });
        let stats = app.history_stats();
        assert_eq!(stats.total, 1);
        assert_eq!(stats.successful, 1);
        assert_eq!(stats.total_space_saved, 500);
    }

    /// The settings panel shows the settings the chosen profile uses, and
    /// for a profile this build cannot carry out, why.
    #[test]
    fn the_settings_panel_shows_what_applies_and_what_cannot_be_done() {
        let mut app = MediaConvertApp::new();
        let drawn = texts(&app);
        for label in ["SAMPLE RATE", "CHANNELS", "SAMPLES"] {
            assert!(
                drawn.iter().any(|t| t == label),
                "{label} is not drawn for a WAV profile"
            );
        }
        app.select_profile(profile(&app, "BMP picture"));
        let drawn = texts(&app);
        assert!(drawn.iter().any(|t| t == "LARGEST WIDTH"));
        assert!(
            !drawn.iter().any(|t| t == "SAMPLE RATE"),
            "a WAV setting drawn for a picture"
        );
        let video = app
            .profiles
            .iter()
            .position(|p| p.name == "Web Optimized (Video)")
            .unwrap();
        app.select_profile(video);
        assert!(
            texts(&app)
                .iter()
                .any(|t| t.starts_with("Not available: ") && t.contains("video")),
            "a video profile did not say it cannot be carried out"
        );
    }

    #[test]
    fn test_render_produces_commands() {
        let mut app = MediaConvertApp::new();
        app.add_source("/a", "song.mp3", 5000, MediaCategory::Audio);
        let cmds = app.render_commands(1280.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_profiles_exist() {
        let profiles = ConversionProfile::builtin_profiles();
        assert_eq!(profiles.len(), 11);
        let available: Vec<&str> = profiles
            .iter()
            .filter(|p| p.availability().is_ok())
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(
            available,
            [
                "WAV, CD quality",
                "WAV, studio",
                "WAV, speech",
                "BMP picture",
                "BMP, screen size"
            ],
            "the profiles this build can carry out are not the first ones"
        );
    }

    #[test]
    fn test_crop_settings() {
        let crop = CropSettings::new(10, 20, 30, 40);
        assert_eq!(crop.left, 10);
        assert_eq!(crop.bottom, 40);
    }

    #[test]
    fn test_output_format_category() {
        let af = OutputFormat::Audio(AudioFormat::Mp3);
        assert_eq!(af.category(), MediaCategory::Audio);
        let vf = OutputFormat::Video(VideoFormat::Mp4);
        assert_eq!(vf.category(), MediaCategory::Video);
        let imgf = OutputFormat::Image(ImageFormat::Jpeg);
        assert_eq!(imgf.category(), MediaCategory::Image);
    }

    #[test]
    fn test_human_file_size() {
        assert_eq!(human_file_size(500), "500 B");
        assert_eq!(human_file_size(1024), "1.0 KiB");
        assert_eq!(human_file_size(1048576), "1.0 MiB");
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

        fn fills(app: &mut MediaConvertApp) -> Vec<Color> {
            app.render(1000.0, 700.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let mut app = MediaConvertApp::new();

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
}
