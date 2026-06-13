//! Camera/Webcam Viewer application for SlateOS.
//!
//! Full-featured camera application with live viewfinder, photo capture,
//! video recording, camera settings, multiple camera support, photo gallery,
//! timer mode, and image filters. Uses simulated frame data for the video
//! capture pipeline.

#![allow(dead_code)]

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
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ============================================================================
// Resolution and frame rate types
// ============================================================================

/// A camera resolution (width x height).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

impl Resolution {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    pub fn label(&self) -> String {
        let tag = match self.height {
            0..=360 => " (360p)",
            361..=480 => " (480p)",
            481..=720 => " (720p HD)",
            721..=1080 => " (1080p FHD)",
            1081..=1440 => " (1440p QHD)",
            1441..=2160 => " (4K UHD)",
            _ => "",
        };
        format!("{}x{}{}", self.width, self.height, tag)
    }

    /// Total pixels in one frame.
    pub fn pixel_count(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// Estimated bytes per frame (RGBA).
    pub fn frame_bytes(&self) -> u64 {
        self.pixel_count().saturating_mul(4)
    }

    /// Aspect ratio as a simplified string.
    pub fn aspect_ratio(&self) -> String {
        if self.height == 0 {
            return "N/A".to_string();
        }
        let ratio = self.width as f64 / self.height as f64;
        if (ratio - 16.0 / 9.0).abs() < 0.05 {
            "16:9".to_string()
        } else if (ratio - 4.0 / 3.0).abs() < 0.05 {
            "4:3".to_string()
        } else if (ratio - 1.0).abs() < 0.05 {
            "1:1".to_string()
        } else {
            format!("{ratio:.2}:1")
        }
    }
}

/// Common camera resolutions.
const RESOLUTIONS: &[Resolution] = &[
    Resolution::new(640, 480),
    Resolution::new(1280, 720),
    Resolution::new(1920, 1080),
    Resolution::new(2560, 1440),
    Resolution::new(3840, 2160),
];

/// Common frame rates.
const FRAME_RATES: &[u32] = &[15, 24, 30, 60, 120];

// ============================================================================
// Camera device management
// ============================================================================

/// Camera device connection/recording status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraStatus {
    Connected,
    Disconnected,
    Recording,
    Error,
}

impl CameraStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Connected => "Connected",
            Self::Disconnected => "Disconnected",
            Self::Recording => "Recording",
            Self::Error => "Error",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Connected => GREEN,
            Self::Disconnected => OVERLAY0,
            Self::Recording => RED,
            Self::Error => YELLOW,
        }
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Connected | Self::Recording)
    }
}

/// Represents a single camera device.
#[derive(Debug, Clone)]
pub struct CameraDevice {
    pub id: u32,
    pub name: String,
    pub supported_resolutions: Vec<Resolution>,
    pub current_resolution_idx: usize,
    pub framerate: u32,
    pub status: CameraStatus,
    pub manufacturer: String,
    pub model_name: String,
    pub has_autofocus: bool,
    pub has_optical_zoom: bool,
}

impl CameraDevice {
    pub fn new(id: u32, name: &str, manufacturer: &str, model_name: &str) -> Self {
        Self {
            id,
            name: name.to_string(),
            supported_resolutions: RESOLUTIONS.to_vec(),
            current_resolution_idx: 2, // default to 1080p
            framerate: 30,
            status: CameraStatus::Connected,
            manufacturer: manufacturer.to_string(),
            model_name: model_name.to_string(),
            has_autofocus: true,
            has_optical_zoom: false,
        }
    }

    pub fn current_resolution(&self) -> Resolution {
        self.supported_resolutions
            .get(self.current_resolution_idx)
            .copied()
            .unwrap_or(Resolution::new(1920, 1080))
    }

    pub fn set_resolution_idx(&mut self, idx: usize) {
        if idx < self.supported_resolutions.len() {
            self.current_resolution_idx = idx;
        }
    }

    pub fn set_framerate(&mut self, fps: u32) {
        if FRAME_RATES.contains(&fps) {
            self.framerate = fps;
        }
    }

    pub fn device_info(&self) -> String {
        format!(
            "{} {} ({})",
            self.manufacturer, self.model_name, self.name
        )
    }

    pub fn info_lines(&self) -> Vec<String> {
        let res = self.current_resolution();
        vec![
            format!("Device: {}", self.name),
            format!("Manufacturer: {}", self.manufacturer),
            format!("Model: {}", self.model_name),
            format!("Resolution: {}", res.label()),
            format!("Frame Rate: {} fps", self.framerate),
            format!("Aspect Ratio: {}", res.aspect_ratio()),
            format!("Autofocus: {}", if self.has_autofocus { "Yes" } else { "No" }),
            format!("Optical Zoom: {}", if self.has_optical_zoom { "Yes" } else { "No" }),
            format!("Status: {}", self.status.label()),
        ]
    }
}

/// Create default simulated camera devices.
fn default_cameras() -> Vec<CameraDevice> {
    vec![
        {
            let mut cam = CameraDevice::new(0, "/dev/video0", "Logitech", "C920 HD Pro");
            cam.has_autofocus = true;
            cam
        },
        {
            let mut cam = CameraDevice::new(1, "/dev/video1", "Microsoft", "LifeCam Studio");
            cam.has_optical_zoom = true;
            cam.current_resolution_idx = 1; // 720p default
            cam
        },
        {
            let mut cam = CameraDevice::new(2, "/dev/video2", "Razer", "Kiyo Pro");
            cam.has_autofocus = true;
            cam.framerate = 60;
            cam
        },
    ]
}

// ============================================================================
// Video frame and capture simulation
// ============================================================================

/// A single captured video frame.
#[derive(Debug, Clone)]
pub struct VideoFrame {
    /// RGBA pixel data (simulated).
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Timestamp in milliseconds since capture start.
    pub timestamp_ms: u64,
    /// Monotonic frame counter.
    pub frame_number: u64,
}

impl VideoFrame {
    /// Create a new frame filled with a test pattern.
    pub fn new_test_pattern(width: u32, height: u32, frame_number: u64, timestamp_ms: u64) -> Self {
        // Generate a simple gradient pattern that varies by frame number
        let pixel_count = width as usize * height as usize;
        let mut pixels = Vec::with_capacity(pixel_count.saturating_mul(4));

        // For efficiency in tests, we generate a small representative sample
        // rather than full resolution pixels.
        let sample_rows: u32 = 4;
        let sample_cols: u32 = 4;
        let total_samples = sample_rows as usize * sample_cols as usize;

        for row in 0..sample_rows {
            for col in 0..sample_cols {
                let r = ((col.saturating_mul(64)).saturating_add(frame_number as u32 * 3)) as u8;
                let g_val = ((row.saturating_mul(64)).saturating_add(frame_number as u32 * 5)) as u8;
                let b = ((row.saturating_add(col)).saturating_mul(32)) as u8;
                pixels.push(r);
                pixels.push(g_val);
                pixels.push(b);
                pixels.push(255);
            }
        }

        // Fill remaining with a base color to reach expected size
        let remaining = pixel_count.saturating_sub(total_samples);
        for _ in 0..remaining {
            pixels.push(30);
            pixels.push(30);
            pixels.push(46);
            pixels.push(255);
        }

        Self {
            pixels,
            width,
            height,
            timestamp_ms,
            frame_number,
        }
    }

    /// Size of pixel data in bytes.
    pub fn data_size(&self) -> usize {
        self.pixels.len()
    }

    /// Apply a filter to the frame pixel data (returns new frame).
    pub fn apply_filter(&self, filter: ImageFilter) -> Self {
        let mut result = self.clone();
        let len = result.pixels.len();
        let mut idx: usize = 0;

        while idx.saturating_add(3) < len {
            let r = result.pixels.get(idx).copied().unwrap_or(0);
            let g_val = result.pixels.get(idx.saturating_add(1)).copied().unwrap_or(0);
            let b = result.pixels.get(idx.saturating_add(2)).copied().unwrap_or(0);

            let (nr, ng, nb) = filter.transform_pixel(r, g_val, b);

            if let Some(p) = result.pixels.get_mut(idx) {
                *p = nr;
            }
            if let Some(p) = result.pixels.get_mut(idx.saturating_add(1)) {
                *p = ng;
            }
            if let Some(p) = result.pixels.get_mut(idx.saturating_add(2)) {
                *p = nb;
            }

            idx = idx.saturating_add(4);
        }
        result
    }
}

// ============================================================================
// Image filters
// ============================================================================

/// Image filters that can be applied to frames/photos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFilter {
    None,
    Grayscale,
    Sepia,
    Negative,
    Blur,
    HighContrast,
    Warm,
    Cool,
}

impl ImageFilter {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Grayscale => "Grayscale",
            Self::Sepia => "Sepia",
            Self::Negative => "Negative",
            Self::Blur => "Blur",
            Self::HighContrast => "High Contrast",
            Self::Warm => "Warm",
            Self::Cool => "Cool",
        }
    }

    pub fn all() -> &'static [ImageFilter] {
        &[
            Self::None,
            Self::Grayscale,
            Self::Sepia,
            Self::Negative,
            Self::Blur,
            Self::HighContrast,
            Self::Warm,
            Self::Cool,
        ]
    }

    /// Transform a single pixel (r, g, b) -> (r, g, b).
    pub fn transform_pixel(self, r: u8, g: u8, b: u8) -> (u8, u8, u8) {
        match self {
            Self::None => (r, g, b),
            Self::Grayscale => {
                // Luminance-weighted grayscale
                let lum = ((r as u16 * 77)
                    .saturating_add(g as u16 * 150)
                    .saturating_add(b as u16 * 29))
                    / 256;
                let l = lum as u8;
                (l, l, l)
            }
            Self::Sepia => {
                let lum = ((r as u16 * 77)
                    .saturating_add(g as u16 * 150)
                    .saturating_add(b as u16 * 29))
                    / 256;
                let sr = (lum.saturating_add(40)).min(255) as u8;
                let sg = (lum.saturating_add(20)).min(255) as u8;
                let sb = lum as u8;
                (sr, sg, sb)
            }
            Self::Negative => (255u8.wrapping_sub(r), 255u8.wrapping_sub(g), 255u8.wrapping_sub(b)),
            Self::Blur => {
                // Simple approximation: blend toward midpoint
                let mid: u8 = 128;
                let blend = |v: u8| -> u8 {
                    ((v as u16).saturating_add(mid as u16) / 2) as u8
                };
                (blend(r), blend(g), blend(b))
            }
            Self::HighContrast => {
                let boost = |v: u8| -> u8 {
                    if v < 128 {
                        v.saturating_sub(30)
                    } else {
                        v.saturating_add(30)
                    }
                };
                (boost(r), boost(g), boost(b))
            }
            Self::Warm => {
                (r.saturating_add(15), g, b.saturating_sub(10))
            }
            Self::Cool => {
                (r.saturating_sub(10), g, b.saturating_add(15))
            }
        }
    }
}

// ============================================================================
// Camera settings
// ============================================================================

/// Adjustable camera capture settings.
#[derive(Debug, Clone)]
pub struct CameraSettings {
    /// Brightness: 0..=100, default 50.
    pub brightness: u32,
    /// Contrast: 0..=100, default 50.
    pub contrast: u32,
    /// Saturation: 0..=100, default 50.
    pub saturation: u32,
    /// Exposure: -5..=5, default 0.
    pub exposure: i32,
    /// White balance in Kelvin: 2500..=10000, default 5500.
    pub white_balance: u32,
    /// Auto white balance enabled.
    pub auto_white_balance: bool,
    /// Zoom level: 1.0 to 10.0.
    pub zoom: f32,
    /// Flip horizontally.
    pub flip_horizontal: bool,
    /// Flip vertically.
    pub flip_vertical: bool,
    /// Mirror mode (like flip_horizontal but conceptually for selfie view).
    pub mirror_mode: bool,
    /// Active image filter.
    pub active_filter: ImageFilter,
    /// Auto-focus enabled.
    pub autofocus: bool,
    /// Noise reduction level: 0..=3.
    pub noise_reduction: u32,
}

impl Default for CameraSettings {
    fn default() -> Self {
        Self {
            brightness: 50,
            contrast: 50,
            saturation: 50,
            exposure: 0,
            white_balance: 5500,
            auto_white_balance: true,
            zoom: 1.0,
            flip_horizontal: false,
            flip_vertical: false,
            mirror_mode: false,
            active_filter: ImageFilter::None,
            autofocus: true,
            noise_reduction: 1,
        }
    }
}

impl CameraSettings {
    pub fn set_brightness(&mut self, val: u32) {
        self.brightness = val.min(100);
    }

