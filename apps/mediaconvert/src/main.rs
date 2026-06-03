//! `OurOS` Media Converter
//!
//! A batch media format conversion tool with:
//! - Audio format support: WAV, MP3, FLAC, AAC, OGG, WMA, AIFF, ALAC, Opus
//! - Video format support: MP4, MKV, AVI, `WebM`, MOV, WMV, FLV
//! - Image format support: JPEG, PNG, BMP, GIF, TIFF, WebP, HEIC, ICO, SVG
//! - Codec selection for video (H.264, H.265, VP9, AV1) and audio (AAC, MP3, Opus, FLAC)
//! - Quality/bitrate presets: low, medium, high, lossless
//! - Batch conversion queue with progress tracking
//! - Preset profiles (Web optimized, Archive, Mobile, etc.)
//! - Audio-specific: sample rate, channels, bit depth adjustment
//! - Video-specific: resolution scaling, framerate, aspect ratio, crop
//! - Image-specific: resize, quality, strip metadata
//! - Output naming templates (original, suffix, prefix, custom pattern)
//! - Conversion history log
//! - Multi-panel UI: source list, settings panel, queue
//!
//! Uses the guitk library for UI rendering.

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

use guitk::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha theme
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);

// ============================================================================
// Layout constants
// ============================================================================