    pub fn set_contrast(&mut self, val: u32) {
        self.contrast = val.min(100);
    }

    pub fn set_saturation(&mut self, val: u32) {
        self.saturation = val.min(100);
    }

    pub fn set_exposure(&mut self, val: i32) {
        self.exposure = val.clamp(-5, 5);
    }

    pub fn set_white_balance(&mut self, kelvin: u32) {
        self.white_balance = kelvin.clamp(2500, 10000);
    }

    pub fn set_zoom(&mut self, level: f32) {
        self.zoom = level.clamp(1.0, 10.0);
    }

    pub fn toggle_flip_horizontal(&mut self) {
        self.flip_horizontal = !self.flip_horizontal;
    }

    pub fn toggle_flip_vertical(&mut self) {
        self.flip_vertical = !self.flip_vertical;
    }

    pub fn toggle_mirror(&mut self) {
        self.mirror_mode = !self.mirror_mode;
    }

    pub fn zoom_in(&mut self) {
        self.zoom = (self.zoom + 0.5).min(10.0);
    }

    pub fn zoom_out(&mut self) {
        self.zoom = (self.zoom - 0.5).max(1.0);
    }

    pub fn zoom_label(&self) -> String {
        if self.zoom == self.zoom.floor() {
            format!("{}x", self.zoom as u32)
        } else {
            format!("{:.1}x", self.zoom)
        }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// White balance temperature label.
    pub fn wb_label(&self) -> String {
        if self.auto_white_balance {
            "Auto".to_string()
        } else {
            format!("{}K", self.white_balance)
        }
    }

    pub fn exposure_label(&self) -> String {
        if self.exposure == 0 {
            "0".to_string()
        } else if self.exposure > 0 {
            format!("+{}", self.exposure)
        } else {
            format!("{}", self.exposure)
        }
    }

    pub fn noise_reduction_label(&self) -> &'static str {
        match self.noise_reduction {
            0 => "Off",
            1 => "Low",
            2 => "Medium",
            3 => "High",
            _ => "Unknown",
        }
    }
}

// ============================================================================
// Photo capture and gallery
// ============================================================================

/// A captured photo snapshot.
#[derive(Debug, Clone)]
pub struct CapturedPhoto {
    pub id: u32,
    pub timestamp_ms: u64,
    pub resolution: Resolution,
    pub data_size: u64,
    pub filter: ImageFilter,
    pub filename: String,
    /// Thumbnail pixel data (small preview).
    pub thumbnail: Vec<u8>,
    pub favorite: bool,
}

impl CapturedPhoto {
    pub fn display_name(&self) -> String {
        self.filename.clone()
    }

    pub fn size_label(&self) -> String {
        format_bytes(self.data_size)
    }

    pub fn resolution_label(&self) -> String {
        self.resolution.label()
    }

    pub fn time_label(&self) -> String {
        let total_secs = self.timestamp_ms / 1000;
        let hours = (total_secs / 3600) % 24;
        let minutes = (total_secs % 3600) / 60;
        let seconds = total_secs % 60;
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    }

    pub fn toggle_favorite(&mut self) {
        self.favorite = !self.favorite;
    }
}

/// Photo gallery holding captured images.
#[derive(Debug, Clone)]
pub struct PhotoGallery {
    pub photos: Vec<CapturedPhoto>,
    pub selected_idx: Option<usize>,
    pub next_id: u32,
    pub scroll_offset: usize,
    pub view_mode: GalleryViewMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GalleryViewMode {
    Grid,
    List,
    Filmstrip,
}

impl GalleryViewMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Grid => "Grid",
            Self::List => "List",
            Self::Filmstrip => "Filmstrip",
        }
    }

    pub fn all() -> &'static [GalleryViewMode] {
        &[Self::Grid, Self::List, Self::Filmstrip]
    }
}

impl Default for PhotoGallery {
    fn default() -> Self {
        Self::new()
    }
}

impl PhotoGallery {
    pub fn new() -> Self {
        Self {
            photos: Vec::new(),
            selected_idx: None,
            next_id: 1,
            scroll_offset: 0,
            view_mode: GalleryViewMode::Grid,
        }
    }

    pub fn add_photo(
        &mut self,
        resolution: Resolution,
        data_size: u64,
        filter: ImageFilter,
        timestamp_ms: u64,
    ) {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let filename = format!("photo_{id:04}.png");
        self.photos.push(CapturedPhoto {
            id,
            timestamp_ms,
            resolution,
            data_size,
            filter,
            filename,
            thumbnail: vec![0u8; 64], // placeholder thumbnail
            favorite: false,
        });
        self.selected_idx = Some(self.photos.len().saturating_sub(1));
    }

    pub fn delete_selected(&mut self) {
        if let Some(idx) = self.selected_idx
            && idx < self.photos.len() {
                self.photos.remove(idx);
                if self.photos.is_empty() {
                    self.selected_idx = None;
                } else if idx >= self.photos.len() {
                    self.selected_idx = Some(self.photos.len().saturating_sub(1));
                }
            }
    }

    pub fn select_next(&mut self) {
        if self.photos.is_empty() {
            return;
        }
        match self.selected_idx {
            Some(idx) if idx.saturating_add(1) < self.photos.len() => {
                self.selected_idx = Some(idx.saturating_add(1));
            }
            None => {
                self.selected_idx = Some(0);
            }
            _ => {}
        }
    }

    pub fn select_prev(&mut self) {
        if self.photos.is_empty() {
            return;
        }
        match self.selected_idx {
            Some(idx) if idx > 0 => {
                self.selected_idx = Some(idx.saturating_sub(1));
            }
            None => {
                self.selected_idx = Some(self.photos.len().saturating_sub(1));
            }
            _ => {}
        }
    }

    pub fn selected_photo(&self) -> Option<&CapturedPhoto> {
        self.selected_idx.and_then(|idx| self.photos.get(idx))
    }

    pub fn toggle_favorite_selected(&mut self) {
        if let Some(idx) = self.selected_idx
            && let Some(photo) = self.photos.get_mut(idx) {
                photo.toggle_favorite();
            }
    }

    pub fn count(&self) -> usize {
        self.photos.len()
    }

    pub fn favorites_count(&self) -> usize {
        self.photos.iter().filter(|p| p.favorite).count()
    }

    pub fn total_size(&self) -> u64 {
        self.photos.iter().map(|p| p.data_size).sum()
    }
}

// ============================================================================
// Video recording
// ============================================================================

/// Recording state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingState {
    Idle,
    Recording,
    Paused,
}

impl RecordingState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Recording => "Recording",
            Self::Paused => "Paused",
        }
    }

    pub fn is_recording(self) -> bool {
        matches!(self, Self::Recording)
    }
}

/// Video recording session data.
#[derive(Debug, Clone)]
pub struct RecordingSession {
    pub state: RecordingState,
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// Estimated file size in bytes.
    pub estimated_size: u64,
    /// Frames recorded so far.
    pub frame_count: u64,
    /// Filename for the recording.
    pub filename: String,
    /// Bitrate in bits per second.
    pub bitrate: u64,
    pub recording_id: u32,
}

impl Default for RecordingSession {
    fn default() -> Self {
        Self::new()
    }
}

impl RecordingSession {
    pub fn new() -> Self {
        Self {
            state: RecordingState::Idle,
            duration_ms: 0,
            estimated_size: 0,
            frame_count: 0,
            filename: String::new(),
            bitrate: 8_000_000, // 8 Mbps default
            recording_id: 0,
        }
    }

    pub fn start(&mut self, recording_id: u32) {
        self.state = RecordingState::Recording;
        self.duration_ms = 0;
        self.estimated_size = 0;
        self.frame_count = 0;
        self.recording_id = recording_id;
        self.filename = format!("recording_{recording_id:04}.mp4");
    }

    pub fn stop(&mut self) {
        self.state = RecordingState::Idle;
    }

    pub fn pause(&mut self) {
        if self.state == RecordingState::Recording {
            self.state = RecordingState::Paused;
        }
    }

    pub fn resume(&mut self) {
        if self.state == RecordingState::Paused {
            self.state = RecordingState::Recording;
        }
    }

    /// Advance recording by the given number of milliseconds.
    pub fn advance(&mut self, delta_ms: u64, framerate: u32) {
        if self.state == RecordingState::Recording {
            self.duration_ms = self.duration_ms.saturating_add(delta_ms);
            // Estimate frames for this delta
            let new_frames = delta_ms.saturating_mul(framerate as u64) / 1000;
            self.frame_count = self.frame_count.saturating_add(new_frames);
            // Estimated size = bitrate * duration / 8
            self.estimated_size = self.bitrate
                .saturating_mul(self.duration_ms)
                / 8000;
        }
    }

    pub fn duration_label(&self) -> String {
        format_duration_ms(self.duration_ms)
    }

    pub fn size_label(&self) -> String {
        format_bytes(self.estimated_size)
    }

    pub fn bitrate_label(&self) -> String {
        let mbps = self.bitrate as f64 / 1_000_000.0;
        format!("{mbps:.1} Mbps")
    }
}

// ============================================================================
// Timer mode
// ============================================================================

/// Self-timer countdown mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerMode {
    Off,
    ThreeSeconds,
    FiveSeconds,
    TenSeconds,
}

impl TimerMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::ThreeSeconds => "3s",
            Self::FiveSeconds => "5s",
            Self::TenSeconds => "10s",
        }
    }

    pub fn duration_ms(self) -> u64 {
        match self {
            Self::Off => 0,
            Self::ThreeSeconds => 3000,
            Self::FiveSeconds => 5000,
            Self::TenSeconds => 10000,
        }
    }

    pub fn all() -> &'static [TimerMode] {
        &[Self::Off, Self::ThreeSeconds, Self::FiveSeconds, Self::TenSeconds]
    }

    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::ThreeSeconds,
            Self::ThreeSeconds => Self::FiveSeconds,
            Self::FiveSeconds => Self::TenSeconds,
            Self::TenSeconds => Self::Off,
        }
    }
}

/// Active timer countdown state.
#[derive(Debug, Clone)]
pub struct TimerCountdown {
    pub active: bool,
    pub remaining_ms: u64,
    pub total_ms: u64,
}

impl Default for TimerCountdown {
    fn default() -> Self {
        Self::new()
    }
}

impl TimerCountdown {
    pub fn new() -> Self {
        Self {
            active: false,
            remaining_ms: 0,
            total_ms: 0,
        }
    }

    pub fn start(&mut self, duration_ms: u64) {
        self.active = true;
        self.remaining_ms = duration_ms;
        self.total_ms = duration_ms;
    }

    pub fn tick(&mut self, delta_ms: u64) -> bool {
        if !self.active {
            return false;
        }
        if self.remaining_ms <= delta_ms {
            self.remaining_ms = 0;
            self.active = false;
            return true; // timer expired
        }
        self.remaining_ms = self.remaining_ms.saturating_sub(delta_ms);
        false
    }

    pub fn cancel(&mut self) {
        self.active = false;
        self.remaining_ms = 0;
    }

    pub fn progress(&self) -> f32 {
        if self.total_ms == 0 {
            return 0.0;
        }
        1.0 - (self.remaining_ms as f32 / self.total_ms as f32)
    }

    pub fn remaining_seconds(&self) -> u32 {
        ((self.remaining_ms.saturating_add(999)) / 1000) as u32
    }

    pub fn display(&self) -> String {
        if !self.active {
            return String::new();
        }
        format!("{}", self.remaining_seconds())
    }
}

// ============================================================================
// UI state
// ============================================================================

/// Active sidebar panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarPanel {
    Settings,
    DeviceInfo,
    Filters,
    Gallery,
}

impl SidebarPanel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Settings => "Settings",
            Self::DeviceInfo => "Device Info",
            Self::Filters => "Filters",
            Self::Gallery => "Gallery",
        }
    }

    pub fn all() -> &'static [SidebarPanel] {
        &[Self::Settings, Self::DeviceInfo, Self::Filters, Self::Gallery]
    }
}

/// Capture mode: photo or video.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureMode {
    Photo,
    Video,
}

impl CaptureMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Photo => "Photo",
            Self::Video => "Video",
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            Self::Photo => Self::Video,
            Self::Video => Self::Photo,
        }
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Format a byte count for display.
fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let kb = bytes / 1024;
    if kb < 1024 {
        return format!("{kb} KB");
    }
    let mb = kb / 1024;
    if mb < 1024 {
        return format!("{mb} MB");
    }
    let gb = mb / 1024;
    format!("{gb} GB")
}

/// Format milliseconds as MM:SS or HH:MM:SS.
fn format_duration_ms(ms: u64) -> String {
    let total_secs = ms / 1000;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

// ============================================================================
// Main application state
// ============================================================================

/// The main camera application.
#[derive(Debug, Clone)]
pub struct CameraApp {
    pub width: f32,
    pub height: f32,

    // Camera devices
    pub cameras: Vec<CameraDevice>,
    pub active_camera_idx: usize,

    // Capture state
    pub capture_mode: CaptureMode,
    pub settings: CameraSettings,
    pub recording: RecordingSession,
    pub gallery: PhotoGallery,

    // Timer
    pub timer_mode: TimerMode,
    pub timer_countdown: TimerCountdown,

    // Frame simulation
    pub current_frame: Option<VideoFrame>,
    pub frame_counter: u64,
    pub elapsed_ms: u64,

    // UI state
    pub sidebar_panel: SidebarPanel,
    pub sidebar_visible: bool,
    pub photo_strip_visible: bool,
    pub fullscreen_preview: bool,
    pub status_message: Option<String>,
    pub show_grid_overlay: bool,
    pub show_histogram: bool,
    pub next_recording_id: u32,

    // Flash effect simulation
    pub flash_remaining_ms: u64,
}

impl CameraApp {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            cameras: default_cameras(),
            active_camera_idx: 0,
            capture_mode: CaptureMode::Photo,
            settings: CameraSettings::default(),
            recording: RecordingSession::new(),
            gallery: PhotoGallery::new(),
            timer_mode: TimerMode::Off,
            timer_countdown: TimerCountdown::new(),
            current_frame: None,
            frame_counter: 0,
            elapsed_ms: 0,
            sidebar_panel: SidebarPanel::Settings,
            sidebar_visible: true,
            photo_strip_visible: true,
            fullscreen_preview: false,
            status_message: None,
            show_grid_overlay: false,
            show_histogram: false,
            next_recording_id: 1,
            flash_remaining_ms: 0,
        }
    }

    // ------------------------------------------------------------------
    // Camera management
    // ------------------------------------------------------------------

    pub fn active_camera(&self) -> Option<&CameraDevice> {
        self.cameras.get(self.active_camera_idx)
    }

    pub fn active_camera_mut(&mut self) -> Option<&mut CameraDevice> {
        self.cameras.get_mut(self.active_camera_idx)
    }

    pub fn switch_camera(&mut self, idx: usize) {
        if idx < self.cameras.len() {
            // Stop recording if switching cameras
            if self.recording.state.is_recording() {
                self.stop_recording();
            }
            self.active_camera_idx = idx;
            self.current_frame = None;
            self.set_status("Camera switched");
        }
    }

    pub fn next_camera(&mut self) {
        if self.cameras.is_empty() {
            return;
        }
        let next = (self.active_camera_idx.saturating_add(1)) % self.cameras.len();
        self.switch_camera(next);
    }

    pub fn camera_count(&self) -> usize {
        self.cameras.len()
    }

    // ------------------------------------------------------------------
    // Frame capture simulation
    // ------------------------------------------------------------------

    pub fn capture_frame(&mut self) {
        let (w, h) = self.cameras
            .get(self.active_camera_idx)
            .map(|c| {
                let res = c.current_resolution();
                (res.width, res.height)
            })
            .unwrap_or((1920, 1080));

        self.frame_counter = self.frame_counter.saturating_add(1);

        let frame = VideoFrame::new_test_pattern(w, h, self.frame_counter, self.elapsed_ms);

        // Apply active filter if any
        let filter = self.settings.active_filter;
        let frame = if filter != ImageFilter::None {
            frame.apply_filter(filter)
        } else {
            frame
        };

        self.current_frame = Some(frame);
    }

    /// Tick the application forward by delta_ms milliseconds.
    pub fn tick(&mut self, delta_ms: u64) {
        self.elapsed_ms = self.elapsed_ms.saturating_add(delta_ms);

        // Advance recording
        let framerate = self.cameras
            .get(self.active_camera_idx)
            .map(|c| c.framerate)
            .unwrap_or(30);
        self.recording.advance(delta_ms, framerate);

        // Tick timer countdown
        let timer_expired = self.timer_countdown.tick(delta_ms);
        if timer_expired {
            self.do_capture();
        }

        // Flash effect
        if self.flash_remaining_ms > 0 {
            self.flash_remaining_ms = self.flash_remaining_ms.saturating_sub(delta_ms);
        }

        // Simulate frame capture
        self.capture_frame();
    }

    // ------------------------------------------------------------------
    // Photo capture
    // ------------------------------------------------------------------

    /// Take a photo (or start timer countdown).
    pub fn take_photo(&mut self) {
        if self.timer_mode != TimerMode::Off && !self.timer_countdown.active {
            let duration = self.timer_mode.duration_ms();
            self.timer_countdown.start(duration);
            self.set_status("Timer started");
        } else if !self.timer_countdown.active {
            self.do_capture();
        }
    }

    /// Actually capture the photo (called directly or after timer).
    fn do_capture(&mut self) {
        let resolution = self.cameras
            .get(self.active_camera_idx)
            .map(|c| c.current_resolution())
            .unwrap_or(Resolution::new(1920, 1080));

        let data_size = resolution.frame_bytes();

        self.gallery.add_photo(
            resolution,
            data_size,
            self.settings.active_filter,
            self.elapsed_ms,
        );

        self.flash_remaining_ms = 200;
        self.set_status("Photo captured!");
    }

    pub fn cancel_timer(&mut self) {
        self.timer_countdown.cancel();
        self.set_status("Timer cancelled");
    }

    // ------------------------------------------------------------------
    // Video recording
    // ------------------------------------------------------------------

    pub fn start_recording(&mut self) {
        let id = self.next_recording_id;
        self.next_recording_id = self.next_recording_id.saturating_add(1);
        self.recording.start(id);

        if let Some(cam) = self.cameras.get_mut(self.active_camera_idx) {
            cam.status = CameraStatus::Recording;
        }

        self.set_status("Recording started");
    }

    pub fn stop_recording(&mut self) {
        self.recording.stop();

        if let Some(cam) = self.cameras.get_mut(self.active_camera_idx) {
            cam.status = CameraStatus::Connected;
        }

        self.set_status("Recording stopped");
    }

    pub fn toggle_recording(&mut self) {
        match self.recording.state {
            RecordingState::Idle => self.start_recording(),
            RecordingState::Recording => self.stop_recording(),
            RecordingState::Paused => self.recording.resume(),
        }
    }

    pub fn pause_recording(&mut self) {
        self.recording.pause();
        self.set_status("Recording paused");
    }

    pub fn is_recording(&self) -> bool {
        self.recording.state.is_recording()
    }

    // ------------------------------------------------------------------
    // UI actions
    // ------------------------------------------------------------------

    pub fn toggle_sidebar(&mut self) {
        self.sidebar_visible = !self.sidebar_visible;
    }

    pub fn toggle_photo_strip(&mut self) {
        self.photo_strip_visible = !self.photo_strip_visible;
    }

    pub fn toggle_fullscreen_preview(&mut self) {
        self.fullscreen_preview = !self.fullscreen_preview;
    }

    pub fn toggle_grid_overlay(&mut self) {
        self.show_grid_overlay = !self.show_grid_overlay;
    }

    pub fn toggle_histogram(&mut self) {
        self.show_histogram = !self.show_histogram;
    }

    pub fn set_sidebar_panel(&mut self, panel: SidebarPanel) {
        self.sidebar_panel = panel;
        self.sidebar_visible = true;
    }

    pub fn set_capture_mode(&mut self, mode: CaptureMode) {
        self.capture_mode = mode;
    }

    pub fn toggle_capture_mode(&mut self) {
        self.capture_mode = self.capture_mode.toggle();
    }

    pub fn cycle_timer(&mut self) {
        self.timer_mode = self.timer_mode.next();
    }

    fn set_status(&mut self, msg: &str) {
        self.status_message = Some(msg.to_string());
    }

    pub fn clear_status(&mut self) {
        self.status_message = None;
    }

    // ------------------------------------------------------------------
    // Rendering
    // ------------------------------------------------------------------

    pub fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // Layout constants
        let toolbar_h: f32 = 48.0;
        let photo_strip_h: f32 = if self.photo_strip_visible { 100.0 } else { 0.0 };
        let sidebar_w: f32 = if self.sidebar_visible { 260.0 } else { 0.0 };
        let status_bar_h: f32 = 28.0;

        let viewfinder_x: f32 = 0.0;
        let viewfinder_y: f32 = toolbar_h;
        let viewfinder_w: f32 = self.width - sidebar_w;
        let viewfinder_h: f32 = self.height - toolbar_h - photo_strip_h - status_bar_h;

        // Render toolbar
        self.render_toolbar(&mut cmds, toolbar_h);

        // Render viewfinder
        self.render_viewfinder(&mut cmds, viewfinder_x, viewfinder_y, viewfinder_w, viewfinder_h);

        // Render grid overlay
        if self.show_grid_overlay {
            self.render_grid_overlay(&mut cmds, viewfinder_x, viewfinder_y, viewfinder_w, viewfinder_h);
        }

        // Timer countdown overlay
        if self.timer_countdown.active {
            self.render_timer_overlay(&mut cmds, viewfinder_x, viewfinder_y, viewfinder_w, viewfinder_h);
        }

        // Flash effect
        if self.flash_remaining_ms > 0 {
            let alpha = ((self.flash_remaining_ms as f32 / 200.0) * 200.0) as u8;
            cmds.push(RenderCommand::FillRect {
                x: viewfinder_x,
                y: viewfinder_y,
                width: viewfinder_w,
                height: viewfinder_h,
                color: Color::rgba(255, 255, 255, alpha),
                corner_radii: CornerRadii::ZERO,
            });
        }

        // Recording indicator overlay
        if self.is_recording() {
            self.render_recording_indicator(&mut cmds, viewfinder_x, viewfinder_y, viewfinder_w);
        }

        // Sidebar
        if self.sidebar_visible {
            let sx = self.width - sidebar_w;
            self.render_sidebar(&mut cmds, sx, toolbar_h, sidebar_w, viewfinder_h);
        }

        // Photo strip
        if self.photo_strip_visible {
            let strip_y = self.height - photo_strip_h - status_bar_h;
            self.render_photo_strip(&mut cmds, 0.0, strip_y, self.width, photo_strip_h);
        }

        // Status bar
        let status_y = self.height - status_bar_h;
        self.render_status_bar(&mut cmds, status_y, status_bar_h);

        // Histogram overlay
        if self.show_histogram {
            self.render_histogram(&mut cmds, viewfinder_x, viewfinder_y);
        }

        // Zoom indicator
        if self.settings.zoom > 1.0 {
            let zoom_label = self.settings.zoom_label();
            let zx = viewfinder_x + viewfinder_w - 70.0;
            let zy = viewfinder_y + viewfinder_h - 36.0;
            cmds.push(RenderCommand::FillRect {
                x: zx,
                y: zy,
                width: 60.0,
                height: 26.0,
                color: Color::rgba(0, 0, 0, 180),
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: zx + 10.0,
                y: zy + 6.0,
                text: zoom_label,
                color: TEXT,
                font_size: 14.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }

        cmds
    }

    fn render_toolbar(&self, cmds: &mut Vec<RenderCommand>, toolbar_h: f32) {
        // Toolbar background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: toolbar_h,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Toolbar bottom border
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: toolbar_h,
            x2: self.width,
            y2: toolbar_h,
            color: SURFACE0,
            width: 1.0,
        });

        let mut tx: f32 = 12.0;
        let ty: f32 = 14.0;

        // App title / camera icon
        cmds.push(RenderCommand::Text {
            x: tx,
            y: ty,
            text: "Camera".to_string(),
            color: BLUE,
            font_size: 18.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        tx += 80.0;

        // Separator
        cmds.push(RenderCommand::Line {
            x1: tx,
            y1: 8.0,
            x2: tx,
            y2: toolbar_h - 8.0,
            color: SURFACE1,
            width: 1.0,
        });
        tx += 16.0;

        // Camera selector
        let cam_name = self.active_camera()
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "No Camera".to_string());
        cmds.push(RenderCommand::Text {
            x: tx,
            y: ty,
            text: cam_name,
            color: TEXT,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(150.0),
        });
        tx += 160.0;

        // Capture mode toggle buttons
        let photo_active = self.capture_mode == CaptureMode::Photo;
        let video_active = self.capture_mode == CaptureMode::Video;

        // Photo button
        cmds.push(RenderCommand::FillRect {
            x: tx,
            y: 8.0,
            width: 60.0,
            height: 32.0,
            color: if photo_active { BLUE } else { SURFACE0 },
            corner_radii: CornerRadii {
                top_left: 6.0,
                top_right: 0.0,
                bottom_right: 0.0,
                bottom_left: 6.0,
            },
        });
        cmds.push(RenderCommand::Text {
            x: tx + 10.0,
            y: ty,
            text: "Photo".to_string(),
            color: if photo_active { CRUST } else { TEXT },
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        tx += 60.0;

        // Video button
        cmds.push(RenderCommand::FillRect {
            x: tx,
            y: 8.0,
            width: 60.0,
            height: 32.0,
            color: if video_active { RED } else { SURFACE0 },
            corner_radii: CornerRadii {
                top_left: 0.0,
                top_right: 6.0,
                bottom_right: 6.0,
                bottom_left: 0.0,
            },
        });
        cmds.push(RenderCommand::Text {
            x: tx + 10.0,
            y: ty,
            text: "Video".to_string(),
            color: if video_active { CRUST } else { TEXT },
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        tx += 76.0;

        // Capture/Record button (big round button)
        let btn_size: f32 = 36.0;
        let btn_x = tx;
        let btn_y: f32 = 6.0;
        let btn_color = match self.capture_mode {
            CaptureMode::Photo => BLUE,
            CaptureMode::Video => {
                if self.is_recording() { RED } else { PEACH }
            }
        };
        cmds.push(RenderCommand::FillRect {
            x: btn_x,
            y: btn_y,
            width: btn_size,
            height: btn_size,
            color: btn_color,
            corner_radii: CornerRadii::all(btn_size / 2.0),
        });

        // Inner circle or square for record
        match self.capture_mode {
            CaptureMode::Photo => {
                cmds.push(RenderCommand::FillRect {
                    x: btn_x + 4.0,
                    y: btn_y + 4.0,
                    width: btn_size - 8.0,
                    height: btn_size - 8.0,
                    color: Color::rgba(255, 255, 255, 200),
                    corner_radii: CornerRadii::all((btn_size - 8.0) / 2.0),
                });
            }
            CaptureMode::Video => {
                if self.is_recording() {
                    // Stop square
                    cmds.push(RenderCommand::FillRect {
                        x: btn_x + 10.0,
                        y: btn_y + 10.0,
                        width: btn_size - 20.0,
                        height: btn_size - 20.0,
                        color: CRUST,
                        corner_radii: CornerRadii::all(3.0),
                    });
                } else {
                    // Record circle
                    cmds.push(RenderCommand::FillRect {
                        x: btn_x + 6.0,
                        y: btn_y + 6.0,
                        width: btn_size - 12.0,
                        height: btn_size - 12.0,
                        color: RED,
                        corner_radii: CornerRadii::all((btn_size - 12.0) / 2.0),
                    });
                }
            }
        }
        tx += btn_size + 16.0;

        // Timer mode indicator
        if self.timer_mode != TimerMode::Off {
            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: 10.0,
                width: 40.0,
                height: 28.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: tx + 8.0,
                y: ty,
                text: self.timer_mode.label().to_string(),
                color: YELLOW,
                font_size: 12.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            tx += 52.0;
        }

        // Grid overlay toggle
        cmds.push(RenderCommand::FillRect {
            x: tx,
            y: 10.0,
            width: 28.0,
            height: 28.0,
            color: if self.show_grid_overlay { SURFACE1 } else { SURFACE0 },
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: tx + 6.0,
            y: ty,
            text: "#".to_string(),
            color: if self.show_grid_overlay { BLUE } else { SUBTEXT0 },
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        tx += 38.0;

        // Zoom display
        cmds.push(RenderCommand::Text {
            x: tx,
            y: ty,
            text: self.settings.zoom_label(),
            color: SUBTEXT0,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Right-aligned: filter indicator
        if self.settings.active_filter != ImageFilter::None {
            let filter_label = self.settings.active_filter.label();
            let fx = self.width - 120.0;
            cmds.push(RenderCommand::FillRect {
                x: fx,
                y: 10.0,
                width: 100.0,
                height: 28.0,
                color: MAUVE,
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: fx + 8.0,
                y: ty,
                text: filter_label.to_string(),
                color: CRUST,
                font_size: 12.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(84.0),
            });
        }
    }

    fn render_viewfinder(
        &self,
        cmds: &mut Vec<RenderCommand>,
        vx: f32,
        vy: f32,
        vw: f32,
        vh: f32,
    ) {
        // Viewfinder background (dark)
        cmds.push(RenderCommand::FillRect {
            x: vx,
            y: vy,
            width: vw,
            height: vh,
            color: Color::rgb(10, 10, 15),
            corner_radii: CornerRadii::ZERO,
        });

        // Simulated frame content — draw a gradient preview area
        if self.current_frame.is_some() {
            let margin: f32 = 4.0;
            let frame_x = vx + margin;
            let frame_y = vy + margin;
            let frame_w = vw - margin * 2.0;
            let frame_h = vh - margin * 2.0;

            // Simulated camera view: dark scene with colored gradient bands
            let band_count: u32 = 8;
            let band_h = frame_h / band_count as f32;
            for i in 0..band_count {
                let progress = i as f32 / band_count as f32;
                let r_val = (30.0 + progress * 40.0) as u8;
                let g_v = (30.0 + (1.0 - progress) * 30.0) as u8;
                let b_val = (46.0 + progress * 20.0) as u8;
                cmds.push(RenderCommand::FillRect {
                    x: frame_x,
                    y: frame_y + i as f32 * band_h,
                    width: frame_w,
                    height: band_h + 1.0,
                    color: Color::rgb(r_val, g_v, b_val),
                    corner_radii: CornerRadii::ZERO,
                });
            }

            // Center crosshair for focus indicator
            let cx = frame_x + frame_w / 2.0;
            let cy = frame_y + frame_h / 2.0;
            let cross_size: f32 = 30.0;
            let cross_color = Color::rgba(255, 255, 255, 100);

            // Horizontal line
            cmds.push(RenderCommand::Line {
                x1: cx - cross_size,
                y1: cy,
                x2: cx + cross_size,
                y2: cy,
                color: cross_color,
                width: 1.0,
            });
            // Vertical line
            cmds.push(RenderCommand::Line {
                x1: cx,
                y1: cy - cross_size,
                x2: cx,
                y2: cy + cross_size,
                color: cross_color,
                width: 1.0,
            });

            // Focus bracket corners
            let bracket: f32 = 40.0;
            let bracket_len: f32 = 12.0;
            let bracket_color = Color::rgba(255, 255, 255, 150);

            // Top-left corner
            cmds.push(RenderCommand::Line {
                x1: cx - bracket,
                y1: cy - bracket,
                x2: cx - bracket + bracket_len,
                y2: cy - bracket,
                color: bracket_color,
                width: 2.0,
            });
            cmds.push(RenderCommand::Line {
                x1: cx - bracket,
                y1: cy - bracket,
                x2: cx - bracket,
                y2: cy - bracket + bracket_len,
                color: bracket_color,
                width: 2.0,
            });

            // Top-right corner
            cmds.push(RenderCommand::Line {
                x1: cx + bracket - bracket_len,
                y1: cy - bracket,
                x2: cx + bracket,
                y2: cy - bracket,
                color: bracket_color,
                width: 2.0,
            });
            cmds.push(RenderCommand::Line {
                x1: cx + bracket,
                y1: cy - bracket,
                x2: cx + bracket,
                y2: cy - bracket + bracket_len,
                color: bracket_color,
                width: 2.0,
            });

            // Bottom-left corner
            cmds.push(RenderCommand::Line {
                x1: cx - bracket,
                y1: cy + bracket - bracket_len,
                x2: cx - bracket,
                y2: cy + bracket,
                color: bracket_color,
                width: 2.0,
            });
            cmds.push(RenderCommand::Line {
                x1: cx - bracket,
                y1: cy + bracket,
                x2: cx - bracket + bracket_len,
                y2: cy + bracket,
                color: bracket_color,
                width: 2.0,
            });

            // Bottom-right corner
            cmds.push(RenderCommand::Line {
                x1: cx + bracket,
                y1: cy + bracket - bracket_len,
                x2: cx + bracket,
                y2: cy + bracket,
                color: bracket_color,
                width: 2.0,
            });
            cmds.push(RenderCommand::Line {
                x1: cx + bracket - bracket_len,
                y1: cy + bracket,
                x2: cx + bracket,
                y2: cy + bracket,
                color: bracket_color,
                width: 2.0,
            });

            // Resolution / framerate label top-left
            let res_text = self.active_camera()
                .map(|c| {
                    let r = c.current_resolution();
                    format!("{}x{} @ {}fps", r.width, r.height, c.framerate)
                })
                .unwrap_or_default();
            if !res_text.is_empty() {
                cmds.push(RenderCommand::FillRect {
                    x: frame_x + 8.0,
                    y: frame_y + 8.0,
                    width: 180.0,
                    height: 22.0,
                    color: Color::rgba(0, 0, 0, 160),
                    corner_radii: CornerRadii::all(4.0),
                });
                cmds.push(RenderCommand::Text {
                    x: frame_x + 14.0,
                    y: frame_y + 12.0,
                    text: res_text,
                    color: Color::rgba(255, 255, 255, 200),
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }

            // Frame counter top-right
            cmds.push(RenderCommand::FillRect {
                x: frame_x + frame_w - 108.0,
                y: frame_y + 8.0,
                width: 100.0,
                height: 22.0,
                color: Color::rgba(0, 0, 0, 160),
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: frame_x + frame_w - 102.0,
                y: frame_y + 12.0,
                text: format!("F: {}", self.frame_counter),
                color: Color::rgba(255, 255, 255, 180),
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        } else {
            // No frame — show "No Signal" message
            cmds.push(RenderCommand::Text {
                x: vx + vw / 2.0 - 40.0,
                y: vy + vh / 2.0 - 8.0,
                text: "No Signal".to_string(),
                color: OVERLAY0,
                font_size: 18.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
    }

    fn render_grid_overlay(
        &self,
        cmds: &mut Vec<RenderCommand>,
        vx: f32,
        vy: f32,
        vw: f32,
        vh: f32,
    ) {
        let grid_color = Color::rgba(255, 255, 255, 50);
        let third_w = vw / 3.0;
        let third_h = vh / 3.0;

        // Vertical lines
        for i in 1..3u32 {
            cmds.push(RenderCommand::Line {
                x1: vx + third_w * i as f32,
                y1: vy,
                x2: vx + third_w * i as f32,
                y2: vy + vh,
                color: grid_color,
                width: 1.0,
            });
        }

        // Horizontal lines
        for i in 1..3u32 {
            cmds.push(RenderCommand::Line {
                x1: vx,
                y1: vy + third_h * i as f32,
                x2: vx + vw,
                y2: vy + third_h * i as f32,
                color: grid_color,
                width: 1.0,
            });
        }
    }

    fn render_timer_overlay(
        &self,
        cmds: &mut Vec<RenderCommand>,
        vx: f32,
        vy: f32,
        vw: f32,
        vh: f32,
    ) {
        // Semi-transparent overlay
        cmds.push(RenderCommand::FillRect {
            x: vx,
            y: vy,
            width: vw,
            height: vh,
            color: Color::rgba(0, 0, 0, 100),
            corner_radii: CornerRadii::ZERO,
        });

        // Big countdown number
        let remaining = self.timer_countdown.remaining_seconds();
        let text = format!("{remaining}");
        let cx = vx + vw / 2.0;
        let cy = vy + vh / 2.0;

        // Circle background for countdown
        let circle_r: f32 = 60.0;
        cmds.push(RenderCommand::FillRect {
            x: cx - circle_r,
            y: cy - circle_r,
            width: circle_r * 2.0,
            height: circle_r * 2.0,
            color: Color::rgba(0, 0, 0, 180),
            corner_radii: CornerRadii::all(circle_r),
        });

        cmds.push(RenderCommand::Text {
            x: cx - 20.0,
            y: cy - 28.0,
            text,
            color: YELLOW,
            font_size: 56.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Progress ring (simulated as a border)
        cmds.push(RenderCommand::StrokeRect {
            x: cx - circle_r - 4.0,
            y: cy - circle_r - 4.0,
            width: (circle_r + 4.0) * 2.0,
            height: (circle_r + 4.0) * 2.0,
            color: YELLOW,
            line_width: 3.0,
            corner_radii: CornerRadii::all(circle_r + 4.0),
        });
    }

    fn render_recording_indicator(
        &self,
        cmds: &mut Vec<RenderCommand>,
        vx: f32,
        vy: f32,
        _vw: f32,
    ) {
        let rx = vx + 16.0;
        let ry = vy + 16.0;

        // Recording dot
        cmds.push(RenderCommand::FillRect {
            x: rx,
            y: ry,
            width: 12.0,
            height: 12.0,
            color: RED,
            corner_radii: CornerRadii::all(6.0),
        });

        // "REC" label
        cmds.push(RenderCommand::Text {
            x: rx + 18.0,
            y: ry - 1.0,
            text: "REC".to_string(),
            color: RED,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Duration
        cmds.push(RenderCommand::Text {
            x: rx + 52.0,
            y: ry - 1.0,
            text: self.recording.duration_label(),
            color: Color::rgba(255, 255, 255, 200),
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // File size estimate
        cmds.push(RenderCommand::Text {
            x: rx + 130.0,
            y: ry - 1.0,
            text: self.recording.size_label(),
            color: Color::rgba(255, 255, 255, 150),
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_sidebar(
        &self,
        cmds: &mut Vec<RenderCommand>,
        sx: f32,
        sy: f32,
        sw: f32,
        sh: f32,
    ) {
        // Sidebar background
        cmds.push(RenderCommand::FillRect {
            x: sx,
            y: sy,
            width: sw,
            height: sh,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Left border
        cmds.push(RenderCommand::Line {
            x1: sx,
            y1: sy,
            x2: sx,
            y2: sy + sh,
            color: SURFACE0,
            width: 1.0,
        });

        // Tab buttons
        let tab_h: f32 = 30.0;
        let tabs = SidebarPanel::all();
        let tab_w = sw / tabs.len() as f32;
        for (i, panel) in tabs.iter().enumerate() {
            let tx = sx + i as f32 * tab_w;
            let active = *panel == self.sidebar_panel;
            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: sy,
                width: tab_w,
                height: tab_h,
                color: if active { SURFACE0 } else { MANTLE },
                corner_radii: CornerRadii::ZERO,
            });
            cmds.push(RenderCommand::Text {
                x: tx + 4.0,
                y: sy + 8.0,
                text: panel.label().to_string(),
                color: if active { BLUE } else { SUBTEXT0 },
                font_size: 11.0,
                font_weight: if active { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(tab_w - 8.0),
            });
        }

        // Content area
        let content_y = sy + tab_h + 8.0;
        let content_x = sx + 12.0;
        let content_w = sw - 24.0;

        match self.sidebar_panel {
            SidebarPanel::Settings => {
                self.render_settings_panel(cmds, content_x, content_y, content_w);
            }
            SidebarPanel::DeviceInfo => {
                self.render_device_info_panel(cmds, content_x, content_y, content_w);
            }
            SidebarPanel::Filters => {
                self.render_filters_panel(cmds, content_x, content_y, content_w);
            }
            SidebarPanel::Gallery => {
                self.render_gallery_panel(cmds, content_x, content_y, content_w);
            }
        }
    }

    fn render_settings_panel(
        &self,
        cmds: &mut Vec<RenderCommand>,
        px: f32,
        py: f32,
        pw: f32,
    ) {
        let mut y = py;
        let row_h: f32 = 24.0;

        // Section: Image Adjustments
        cmds.push(RenderCommand::Text {
            x: px,
            y,
            text: "Image Adjustments".to_string(),
            color: BLUE,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        y += row_h;

        // Brightness slider
        self.render_setting_slider(cmds, px, y, pw, "Brightness", self.settings.brightness, 100);
        y += row_h;

        // Contrast slider
        self.render_setting_slider(cmds, px, y, pw, "Contrast", self.settings.contrast, 100);
        y += row_h;

        // Saturation slider
        self.render_setting_slider(cmds, px, y, pw, "Saturation", self.settings.saturation, 100);
        y += row_h;

        // Exposure
        let exp_label = self.settings.exposure_label();
        cmds.push(RenderCommand::Text {
            x: px,
            y,
            text: "Exposure".to_string(),
            color: SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: px + pw - 40.0,
            y,
            text: exp_label,
            color: TEXT,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        y += row_h;

        // White balance
        let wb_label = self.settings.wb_label();
        cmds.push(RenderCommand::Text {
            x: px,
            y,
            text: "White Balance".to_string(),
            color: SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: px + pw - 50.0,
            y,
            text: wb_label,
            color: TEXT,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        y += row_h + 8.0;

        // Section: Transform
        cmds.push(RenderCommand::Text {
            x: px,
            y,
            text: "Transform".to_string(),
            color: BLUE,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        y += row_h;

        // Zoom
        cmds.push(RenderCommand::Text {
            x: px,
            y,
            text: "Zoom".to_string(),
            color: SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: px + pw - 40.0,
            y,
            text: self.settings.zoom_label(),
            color: TEXT,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        y += row_h;

        // Flip/Mirror toggles
        let toggle_items: &[(&str, bool)] = &[
            ("Flip H", self.settings.flip_horizontal),
            ("Flip V", self.settings.flip_vertical),
            ("Mirror", self.settings.mirror_mode),
        ];
        for (label, active) in toggle_items {
            cmds.push(RenderCommand::Text {
                x: px,
                y,
                text: label.to_string(),
                color: SUBTEXT0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            // Toggle indicator
            let indicator_color = if *active { GREEN } else { SURFACE1 };
            cmds.push(RenderCommand::FillRect {
                x: px + pw - 36.0,
                y: y + 1.0,
                width: 32.0,
                height: 16.0,
                color: indicator_color,
                corner_radii: CornerRadii::all(8.0),
            });
            // Knob
            let knob_x = if *active {
                px + pw - 36.0 + 16.0
            } else {
                px + pw - 36.0
            };
            cmds.push(RenderCommand::FillRect {
                x: knob_x + 1.0,
                y: y + 2.0,
                width: 14.0,
                height: 14.0,
                color: TEXT,
                corner_radii: CornerRadii::all(7.0),
            });
            y += row_h;
        }

        y += 8.0;

        // Section: Advanced
        cmds.push(RenderCommand::Text {
            x: px,
            y,
            text: "Advanced".to_string(),
            color: BLUE,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        y += row_h;

        // Autofocus
        cmds.push(RenderCommand::Text {
            x: px,
            y,
            text: "Autofocus".to_string(),
            color: SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: px + pw - 30.0,
            y,
            text: if self.settings.autofocus { "On" } else { "Off" }.to_string(),
            color: if self.settings.autofocus { GREEN } else { OVERLAY0 },
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        y += row_h;

        // Noise reduction
        cmds.push(RenderCommand::Text {
            x: px,
            y,
            text: "Noise Reduction".to_string(),
            color: SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: px + pw - 50.0,
            y,
            text: self.settings.noise_reduction_label().to_string(),
            color: TEXT,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    // 8 args mirror the slider geometry + value pair; grouping into a struct
    // would add an alloc on every render frame for no readability win.
    #[allow(clippy::too_many_arguments)]
    fn render_setting_slider(
        &self,
        cmds: &mut Vec<RenderCommand>,
        sx: f32,
        sy: f32,
        sw: f32,
        label: &str,
        value: u32,
        max_val: u32,
    ) {
        // Label
        cmds.push(RenderCommand::Text {
            x: sx,
            y: sy,
            text: label.to_string(),
            color: SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Value text
        cmds.push(RenderCommand::Text {
            x: sx + sw - 30.0,
            y: sy,
            text: format!("{value}"),
            color: TEXT,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Slider track
        let track_y = sy + 16.0;
        let track_w = sw - 40.0;
        cmds.push(RenderCommand::FillRect {
            x: sx,
            y: track_y,
            width: track_w,
            height: 4.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(2.0),
        });

        // Filled portion
        let fill_frac = if max_val > 0 {
            value as f32 / max_val as f32
        } else {
            0.0
        };
        cmds.push(RenderCommand::FillRect {
            x: sx,
            y: track_y,
            width: track_w * fill_frac,
            height: 4.0,
            color: BLUE,
            corner_radii: CornerRadii::all(2.0),
        });

        // Knob
        let knob_x = sx + track_w * fill_frac - 5.0;
        cmds.push(RenderCommand::FillRect {
            x: knob_x,
            y: track_y - 3.0,
            width: 10.0,
            height: 10.0,
            color: BLUE,
            corner_radii: CornerRadii::all(5.0),
        });
    }

    fn render_device_info_panel(
        &self,
        cmds: &mut Vec<RenderCommand>,
        px: f32,
        py: f32,
        pw: f32,
    ) {
        let mut y = py;
        let row_h: f32 = 22.0;

        // Camera list
        cmds.push(RenderCommand::Text {
            x: px,
            y,
            text: format!("Cameras ({})", self.cameras.len()),
            color: BLUE,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        y += row_h;

        for (i, cam) in self.cameras.iter().enumerate() {
            let active = i == self.active_camera_idx;
            if active {
                cmds.push(RenderCommand::FillRect {
                    x: px - 4.0,
                    y: y - 2.0,
                    width: pw + 8.0,
                    height: row_h,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            // Status dot
            cmds.push(RenderCommand::FillRect {
                x: px,
                y: y + 4.0,
                width: 8.0,
                height: 8.0,
                color: cam.status.color(),
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: px + 14.0,
                y,
                text: cam.device_info(),
                color: if active { TEXT } else { SUBTEXT0 },
                font_size: 11.0,
                font_weight: if active { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(pw - 18.0),
            });
            y += row_h;
        }

        y += 8.0;

        // Active camera details
        if let Some(cam) = self.active_camera() {
            cmds.push(RenderCommand::Text {
                x: px,
                y,
                text: "Active Camera Details".to_string(),
                color: BLUE,
                font_size: 13.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            y += row_h;

            for line in cam.info_lines() {
                cmds.push(RenderCommand::Text {
                    x: px,
                    y,
                    text: line,
                    color: SUBTEXT0,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(pw),
                });
                y += row_h - 4.0;
            }
        }
    }

    fn render_filters_panel(
        &self,
        cmds: &mut Vec<RenderCommand>,
        px: f32,
        py: f32,
        pw: f32,
    ) {
        let mut y = py;
        let row_h: f32 = 28.0;

        cmds.push(RenderCommand::Text {
            x: px,
            y,
            text: "Image Filters".to_string(),
            color: BLUE,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        y += row_h;

        // Filter buttons in a grid
        let filters = ImageFilter::all();
        let col_w = pw / 2.0;
        let btn_h: f32 = 32.0;
        let btn_margin: f32 = 4.0;

        for (i, filter) in filters.iter().enumerate() {
            let col = i % 2;
            let row_idx = i / 2;
            let bx = px + col as f32 * col_w;
            let by = y + row_idx as f32 * (btn_h + btn_margin);

            let active = *filter == self.settings.active_filter;

            cmds.push(RenderCommand::FillRect {
                x: bx,
                y: by,
                width: col_w - btn_margin,
                height: btn_h,
                color: if active { MAUVE } else { SURFACE0 },
                corner_radii: CornerRadii::all(6.0),
            });

            cmds.push(RenderCommand::Text {
                x: bx + 8.0,
                y: by + 9.0,
                text: filter.label().to_string(),
                color: if active { CRUST } else { TEXT },
                font_size: 11.0,
                font_weight: if active { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(col_w - 24.0),
            });
        }

        // Preview strip below filters
        let grid_rows = (filters.len().saturating_add(1)) / 2;
        y += grid_rows as f32 * (btn_h + btn_margin) + 16.0;

        cmds.push(RenderCommand::Text {
            x: px,
            y,
            text: "Filter Preview".to_string(),
            color: BLUE,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        y += 20.0;

        // Small preview rectangles showing filter color effect
        let preview_size: f32 = 40.0;
        let preview_margin: f32 = 6.0;
        for (i, filter) in filters.iter().enumerate() {
            let col = i % 4;
            let row_idx = i / 4;
            let prev_x = px + col as f32 * (preview_size + preview_margin);
            let prev_y = y + row_idx as f32 * (preview_size + preview_margin + 14.0);

            // Apply filter transform to a sample color
            let (fr, fg, fb) = filter.transform_pixel(100, 150, 200);
            cmds.push(RenderCommand::FillRect {
                x: prev_x,
                y: prev_y,
                width: preview_size,
                height: preview_size,
                color: Color::rgb(fr, fg, fb),
                corner_radii: CornerRadii::all(4.0),
            });

            // Label below
            cmds.push(RenderCommand::Text {
                x: prev_x,
                y: prev_y + preview_size + 2.0,
                text: filter.label().to_string(),
                color: OVERLAY0,
                font_size: 9.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(preview_size),
            });
        }
    }

    fn render_gallery_panel(
        &self,
        cmds: &mut Vec<RenderCommand>,
        px: f32,
        py: f32,
        pw: f32,
    ) {
        let mut y = py;
        let row_h: f32 = 20.0;

        // Gallery stats
        let count = self.gallery.count();
        let favorites = self.gallery.favorites_count();
        let total_size = format_bytes(self.gallery.total_size());

        cmds.push(RenderCommand::Text {
            x: px,
            y,
            text: format!("Gallery ({count} photos)"),
            color: BLUE,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        y += row_h;

        cmds.push(RenderCommand::Text {
            x: px,
            y,
            text: format!("Favorites: {favorites} | Size: {total_size}"),
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(pw),
        });
        y += row_h + 8.0;

        // Photo list
        if self.gallery.photos.is_empty() {
            cmds.push(RenderCommand::Text {
                x: px,
                y,
                text: "No photos yet".to_string(),
                color: OVERLAY0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        } else {
            let visible_count = 12usize;
            let start = self.gallery.scroll_offset;
            let end = (start.saturating_add(visible_count)).min(self.gallery.photos.len());

            for i in start..end {
                if let Some(photo) = self.gallery.photos.get(i) {
                    let selected = self.gallery.selected_idx == Some(i);

                    if selected {
                        cmds.push(RenderCommand::FillRect {
                            x: px - 4.0,
                            y: y - 2.0,
                            width: pw + 8.0,
                            height: 34.0,
                            color: SURFACE0,
                            corner_radii: CornerRadii::all(4.0),
                        });
                    }

                    // Thumbnail placeholder
                    cmds.push(RenderCommand::FillRect {
                        x: px,
                        y,
                        width: 28.0,
                        height: 28.0,
                        color: SURFACE1,
                        corner_radii: CornerRadii::all(4.0),
                    });

                    // Favorite star
                    if photo.favorite {
                        cmds.push(RenderCommand::Text {
                            x: px + 6.0,
                            y: y + 6.0,
                            text: "*".to_string(),
                            color: YELLOW,
                            font_size: 14.0,
                            font_weight: FontWeightHint::Bold,
                            max_width: None,
                        });
                    }

                    // Filename
                    cmds.push(RenderCommand::Text {
                        x: px + 34.0,
                        y: y + 2.0,
                        text: photo.display_name(),
                        color: if selected { TEXT } else { SUBTEXT0 },
                        font_size: 11.0,
                        font_weight: if selected { FontWeightHint::Bold } else { FontWeightHint::Regular },
                        max_width: Some(pw - 38.0),
                    });

                    // Size + resolution info
                    cmds.push(RenderCommand::Text {
                        x: px + 34.0,
                        y: y + 16.0,
                        text: format!("{} | {}", photo.size_label(), photo.resolution_label()),
                        color: OVERLAY0,
                        font_size: 10.0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(pw - 38.0),
                    });

                    y += 36.0;
                }
            }
        }
    }

    fn render_photo_strip(
        &self,
        cmds: &mut Vec<RenderCommand>,
        sx: f32,
        sy: f32,
        sw: f32,
        sh: f32,
    ) {
        // Strip background
        cmds.push(RenderCommand::FillRect {
            x: sx,
            y: sy,
            width: sw,
            height: sh,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Top border
        cmds.push(RenderCommand::Line {
            x1: sx,
            y1: sy,
            x2: sx + sw,
            y2: sy,
            color: SURFACE0,
            width: 1.0,
        });

        let thumb_size: f32 = 72.0;
        let thumb_margin: f32 = 8.0;
        let start_x = sx + 12.0;
        let thumb_y = sy + (sh - thumb_size) / 2.0;

        if self.gallery.photos.is_empty() {
            cmds.push(RenderCommand::Text {
                x: start_x,
                y: sy + sh / 2.0 - 6.0,
                text: "Captured photos will appear here".to_string(),
                color: OVERLAY0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(sw - 24.0),
            });
        } else {
            let max_visible = ((sw - 24.0) / (thumb_size + thumb_margin)) as usize;
            let start = if self.gallery.photos.len() > max_visible {
                self.gallery.photos.len().saturating_sub(max_visible)
            } else {
                0
            };

            for (i, photo) in self.gallery.photos.iter().enumerate().skip(start) {
                let offset = (i.saturating_sub(start)) as f32;
                let tx = start_x + offset * (thumb_size + thumb_margin);

                if tx + thumb_size > sx + sw {
                    break;
                }

                let selected = self.gallery.selected_idx == Some(i);

                // Thumbnail background
                cmds.push(RenderCommand::FillRect {
                    x: tx,
                    y: thumb_y,
                    width: thumb_size,
                    height: thumb_size,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(6.0),
                });

                // Selection highlight
                if selected {
                    cmds.push(RenderCommand::StrokeRect {
                        x: tx - 2.0,
                        y: thumb_y - 2.0,
                        width: thumb_size + 4.0,
                        height: thumb_size + 4.0,
                        color: BLUE,
                        line_width: 2.0,
                        corner_radii: CornerRadii::all(8.0),
                    });
                }

                // Photo number
                cmds.push(RenderCommand::Text {
                    x: tx + thumb_size / 2.0 - 8.0,
                    y: thumb_y + thumb_size / 2.0 - 6.0,
                    text: format!("{}", photo.id),
                    color: SUBTEXT0,
                    font_size: 14.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });

                // Favorite indicator
                if photo.favorite {
                    cmds.push(RenderCommand::FillRect {
                        x: tx + thumb_size - 14.0,
                        y: thumb_y + 2.0,
                        width: 12.0,
                        height: 12.0,
                        color: YELLOW,
                        corner_radii: CornerRadii::all(6.0),
                    });
                }

                // Filter indicator
                if photo.filter != ImageFilter::None {
                    cmds.push(RenderCommand::FillRect {
                        x: tx + 2.0,
                        y: thumb_y + thumb_size - 14.0,
                        width: 12.0,
                        height: 12.0,
                        color: MAUVE,
                        corner_radii: CornerRadii::all(3.0),
                    });
                }
            }
        }
    }

    fn render_histogram(
        &self,
        cmds: &mut Vec<RenderCommand>,
        vx: f32,
        vy: f32,
    ) {
        let hx = vx + 12.0;
        let hy = vy + 12.0;
        let hw: f32 = 160.0;
        let hh: f32 = 80.0;

        // Background
        cmds.push(RenderCommand::FillRect {
            x: hx,
            y: hy,
            width: hw,
            height: hh,
            color: Color::rgba(0, 0, 0, 180),
            corner_radii: CornerRadii::all(6.0),
        });

        // Simulated histogram bars
        let bar_count: u32 = 16;
        let bar_w = (hw - 8.0) / bar_count as f32;
        let max_h = hh - 16.0;

        for i in 0..bar_count {
            // Simulated distribution (bell-curve-ish)
            let center = bar_count as f32 / 2.0;
            let dist = (i as f32 - center).abs() / center;
            let bar_height = max_h * (1.0 - dist * dist);

            let bar_x = hx + 4.0 + i as f32 * bar_w;
            let bar_y = hy + hh - 8.0 - bar_height;

            cmds.push(RenderCommand::FillRect {
                x: bar_x,
                y: bar_y,
                width: bar_w - 1.0,
                height: bar_height,
                color: Color::rgba(200, 200, 200, 120),
                corner_radii: CornerRadii::ZERO,
            });
        }

        // Label
        cmds.push(RenderCommand::Text {
            x: hx + 4.0,
            y: hy + 2.0,
            text: "Histogram".to_string(),
            color: Color::rgba(255, 255, 255, 180),
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_status_bar(
        &self,
        cmds: &mut Vec<RenderCommand>,
        sy: f32,
        sh: f32,
    ) {
        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: sy,
            width: self.width,
            height: sh,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // Top border
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: sy,
            x2: self.width,
            y2: sy,
            color: SURFACE0,
            width: 1.0,
        });

        let text_y = sy + 7.0;
        let mut x: f32 = 12.0;

        // Camera status
        let status = self.active_camera()
            .map(|c| c.status)
            .unwrap_or(CameraStatus::Disconnected);
        cmds.push(RenderCommand::FillRect {
            x,
            y: text_y + 2.0,
            width: 8.0,
            height: 8.0,
            color: status.color(),
            corner_radii: CornerRadii::all(4.0),
        });
        x += 14.0;
        cmds.push(RenderCommand::Text {
            x,
            y: text_y,
            text: status.label().to_string(),
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        x += 90.0;

        // Recording status
        if self.is_recording() {
            cmds.push(RenderCommand::Text {
                x,
                y: text_y,
                text: format!("REC {} | {}", self.recording.duration_label(), self.recording.size_label()),
                color: RED,
                font_size: 11.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            x += 160.0;
        }

        // Photo count
        cmds.push(RenderCommand::Text {
            x,
            y: text_y,
            text: format!("Photos: {}", self.gallery.count()),
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        x += 80.0;

        // Timer mode
        if self.timer_mode != TimerMode::Off {
            cmds.push(RenderCommand::Text {
                x,
                y: text_y,
                text: format!("Timer: {}", self.timer_mode.label()),
                color: YELLOW,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            x += 80.0;
        }

        // Filter
        if self.settings.active_filter != ImageFilter::None {
            cmds.push(RenderCommand::Text {
                x,
                y: text_y,
                text: format!("Filter: {}", self.settings.active_filter.label()),
                color: MAUVE,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            x += 120.0;
        }

        // Status message (right-aligned)
        if let Some(ref msg) = self.status_message {
            let msg_x = self.width - 200.0;
            cmds.push(RenderCommand::Text {
                x: msg_x,
                y: text_y,
                text: msg.clone(),
                color: GREEN,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(190.0),
            });
        }

        // Suppress unused variable warning
        let _ = x;
    }
}

// ============================================================================
// Keyboard shortcut reference
// ============================================================================

pub struct Shortcuts;

impl Shortcuts {
    pub fn list() -> Vec<(&'static str, &'static str)> {
        vec![
            ("Space", "Take Photo / Toggle Recording"),
            ("Enter", "Take Photo"),
            ("R", "Start/Stop Recording"),
            ("P", "Pause Recording"),
            ("T", "Cycle Timer Mode"),
            ("Escape", "Cancel Timer"),
            ("Tab", "Switch Camera"),
            ("M", "Toggle Capture Mode"),
            ("G", "Toggle Grid Overlay"),
            ("H", "Toggle Histogram"),
            ("S", "Toggle Sidebar"),
            ("F", "Toggle Fullscreen Preview"),
            ("+", "Zoom In"),
            ("-", "Zoom Out"),
            ("0", "Reset Zoom"),
            ("Ctrl+H", "Flip Horizontal"),
            ("Ctrl+V", "Flip Vertical"),
            ("Ctrl+M", "Mirror Mode"),
            ("1-8", "Select Filter"),
            ("Left", "Previous Photo"),
            ("Right", "Next Photo"),
            ("Delete", "Delete Selected Photo"),
            ("Ctrl+S", "Save Photo"),
            ("Ctrl+R", "Reset Settings"),
        ]
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let mut app = CameraApp::new(1280.0, 720.0);

    // Simulate some frames
    app.tick(33);
    app.tick(33);
    app.tick(33);

    // Take a photo
    app.take_photo();

    // Switch filter and take another
    app.settings.active_filter = ImageFilter::Sepia;
    app.tick(33);
    app.take_photo();

    // Switch to video mode and record
    app.set_capture_mode(CaptureMode::Video);
    app.start_recording();
    app.tick(1000);
    app.tick(1000);
    app.stop_recording();

    // Switch cameras
    app.next_camera();
    app.tick(33);

    // Zoom in
    app.settings.zoom_in();
    app.settings.zoom_in();
    app.tick(33);

    // Enable grid overlay
    app.toggle_grid_overlay();

    // Browse gallery
    app.gallery.select_next();
    app.gallery.toggle_favorite_selected();

    // Render all sidebar panels
    for panel in SidebarPanel::all() {
        app.set_sidebar_panel(*panel);
        let cmds = app.render();
        let _ = cmds.len();
    }

    // Render with histogram
    app.toggle_histogram();
    let cmds = app.render();
    let _ = cmds.len();

    // Render with timer countdown
    app.timer_mode = TimerMode::ThreeSeconds;
    app.timer_countdown.start(3000);
    let cmds = app.render();
    let _ = cmds.len();
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Resolution tests ---

    #[test]
    fn test_resolution_label_480p() {
        let res = Resolution::new(640, 480);
        assert!(res.label().contains("480p"));
    }

    #[test]
    fn test_resolution_label_1080p() {
        let res = Resolution::new(1920, 1080);
        assert!(res.label().contains("1080p"));
    }

    #[test]
    fn test_resolution_label_4k() {
        let res = Resolution::new(3840, 2160);
        assert!(res.label().contains("4K"));
    }

    #[test]
    fn test_resolution_pixel_count() {
        let res = Resolution::new(1920, 1080);
        assert_eq!(res.pixel_count(), 2_073_600);
    }

    #[test]
    fn test_resolution_frame_bytes() {
        let res = Resolution::new(640, 480);
        assert_eq!(res.frame_bytes(), 640 * 480 * 4);
    }

    #[test]
    fn test_resolution_aspect_ratio_16_9() {
        let res = Resolution::new(1920, 1080);
        assert_eq!(res.aspect_ratio(), "16:9");
    }

    #[test]
    fn test_resolution_aspect_ratio_4_3() {
        let res = Resolution::new(640, 480);
        assert_eq!(res.aspect_ratio(), "4:3");
    }

    #[test]
    fn test_resolution_aspect_ratio_zero_height() {
        let res = Resolution::new(100, 0);
        assert_eq!(res.aspect_ratio(), "N/A");
    }

    // --- CameraStatus tests ---

    #[test]
    fn test_camera_status_labels() {
        assert_eq!(CameraStatus::Connected.label(), "Connected");
        assert_eq!(CameraStatus::Disconnected.label(), "Disconnected");
        assert_eq!(CameraStatus::Recording.label(), "Recording");
        assert_eq!(CameraStatus::Error.label(), "Error");
    }

    #[test]
    fn test_camera_status_is_active() {
        assert!(CameraStatus::Connected.is_active());
        assert!(CameraStatus::Recording.is_active());
        assert!(!CameraStatus::Disconnected.is_active());
        assert!(!CameraStatus::Error.is_active());
    }

    // --- CameraDevice tests ---

    #[test]
    fn test_camera_device_new() {
        let cam = CameraDevice::new(0, "/dev/video0", "Logitech", "C920");
        assert_eq!(cam.id, 0);
        assert_eq!(cam.framerate, 30);
        assert_eq!(cam.status, CameraStatus::Connected);
    }

    #[test]
    fn test_camera_device_resolution() {
        let cam = CameraDevice::new(0, "/dev/video0", "Logitech", "C920");
        let res = cam.current_resolution();
        assert_eq!(res.width, 1920);
        assert_eq!(res.height, 1080);
    }

    #[test]
    fn test_camera_device_set_resolution() {
        let mut cam = CameraDevice::new(0, "/dev/video0", "Logitech", "C920");
        cam.set_resolution_idx(0);
        assert_eq!(cam.current_resolution().width, 640);
    }

    #[test]
    fn test_camera_device_set_framerate() {
        let mut cam = CameraDevice::new(0, "/dev/video0", "Logitech", "C920");
        cam.set_framerate(60);
        assert_eq!(cam.framerate, 60);
        cam.set_framerate(99); // invalid, should not change
        assert_eq!(cam.framerate, 60);
    }

    #[test]
    fn test_camera_device_info() {
        let cam = CameraDevice::new(0, "/dev/video0", "Logitech", "C920");
        let info = cam.device_info();
        assert!(info.contains("Logitech"));
        assert!(info.contains("C920"));
    }

    #[test]
    fn test_camera_device_info_lines() {
        let cam = CameraDevice::new(0, "/dev/video0", "Logitech", "C920");
        let lines = cam.info_lines();
        assert!(lines.len() >= 5);
    }

    // --- ImageFilter tests ---

    #[test]
    fn test_filter_labels() {
        assert_eq!(ImageFilter::None.label(), "None");
        assert_eq!(ImageFilter::Grayscale.label(), "Grayscale");
        assert_eq!(ImageFilter::Sepia.label(), "Sepia");
    }

    #[test]
    fn test_filter_all_count() {
        let all = ImageFilter::all();
        assert_eq!(all.len(), 8);
    }

    #[test]
    fn test_filter_none_passthrough() {
        let (r, g, b) = ImageFilter::None.transform_pixel(100, 150, 200);
        assert_eq!((r, g, b), (100, 150, 200));
    }

    #[test]
    fn test_filter_grayscale() {
        let (r, g, b) = ImageFilter::Grayscale.transform_pixel(255, 0, 0);
        assert_eq!(r, g);
        assert_eq!(g, b);
    }

    #[test]
    fn test_filter_negative() {
        let (r, g, b) = ImageFilter::Negative.transform_pixel(100, 150, 200);
        assert_eq!(r, 155);
        assert_eq!(g, 105);
        assert_eq!(b, 55);
    }

    #[test]
    fn test_filter_sepia() {
        let (r, g, b) = ImageFilter::Sepia.transform_pixel(100, 100, 100);
        // Sepia should have r > g > b
        assert!(r >= g);
        assert!(g >= b);
    }

    #[test]
    fn test_filter_warm() {
        let (r, _g, b) = ImageFilter::Warm.transform_pixel(100, 100, 100);
        assert!(r > 100);
        assert!(b < 100);
    }

    #[test]
    fn test_filter_cool() {
        let (r, _g, b) = ImageFilter::Cool.transform_pixel(100, 100, 100);
        assert!(r < 100);
        assert!(b > 100);
    }

    // --- CameraSettings tests ---

    #[test]
    fn test_settings_default() {
        let s = CameraSettings::default();
        assert_eq!(s.brightness, 50);
        assert_eq!(s.contrast, 50);
        assert_eq!(s.zoom, 1.0);
        assert!(s.auto_white_balance);
    }

    #[test]
    fn test_settings_brightness_clamp() {
        let mut s = CameraSettings::default();
        s.set_brightness(200);
        assert_eq!(s.brightness, 100);
    }

    #[test]
    fn test_settings_exposure_clamp() {
        let mut s = CameraSettings::default();
        s.set_exposure(10);
        assert_eq!(s.exposure, 5);
        s.set_exposure(-10);
        assert_eq!(s.exposure, -5);
    }

    #[test]
    fn test_settings_zoom_clamp() {
        let mut s = CameraSettings::default();
        s.set_zoom(20.0);
        assert_eq!(s.zoom, 10.0);
        s.set_zoom(-1.0);
        assert_eq!(s.zoom, 1.0);
    }

    #[test]
    fn test_settings_zoom_in_out() {
        let mut s = CameraSettings::default();
        s.zoom_in();
        assert_eq!(s.zoom, 1.5);
        s.zoom_out();
        assert_eq!(s.zoom, 1.0);
        s.zoom_out(); // should not go below 1.0
        assert_eq!(s.zoom, 1.0);
    }

    #[test]
    fn test_settings_zoom_label() {
        let mut s = CameraSettings::default();
        assert_eq!(s.zoom_label(), "1x");
        s.zoom_in();
        assert_eq!(s.zoom_label(), "1.5x");
    }

    #[test]
    fn test_settings_toggle_flip() {
        let mut s = CameraSettings::default();
        assert!(!s.flip_horizontal);
        s.toggle_flip_horizontal();
        assert!(s.flip_horizontal);
    }

    #[test]
    fn test_settings_toggle_mirror() {
        let mut s = CameraSettings::default();
        assert!(!s.mirror_mode);
        s.toggle_mirror();
        assert!(s.mirror_mode);
    }

    #[test]
    fn test_settings_reset() {
        let mut s = CameraSettings {
            brightness: 100,
            zoom: 5.0,
            flip_horizontal: true,
            ..CameraSettings::default()
        };
        s.reset();
        assert_eq!(s.brightness, 50);
        assert_eq!(s.zoom, 1.0);
        assert!(!s.flip_horizontal);
    }

    #[test]
    fn test_settings_wb_label() {
        let mut s = CameraSettings::default();
        assert_eq!(s.wb_label(), "Auto");
        s.auto_white_balance = false;
        assert!(s.wb_label().contains("5500"));
    }

    #[test]
    fn test_settings_exposure_label() {
        let mut s = CameraSettings::default();
        assert_eq!(s.exposure_label(), "0");
        s.set_exposure(3);
        assert_eq!(s.exposure_label(), "+3");
        s.set_exposure(-2);
        assert_eq!(s.exposure_label(), "-2");
    }

    #[test]
    fn test_settings_noise_reduction_label() {
        let s = CameraSettings::default();
        assert_eq!(s.noise_reduction_label(), "Low");
    }

    // --- PhotoGallery tests ---

    #[test]
    fn test_gallery_new_empty() {
        let g = PhotoGallery::new();
        assert_eq!(g.count(), 0);
        assert!(g.selected_photo().is_none());
    }

    #[test]
    fn test_gallery_add_photo() {
        let mut g = PhotoGallery::new();
        g.add_photo(Resolution::new(1920, 1080), 8000, ImageFilter::None, 1000);
        assert_eq!(g.count(), 1);
        assert_eq!(g.selected_idx, Some(0));
    }

    #[test]
    fn test_gallery_delete_selected() {
        let mut g = PhotoGallery::new();
        g.add_photo(Resolution::new(640, 480), 1000, ImageFilter::None, 100);
        g.add_photo(Resolution::new(640, 480), 1000, ImageFilter::None, 200);
        assert_eq!(g.count(), 2);
        g.selected_idx = Some(0);
        g.delete_selected();
        assert_eq!(g.count(), 1);
    }

    #[test]
    fn test_gallery_navigate() {
        let mut g = PhotoGallery::new();
        g.add_photo(Resolution::new(640, 480), 1000, ImageFilter::None, 100);
        g.add_photo(Resolution::new(640, 480), 1000, ImageFilter::None, 200);
        g.add_photo(Resolution::new(640, 480), 1000, ImageFilter::None, 300);
        g.selected_idx = Some(0);
        g.select_next();
        assert_eq!(g.selected_idx, Some(1));
        g.select_next();
        assert_eq!(g.selected_idx, Some(2));
        g.select_next(); // at end, should not wrap
        assert_eq!(g.selected_idx, Some(2));
        g.select_prev();
        assert_eq!(g.selected_idx, Some(1));
    }

    #[test]
    fn test_gallery_favorites() {
        let mut g = PhotoGallery::new();
        g.add_photo(Resolution::new(640, 480), 1000, ImageFilter::None, 100);
        g.add_photo(Resolution::new(640, 480), 1000, ImageFilter::None, 200);
        assert_eq!(g.favorites_count(), 0);
        g.selected_idx = Some(0);
        g.toggle_favorite_selected();
        assert_eq!(g.favorites_count(), 1);
    }

    #[test]
    fn test_gallery_total_size() {
        let mut g = PhotoGallery::new();
        g.add_photo(Resolution::new(640, 480), 1000, ImageFilter::None, 100);
        g.add_photo(Resolution::new(640, 480), 2000, ImageFilter::None, 200);
        assert_eq!(g.total_size(), 3000);
    }

    #[test]
    fn test_gallery_view_modes() {
        assert_eq!(GalleryViewMode::Grid.label(), "Grid");
        assert_eq!(GalleryViewMode::all().len(), 3);
    }

    // --- RecordingSession tests ---

    #[test]
    fn test_recording_new() {
        let r = RecordingSession::new();
        assert_eq!(r.state, RecordingState::Idle);
        assert_eq!(r.duration_ms, 0);
    }

    #[test]
    fn test_recording_start_stop() {
        let mut r = RecordingSession::new();
        r.start(1);
        assert_eq!(r.state, RecordingState::Recording);
        assert!(r.filename.contains("0001"));
        r.stop();
        assert_eq!(r.state, RecordingState::Idle);
    }

    #[test]
    fn test_recording_pause_resume() {
        let mut r = RecordingSession::new();
        r.start(1);
        r.pause();
        assert_eq!(r.state, RecordingState::Paused);
        r.resume();
        assert_eq!(r.state, RecordingState::Recording);
    }

    #[test]
    fn test_recording_advance() {
        let mut r = RecordingSession::new();
        r.start(1);
        r.advance(5000, 30);
        assert_eq!(r.duration_ms, 5000);
        assert!(r.frame_count > 0);
        assert!(r.estimated_size > 0);
    }

    #[test]
    fn test_recording_paused_no_advance() {
        let mut r = RecordingSession::new();
        r.start(1);
        r.pause();
        r.advance(5000, 30);
        assert_eq!(r.duration_ms, 0);
    }

    #[test]
    fn test_recording_duration_label() {
        let mut r = RecordingSession::new();
        r.start(1);
        r.advance(65000, 30);
        let label = r.duration_label();
        assert!(label.contains("1:05"));
    }

    #[test]
    fn test_recording_state_labels() {
        assert_eq!(RecordingState::Idle.label(), "Idle");
        assert_eq!(RecordingState::Recording.label(), "Recording");
        assert_eq!(RecordingState::Paused.label(), "Paused");
    }

    // --- TimerMode tests ---

    #[test]
    fn test_timer_mode_labels() {
        assert_eq!(TimerMode::Off.label(), "Off");
        assert_eq!(TimerMode::ThreeSeconds.label(), "3s");
    }

    #[test]
    fn test_timer_mode_durations() {
        assert_eq!(TimerMode::Off.duration_ms(), 0);
        assert_eq!(TimerMode::ThreeSeconds.duration_ms(), 3000);
        assert_eq!(TimerMode::FiveSeconds.duration_ms(), 5000);
        assert_eq!(TimerMode::TenSeconds.duration_ms(), 10000);
    }

    #[test]
    fn test_timer_mode_cycle() {
        let m = TimerMode::Off;
        let m = m.next();
        assert_eq!(m, TimerMode::ThreeSeconds);
        let m = m.next();
        assert_eq!(m, TimerMode::FiveSeconds);
        let m = m.next();
        assert_eq!(m, TimerMode::TenSeconds);
        let m = m.next();
        assert_eq!(m, TimerMode::Off);
    }

    #[test]
    fn test_timer_mode_all() {
        assert_eq!(TimerMode::all().len(), 4);
    }

    // --- TimerCountdown tests ---

    #[test]
    fn test_countdown_new() {
        let c = TimerCountdown::new();
        assert!(!c.active);
    }

    #[test]
    fn test_countdown_start_and_tick() {
        let mut c = TimerCountdown::new();
        c.start(3000);
        assert!(c.active);
        assert_eq!(c.remaining_ms, 3000);

        let expired = c.tick(1000);
        assert!(!expired);
        assert_eq!(c.remaining_ms, 2000);

        let expired = c.tick(2000);
        assert!(expired);
        assert!(!c.active);
    }

    #[test]
    fn test_countdown_cancel() {
        let mut c = TimerCountdown::new();
        c.start(5000);
        c.cancel();
        assert!(!c.active);
        assert_eq!(c.remaining_ms, 0);
    }

    #[test]
    fn test_countdown_progress() {
        let mut c = TimerCountdown::new();
        c.start(4000);
        c.tick(2000);
        let progress = c.progress();
        assert!((progress - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_countdown_remaining_seconds() {
        let mut c = TimerCountdown::new();
        c.start(3500);
        assert_eq!(c.remaining_seconds(), 4); // rounds up
    }

    // --- VideoFrame tests ---

    #[test]
    fn test_video_frame_creation() {
        let frame = VideoFrame::new_test_pattern(640, 480, 0, 0);
        assert_eq!(frame.width, 640);
        assert_eq!(frame.height, 480);
        assert!(frame.data_size() > 0);
    }

    #[test]
    fn test_video_frame_filter_apply() {
        let frame = VideoFrame::new_test_pattern(64, 64, 1, 100);
        let filtered = frame.apply_filter(ImageFilter::Grayscale);
        assert_eq!(filtered.width, frame.width);
        assert_eq!(filtered.frame_number, frame.frame_number);
    }

    #[test]
    fn test_video_frame_filter_none_unchanged() {
        let frame = VideoFrame::new_test_pattern(4, 4, 0, 0);
        let filtered = frame.apply_filter(ImageFilter::None);
        assert_eq!(frame.pixels, filtered.pixels);
    }

    // --- CapturedPhoto tests ---

    #[test]
    fn test_captured_photo_display_name() {
        let photo = CapturedPhoto {
            id: 1,
            timestamp_ms: 5000,
            resolution: Resolution::new(1920, 1080),
            data_size: 4096,
            filter: ImageFilter::None,
            filename: "photo_0001.png".to_string(),
            thumbnail: vec![0; 64],
            favorite: false,
        };
        assert_eq!(photo.display_name(), "photo_0001.png");
    }

    #[test]
    fn test_captured_photo_time_label() {
        let photo = CapturedPhoto {
            id: 1,
            timestamp_ms: 3661000, // 1h 1m 1s
            resolution: Resolution::new(640, 480),
            data_size: 1024,
            filter: ImageFilter::None,
            filename: "test.png".to_string(),
            thumbnail: vec![],
            favorite: false,
        };
        let label = photo.time_label();
        assert!(label.contains("01:01:01"));
    }

    #[test]
    fn test_captured_photo_toggle_favorite() {
        let mut photo = CapturedPhoto {
            id: 1,
            timestamp_ms: 0,
            resolution: Resolution::new(640, 480),
            data_size: 1024,
            filter: ImageFilter::None,
            filename: "test.png".to_string(),
            thumbnail: vec![],
            favorite: false,
        };
        assert!(!photo.favorite);
        photo.toggle_favorite();
        assert!(photo.favorite);
    }

    // --- Helper function tests ---

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(2048), "2 KB");
        assert!(format_bytes(2_000_000).contains("MB"));
        assert!(format_bytes(3_000_000_000).contains("GB"));
    }

    #[test]
    fn test_format_duration_ms() {
        assert_eq!(format_duration_ms(0), "0:00");
        assert_eq!(format_duration_ms(5000), "0:05");
        assert_eq!(format_duration_ms(65000), "1:05");
        assert_eq!(format_duration_ms(3661000), "1:01:01");
    }

    // --- CameraApp tests ---

    #[test]
    fn test_app_new() {
        let app = CameraApp::new(800.0, 600.0);
        assert_eq!(app.camera_count(), 3);
        assert_eq!(app.active_camera_idx, 0);
        assert_eq!(app.capture_mode, CaptureMode::Photo);
    }

    #[test]
    fn test_app_switch_camera() {
        let mut app = CameraApp::new(800.0, 600.0);
        app.switch_camera(1);
        assert_eq!(app.active_camera_idx, 1);
    }

    #[test]
    fn test_app_next_camera_wraps() {
        let mut app = CameraApp::new(800.0, 600.0);
        app.next_camera();
        assert_eq!(app.active_camera_idx, 1);
        app.next_camera();
        assert_eq!(app.active_camera_idx, 2);
        app.next_camera(); // wraps
        assert_eq!(app.active_camera_idx, 0);
    }

    #[test]
    fn test_app_take_photo() {
        let mut app = CameraApp::new(800.0, 600.0);
        app.tick(33);
        app.take_photo();
        assert_eq!(app.gallery.count(), 1);
        assert!(app.flash_remaining_ms > 0);
    }

    #[test]
    fn test_app_take_photo_with_timer() {
        let mut app = CameraApp::new(800.0, 600.0);
        app.timer_mode = TimerMode::ThreeSeconds;
        app.take_photo();
        assert!(app.timer_countdown.active);
        assert_eq!(app.gallery.count(), 0); // photo not yet taken

        // Tick past timer
        app.tick(3500);
        assert!(!app.timer_countdown.active);
        assert_eq!(app.gallery.count(), 1);
    }

    #[test]
    fn test_app_cancel_timer() {
        let mut app = CameraApp::new(800.0, 600.0);
        app.timer_mode = TimerMode::FiveSeconds;
        app.take_photo();
        assert!(app.timer_countdown.active);
        app.cancel_timer();
        assert!(!app.timer_countdown.active);
    }

    #[test]
    fn test_app_recording() {
        let mut app = CameraApp::new(800.0, 600.0);
        app.start_recording();
        assert!(app.is_recording());
        assert_eq!(
            app.active_camera().map(|c| c.status),
            Some(CameraStatus::Recording)
        );

        app.tick(2000);
        assert!(app.recording.duration_ms > 0);

        app.stop_recording();
        assert!(!app.is_recording());
    }

    #[test]
    fn test_app_toggle_recording() {
        let mut app = CameraApp::new(800.0, 600.0);
        app.toggle_recording();
        assert!(app.is_recording());
        app.toggle_recording();
        assert!(!app.is_recording());
    }

    #[test]
    fn test_app_toggle_capture_mode() {
        let mut app = CameraApp::new(800.0, 600.0);
        assert_eq!(app.capture_mode, CaptureMode::Photo);
        app.toggle_capture_mode();
        assert_eq!(app.capture_mode, CaptureMode::Video);
        app.toggle_capture_mode();
        assert_eq!(app.capture_mode, CaptureMode::Photo);
    }

    #[test]
    fn test_app_toggle_sidebar() {
        let mut app = CameraApp::new(800.0, 600.0);
        assert!(app.sidebar_visible);
        app.toggle_sidebar();
        assert!(!app.sidebar_visible);
    }

    #[test]
    fn test_app_toggle_grid_overlay() {
        let mut app = CameraApp::new(800.0, 600.0);
        assert!(!app.show_grid_overlay);
        app.toggle_grid_overlay();
        assert!(app.show_grid_overlay);
    }

    #[test]
    fn test_app_cycle_timer() {
        let mut app = CameraApp::new(800.0, 600.0);
        assert_eq!(app.timer_mode, TimerMode::Off);
        app.cycle_timer();
        assert_eq!(app.timer_mode, TimerMode::ThreeSeconds);
    }

    #[test]
    fn test_app_render_default() {
        let app = CameraApp::new(1280.0, 720.0);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_with_frame() {
        let mut app = CameraApp::new(1280.0, 720.0);
        app.tick(33);
        let cmds = app.render();
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_app_render_all_sidebar_panels() {
        let mut app = CameraApp::new(1280.0, 720.0);
        app.tick(33);
        app.take_photo();

        for panel in SidebarPanel::all() {
            app.set_sidebar_panel(*panel);
            let cmds = app.render();
            assert!(!cmds.is_empty(), "Panel {:?} produced no commands", panel);
        }
    }

    #[test]
    fn test_app_render_recording_state() {
        let mut app = CameraApp::new(1280.0, 720.0);
        app.start_recording();
        app.tick(1000);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_timer_overlay() {
        let mut app = CameraApp::new(1280.0, 720.0);
        app.timer_countdown.start(3000);
        app.tick(33);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_with_histogram() {
        let mut app = CameraApp::new(1280.0, 720.0);
        app.toggle_histogram();
        app.tick(33);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_with_grid() {
        let mut app = CameraApp::new(1280.0, 720.0);
        app.toggle_grid_overlay();
        app.tick(33);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_with_zoom() {
        let mut app = CameraApp::new(1280.0, 720.0);
        app.settings.set_zoom(3.0);
        app.tick(33);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_with_filter() {
        let mut app = CameraApp::new(1280.0, 720.0);
        app.settings.active_filter = ImageFilter::Sepia;
        app.tick(33);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_sidebar_hidden() {
        let mut app = CameraApp::new(1280.0, 720.0);
        app.toggle_sidebar();
        app.tick(33);
        let cmds_hidden = app.render();
        app.toggle_sidebar();
        let cmds_visible = app.render();
        // With sidebar visible, we should have more commands
        assert!(cmds_visible.len() > cmds_hidden.len());
    }

    #[test]
    fn test_app_render_photo_strip_empty() {
        let app = CameraApp::new(1280.0, 720.0);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_photo_strip_with_photos() {
        let mut app = CameraApp::new(1280.0, 720.0);
        app.tick(33);
        for _ in 0..5 {
            app.take_photo();
            app.tick(100);
        }
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_flash_effect() {
        let mut app = CameraApp::new(1280.0, 720.0);
        app.tick(33);
        app.take_photo();
        // Flash should be active immediately after photo
        assert!(app.flash_remaining_ms > 0);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_shortcuts_list() {
        let shortcuts = Shortcuts::list();
        assert!(shortcuts.len() > 20);
        assert!(shortcuts.iter().any(|(k, _)| *k == "Space"));
        assert!(shortcuts.iter().any(|(_, a)| a.contains("Photo")));
    }

    #[test]
    fn test_capture_mode_labels() {
        assert_eq!(CaptureMode::Photo.label(), "Photo");
        assert_eq!(CaptureMode::Video.label(), "Video");
    }

    #[test]
    fn test_capture_mode_toggle() {
        assert_eq!(CaptureMode::Photo.toggle(), CaptureMode::Video);
        assert_eq!(CaptureMode::Video.toggle(), CaptureMode::Photo);
    }

    #[test]
    fn test_sidebar_panel_labels() {
        for panel in SidebarPanel::all() {
            assert!(!panel.label().is_empty());
        }
    }

    #[test]
    fn test_sidebar_panel_all_count() {
        assert_eq!(SidebarPanel::all().len(), 4);
    }

    #[test]
    fn test_default_cameras() {
        let cams = default_cameras();
        assert_eq!(cams.len(), 3);
        assert!(cams.iter().all(|c| c.status == CameraStatus::Connected));
    }

    #[test]
    fn test_recording_bitrate_label() {
        let r = RecordingSession::new();
        assert!(r.bitrate_label().contains("Mbps"));
    }

    #[test]
    fn test_app_switch_camera_stops_recording() {
        let mut app = CameraApp::new(800.0, 600.0);
        app.start_recording();
        assert!(app.is_recording());
        app.switch_camera(1);
        assert!(!app.is_recording());
    }
}