const SIDEBAR_WIDTH: f32 = 300.0;
const SETTINGS_PANEL_WIDTH: f32 = 280.0;
const TOOLBAR_HEIGHT: f32 = 40.0;
const STATUS_BAR_HEIGHT: f32 = 24.0;
const ITEM_HEIGHT: f32 = 32.0;
const CORNER_RADIUS: f32 = 4.0;

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

    /// All built-in profiles.
    pub fn builtin_profiles() -> Vec<Self> {
        vec![
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
                .replace("{date}", "20260518"),
        }
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
    pub path: String,
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
            path: path.to_owned(),
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
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
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

    pub fn color(self) -> Color {
        match self {
            Self::Queued => OVERLAY0,
            Self::Running => BLUE,
            Self::Completed => GREEN,
            Self::Failed => RED,
            Self::Cancelled => YELLOW,
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
    pub output_path: String,
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
        output_dir: &str,
    ) -> Self {
        let output_path = format!("{output_dir}/{output_name}");
        Self {
            id,
            source,
            output_format,
            output_name,
            output_path,
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
    pub source_path: String,
    pub output_path: String,
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

/// The media converter application.
pub struct MediaConvertApp {
    pub sources: Vec<SourceFile>,
    pub jobs: Vec<ConversionJob>,
    pub history: Vec<HistoryEntry>,
    pub profiles: Vec<ConversionProfile>,
    pub selected_source: Option<u64>,
    pub selected_profile_idx: usize,
    pub output_naming: OutputNaming,
    pub output_dir: String,
    pub quality_preset: QualityPreset,
    pub active_panel: ActivePanel,
    pub audio_settings: AudioSettings,
    pub video_settings: VideoSettings,
    pub image_settings: ImageSettings,
    pub show_queue: bool,
    pub window_width: f32,
    pub window_height: f32,
    id_gen: IdGen,
    timestamp: u64,
}

impl Default for MediaConvertApp {
    fn default() -> Self {
        Self::new()
    }
}

impl MediaConvertApp {
    pub fn new() -> Self {
        Self {
            sources: Vec::new(),
            jobs: Vec::new(),
            history: Vec::new(),
            profiles: ConversionProfile::builtin_profiles(),
            selected_source: None,
            selected_profile_idx: 0,
            output_naming: OutputNaming::KeepOriginal,
            output_dir: "/home/converted".to_owned(),
            quality_preset: QualityPreset::Medium,
            active_panel: ActivePanel::SourceList,
            audio_settings: AudioSettings::default(),
            video_settings: VideoSettings::default(),
            image_settings: ImageSettings::default(),
            show_queue: true,
            window_width: 1280.0,
            window_height: 800.0,
            id_gen: IdGen::new(1),
            timestamp: 1000,
        }
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
        }
    }

    /// Set quality preset and update settings.
    pub fn set_quality_preset(&mut self, preset: QualityPreset) {
        self.quality_preset = preset;
        self.audio_settings = AudioSettings::from_preset(preset);
        self.video_settings = VideoSettings::from_preset(preset);
        self.image_settings = ImageSettings::from_preset(preset);
    }

    // -----------------------------------------------------------------------
    // Job queue
    // -----------------------------------------------------------------------

    /// Queue all sources for conversion with current settings.
    pub fn queue_all(&mut self) -> usize {
        let sources: Vec<SourceFile> = self.sources.clone();
        let output_format = self
            .profiles
            .get(self.selected_profile_idx)
            .map_or(OutputFormat::Audio(AudioFormat::Mp3), |p| {
                p.output_format.clone()
            });

        let mut count = 0usize;
        for (idx, src) in sources.iter().enumerate() {
            let out_name = self
                .output_naming
                .apply(&src.file_name, output_format.extension(), idx);
            let id = self.id_gen.next_id();
            self.jobs.push(ConversionJob::new(
                id,
                src.clone(),
                output_format.clone(),
                out_name,
                &self.output_dir,
            ));
            count = count.saturating_add(1);
        }
        count
    }

    /// Queue a single source.
    pub fn queue_source(&mut self, source_id: u64) -> Option<u64> {
        let src = self.find_source(source_id)?.clone();
        let output_format = self
            .profiles
            .get(self.selected_profile_idx)
            .map_or(OutputFormat::Audio(AudioFormat::Mp3), |p| {
                p.output_format.clone()
            });
        let out_name =
            self.output_naming
                .apply(&src.file_name, output_format.extension(), self.jobs.len());
        let id = self.id_gen.next_id();
        self.jobs.push(ConversionJob::new(
            id,
            src,
            output_format,
            out_name,
            &self.output_dir,
        ));
        Some(id)
    }

    /// Start the next queued job (simulated).
    pub fn start_next_job(&mut self) -> bool {
        let ts = self.tick();
        if let Some(job) = self.jobs.iter_mut().find(|j| j.status == JobStatus::Queued) {
            job.start(ts);
            true
        } else {
            false
        }
    }

    /// Complete a running job (simulated).
    pub fn complete_job(&mut self, job_id: u64, output_size: u64) -> bool {
        let ts = self.tick();
        if let Some(job) = self
            .jobs
            .iter_mut()
            .find(|j| j.id == job_id && j.status == JobStatus::Running)
        {
            job.complete(ts, output_size);
            // Add to history
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
            true
        } else {
            false
        }
    }

    /// Cancel a queued or running job.
    pub fn cancel_job(&mut self, job_id: u64) -> bool {
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
        let queued = self
            .jobs
            .iter()
            .filter(|j| j.status == JobStatus::Queued)
            .count();
        let running = self
            .jobs
            .iter()
            .filter(|j| j.status == JobStatus::Running)
            .count();
        let completed = self
            .jobs
            .iter()
            .filter(|j| j.status == JobStatus::Completed)
            .count();
        let failed = self
            .jobs
            .iter()
            .filter(|j| j.status == JobStatus::Failed)
            .count();
        let total_source: u64 = self.jobs.iter().map(|j| j.source.file_size).sum();
        let total_output: u64 = self.jobs.iter().filter_map(|j| j.actual_size).sum();

        QueueStats {
            queued,
            running,
            completed,
            failed,
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

    pub fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_toolbar(&mut cmds, width);
        self.render_status_bar(&mut cmds, width, height);

        let content_y = TOOLBAR_HEIGHT;
        let content_h = height - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT;

        // Source list (left)
        self.render_source_list(&mut cmds, content_y, content_h);

        // Settings panel (middle)
        let settings_x = SIDEBAR_WIDTH;
        self.render_settings_panel(
            &mut cmds,
            settings_x,
            content_y,
            SETTINGS_PANEL_WIDTH,
            content_h,
        );

        // Queue (right)
        let queue_x = SIDEBAR_WIDTH + SETTINGS_PANEL_WIDTH;
        let queue_w = width - queue_x;
        if self.show_queue {
            self.render_queue_panel(&mut cmds, queue_x, content_y, queue_w, content_h);
        }

        cmds
    }

    fn render_toolbar(&self, cmds: &mut Vec<RenderCommand>, width: f32) {
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height: TOOLBAR_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: 12.0,
            text: "Media Converter".to_owned(),
            color: BLUE,
            font_size: 15.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(160.0),
        });

        // Profile selector
        let profile_name = self
            .profiles
            .get(self.selected_profile_idx)
            .map_or("None", |p| &p.name);
        cmds.push(RenderCommand::FillRect {
            x: 180.0,
            y: 8.0,
            width: 200.0,
            height: 24.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: 188.0,
            y: 14.0,
            text: format!("Profile: {profile_name}"),
            color: TEXT,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(190.0),
        });

        // Quality preset
        let qual_label = format!("Quality: {}", self.quality_preset.label());
        cmds.push(RenderCommand::FillRect {
            x: 392.0,
            y: 8.0,
            width: 120.0,
            height: 24.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: 400.0,
            y: 14.0,
            text: qual_label,
            color: TEXT,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(110.0),
        });

        // Convert button
        let convert_x = width - 120.0;
        cmds.push(RenderCommand::FillRect {
            x: convert_x,
            y: 8.0,
            width: 100.0,
            height: 24.0,
            color: GREEN,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: convert_x + 16.0,
            y: 14.0,
            text: "Convert All".to_owned(),
            color: CRUST,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(80.0),
        });

        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: TOOLBAR_HEIGHT,
            x2: width,
            y2: TOOLBAR_HEIGHT,
            color: SURFACE0,
            width: 1.0,
        });
    }

    fn render_status_bar(&self, cmds: &mut Vec<RenderCommand>, width: f32, height: f32) {
        let bar_y = height - STATUS_BAR_HEIGHT;
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: bar_y,
            width,
            height: STATUS_BAR_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        let stats = self.queue_stats();
        let status = format!(
            "{} sources  |  {} queued  |  {} running  |  {} completed  |  {} failed",
            self.sources.len(),
            stats.queued,
            stats.running,
            stats.completed,
            stats.failed,
        );
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: bar_y + 6.0,
            text: status,
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 24.0),
        });
    }

    fn render_source_list(&self, cmds: &mut Vec<RenderCommand>, y: f32, height: f32) {
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: SIDEBAR_WIDTH,
            height,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Line {
            x1: SIDEBAR_WIDTH,
            y1: y,
            x2: SIDEBAR_WIDTH,
            y2: y + height,
            color: SURFACE0,
            width: 1.0,
        });

        // Header
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: y + 10.0,
            text: format!("SOURCE FILES ({})", self.sources.len()),
            color: OVERLAY0,
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(SIDEBAR_WIDTH - 24.0),
        });

        let mut cy = y + 32.0;
        for src in &self.sources {
            if cy > y + height {
                break;
            }

            let is_selected = self.selected_source == Some(src.id);
            if is_selected {
                cmds.push(RenderCommand::FillRect {
                    x: 4.0,
                    y: cy,
                    width: SIDEBAR_WIDTH - 8.0,
                    height: ITEM_HEIGHT,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(CORNER_RADIUS),
                });
            }

            let cat_color = match src.category {
                MediaCategory::Audio => TEAL,
                MediaCategory::Video => MAUVE,
                MediaCategory::Image => PEACH,
            };

            // Category dot
            cmds.push(RenderCommand::FillRect {
                x: 12.0,
                y: cy + 11.0,
                width: 8.0,
                height: 8.0,
                color: cat_color,
                corner_radii: CornerRadii::all(4.0),
            });

            // File name
            cmds.push(RenderCommand::Text {
                x: 26.0,
                y: cy + 6.0,
                text: src.file_name.clone(),
                color: if is_selected { TEXT } else { SUBTEXT1 },
                font_size: 11.0,
                font_weight: if is_selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(SIDEBAR_WIDTH - 60.0),
            });

            // Size + duration
            let info = if let Some(dur) = src.duration_secs {
                format!("{} | {}", src.human_size(), format_duration(dur))
            } else {
                src.human_size()
            };
            cmds.push(RenderCommand::Text {
                x: 26.0,
                y: cy + 20.0,
                text: info,
                color: OVERLAY0,
                font_size: 9.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - 60.0),
            });

            cy += ITEM_HEIGHT;
        }

        if self.sources.is_empty() {
            cmds.push(RenderCommand::Text {
                x: SIDEBAR_WIDTH / 2.0 - 50.0,
                y: y + height / 2.0,
                text: "Drop files here".to_owned(),
                color: OVERLAY0,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    fn render_settings_panel(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Line {
            x1: x + width,
            y1: y,
            x2: x + width,
            y2: y + height,
            color: SURFACE0,
            width: 1.0,
        });

        let lx = x + 12.0;
        let max_w = width - 24.0;
        let mut cy = y + 12.0;

        // Output format
        cmds.push(RenderCommand::Text {
            x: lx,
            y: cy,
            text: "OUTPUT FORMAT".to_owned(),
            color: OVERLAY0,
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(max_w),
        });
        cy += 18.0;

        let fmt_label = self
            .profiles
            .get(self.selected_profile_idx)
            .map_or("N/A".to_owned(), |p| p.output_format.label());
        cmds.push(RenderCommand::FillRect {
            x: lx,
            y: cy,
            width: max_w,
            height: 24.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: lx + 8.0,
            y: cy + 6.0,
            text: fmt_label,
            color: TEXT,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(max_w - 16.0),
        });
        cy += 36.0;

        // Settings summary based on profile category
        let profile_category = self
            .profiles
            .get(self.selected_profile_idx)
            .map(|p| p.output_format.category());

        match profile_category {
            Some(MediaCategory::Audio) => {
                cmds.push(RenderCommand::Text {
                    x: lx,
                    y: cy,
                    text: "AUDIO SETTINGS".to_owned(),
                    color: OVERLAY0,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(max_w),
                });
                cy += 18.0;

                let settings_lines = [
                    format!("Codec: {}", self.audio_settings.codec.label()),
                    format!("Bitrate: {} kbps", self.audio_settings.bitrate_kbps),
                    format!("Sample Rate: {} Hz", self.audio_settings.sample_rate),
                    format!("Channels: {}", self.audio_settings.channels),
                    format!("Bit Depth: {}", self.audio_settings.bit_depth),
                ];
                for line in &settings_lines {
                    cmds.push(RenderCommand::Text {
                        x: lx + 4.0,
                        y: cy,
                        text: line.clone(),
                        color: TEXT,
                        font_size: 11.0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(max_w - 8.0),
                    });
                    cy += 16.0;
                }
            }
            Some(MediaCategory::Video) => {
                cmds.push(RenderCommand::Text {
                    x: lx,
                    y: cy,
                    text: "VIDEO SETTINGS".to_owned(),
                    color: OVERLAY0,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(max_w),
                });
                cy += 18.0;

                let settings_lines = [
                    format!("Video: {}", self.video_settings.video_codec.label()),
                    format!("Audio: {}", self.video_settings.audio_codec.label()),
                    self.video_settings
                        .resolution
                        .as_ref()
                        .map_or("Resolution: Original".to_owned(), |r| {
                            format!("Resolution: {}", r.label())
                        }),
                    self.video_settings
                        .framerate
                        .map_or("Framerate: Original".to_owned(), |f| {
                            format!("Framerate: {f:.0} fps")
                        }),
                    format!(
                        "Video Bitrate: {} kbps",
                        self.video_settings.video_bitrate_kbps
                    ),
                    format!(
                        "Two-pass: {}",
                        if self.video_settings.two_pass {
                            "Yes"
                        } else {
                            "No"
                        }
                    ),
                ];
                for line in &settings_lines {
                    cmds.push(RenderCommand::Text {
                        x: lx + 4.0,
                        y: cy,
                        text: line.clone(),
                        color: TEXT,
                        font_size: 11.0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(max_w - 8.0),
                    });
                    cy += 16.0;
                }
            }
            Some(MediaCategory::Image) => {
                cmds.push(RenderCommand::Text {
                    x: lx,
                    y: cy,
                    text: "IMAGE SETTINGS".to_owned(),
                    color: OVERLAY0,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(max_w),
                });
                cy += 18.0;

                let settings_lines = [
                    format!("Quality: {}%", self.image_settings.quality),
                    self.image_settings
                        .max_width
                        .map_or("Max Width: None".to_owned(), |w| format!("Max Width: {w}")),
                    format!(
                        "Strip Metadata: {}",
                        if self.image_settings.strip_metadata {
                            "Yes"
                        } else {
                            "No"
                        }
                    ),
                    format!(
                        "Preserve Aspect: {}",
                        if self.image_settings.preserve_aspect {
                            "Yes"
                        } else {
                            "No"
                        }
                    ),
                ];
                for line in &settings_lines {
                    cmds.push(RenderCommand::Text {
                        x: lx + 4.0,
                        y: cy,
                        text: line.clone(),
                        color: TEXT,
                        font_size: 11.0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(max_w - 8.0),
                    });
                    cy += 16.0;
                }
            }
            None => {
                cmds.push(RenderCommand::Text {
                    x: lx,
                    y: cy,
                    text: "Select a profile".to_owned(),
                    color: OVERLAY0,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(max_w),
                });
            }
        }

        // Output naming
        cy += 20.0;
        cmds.push(RenderCommand::Text {
            x: lx,
            y: cy,
            text: "OUTPUT NAMING".to_owned(),
            color: OVERLAY0,
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(max_w),
        });
        cy += 18.0;
        cmds.push(RenderCommand::Text {
            x: lx + 4.0,
            y: cy,
            text: format!("Mode: {}", self.output_naming.label()),
            color: TEXT,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(max_w - 8.0),
        });
        cy += 16.0;
        cmds.push(RenderCommand::Text {
            x: lx + 4.0,
            y: cy,
            text: format!("Dir: {}", self.output_dir),
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(max_w - 8.0),
        });
    }

    fn render_queue_panel(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        let stats = self.queue_stats();
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: y + 10.0,
            text: format!("CONVERSION QUEUE ({})", stats.total_jobs),
            color: OVERLAY0,
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 24.0),
        });

        let mut cy = y + 32.0;
        for job in &self.jobs {
            if cy > y + height {
                break;
            }

            // Job row
            cmds.push(RenderCommand::FillRect {
                x: x + 4.0,
                y: cy,
                width: width - 8.0,
                height: ITEM_HEIGHT + 8.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });

            // Status indicator
            cmds.push(RenderCommand::FillRect {
                x: x + 10.0,
                y: cy + 12.0,
                width: 8.0,
                height: 8.0,
                color: job.status.color(),
                corner_radii: CornerRadii::all(4.0),
            });

            // Filename
            cmds.push(RenderCommand::Text {
                x: x + 24.0,
                y: cy + 6.0,
                text: job.source.file_name.clone(),
                color: TEXT,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 100.0),
            });

            // Conversion type
            cmds.push(RenderCommand::Text {
                x: x + 24.0,
                y: cy + 20.0,
                text: job.conversion_label(),
                color: SUBTEXT0,
                font_size: 9.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 100.0),
            });

            // Status label
            cmds.push(RenderCommand::Text {
                x: x + width - 80.0,
                y: cy + 6.0,
                text: job.status.label().to_owned(),
                color: job.status.color(),
                font_size: 10.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(70.0),
            });

            // Progress bar (if running)
            if job.status == JobStatus::Running {
                let bar_x = x + 24.0;
                let bar_w = width - 110.0;
                let bar_y = cy + 32.0;
                cmds.push(RenderCommand::FillRect {
                    x: bar_x,
                    y: bar_y,
                    width: bar_w,
                    height: 4.0,
                    color: SURFACE1,
                    corner_radii: CornerRadii::all(2.0),
                });
                let fill_w = bar_w * (job.progress / 100.0);
                cmds.push(RenderCommand::FillRect {
                    x: bar_x,
                    y: bar_y,
                    width: fill_w,
                    height: 4.0,
                    color: BLUE,
                    corner_radii: CornerRadii::all(2.0),
                });
            }

            cy += ITEM_HEIGHT + 12.0;
        }

        if self.jobs.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + width / 2.0 - 60.0,
                y: y + height / 2.0,
                text: "No jobs in queue".to_owned(),
                color: OVERLAY0,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
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

fn main() {
    let mut app = MediaConvertApp::new();

    app.add_source_with_duration(
        "/music/song.flac",
        "song.flac",
        50_000_000,
        MediaCategory::Audio,
        243.5,
        "FLAC",
    );
    app.add_source_with_duration(
        "/videos/clip.mkv",
        "clip.mkv",
        1_500_000_000,
        MediaCategory::Video,
        3600.0,
        "MKV/H.264",
    );
    app.add_source(
        "/photos/beach.png",
        "beach.png",
        8_000_000,
        MediaCategory::Image,
    );

    let cmds = app.render(1280.0, 800.0);
    let _ = cmds.len();
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(s.resolution.is_some());
        let res = s.resolution.unwrap();
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
        assert_eq!(s.human_size(), "5.0 MB");
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
            "/out",
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
            "/out",
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
            "/out",
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
            "/out",
        );
        assert_eq!(job.conversion_label(), "FLAC -> MP3");
    }

    // --- History ---

    #[test]
    fn test_history_compression_ratio() {
        let entry = HistoryEntry {
            source_path: "/a".to_owned(),
            output_path: "/b".to_owned(),
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
        app.add_source("/a", "song.flac", 1000, MediaCategory::Audio);
        app.add_source("/b", "clip.avi", 2000, MediaCategory::Video);
        let queued = app.queue_all();
        assert_eq!(queued, 2);
        assert_eq!(app.jobs.len(), 2);
    }

    #[test]
    fn test_app_queue_single() {
        let mut app = MediaConvertApp::new();
        let id = app.add_source("/a", "song.flac", 1000, MediaCategory::Audio);
        let job_id = app.queue_source(id);
        assert!(job_id.is_some());
        assert_eq!(app.jobs.len(), 1);
    }

    #[test]
    fn test_app_start_and_complete() {
        let mut app = MediaConvertApp::new();
        app.add_source("/a", "test.wav", 10000, MediaCategory::Audio);
        app.queue_all();
        assert!(app.start_next_job());

        let job_id = app.jobs.first().map(|j| j.id).unwrap();
        assert!(app.complete_job(job_id, 5000));
        assert_eq!(app.history.len(), 1);
    }

    #[test]
    fn test_app_fail_job() {
        let mut app = MediaConvertApp::new();
        app.add_source("/a", "test.wav", 10000, MediaCategory::Audio);
        app.queue_all();
        app.start_next_job();

        let job_id = app.jobs.first().map(|j| j.id).unwrap();
        assert!(app.fail_job(job_id, "Test error"));
    }

    #[test]
    fn test_app_cancel_all_queued() {
        let mut app = MediaConvertApp::new();
        app.add_source("/a", "a", 100, MediaCategory::Audio);
        app.add_source("/b", "b", 200, MediaCategory::Audio);
        app.add_source("/c", "c", 300, MediaCategory::Audio);
        app.queue_all();
        let cancelled = app.cancel_all_queued();
        assert_eq!(cancelled, 3);
    }

    #[test]
    fn test_app_clear_finished() {
        let mut app = MediaConvertApp::new();
        app.add_source("/a", "a", 100, MediaCategory::Audio);
        app.queue_all();
        app.start_next_job();
        let job_id = app.jobs.first().map(|j| j.id).unwrap();
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
        app.add_source("/a", "a", 100, MediaCategory::Audio);
        app.add_source("/b", "b", 200, MediaCategory::Video);
        app.queue_all();
        app.start_next_job();

        let stats = app.queue_stats();
        assert_eq!(stats.total_jobs, 2);
        assert_eq!(stats.running, 1);
        assert_eq!(stats.queued, 1);
    }

    #[test]
    fn test_app_history_stats() {
        let mut app = MediaConvertApp::new();
        app.history.push(HistoryEntry {
            source_path: "/a".to_owned(),
            output_path: "/b".to_owned(),
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

    #[test]
    fn test_render_produces_commands() {
        let mut app = MediaConvertApp::new();
        app.add_source("/a", "song.mp3", 5000, MediaCategory::Audio);
        let cmds = app.render(1280.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_profiles_exist() {
        let profiles = ConversionProfile::builtin_profiles();
        assert_eq!(profiles.len(), 6);
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
        assert_eq!(human_file_size(1024), "1.0 KB");
        assert_eq!(human_file_size(1048576), "1.0 MB");
    }
}
