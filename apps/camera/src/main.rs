//! Camera/Webcam Viewer application for SlateOS.
//!
//! Full-featured camera application with live viewfinder, photo capture,
//! video recording, camera settings, multiple camera support, photo gallery,
//! timer mode, and image filters. Uses simulated frame data for the video
//! capture pipeline.
//!
//! The window is real: a [`Layout`] is solved from the live window size every
//! frame, the drawing pass records the hit box of everything it paints, and a
//! click is answered by whatever was actually drawn under it. See the roadmap
//! entry for the faults that wiring it exposed.

// Lint policy is inherited from the workspace (`[lints] workspace = true`).
// There used to be a crate-level `#![allow(dead_code)]` here. A blanket allow
// is not a decision about a line of code, it is a decision not to look at any
// of them -- and in a program whose `main` rendered into a `Vec` and dropped
// it, "dead" described most of the file. It is gone.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};

use std::cmp::Ordering;
use std::process::ExitCode;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
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
// The window
// ============================================================================

/// The size the window opens at. Chosen so the layout can afford a sidebar and
/// a photo strip at once -- opening narrower would show the fallback picture,
/// which is a program that looks broken the first time it is run.
const WINDOW_WIDTH: f32 = 1100.0;
/// See [`WINDOW_WIDTH`].
const WINDOW_HEIGHT: f32 = 720.0;

/// How often the window is asked for a frame. The viewfinder is a live picture
/// and the recording clock counts in tenths, so anything slower is visible.
const TICK_MS: u64 = 33;

/// Narrower than this and a sidebar is not a panel, it is a stripe.
const MIN_SIDEBAR_W: f32 = 150.0;
/// The viewfinder is the point of the program; it is never given up for a
/// panel that only reports on it.
const MIN_VIEWFINDER_W: f32 = 200.0;
/// A photo strip below this is a row of slivers with no readable caption.
const MIN_STRIP_H: f32 = 56.0;
/// The viewfinder's floor in the vertical direction, for the same reason as
/// [`MIN_VIEWFINDER_W`].
const MIN_VIEWFINDER_H: f32 = 120.0;

/// How long the white flash after a shutter lasts.
const FLASH_MS: u64 = 200;

/// Everything in the picture that a click can land on.
///
/// Every variant is recorded as a hit box by the pass that paints it, so a
/// control that stops being drawn stops being clickable in the same edit --
/// which is the property a coordinate-literal test can never hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The Photo half of the mode pair.
    PhotoMode,
    /// The Video half of the mode pair.
    VideoMode,
    /// The big round shutter / record button.
    Shutter,
    /// The self-timer cycle.
    Timer,
    /// The rule-of-thirds overlay toggle.
    Grid,
    /// The histogram toggle.
    Histogram,
    /// The sidebar show/hide toggle.
    Sidebar,
    /// The photo-strip show/hide toggle.
    Strip,
    /// Step to the next camera device.
    NextCamera,
    /// One of the sidebar's tabs.
    Panel(SidebarPanel),
    /// One filter in the filter panel.
    Filter(ImageFilter),
    /// One of the device panel's resolution choices, by index into the active
    /// camera's `supported_resolutions`.
    Resolution(usize),
    /// One of the device panel's frame-rate choices, by index into
    /// `FRAME_RATES`.
    Framerate(usize),
    /// One photo, by index into the gallery.
    Photo(usize),
    /// Mark the selected photo a favourite.
    Favorite,
    /// Delete the selected photo.
    Delete,
    /// One end of one settings slider.
    Setting(Setting, Nudge),
    /// The viewfinder itself: clicking it takes the picture, which is what a
    /// camera's screen does.
    Viewfinder,
}

/// A slider in the settings panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    Brightness,
    Contrast,
    Saturation,
    Exposure,
    WhiteBalance,
    NoiseReduction,
    Zoom,
}

impl Setting {
    /// Every slider, in the order the panel draws them.
    pub const fn all() -> &'static [Setting] {
        &[
            Setting::Brightness,
            Setting::Contrast,
            Setting::Saturation,
            Setting::Exposure,
            Setting::WhiteBalance,
            Setting::NoiseReduction,
            Setting::Zoom,
        ]
    }

    /// The name the panel prints.
    pub const fn label(self) -> &'static str {
        match self {
            Setting::Brightness => "Brightness",
            Setting::Contrast => "Contrast",
            Setting::Saturation => "Saturation",
            Setting::Exposure => "Exposure",
            Setting::WhiteBalance => "White Balance",
            Setting::NoiseReduction => "Noise Reduction",
            Setting::Zoom => "Zoom",
        }
    }
}

/// Which way a slider's end moves it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nudge {
    Down,
    Up,
}

// ============================================================================
// Layout
// ============================================================================

/// Where every pane goes, solved from the live window size.
///
/// Nothing here is a constant. The previous version of this program laid the
/// whole picture out from compile-time numbers -- a 260 px sidebar, a 100 px
/// photo strip, a 48 px toolbar -- and then subtracted them from the window,
/// so a window narrower than the sidebar gave the viewfinder a *negative*
/// width. It never showed, because there was no window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub window: Rect,
    /// The top strip: mode pair, shutter, timer, grid, zoom, filter badge.
    pub toolbar: Rect,
    /// The live picture. Never empty -- everything else is given up first.
    pub viewfinder: Rect,
    /// Settings / device / filters / gallery. Empty when the window is narrow.
    pub sidebar: Rect,
    /// The row of recent photos. Empty when the window is short.
    pub strip: Rect,
    /// The one line of prose along the bottom.
    pub status: Rect,
    /// The height of one list row, one settings row, one tab.
    pub row: f32,
    /// The height of a toolbar button.
    pub button: f32,
    pub pad: f32,
    pub heading: f32,
    pub font: f32,
    pub small: f32,
}

impl Layout {
    /// Solve the layout for a window of `w` x `h`.
    ///
    /// The order the panes are given up in is the order of how much they are
    /// worth: the status line is one line and always affordable, the strip
    /// goes before the sidebar, and the viewfinder is never given up at all.
    pub fn solve(w: f32, h: f32) -> Self {
        let w = w.max(0.0);
        let h = h.max(0.0);
        let window = Rect::new(0.0, 0.0, w, h);

        let pad = (w.min(h) * 0.02).clamp(3.0, 12.0);
        let font = (h / 50.0).clamp(9.0, 15.0);
        let small = (font - 2.0).max(8.0);
        let heading = (font * 1.25).clamp(11.0, 20.0);
        let row = (font * 2.0).max(14.0);

        // The button height is what the toolbar can *afford*, not what the
        // font would like. The toolbar is cut to the window in a short one, and
        // a button sized from the font alone would then be taller than the
        // strip it sits in and paint over the viewfinder -- or, in a window
        // shorter than the button, straight off the bottom edge.
        let wanted_button = (small * 2.4).max(12.0);
        let toolbar_h = (wanted_button + pad * 1.6).min(h);
        let button = (toolbar_h - pad * 1.6).clamp(0.0, wanted_button);
        let toolbar = Rect::new(0.0, 0.0, w, toolbar_h);
        let status_h = (small + pad * 1.2).min((h - toolbar_h).max(0.0));
        let status = Rect::new(0.0, h - status_h, w, status_h);

        let content_y = toolbar.bottom();
        let content_h = (status.y - content_y).max(0.0);

        // The strip is horizontal, so it competes with the viewfinder for
        // height; the sidebar is vertical, so it competes for width. Each is
        // taken only if it is big enough to be worth having *and* leaves the
        // viewfinder big enough to be a viewfinder.
        let mut strip_h = (content_h * 0.16).clamp(0.0, 110.0);
        if strip_h < MIN_STRIP_H || content_h - strip_h < MIN_VIEWFINDER_H {
            strip_h = 0.0;
        }
        let mut sidebar_w = (w * 0.26).clamp(0.0, 300.0);
        if sidebar_w < MIN_SIDEBAR_W || w - sidebar_w < MIN_VIEWFINDER_W {
            sidebar_w = 0.0;
        }

        let viewfinder = Rect::new(
            0.0,
            content_y,
            (w - sidebar_w).max(0.0),
            (content_h - strip_h).max(0.0),
        );
        let sidebar = Rect::new(viewfinder.right(), content_y, sidebar_w, content_h);
        let strip = Rect::new(0.0, viewfinder.bottom(), viewfinder.w, strip_h);

        Self {
            window,
            toolbar,
            viewfinder,
            sidebar,
            strip,
            status,
            row,
            button,
            pad,
            heading,
            font,
            small,
        }
    }

    /// How many rows of `row` height fit in `r`, after `taken` has been used.
    pub fn rows_in(&self, r: Rect, taken: f32) -> usize {
        if self.row <= 0.0 {
            return 0;
        }
        let left = (r.h - taken).max(0.0);
        // `as` on a non-negative, finite f32 that has already been divided by a
        // positive row height cannot overflow a usize on any target this runs
        // on, but `floor` first so the count is never rounded *up* into a row
        // that does not fit.
        let n = (left / self.row).floor();
        if n <= 0.0 { 0 } else { n as usize }
    }
}

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
        u64::from(self.width).saturating_mul(u64::from(self.height))
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
        format!("{} {} ({})", self.manufacturer, self.model_name, self.name)
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
            format!(
                "Autofocus: {}",
                if self.has_autofocus { "Yes" } else { "No" }
            ),
            format!(
                "Optical Zoom: {}",
                if self.has_optical_zoom { "Yes" } else { "No" }
            ),
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
        let pixel_count = usize_of_u32(width).saturating_mul(usize_of_u32(height));
        let mut pixels = Vec::with_capacity(pixel_count.saturating_mul(4));

        // For efficiency in tests, we generate a small representative sample
        // rather than full resolution pixels.
        let sample_rows: u32 = 4;
        let sample_cols: u32 = 4;
        let total_samples = usize_of_u32(sample_rows).saturating_mul(usize_of_u32(sample_cols));

        // The pattern only has to differ from one frame to the next, so the
        // channel arithmetic is done modulo 256 on purpose rather than left to
        // a truncating cast: the wrap is the effect, not an accident.
        let phase = frame_number % 256;
        for row in 0..sample_rows {
            for col in 0..sample_cols {
                let r = wrap_u8(
                    u64::from(col)
                        .saturating_mul(64)
                        .saturating_add(phase.saturating_mul(3)),
                );
                let g_val = wrap_u8(
                    u64::from(row)
                        .saturating_mul(64)
                        .saturating_add(phase.saturating_mul(5)),
                );
                let b = wrap_u8(u64::from(row.saturating_add(col)).saturating_mul(32));
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
            let g_val = result
                .pixels
                .get(idx.saturating_add(1))
                .copied()
                .unwrap_or(0);
            let b = result
                .pixels
                .get(idx.saturating_add(2))
                .copied()
                .unwrap_or(0);

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
                let l = clamp_u8(luminance(r, g, b));
                (l, l, l)
            }
            Self::Sepia => {
                let lum = luminance(r, g, b);
                let sr = clamp_u8(lum.saturating_add(40));
                let sg = clamp_u8(lum.saturating_add(20));
                let sb = clamp_u8(lum);
                (sr, sg, sb)
            }
            Self::Negative => (
                255u8.wrapping_sub(r),
                255u8.wrapping_sub(g),
                255u8.wrapping_sub(b),
            ),
            Self::Blur => {
                // Simple approximation: blend toward midpoint
                let mid: u8 = 128;
                let blend = |v: u8| -> u8 { ((v as u16).saturating_add(mid as u16) / 2) as u8 };
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
            Self::Warm => (r.saturating_add(15), g, b.saturating_sub(10)),
            Self::Cool => (r.saturating_sub(10), g, b.saturating_add(15)),
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
        // A whole-number zoom reads "2x", not "2.0x". The test is on the
        // fractional part rather than `zoom == zoom.floor()` because comparing
        // two floats for equality is only ever right by luck.
        if self.zoom.fract().abs() < f32::EPSILON {
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
        match self.exposure.cmp(&0) {
            Ordering::Equal => "0".to_string(),
            Ordering::Greater => format!("+{}", self.exposure),
            Ordering::Less => format!("{}", self.exposure),
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
            && idx < self.photos.len()
        {
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
            && let Some(photo) = self.photos.get_mut(idx)
        {
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
            self.estimated_size = self.bitrate.saturating_mul(self.duration_ms) / 8000;
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
        &[
            Self::Off,
            Self::ThreeSeconds,
            Self::FiveSeconds,
            Self::TenSeconds,
        ]
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
        &[
            Self::Settings,
            Self::DeviceInfo,
            Self::Filters,
            Self::Gallery,
        ]
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
    guitk::bytes::iec(bytes)
}

/// Format a recording's length as a clock reading.
///
/// This is the same object the screen recorder lists — a captured video and
/// how long it runs — so it gets the same rendering. It used to leave the
/// leading minutes digit unpadded (`1:05` beside the recorder's `01:05`),
/// which made two lists of recordings disagree about the same quantity.
fn format_duration_ms(ms: u64) -> String {
    guitk::duration::clock(ms / 1000)
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
        let next = self
            .active_camera_idx
            .saturating_add(1)
            .checked_rem(self.cameras.len())
            .unwrap_or(0);
        self.switch_camera(next);
    }

    pub fn camera_count(&self) -> usize {
        self.cameras.len()
    }

    // ------------------------------------------------------------------
    // Frame capture simulation
    // ------------------------------------------------------------------

    pub fn capture_frame(&mut self) {
        let (w, h) = self
            .cameras
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

    /// Tick the application forward by `delta_ms` milliseconds.
    ///
    /// Returns whether anything in the picture moved, which is what decides
    /// whether the window is asked for a frame. A tick that changed nothing
    /// must not ask for one, or a camera whose device has gone away repaints
    /// thirty times a second for ever showing the same still words.
    ///
    /// The liveness test is also what stops the frame counter running while
    /// there is no device: the previous version called `capture_frame`
    /// unconditionally, so a disconnected camera still produced a stream of
    /// invented frames, each stamped with a resolution nothing was reading.
    pub fn tick(&mut self, delta_ms: u64) -> bool {
        self.elapsed_ms = self.elapsed_ms.saturating_add(delta_ms);

        // Advance recording
        let framerate = self
            .cameras
            .get(self.active_camera_idx)
            .map(|c| c.framerate)
            .unwrap_or(30);
        let was_recording = self.recording.state.is_recording();
        self.recording.advance(delta_ms, framerate);

        // Tick timer countdown
        let was_counting = self.timer_countdown.active;
        let timer_expired = self.timer_countdown.tick(delta_ms);
        if timer_expired {
            self.do_capture();
        }

        // Flash effect
        let was_flashing = self.flash_remaining_ms > 0;
        if was_flashing {
            self.flash_remaining_ms = self.flash_remaining_ms.saturating_sub(delta_ms);
        }

        // Simulate frame capture, but only from a device that is producing.
        let live = self.active_camera().is_some_and(|c| c.status.is_active());
        if live {
            self.capture_frame();
        }

        live || was_recording || was_counting || was_flashing
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
        let resolution = self
            .cameras
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

        self.flash_remaining_ms = FLASH_MS;
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
    // Drawing
    // ------------------------------------------------------------------

    /// Draw the whole window at `w` x `h`, recording a hit box for every
    /// control painted.
    ///
    /// This is the only drawing entry point, and it takes the size rather than
    /// reading a remembered one, because the size the compositor gives is the
    /// only size that is true. The previous version drew from `self.width` and
    /// `self.height`, fields set once in `new()` -- which is a picture of the
    /// window the program was born in, painted into whatever window it is
    /// actually in.
    pub fn frame(&self, w: f32, h: f32) -> Frame<Target> {
        let l = Layout::solve(w, h);
        let mut f = Frame::new(w, h);

        fill(&mut f, l.window, CRUST, CornerRadii::ZERO);

        self.draw_toolbar(&mut f, &l);
        self.draw_viewfinder(&mut f, &l);
        if !l.sidebar.is_empty() {
            self.draw_sidebar(&mut f, &l);
        }
        if !l.strip.is_empty() {
            self.draw_strip(&mut f, &l);
        }
        self.draw_status(&mut f, &l);

        // The overlays come last because they are *over*: an overlay drawn
        // before the sidebar would be under it, and `Frame::hit_test` returns
        // the last-painted match, so a control under a modal must not resolve.
        if self.timer_countdown.active {
            self.draw_timer_overlay(&mut f, &l);
        }
        if self.flash_remaining_ms > 0 {
            self.draw_flash(&mut f, &l);
        }

        f
    }

    /// The picture as the compositor wants it.
    pub fn render_tree(&self, w: f32, h: f32) -> RenderTree {
        self.frame(w, h).into_tree()
    }

    // ------------------------------------------------------------------
    // Toolbar
    // ------------------------------------------------------------------

    /// The top strip: title, camera, the Photo/Video pair, the shutter, and
    /// the toggles.
    ///
    /// Everything here is *measured and packed*, never advanced by a guess.
    /// The previous version walked a cursor along the strip with literals --
    /// `tx += 80.0` after the word "Camera", `tx += 160.0` after the camera
    /// name -- so a longer camera name ran into the mode buttons and a shorter
    /// one left a hole. Each item is measured, and an item that does not fit
    /// in what is left is not drawn at all rather than drawn off the edge.
    fn draw_toolbar(&self, f: &mut Frame<Target>, l: &Layout) {
        let bar = l.toolbar;
        if bar.is_empty() {
            return;
        }
        fill(f, bar, MANTLE, CornerRadii::ZERO);
        f.push(RenderCommand::Line {
            x1: bar.x,
            y1: bar.bottom(),
            x2: bar.right(),
            y2: bar.bottom(),
            color: SURFACE0,
            width: 1.0,
        });

        // Right-hand items are placed from the right edge inwards, and what
        // they leave is the budget for the left-hand ones. Doing it in this
        // order is what stops the two groups overlapping in a narrow window.
        let mut right = bar.right() - l.pad;

        if self.settings.active_filter != ImageFilter::None {
            let label = self.settings.active_filter.label();
            let bw = (text::measure(label, l.small, FontWeightHint::Bold) + l.pad * 2.0)
                .min(bar.w * 0.3);
            let r = Rect::new(right - bw, bar.y + l.pad * 0.6, bw, l.button);
            if r.x > bar.x {
                fill(f, r, MAUVE, CornerRadii::all(6.0));
                centred(f, r, label, CRUST, l.small, FontWeightHint::Bold);
                right = r.x - l.pad * 0.5;
            }
        }

        for (target, label, on) in [
            (Target::Histogram, "H", self.show_histogram),
            (Target::Strip, "S", self.photo_strip_visible),
            (Target::Sidebar, "|", self.sidebar_visible),
        ] {
            let bw =
                (text::measure(label, l.small, FontWeightHint::Bold) + l.pad * 1.6).max(l.button);
            let r = Rect::new(right - bw, bar.y + l.pad * 0.6, bw, l.button);
            if r.x <= bar.x {
                break;
            }
            fill(
                f,
                r,
                if on { SURFACE1 } else { SURFACE0 },
                CornerRadii::all(6.0),
            );
            centred(
                f,
                r,
                label,
                if on { BLUE } else { SUBTEXT0 },
                l.small,
                FontWeightHint::Bold,
            );
            f.hit(target, r);
            right = r.x - l.pad * 0.5;
        }

        // Left-hand group, packed against the budget the right-hand group left.
        let mut x = bar.x + l.pad;
        let avail = |x: f32| (right - x).max(0.0);

        let title = "Camera";
        let tw = text::measure(title, l.heading, FontWeightHint::Bold);
        if tw <= avail(x) {
            bounded(
                f,
                Rect::new(x, bar.y, tw, bar.h),
                title,
                BLUE,
                l.heading,
                FontWeightHint::Bold,
            );
            x += tw + l.pad;
        }

        // The camera name is the one item allowed to be squeezed rather than
        // dropped -- it is how the user knows which device is live -- so it
        // gets whatever is left after the fixed-size controls have been paid
        // for, and ellipsises inside that.
        let controls_w = l.button * 2.0 + l.button * 1.2 + l.pad * 4.0;
        let name_w = (avail(x) - controls_w).clamp(0.0, bar.w * 0.22);
        if name_w > l.small {
            let name = self
                .active_camera()
                .map_or_else(|| "No Camera".to_string(), |c| c.name.clone());
            let r = Rect::new(x, bar.y, name_w, bar.h);
            bounded(f, r, &name, TEXT, l.font, FontWeightHint::Regular);
            f.hit(Target::NextCamera, r);
            x += name_w + l.pad;
        }

        // The mode pair.
        let mode_w = (avail(x) * 0.3).clamp(0.0, l.button * 2.2);
        if mode_w >= l.small * 2.0 {
            let half = mode_w / 2.0;
            let y = bar.y + l.pad * 0.6;
            let photo = Rect::new(x, y, half, l.button);
            let video = Rect::new(x + half, y, half, l.button);
            let on_photo = self.capture_mode == CaptureMode::Photo;
            fill(
                f,
                photo,
                if on_photo { BLUE } else { SURFACE0 },
                CornerRadii {
                    top_left: 6.0,
                    top_right: 0.0,
                    bottom_right: 0.0,
                    bottom_left: 6.0,
                },
            );
            centred(
                f,
                photo,
                "Photo",
                if on_photo { CRUST } else { TEXT },
                l.small,
                FontWeightHint::Bold,
            );
            f.hit(Target::PhotoMode, photo);

            let on_video = self.capture_mode == CaptureMode::Video;
            fill(
                f,
                video,
                if on_video { RED } else { SURFACE0 },
                CornerRadii {
                    top_left: 0.0,
                    top_right: 6.0,
                    bottom_right: 6.0,
                    bottom_left: 0.0,
                },
            );
            centred(
                f,
                video,
                "Video",
                if on_video { CRUST } else { TEXT },
                l.small,
                FontWeightHint::Bold,
            );
            f.hit(Target::VideoMode, video);
            x += mode_w + l.pad;
        }

        // The shutter. Round, and the one control the program is named for.
        let d = l.button.min(avail(x));
        if d > 0.0 {
            let r = Rect::new(x, bar.y + (bar.h - d) / 2.0, d, d);
            let colour = match self.capture_mode {
                CaptureMode::Photo => BLUE,
                CaptureMode::Video if self.is_recording() => RED,
                CaptureMode::Video => PEACH,
            };
            fill(f, r, colour, CornerRadii::all(d / 2.0));
            let inner = inset(r, d * 0.18);
            match self.capture_mode {
                CaptureMode::Photo => fill(
                    f,
                    inner,
                    Color::rgba(255, 255, 255, 200),
                    CornerRadii::all(inner.w / 2.0),
                ),
                CaptureMode::Video if self.is_recording() => {
                    fill(f, inset(r, d * 0.3), CRUST, CornerRadii::all(3.0));
                }
                CaptureMode::Video => fill(f, inner, RED, CornerRadii::all(inner.w / 2.0)),
            }
            f.hit(Target::Shutter, r);
            x += d + l.pad;
        }

        // Timer and grid, each dropped rather than drawn past the budget.
        for (target, label, on, colour) in [
            (
                Target::Timer,
                self.timer_mode.label(),
                self.timer_mode != TimerMode::Off,
                YELLOW,
            ),
            (Target::Grid, "#", self.show_grid_overlay, BLUE),
        ] {
            let bw =
                (text::measure(label, l.small, FontWeightHint::Bold) + l.pad * 1.6).max(l.button);
            if bw > avail(x) {
                continue;
            }
            let r = Rect::new(x, bar.y + l.pad * 0.6, bw, l.button);
            fill(
                f,
                r,
                if on { SURFACE1 } else { SURFACE0 },
                CornerRadii::all(6.0),
            );
            centred(
                f,
                r,
                label,
                if on { colour } else { SUBTEXT0 },
                l.small,
                FontWeightHint::Bold,
            );
            f.hit(target, r);
            x += bw + l.pad * 0.5;
        }

        // The zoom read-out, last, and only if there is genuinely room for it.
        let zoom = self.settings.zoom_label();
        let zw = text::measure(&zoom, l.small, FontWeightHint::Regular);
        if zw <= avail(x) {
            bounded(
                f,
                Rect::new(x, bar.y, zw, bar.h),
                &zoom,
                SUBTEXT0,
                l.small,
                FontWeightHint::Regular,
            );
        }
    }

    // ------------------------------------------------------------------
    // Viewfinder
    // ------------------------------------------------------------------

    /// The live picture, its overlays, and the badges that sit on top of it.
    ///
    /// Everything drawn here is placed as a *fraction of the viewfinder*, not
    /// at an offset from the window origin. The previous version drew the
    /// recording badge at `(x + 16, y + 16)` and the histogram at
    /// `(right - 220, bottom - 140)` with the 220 and the 140 written down, so
    /// in a small window the histogram started off the left edge of its own
    /// pane and the two overlapped.
    fn draw_viewfinder(&self, f: &mut Frame<Target>, l: &Layout) {
        let v = l.viewfinder;
        if v.is_empty() {
            return;
        }
        fill(f, v, CRUST, CornerRadii::ZERO);
        f.hit(Target::Viewfinder, v);

        // The picture itself. There is no camera, so what is drawn is the
        // frame's own test pattern -- but it is drawn at the frame's aspect
        // ratio inside the pane, letterboxed, because that is what a real
        // preview does and a stretched preview is a lie about the sensor.
        let cam_res = self.active_camera().map_or(
            Resolution::new(1920, 1080),
            CameraDevice::current_resolution,
        );
        let picture = letterbox(v, cam_res.width as f32, cam_res.height as f32);
        if !picture.is_empty() {
            f.clip(picture);
            self.draw_test_pattern(f, picture);
            if self.show_grid_overlay {
                draw_thirds(f, picture);
            }
            f.unclip();
        }

        // A disconnected camera is the one state where the picture is not the
        // point: say so in words, over the top, rather than showing a plausible
        // pattern that is not coming from anywhere.
        let status = self
            .active_camera()
            .map_or(CameraStatus::Error, |c| c.status);
        if !status.is_active() {
            let band = Rect::new(v.x, v.y + (v.h - l.row) / 2.0, v.w, l.row);
            fill(f, band, Color::rgba(17, 17, 27, 220), CornerRadii::ZERO);
            centred(
                f,
                band,
                match status {
                    CameraStatus::Disconnected => "Camera disconnected",
                    _ => "Camera error",
                },
                status.color(),
                l.font,
                FontWeightHint::Bold,
            );
        }

        // Badges. Each is measured, and each is skipped if the pane is too
        // small to hold it -- a badge that does not fit is not shrunk, because
        // a two-pixel-tall badge tells the user less than no badge at all.
        let m = l.pad;
        if self.is_recording() || self.recording.state == RecordingState::Paused {
            let live = self.is_recording();
            let label = format!(
                "{} {}",
                if live { "REC" } else { "PAUSED" },
                self.recording.duration_label()
            );
            let bw = text::measure(&label, l.small, FontWeightHint::Bold) + m * 2.0;
            let bh = l.small + m;
            if bw <= v.w - m * 2.0 && bh <= v.h - m * 2.0 {
                let r = Rect::new(v.x + m, v.y + m, bw, bh);
                fill(
                    f,
                    r,
                    if live { RED } else { PEACH },
                    CornerRadii::all(bh / 2.0),
                );
                centred(f, r, &label, CRUST, l.small, FontWeightHint::Bold);
            }
        }

        // The zoom badge sits opposite the recording badge so the two can never
        // collide however wide the recording clock grows.
        if self.settings.zoom > 1.0 {
            let label = self.settings.zoom_label();
            let bw = text::measure(&label, l.small, FontWeightHint::Bold) + m * 2.0;
            let bh = l.small + m;
            if bw <= v.w - m * 2.0 && bh <= v.h - m * 2.0 {
                let r = Rect::new(v.right() - m - bw, v.y + m, bw, bh);
                fill(
                    f,
                    r,
                    Color::rgba(17, 17, 27, 190),
                    CornerRadii::all(bh / 2.0),
                );
                centred(f, r, &label, TEXT, l.small, FontWeightHint::Bold);
            }
        }

        if self.show_histogram {
            self.draw_histogram(f, l, v);
        }
    }

    /// The stand-in for a sensor: a coarse grid of cells whose colour walks
    /// with the frame counter, so a running program is visibly running.
    ///
    /// The cell count is derived from the pane, not fixed, because a fixed
    /// 16-across grid in a 200 px pane is twelve-pixel cells.
    fn draw_test_pattern(&self, f: &mut Frame<Target>, r: Rect) {
        // A count of cells is not signed, and `as usize` on a NaN or negative
        // pane width saturates to 0 rather than wrapping to `usize::MAX`, so
        // the clamp still has something sane to clamp.
        let cols = ((r.w / 90.0).round() as usize).clamp(3, 16);
        let rows = ((r.h / 90.0).round() as usize).clamp(3, 12);
        let cw = r.w / usize_f32(cols);
        let ch = r.h / usize_f32(rows);
        let phase = (self.frame_counter % 64) as f32 / 64.0;
        for row in 0..rows {
            for col in 0..cols {
                let fx = (usize_f32(col) + 0.5) / usize_f32(cols);
                let fy = (usize_f32(row) + 0.5) / usize_f32(rows);
                let base = (
                    (30.0 + fx * 150.0),
                    (30.0 + fy * 120.0),
                    (46.0 + (1.0 - fx) * 90.0),
                );
                // The filter is applied to the preview, not only to the saved
                // photo. A preview that ignores the filter is a preview of a
                // picture the program is not going to take.
                let (cr, cg, cb) = self.settings.active_filter.transform_pixel(
                    channel(base.0 + phase * 20.0),
                    channel(base.1 + phase * 12.0),
                    channel(base.2),
                );
                fill(
                    f,
                    Rect::new(r.x + usize_f32(col) * cw, r.y + usize_f32(row) * ch, cw, ch),
                    Color::rgb(cr, cg, cb),
                    CornerRadii::ZERO,
                );
            }
        }
    }

    /// The brightness histogram, drawn inside the viewfinder's bottom-right
    /// corner and clamped to a share of the pane rather than a pixel count.
    fn draw_histogram(&self, f: &mut Frame<Target>, l: &Layout, v: Rect) {
        let w = (v.w * 0.3).clamp(0.0, 220.0);
        let h = (v.h * 0.25).clamp(0.0, 130.0);
        if w < l.small * 4.0 || h < l.small * 2.0 {
            return;
        }
        let r = Rect::new(v.right() - l.pad - w, v.bottom() - l.pad - h, w, h);
        fill(f, r, Color::rgba(17, 17, 27, 200), CornerRadii::all(4.0));

        // Sixteen bins, shaped by the brightness and contrast settings so the
        // picture responds to the sliders beside it.
        let bins = 16usize;
        let bw = r.w / usize_f32(bins);
        let bright = self.settings.brightness as f32 / 100.0;
        let contrast = self.settings.contrast as f32 / 100.0;
        for i in 0..bins {
            let t = (usize_f32(i) + 0.5) / usize_f32(bins);
            let centre = bright.clamp(0.05, 0.95);
            let spread = (1.05 - contrast).max(0.12);
            let d = (t - centre) / spread;
            let mag = (-(d * d)).exp().clamp(0.02, 1.0);
            let bh = (r.h - 2.0) * mag;
            fill(
                f,
                Rect::new(
                    r.x + usize_f32(i) * bw,
                    r.bottom() - 1.0 - bh,
                    (bw - 1.0).max(1.0),
                    bh,
                ),
                TEAL,
                CornerRadii::ZERO,
            );
        }
        f.push(RenderCommand::StrokeRect {
            x: r.x,
            y: r.y,
            width: r.w,
            height: r.h,
            color: SURFACE1,
            corner_radii: CornerRadii::all(4.0),
            line_width: 1.0,
        });
    }

    // ------------------------------------------------------------------
    // Photo strip
    // ------------------------------------------------------------------

    /// The row of recent photos under the viewfinder.
    ///
    /// The number of thumbnails is a consequence of the width, not a constant:
    /// the previous version drew a fixed six at 90 px each and let the seventh
    /// onward run off the pane, which in a narrow window meant every thumbnail
    /// after the second was invisible but still counted.
    fn draw_strip(&self, f: &mut Frame<Target>, l: &Layout) {
        let s = l.strip;
        if s.is_empty() {
            return;
        }
        fill(f, s, MANTLE, CornerRadii::ZERO);
        f.push(RenderCommand::Line {
            x1: s.x,
            y1: s.y,
            x2: s.right(),
            y2: s.y,
            color: SURFACE0,
            width: 1.0,
        });

        if self.gallery.photos.is_empty() {
            bounded(
                f,
                Rect::new(s.x + l.pad, s.y, s.w - l.pad * 2.0, s.h),
                "No photos yet -- press Space to take one",
                OVERLAY0,
                l.small,
                FontWeightHint::Regular,
            );
            return;
        }

        let inner_h = (s.h - l.pad * 2.0).max(0.0);
        let tile_w = (inner_h * 1.4).max(l.small * 3.0);
        let step = tile_w + l.pad * 0.6;
        if step <= 0.0 || inner_h <= 0.0 {
            return;
        }
        let fits = ((s.w - l.pad * 2.0 + l.pad * 0.6) / step).floor();
        let fits = if fits <= 0.0 { 0usize } else { fits as usize };
        if fits == 0 {
            return;
        }

        // Which window of the gallery is shown is chosen so the *selected*
        // photo is in it. A strip that always starts at zero cannot show the
        // photo just taken once there are more photos than tiles -- which is
        // exactly when the strip starts mattering.
        let total = self.gallery.photos.len();
        let sel = self.gallery.selected_idx.unwrap_or(0);
        let start = if total <= fits {
            0
        } else {
            sel.saturating_sub(fits.saturating_sub(1))
                .min(total.saturating_sub(fits))
        };

        f.clip(s);
        for slot in 0..fits {
            let idx = start.saturating_add(slot);
            let Some(photo) = self.gallery.photos.get(idx) else {
                break;
            };
            let r = Rect::new(
                s.x + l.pad + usize_f32(slot) * step,
                s.y + l.pad,
                tile_w,
                inner_h,
            );
            let chosen = self.gallery.selected_idx == Some(idx);
            fill(
                f,
                r,
                if chosen { SURFACE1 } else { SURFACE0 },
                CornerRadii::all(4.0),
            );
            if chosen {
                f.push(RenderCommand::StrokeRect {
                    x: r.x,
                    y: r.y,
                    width: r.w,
                    height: r.h,
                    color: BLUE,
                    corner_radii: CornerRadii::all(4.0),
                    line_width: 2.0,
                });
            }
            // A star, if it is a favourite, in the corner rather than in the
            // caption -- the caption is the first thing the width takes away.
            if photo.favorite {
                let d = (inner_h * 0.3).min(l.small);
                bounded(
                    f,
                    Rect::new(r.right() - d - 2.0, r.y + 1.0, d, d),
                    "*",
                    YELLOW,
                    l.small,
                    FontWeightHint::Bold,
                );
            }
            bounded(
                f,
                Rect::new(r.x + 3.0, r.y, r.w - 6.0, r.h),
                &photo.display_name(),
                if chosen { TEXT } else { SUBTEXT0 },
                l.small,
                FontWeightHint::Regular,
            );
            f.hit(Target::Photo(idx), r);
        }
        f.unclip();
    }

    // ------------------------------------------------------------------
    // Status line
    // ------------------------------------------------------------------

    /// The one line along the bottom: what just happened on the left, the
    /// standing facts on the right.
    ///
    /// The right-hand facts are placed first and the message gets what is left,
    /// so a long status message elides rather than running under the counts.
    fn draw_status(&self, f: &mut Frame<Target>, l: &Layout) {
        let s = l.status;
        if s.is_empty() {
            return;
        }
        fill(f, s, MANTLE, CornerRadii::ZERO);
        f.push(RenderCommand::Line {
            x1: s.x,
            y1: s.y,
            x2: s.right(),
            y2: s.y,
            color: SURFACE0,
            width: 1.0,
        });

        let facts = format!(
            "{} photo(s) - {} - {}",
            self.gallery.count(),
            format_bytes(self.gallery.total_size()),
            self.active_camera().map_or_else(
                || "no device".to_string(),
                |c| c.current_resolution().label()
            )
        );
        let fw = text::measure(&facts, l.small, FontWeightHint::Regular);
        let mut left_w = s.w - l.pad * 2.0;
        if fw <= (s.w - l.pad * 3.0) * 0.65 {
            bounded(
                f,
                Rect::new(s.right() - l.pad - fw, s.y, fw, s.h),
                &facts,
                SUBTEXT0,
                l.small,
                FontWeightHint::Regular,
            );
            left_w = (s.w - l.pad * 3.0 - fw).max(0.0);
        }

        let (msg, colour) = match (&self.status_message, self.timer_countdown.active) {
            (_, true) => (
                format!("Timer: {}s", self.timer_countdown.remaining_seconds()),
                YELLOW,
            ),
            (Some(m), _) => (m.clone(), TEXT),
            (None, _) => (
                match self.capture_mode {
                    CaptureMode::Photo => "Ready".to_string(),
                    CaptureMode::Video if self.is_recording() => "Recording".to_string(),
                    CaptureMode::Video => "Ready to record".to_string(),
                },
                SUBTEXT0,
            ),
        };
        if left_w > 0.0 {
            bounded(
                f,
                Rect::new(s.x + l.pad, s.y, left_w, s.h),
                &msg,
                colour,
                l.small,
                FontWeightHint::Regular,
            );
        }
    }

    // ------------------------------------------------------------------
    // Overlays
    // ------------------------------------------------------------------

    /// The self-timer's count, over the middle of the viewfinder.
    fn draw_timer_overlay(&self, f: &mut Frame<Target>, l: &Layout) {
        let v = l.viewfinder;
        if v.is_empty() {
            return;
        }
        let d = v.w.min(v.h) * 0.4;
        if d <= 0.0 {
            return;
        }
        let r = Rect::new(v.x + (v.w - d) / 2.0, v.y + (v.h - d) / 2.0, d, d);
        fill(
            f,
            r,
            Color::rgba(17, 17, 27, 210),
            CornerRadii::all(d / 2.0),
        );
        f.push(RenderCommand::StrokeRect {
            x: r.x,
            y: r.y,
            width: r.w,
            height: r.h,
            color: YELLOW,
            corner_radii: CornerRadii::all(d / 2.0),
            line_width: 3.0,
        });
        // The digit is sized from the circle, so it fills it at any window
        // size instead of being a fixed 64 pt that overflows a small one.
        centred(
            f,
            r,
            &self.timer_countdown.display(),
            YELLOW,
            (d * 0.45).max(l.font),
            FontWeightHint::Bold,
        );
    }

    /// The white flash after the shutter, over everything.
    ///
    /// Its opacity is what is left of [`FLASH_MS`], so the flash *fades* rather
    /// than blinking on and off -- and a frame drawn after it has expired is
    /// not drawn at all, which is why `frame` guards on the remaining time.
    fn draw_flash(&self, f: &mut Frame<Target>, l: &Layout) {
        let t = (self.flash_remaining_ms.min(FLASH_MS) as f32) / (FLASH_MS as f32);
        let alpha = (t * 220.0) as u8;
        fill(
            f,
            l.window,
            Color::rgba(255, 255, 255, alpha),
            CornerRadii::ZERO,
        );
    }

    // ------------------------------------------------------------------
    // Sidebar
    // ------------------------------------------------------------------

    /// The right-hand pane: a row of tabs and whichever panel they chose.
    ///
    /// The tabs are laid out by dividing the pane, so four tabs fit a narrow
    /// sidebar as four narrow tabs rather than three tabs and one off the edge.
    /// Each panel is then given the rectangle *below* the tabs and clips to it,
    /// so a panel with more rows than room shows the rows it has room for
    /// instead of painting over the strip below.
    fn draw_sidebar(&self, f: &mut Frame<Target>, l: &Layout) {
        let s = l.sidebar;
        if s.is_empty() {
            return;
        }
        fill(f, s, MANTLE, CornerRadii::ZERO);
        f.push(RenderCommand::Line {
            x1: s.x,
            y1: s.y,
            x2: s.x,
            y2: s.bottom(),
            color: SURFACE0,
            width: 1.0,
        });

        let tabs = SidebarPanel::all();
        let tab_h = (l.row * 0.85).min(s.h);
        let tab_w = s.w / usize_f32(tabs.len());
        for (i, panel) in tabs.iter().enumerate() {
            let r = Rect::new(s.x + usize_f32(i) * tab_w, s.y, tab_w, tab_h);
            let on = self.sidebar_panel == *panel;
            fill(f, r, if on { SURFACE1 } else { MANTLE }, CornerRadii::ZERO);
            if on {
                fill(
                    f,
                    Rect::new(r.x, r.bottom() - 2.0, r.w, 2.0),
                    BLUE,
                    CornerRadii::ZERO,
                );
            }
            // The tab label is the panel's own name, elided to the tab. Four
            // tabs in a 150 px sidebar is 37 px each, which fits "Sett…" and
            // nothing more -- and that is still four reachable tabs.
            bounded(
                f,
                r,
                panel.label(),
                if on { TEXT } else { SUBTEXT0 },
                l.small,
                if on {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
            );
            f.hit(Target::Panel(*panel), r);
        }

        let body = Rect::new(s.x, s.y + tab_h, s.w, (s.h - tab_h).max(0.0));
        if body.is_empty() {
            return;
        }
        f.clip(body);
        match self.sidebar_panel {
            SidebarPanel::Settings => self.draw_settings_panel(f, l, body),
            SidebarPanel::DeviceInfo => self.draw_device_panel(f, l, body),
            SidebarPanel::Filters => self.draw_filters_panel(f, l, body),
            SidebarPanel::Gallery => self.draw_gallery_panel(f, l, body),
        }
        f.unclip();
    }

    /// The sliders.
    ///
    /// Each row is a name, a value, and a pair of ends that step it. The ends
    /// are square and sized from the row, so they stay clickable in a narrow
    /// sidebar where a fixed 24 px button would have overlapped the name.
    fn draw_settings_panel(&self, f: &mut Frame<Target>, l: &Layout, body: Rect) {
        let mut y = body.y + l.pad * 0.5;
        let bw = (l.row * 0.6).min(body.w / 4.0);
        for setting in Setting::all() {
            if y + l.row > body.bottom() {
                break;
            }
            let row = Rect::new(body.x + l.pad * 0.5, y, body.w - l.pad, l.row);

            // Name on the top half, value and the two ends on the bottom, so a
            // narrow sidebar does not have to fit all four on one line.
            let name_h = row.h * 0.5;
            bounded(
                f,
                Rect::new(row.x, row.y, row.w, name_h),
                setting.label(),
                SUBTEXT0,
                l.small,
                FontWeightHint::Regular,
            );

            let lower = Rect::new(row.x, row.y + name_h, row.w, row.h - name_h);
            let down = Rect::new(lower.x, lower.y, bw, lower.h);
            let up = Rect::new(lower.right() - bw, lower.y, bw, lower.h);
            for (r, label, nudge) in [(down, "-", Nudge::Down), (up, "+", Nudge::Up)] {
                fill(f, r, SURFACE0, CornerRadii::all(3.0));
                centred(f, r, label, TEXT, l.small, FontWeightHint::Bold);
                f.hit(Target::Setting(*setting, nudge), r);
            }

            let mid = Rect::new(
                down.right() + 2.0,
                lower.y,
                (up.x - down.right() - 4.0).max(0.0),
                lower.h,
            );
            if !mid.is_empty() {
                // The bar behind the value is the value: it is what tells the
                // user that 62 is nearly two thirds without their having to
                // remember the range.
                let frac = self.setting_fraction(*setting);
                fill(f, mid, SURFACE0, CornerRadii::all(2.0));
                fill(
                    f,
                    Rect::new(mid.x, mid.y, mid.w * frac, mid.h),
                    LAVENDER,
                    CornerRadii::all(2.0),
                );
                centred(
                    f,
                    mid,
                    &self.setting_value(*setting),
                    CRUST,
                    l.small,
                    FontWeightHint::Bold,
                );
            }
            y += l.row;
        }

        // The toggles that are not sliders, if there is room left for them.
        for (label, on) in [
            ("Auto White Balance", self.settings.auto_white_balance),
            ("Autofocus", self.settings.autofocus),
            ("Mirror", self.settings.mirror_mode),
            ("Flip H", self.settings.flip_horizontal),
            ("Flip V", self.settings.flip_vertical),
        ] {
            let h = l.row * 0.6;
            if y + h > body.bottom() {
                break;
            }
            let r = Rect::new(body.x + l.pad * 0.5, y, body.w - l.pad, h);
            bounded(
                f,
                r,
                &format!("{} {}", if on { "[x]" } else { "[ ]" }, label),
                if on { TEXT } else { OVERLAY0 },
                l.small,
                FontWeightHint::Regular,
            );
            y += h;
        }
    }

    /// The device panel: what the camera is, and the two things about it that
    /// can be changed.
    fn draw_device_panel(&self, f: &mut Frame<Target>, l: &Layout, body: Rect) {
        let Some(cam) = self.active_camera() else {
            bounded(
                f,
                Rect::new(body.x + l.pad, body.y, body.w - l.pad * 2.0, l.row),
                "No camera",
                OVERLAY0,
                l.small,
                FontWeightHint::Regular,
            );
            return;
        };

        let line_h = l.row * 0.6;
        let mut y = body.y + l.pad * 0.5;
        for line in cam.info_lines() {
            if y + line_h > body.bottom() {
                return;
            }
            bounded(
                f,
                Rect::new(body.x + l.pad * 0.5, y, body.w - l.pad, line_h),
                &line,
                SUBTEXT0,
                l.small,
                FontWeightHint::Regular,
            );
            y += line_h;
        }

        y += l.pad * 0.5;

        let resolutions: Vec<String> = cam
            .supported_resolutions
            .iter()
            .map(Resolution::label)
            .collect();
        y = draw_choices(
            f,
            l,
            body,
            y,
            "Resolution",
            &resolutions,
            cam.current_resolution_idx,
            Target::Resolution,
        );

        let framerates: Vec<String> = FRAME_RATES.iter().map(|r| format!("{r} fps")).collect();
        let chosen = FRAME_RATES
            .iter()
            .position(|r| *r == cam.framerate)
            .unwrap_or(usize::MAX);
        let _ = draw_choices(
            f,
            l,
            body,
            y,
            "Frame Rate",
            &framerates,
            chosen,
            Target::Framerate,
        );
    }

    /// The filter list. One row each, the active one marked.
    fn draw_filters_panel(&self, f: &mut Frame<Target>, l: &Layout, body: Rect) {
        let mut y = body.y + l.pad * 0.5;
        for filter in ImageFilter::all() {
            if y + l.row * 0.7 > body.bottom() {
                break;
            }
            let r = Rect::new(body.x + l.pad * 0.5, y, body.w - l.pad, l.row * 0.7);
            let on = self.settings.active_filter == *filter;
            fill(
                f,
                r,
                if on { SURFACE1 } else { SURFACE0 },
                CornerRadii::all(4.0),
            );
            bounded(
                f,
                Rect::new(r.x + l.pad * 0.5, r.y, r.w - l.pad, r.h),
                filter.label(),
                if on { MAUVE } else { SUBTEXT0 },
                l.small,
                if on {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
            );
            f.hit(Target::Filter(*filter), r);
            y += l.row * 0.7 + 2.0;
        }
    }

    /// The gallery panel: the selected photo's facts, then the list.
    ///
    /// The list scrolls to keep the selection visible for the same reason the
    /// strip does -- see `draw_strip`.
    fn draw_gallery_panel(&self, f: &mut Frame<Target>, l: &Layout, body: Rect) {
        let line_h = l.row * 0.6;
        let mut y = body.y + l.pad * 0.5;

        if let Some(photo) = self.gallery.selected_photo() {
            for line in [
                photo.display_name(),
                photo.resolution_label(),
                photo.size_label(),
                format!("Filter: {}", photo.filter.label()),
                format!("Taken: {}", photo.time_label()),
            ] {
                if y + line_h > body.bottom() {
                    return;
                }
                bounded(
                    f,
                    Rect::new(body.x + l.pad * 0.5, y, body.w - l.pad, line_h),
                    &line,
                    SUBTEXT0,
                    l.small,
                    FontWeightHint::Regular,
                );
                y += line_h;
            }

            let bh = l.row * 0.7;
            if y + bh <= body.bottom() {
                let half = (body.w - l.pad * 1.5) / 2.0;
                let fav = Rect::new(body.x + l.pad * 0.5, y, half, bh);
                let del = Rect::new(fav.right() + l.pad * 0.5, y, half, bh);
                fill(
                    f,
                    fav,
                    if photo.favorite { YELLOW } else { SURFACE0 },
                    CornerRadii::all(4.0),
                );
                centred(
                    f,
                    fav,
                    "Favourite",
                    if photo.favorite { CRUST } else { TEXT },
                    l.small,
                    FontWeightHint::Bold,
                );
                f.hit(Target::Favorite, fav);
                fill(f, del, SURFACE0, CornerRadii::all(4.0));
                centred(f, del, "Delete", RED, l.small, FontWeightHint::Bold);
                f.hit(Target::Delete, del);
                y += bh + l.pad * 0.5;
            }
        } else {
            if y + line_h > body.bottom() {
                return;
            }
            bounded(
                f,
                Rect::new(body.x + l.pad * 0.5, y, body.w - l.pad, line_h),
                "No photo selected",
                OVERLAY0,
                l.small,
                FontWeightHint::Regular,
            );
            y += line_h;
        }

        let rows = l.rows_in(body, y - body.y);
        if rows == 0 {
            return;
        }
        let total = self.gallery.photos.len();
        let sel = self.gallery.selected_idx.unwrap_or(0);
        let start = if total <= rows {
            0
        } else {
            sel.saturating_sub(rows.saturating_sub(1))
                .min(total.saturating_sub(rows))
        };
        for slot in 0..rows {
            let idx = start.saturating_add(slot);
            let Some(photo) = self.gallery.photos.get(idx) else {
                break;
            };
            let r = Rect::new(body.x + l.pad * 0.5, y, body.w - l.pad, l.row);
            let on = self.gallery.selected_idx == Some(idx);
            if on {
                fill(f, r, SURFACE1, CornerRadii::all(3.0));
            }
            bounded(
                f,
                Rect::new(r.x + l.pad * 0.5, r.y, r.w - l.pad, r.h),
                &format!(
                    "{}{}",
                    if photo.favorite { "* " } else { "" },
                    photo.display_name()
                ),
                if on { TEXT } else { SUBTEXT0 },
                l.small,
                FontWeightHint::Regular,
            );
            f.hit(Target::Photo(idx), r);
            y += l.row;
        }
    }

    // ------------------------------------------------------------------
    // Settings, as the panel sees them
    // ------------------------------------------------------------------

    /// The value of a slider, as the panel prints it.
    fn setting_value(&self, s: Setting) -> String {
        match s {
            Setting::Brightness => self.settings.brightness.to_string(),
            Setting::Contrast => self.settings.contrast.to_string(),
            Setting::Saturation => self.settings.saturation.to_string(),
            Setting::Exposure => self.settings.exposure_label(),
            Setting::WhiteBalance => self.settings.wb_label(),
            Setting::NoiseReduction => self.settings.noise_reduction_label().to_string(),
            Setting::Zoom => self.settings.zoom_label(),
        }
    }

    /// How far along its own range a slider is, in `0.0..=1.0`.
    ///
    /// Every setting has a different range and two of them do not start at
    /// zero, so the bar cannot be drawn from the raw value -- which is what the
    /// previous version's settings rows did not draw at all.
    fn setting_fraction(&self, s: Setting) -> f32 {
        let (v, lo, hi) = match s {
            Setting::Brightness => (self.settings.brightness as f32, 0.0, 100.0),
            Setting::Contrast => (self.settings.contrast as f32, 0.0, 100.0),
            Setting::Saturation => (self.settings.saturation as f32, 0.0, 100.0),
            Setting::Exposure => (self.settings.exposure as f32, -5.0, 5.0),
            Setting::WhiteBalance => (self.settings.white_balance as f32, 2500.0, 10000.0),
            Setting::NoiseReduction => (self.settings.noise_reduction as f32, 0.0, 3.0),
            Setting::Zoom => (self.settings.zoom, 1.0, 10.0),
        };
        if hi <= lo {
            return 0.0;
        }
        ((v - lo) / (hi - lo)).clamp(0.0, 1.0)
    }

    /// Step a slider one notch.
    ///
    /// Each step goes through the setter that owns the range, so no caller can
    /// push a value out of it -- the panel does not know what the ranges are
    /// and must not have to.
    pub fn nudge(&mut self, s: Setting, n: Nudge) {
        let up = n == Nudge::Up;
        match s {
            Setting::Brightness => {
                let v = step_u32(self.settings.brightness, up, 5, 100);
                self.settings.set_brightness(v);
            }
            Setting::Contrast => {
                let v = step_u32(self.settings.contrast, up, 5, 100);
                self.settings.set_contrast(v);
            }
            Setting::Saturation => {
                let v = step_u32(self.settings.saturation, up, 5, 100);
                self.settings.set_saturation(v);
            }
            Setting::Exposure => {
                let v = if up {
                    self.settings.exposure.saturating_add(1)
                } else {
                    self.settings.exposure.saturating_sub(1)
                };
                self.settings.set_exposure(v);
            }
            Setting::WhiteBalance => {
                // Nudging the temperature is also the way off Auto: a user who
                // reaches for the value is asking for the value, and leaving
                // Auto on would silently discard the change.
                self.settings.auto_white_balance = false;
                let v = step_u32(self.settings.white_balance, up, 250, 10000);
                self.settings.set_white_balance(v.max(2500));
            }
            Setting::NoiseReduction => {
                self.settings.noise_reduction = step_u32(self.settings.noise_reduction, up, 1, 3);
            }
            Setting::Zoom => {
                if up {
                    self.settings.zoom_in();
                } else {
                    self.settings.zoom_out();
                }
            }
        }
        let label = self.setting_value(s);
        self.set_status(&format!("{}: {label}", s.label()));
    }

    // ------------------------------------------------------------------
    // Events
    // ------------------------------------------------------------------

    /// Answer one event from the window.
    ///
    /// Every event the program acts on arrives here, including the clock.
    /// `Event::Tick` is not decoration: without it the viewfinder is a still
    /// picture, the recording clock reads `00:00` for the life of the process,
    /// and a self-timer started with `T` never fires -- while the window still
    /// lays out, still repaints on resize and still answers the keyboard, so
    /// nothing looks wrong (known-issues.md lesson 102).
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Resize { width, height } => {
                // The remembered size is only ever *written* here. Everything
                // that draws takes the size it is given, so a stale field can
                // no longer be the thing a picture is laid out from.
                self.width = f32_from_u32(*width);
                self.height = f32_from_u32(*height);
                EventResult::Consumed
            }
            Event::Tick { elapsed_ms } => {
                if self.tick(*elapsed_ms) {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::Key(key) => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            _ => EventResult::Ignored,
        }
    }

    /// Answer one key.
    ///
    /// The letters are read from `text`, not from the key code, because a key
    /// code is a *position on a keyboard* and the letter it produces depends on
    /// the layout: `Key::H` is wherever H is on a QWERTY board, and on a Dvorak
    /// one that position is J.
    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        // A release is not a keystroke. Acting on both halves would fire every
        // shortcut twice, which for the shutter is two photographs.
        if !key.pressed {
            return EventResult::Ignored;
        }
        let ctrl = key.modifiers.ctrl;
        if ctrl {
            let letter = key.text.chars().next().map(|c| c.to_ascii_lowercase());
            match letter {
                Some('h') => {
                    self.settings.toggle_flip_horizontal();
                    self.set_status("Flip horizontal");
                    return EventResult::Consumed;
                }
                Some('v') => {
                    self.settings.toggle_flip_vertical();
                    self.set_status("Flip vertical");
                    return EventResult::Consumed;
                }
                Some('m') => {
                    self.settings.toggle_mirror();
                    self.set_status("Mirror");
                    return EventResult::Consumed;
                }
                Some('r') => {
                    self.settings.reset();
                    self.set_status("Settings reset");
                    return EventResult::Consumed;
                }
                Some('s') => {
                    // "Save" is what the gallery already is: there is no disk
                    // behind this program. Saying so is better than a button
                    // that reports success for something that did not happen.
                    let msg = match self.gallery.selected_photo() {
                        Some(p) => format!("Saved {}", p.display_name()),
                        None => "Nothing to save".to_string(),
                    };
                    self.set_status(&msg);
                    return EventResult::Consumed;
                }
                _ => return EventResult::Ignored,
            }
        }

        match key.key {
            Key::Escape => {
                if self.timer_countdown.active {
                    self.cancel_timer();
                } else {
                    self.clear_status();
                }
                return EventResult::Consumed;
            }
            Key::Enter | Key::Space => {
                match self.capture_mode {
                    CaptureMode::Photo => self.take_photo(),
                    CaptureMode::Video => self.toggle_recording(),
                }
                return EventResult::Consumed;
            }
            Key::Tab => {
                self.next_camera();
                return EventResult::Consumed;
            }
            Key::Left => {
                self.gallery.select_prev();
                return EventResult::Consumed;
            }
            Key::Right => {
                self.gallery.select_next();
                return EventResult::Consumed;
            }
            Key::Delete => {
                if self.gallery.selected_photo().is_some() {
                    self.gallery.delete_selected();
                    self.set_status("Photo deleted");
                    return EventResult::Consumed;
                }
                return EventResult::Ignored;
            }
            _ => {}
        }

        let Some(c) = key.text.chars().next() else {
            return EventResult::Ignored;
        };
        // A filter is chosen by its position in the list, so the digits stay
        // right when a filter is added -- the previous version matched each
        // digit to a named filter in a second, separate list that had to be
        // kept in step with the first by hand.
        if let Some(d) = c.to_digit(10) {
            if d == 0 {
                self.settings.set_zoom(1.0);
                self.set_status("Zoom reset");
                return EventResult::Consumed;
            }
            // Digit `n` picks the n-th filter, so the index is one less: `1`
            // is the first entry in the list, not the second.
            let nth = d.checked_sub(1).and_then(|i| usize::try_from(i).ok());
            if let Some(filter) = nth.and_then(|i| ImageFilter::all().get(i)) {
                self.settings.active_filter = *filter;
                self.set_status(&format!("Filter: {}", filter.label()));
                return EventResult::Consumed;
            }
            return EventResult::Ignored;
        }

        match c.to_ascii_lowercase() {
            'r' => self.toggle_recording(),
            'p' => self.pause_recording(),
            't' => {
                self.cycle_timer();
                let label = self.timer_mode.label();
                self.set_status(&format!("Timer: {label}"));
            }
            'm' => self.toggle_capture_mode(),
            'g' => self.toggle_grid_overlay(),
            'h' => self.toggle_histogram(),
            's' => self.toggle_sidebar(),
            'b' => self.toggle_photo_strip(),
            'f' => self.toggle_fullscreen_preview(),
            '+' | '=' => {
                self.settings.zoom_in();
                let label = self.settings.zoom_label();
                self.set_status(&format!("Zoom {label}"));
            }
            '-' | '_' => {
                self.settings.zoom_out();
                let label = self.settings.zoom_label();
                self.set_status(&format!("Zoom {label}"));
            }
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }

    /// Answer one mouse event, by asking the picture what is under it.
    ///
    /// The picture is drawn at the remembered size, which is the size the last
    /// `Resize` reported -- the same size the last frame was drawn at, so the
    /// boxes a click is tested against are the boxes the user is looking at.
    fn handle_mouse(&mut self, mouse: &MouseEvent) -> EventResult {
        if mouse.kind != MouseEventKind::Press(MouseButton::Left) {
            return EventResult::Ignored;
        }
        let frame = self.frame(self.width, self.height);
        match frame.hit_test(mouse.x, mouse.y) {
            Some(target) => {
                self.activate(target);
                EventResult::Consumed
            }
            None => EventResult::Ignored,
        }
    }

    /// Do what a control does. One funnel, so a control cannot be reachable by
    /// the mouse and mean something different from the same control reached any
    /// other way.
    pub fn activate(&mut self, target: Target) {
        match target {
            Target::PhotoMode => self.set_capture_mode(CaptureMode::Photo),
            Target::VideoMode => self.set_capture_mode(CaptureMode::Video),
            Target::Shutter | Target::Viewfinder => match self.capture_mode {
                CaptureMode::Photo => self.take_photo(),
                CaptureMode::Video => self.toggle_recording(),
            },
            Target::Timer => {
                self.cycle_timer();
                let label = self.timer_mode.label();
                self.set_status(&format!("Timer: {label}"));
            }
            Target::Grid => self.toggle_grid_overlay(),
            Target::Histogram => self.toggle_histogram(),
            Target::Sidebar => self.toggle_sidebar(),
            Target::Strip => self.toggle_photo_strip(),
            Target::NextCamera => self.next_camera(),
            Target::Panel(panel) => self.set_sidebar_panel(panel),
            Target::Filter(filter) => {
                self.settings.active_filter = filter;
                self.set_status(&format!("Filter: {}", filter.label()));
            }
            Target::Resolution(idx) => {
                if let Some(cam) = self.active_camera_mut() {
                    cam.set_resolution_idx(idx);
                }
                let label = self
                    .active_camera()
                    .map_or_else(String::new, |c| c.current_resolution().label());
                self.set_status(&format!("Resolution: {label}"));
            }
            Target::Framerate(idx) => {
                if let Some(fps) = FRAME_RATES.get(idx).copied()
                    && let Some(cam) = self.active_camera_mut()
                {
                    cam.set_framerate(fps);
                }
                let fps = self.active_camera().map_or(0, |c| c.framerate);
                self.set_status(&format!("Frame rate: {fps} fps"));
            }
            Target::Photo(idx) => {
                if idx < self.gallery.photos.len() {
                    self.gallery.selected_idx = Some(idx);
                }
            }
            Target::Favorite => self.gallery.toggle_favorite_selected(),
            Target::Delete => {
                if self.gallery.selected_photo().is_some() {
                    self.gallery.delete_selected();
                    self.set_status("Photo deleted");
                }
            }
            Target::Setting(setting, nudge) => self.nudge(setting, nudge),
        }
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
// Drawing primitives
// ============================================================================

/// A filled rectangle, skipped when it would enclose nothing.
///
/// The skip is not an optimisation. A zero-width fill paints no pixels either
/// way, but it is still a command in the frame, and a test that asks the
/// picture what is in a pane would find one -- so a pane that was squeezed out
/// of existence would answer "something is drawn here". See known-issues.md
/// lesson 103.
fn fill(f: &mut Frame<Target>, r: Rect, color: Color, corner_radii: CornerRadii) {
    if r.is_empty() {
        return;
    }
    f.push(RenderCommand::FillRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        corner_radii,
    });
}

/// One run of text inside `r`: left-aligned, vertically centred, and cut with
/// an ellipsis rather than allowed to run past the box.
///
/// Every string this program draws goes through here or through [`centred`].
/// The previous version passed raw `(x, y)` pairs with no width at all, so
/// every label was drawn at its full natural width whatever was to its right;
/// a long camera name wrote over the mode buttons and a long status message
/// wrote over the photo count.
fn bounded(
    f: &mut Frame<Target>,
    r: Rect,
    s: &str,
    color: Color,
    size: f32,
    weight: FontWeightHint,
) {
    if r.w <= 0.0 || !r.w.is_finite() || s.is_empty() {
        return;
    }
    let lh = text::line_height(size, weight);
    f.push(RenderCommand::Text {
        x: r.x,
        y: r.y + ((r.h - lh) / 2.0).max(0.0),
        text: String::from(s),
        color,
        font_size: size,
        font_weight: weight,
        max_width: Some(r.w),
        overflow: TextOverflow::Ellipsis,
    });
}

/// One run of text centred in `r`, horizontally as well as vertically.
///
/// The offset is floored at zero and the bound handed on is the room left from
/// where the run starts, so a run too wide to centre begins at the left edge
/// and still elides at the box's own right edge rather than `offset` pixels
/// past it.
fn centred(
    f: &mut Frame<Target>,
    r: Rect,
    s: &str,
    color: Color,
    size: f32,
    weight: FontWeightHint,
) {
    let offset = ((r.w - text::measure(s, size, weight)) / 2.0).max(0.0);
    bounded(
        f,
        Rect::new(r.x + offset, r.y, (r.w - offset).max(0.0), r.h),
        s,
        color,
        size,
        weight,
    );
}

/// `r` shrunk by `d` on every side, never past nothing.
fn inset(r: Rect, d: f32) -> Rect {
    Rect::new(
        r.x + d,
        r.y + d,
        (r.w - d * 2.0).max(0.0),
        (r.h - d * 2.0).max(0.0),
    )
}

/// The largest `aw` x `ah`-shaped rectangle that fits centred inside `r`.
///
/// A preview stretched to its pane is a lie about the sensor: it shows the
/// user a framing they will not get. Letterboxing is what a real viewfinder
/// does, and it is why the sensor's resolution is read here rather than the
/// pane's shape being assumed to match it.
fn letterbox(r: Rect, aw: f32, ah: f32) -> Rect {
    if r.is_empty() || aw <= 0.0 || ah <= 0.0 {
        return Rect::EMPTY;
    }
    let scale = (r.w / aw).min(r.h / ah);
    let w = aw * scale;
    let h = ah * scale;
    Rect::new(r.x + (r.w - w) / 2.0, r.y + (r.h - h) / 2.0, w, h)
}

/// The rule-of-thirds guides, over the picture.
fn draw_thirds(f: &mut Frame<Target>, r: Rect) {
    let guide = Color::rgba(205, 214, 244, 90);
    for i in 1..3 {
        let t = i as f32 / 3.0;
        f.push(RenderCommand::Line {
            x1: r.x + r.w * t,
            y1: r.y,
            x2: r.x + r.w * t,
            y2: r.bottom(),
            color: guide,
            width: 1.0,
        });
        f.push(RenderCommand::Line {
            x1: r.x,
            y1: r.y + r.h * t,
            x2: r.right(),
            y2: r.y + r.h * t,
            color: guide,
            width: 1.0,
        });
    }
}

/// One heading and the list of choices under it, returning the `y` it stopped
/// at.
///
/// Returning the cursor is what lets two lists be stacked without either
/// knowing how tall the other turned out to be -- the device panel used to
/// place its frame-rate list at a fixed offset that assumed exactly five
/// resolutions.
#[expect(
    clippy::too_many_arguments,
    reason = "each argument is a distinct fact about the list, and bundling \
              them into a struct would only move the same seven names"
)]
fn draw_choices(
    f: &mut Frame<Target>,
    l: &Layout,
    body: Rect,
    mut y: f32,
    heading: &str,
    labels: &[String],
    chosen: usize,
    target: fn(usize) -> Target,
) -> f32 {
    let line_h = l.row * 0.6;
    if y + line_h > body.bottom() {
        return y;
    }
    bounded(
        f,
        Rect::new(body.x + l.pad * 0.5, y, body.w - l.pad, line_h),
        heading,
        TEXT,
        l.small,
        FontWeightHint::Bold,
    );
    y += line_h;
    for (i, label) in labels.iter().enumerate() {
        if y + line_h > body.bottom() {
            return y;
        }
        let r = Rect::new(body.x + l.pad * 0.5, y, body.w - l.pad, line_h);
        let on = i == chosen;
        if on {
            fill(f, r, SURFACE1, CornerRadii::all(3.0));
        }
        bounded(
            f,
            Rect::new(r.x + l.pad * 0.5, r.y, r.w - l.pad, r.h),
            label,
            if on { BLUE } else { SUBTEXT0 },
            l.small,
            FontWeightHint::Regular,
        );
        f.hit(target(i), r);
        y += line_h;
    }
    y + l.pad * 0.5
}

/// A window dimension as a length. Window sizes are pixel counts in the
/// thousands.
#[expect(
    clippy::cast_precision_loss,
    reason = "a window dimension is orders of magnitude below 2^24"
)]
fn f32_from_u32(v: u32) -> f32 {
    v as f32
}

/// A count of rows or columns as a length.
///
/// A list holds thousands of entries at the very most, so the precision a
/// `usize` loses past 2^24 is not reachable from a window this program can be
/// given.
#[expect(
    clippy::cast_precision_loss,
    reason = "a count of visible rows is orders of magnitude below 2^24"
)]
fn usize_f32(v: usize) -> f32 {
    v as f32
}

/// A pixel count as an index. A `u32` always fits a `usize` on the targets we
/// build for; on a hypothetical 16-bit one, saturating is the only sane
/// answer anyway.
fn usize_of_u32(v: u32) -> usize {
    usize::try_from(v).unwrap_or(usize::MAX)
}

/// The low byte of a value, keeping the wrap explicit.
///
/// The test pattern *wants* to wrap — that is what makes successive frames
/// differ — so the modulo is written out rather than hidden in a truncating
/// cast, where a reader cannot tell deliberate wrapping from an overflow bug.
fn wrap_u8(v: u64) -> u8 {
    u8::try_from(v % 256).unwrap_or(0)
}

/// A value already known to be in byte range, as a byte.
fn clamp_u8(v: u16) -> u8 {
    u8::try_from(v.min(u16::from(u8::MAX))).unwrap_or(u8::MAX)
}

/// Perceptual luminance of a pixel (ITU-R BT.601 weights, 77/150/29 out of
/// 256), returned on the same 0..=255 scale as its inputs.
fn luminance(r: u8, g: u8, b: u8) -> u16 {
    u16::from(r)
        .saturating_mul(77)
        .saturating_add(u16::from(g).saturating_mul(150))
        .saturating_add(u16::from(b).saturating_mul(29))
        / 256
}

/// One channel of the test pattern, clamped into a byte.
///
/// The pattern is computed in floating point from fractions of the pane, so it
/// has to be brought back into range explicitly; `as u8` on an out-of-range
/// float is a saturating cast in Rust, but writing the clamp says which
/// direction was intended rather than relying on the reader knowing that.
fn channel(v: f32) -> u8 {
    v.clamp(0.0, 255.0) as u8
}

/// One notch of a `u32` setting, in either direction, saturating at both ends.
///
/// The floor is zero and the ceiling is the caller's; a setting whose real
/// floor is not zero (white balance starts at 2500K) clamps again in its own
/// setter, which is where the range is written down.
fn step_u32(v: u32, up: bool, by: u32, max: u32) -> u32 {
    if up {
        v.saturating_add(by).min(max)
    } else {
        v.saturating_sub(by)
    }
}

// ============================================================================
// The window
// ============================================================================

impl App for CameraApp {
    fn title(&self) -> String {
        String::from("Camera")
    }

    fn app_id(&self) -> String {
        String::from("camera")
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    fn tick_interval(&self) -> Option<std::time::Duration> {
        // A camera without a clock shows one frame for the life of the
        // process, counts a recording that never gets longer, and runs a
        // self-timer that never fires. `tick` had no caller outside `main`'s
        // simulation and the tests.
        Some(std::time::Duration::from_millis(TICK_MS))
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
        // The remembered size is written here as well as on `Resize`, because
        // this is the size the picture the user is looking at was drawn at, and
        // a click is answered against exactly that picture.
        self.width = width;
        self.height = height;
        self.frame(width, height).into_tree()
    }
}

impl Probe for CameraApp {
    type Target = Target;
    type Outcome = EventResult;
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame<Target> {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> EventResult {
        self.width = size.0;
        self.height = size.1;
        self.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(button),
        }))
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> EventResult {
        self.width = size.0;
        self.height = size.1;
        self.handle_event(&Event::Key(key.clone()))
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() -> ExitCode {
    // The previous `main` was a script: it ticked a few simulated frames, took
    // two photographs nobody saw, rendered into a `Vec` and bound the result to
    // `let _`. Every pixel it produced was discarded, no click or key ever
    // reached the program, and it still exited zero.
    let mut app = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
    app::launch("camera", &mut app)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it -- that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    // `float_cmp` is deliberately *not* in this list: comparing two floats for
    // equality is as wrong in a test as anywhere else, and `assert_zoom` below
    // is what stands in for it.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    use guitk::probe::{self, ctrl, press, release, typing};

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

    /// Assert a zoom factor, without comparing two floats for equality.
    ///
    /// Every zoom below is set from an exact literal and clamped against
    /// exact literals, so `==` would in fact hold today. It is still the
    /// wrong assertion: the moment a zoom arrives from arithmetic — a
    /// pinch gesture, a step of 1.5 applied twice — an exact comparison
    /// fails for a reason no reader of the test can see. The tolerance is
    /// far tighter than any difference the label rounds to.
    #[track_caller]
    fn assert_zoom(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 1e-6,
            "zoom is {actual}, expected {expected}"
        );
    }

    #[test]
    fn test_settings_default() {
        let s = CameraSettings::default();
        assert_eq!(s.brightness, 50);
        assert_eq!(s.contrast, 50);
        assert_zoom(s.zoom, 1.0);
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
        assert_zoom(s.zoom, 10.0);
        s.set_zoom(-1.0);
        assert_zoom(s.zoom, 1.0);
    }

    #[test]
    fn test_settings_zoom_in_out() {
        let mut s = CameraSettings::default();
        s.zoom_in();
        assert_zoom(s.zoom, 1.5);
        s.zoom_out();
        assert_zoom(s.zoom, 1.0);
        s.zoom_out(); // should not go below 1.0
        assert_zoom(s.zoom, 1.0);
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
        assert_zoom(s.zoom, 1.0);
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
        assert_eq!(format_bytes(2048), "2.0 KiB");
        assert!(format_bytes(2_000_000).contains("MiB"));
        assert!(format_bytes(3_000_000_000).contains("GiB"));
    }

    #[test]
    fn test_format_duration_ms() {
        // The minutes field is padded now ("0:05" -> "00:05"), matching the
        // screen recorder's list of the same kind of object.
        assert_eq!(format_duration_ms(0), "00:00");
        assert_eq!(format_duration_ms(5000), "00:05");
        assert_eq!(format_duration_ms(65000), "01:05");
        assert_eq!(format_duration_ms(3661000), "01:01:01");
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

    // =======================================================================
    // The camera in a window
    // =======================================================================
    //
    // Everything below is about this program being a window rather than a
    // simulation: that every pane is a fraction of the size the compositor
    // hands it, that what the pointer reaches is what the painter painted,
    // that the shutter is reachable by mouse and by key and means the same
    // thing either way, and that the frame counter, the recording clock and
    // the self-timer age on `Event::Tick` rather than on a loop in `main`.

    /// The window widths every layout claim is checked at.
    ///
    /// A rule about `Layout::solve` is a rule at *every* size, so a handful of
    /// pretty sizes tests a handful of points and nothing else. The widths
    /// that break a rule are the ones nobody would think to sample: 0 and 3,
    /// where there is no window at all; 200, exactly the viewfinder minimum;
    /// 576 and 578, either side of the width at which `w * 0.26` first reaches
    /// the 150 px a sidebar is worth having, so the sidebar appears between
    /// them; and 2400, where the sidebar is at its 300 px cap and the
    /// proportion no longer governs.
    const GRID_W: [f32; 8] = [0.0, 3.0, 120.0, 200.0, 576.0, 578.0, 1100.0, 2400.0];
    /// The window heights every layout claim is checked at.
    ///
    /// 0 and 5 are windows with no room for even the toolbar; 40 has a toolbar
    /// and a status line and nothing between them; 470 and 480 straddle the
    /// content height at which `content * 0.16` first reaches the 56 px a
    /// photo strip is worth having.
    const GRID_H: [f32; 7] = [0.0, 5.0, 40.0, 200.0, 470.0, 480.0, 1600.0];

    /// Every window size the layout claims sweep: 56 points, not 4.
    fn sizes() -> impl Iterator<Item = (f32, f32)> {
        GRID_W.into_iter().flat_map(|w| GRID_H.map(move |h| (w, h)))
    }

    /// Is `inner` within `outer`, allowing for a pixel-hundredth of rounding?
    ///
    /// A rectangle with no area is "inside" anything: it is the answer the
    /// layout gives for a pane that a window is too small to hold, and a pane
    /// that was not drawn cannot hang off an edge.
    fn inside(outer: Rect, inner: Rect) -> bool {
        inner.is_empty()
            || (inner.x >= outer.x - 0.01
                && inner.y >= outer.y - 0.01
                && inner.right() <= outer.right() + 0.01
                && inner.bottom() <= outer.bottom() + 0.01)
    }

    /// Every pane by name, for a failure message that says which one.
    fn panes(l: &Layout) -> [(&'static str, Rect); 5] {
        [
            ("toolbar", l.toolbar),
            ("viewfinder", l.viewfinder),
            ("sidebar", l.sidebar),
            ("strip", l.strip),
            ("status", l.status),
        ]
    }

    #[test]
    fn every_pane_stays_inside_the_window_at_every_size() {
        // The old layout subtracted compile-time furniture -- a 260 px
        // sidebar, a 100 px strip, a 48 px toolbar -- from whatever window it
        // was given, so a 200 px window gave the viewfinder a *negative*
        // width. Nothing showed it, because there was no window.
        for (w, h) in sizes() {
            let l = Layout::solve(w, h);
            let at = format!("{w}x{h}");
            for (name, r) in panes(&l) {
                assert!(inside(l.window, r), "{name} escapes the window at {at}");
                assert!(
                    r.w >= -0.01 && r.h >= -0.01,
                    "{name} is {}x{} -- negative -- at {at}",
                    r.w,
                    r.h
                );
            }
        }
    }

    #[test]
    fn no_two_panes_overlap_at_any_size() {
        // Panes that overlap are panes that paint over each other, and the one
        // that loses is whichever happens to be drawn first. The claim is
        // stronger than "inside the window" and is the one that actually says
        // the window was divided rather than merely bounded.
        for (w, h) in sizes() {
            let l = Layout::solve(w, h);
            let all = panes(&l);
            for (i, (an, a)) in all.iter().enumerate() {
                for (bn, b) in all.iter().skip(i.saturating_add(1)) {
                    if a.is_empty() || b.is_empty() {
                        continue;
                    }
                    let hit = a.intersect(*b).is_some_and(|r| r.w > 0.01 && r.h > 0.01);
                    assert!(!hit, "{an} {a:?} overlaps {bn} {b:?} in a {w}x{h} window");
                }
            }
        }
    }

    #[test]
    fn the_viewfinder_is_the_last_pane_given_up() {
        // The order the panes are surrendered in is the whole of the layout's
        // policy: a camera with a sidebar and no picture is not a camera. The
        // two minima are what that policy is made of, so both are asserted
        // rather than only the pane that happens to be easier to check.
        for (w, h) in sizes() {
            let l = Layout::solve(w, h);
            let at = format!("{w}x{h}");
            if !l.sidebar.is_empty() {
                assert!(
                    l.sidebar.w >= MIN_SIDEBAR_W - 0.01,
                    "a {} px sidebar is narrower than the {MIN_SIDEBAR_W} it is worth having, at {at}",
                    l.sidebar.w
                );
                assert!(
                    l.viewfinder.w >= MIN_VIEWFINDER_W - 0.01,
                    "the sidebar left the viewfinder {} px at {at}",
                    l.viewfinder.w
                );
            }
            if !l.strip.is_empty() {
                assert!(
                    l.strip.h >= MIN_STRIP_H - 0.01,
                    "a {} px strip is shorter than the {MIN_STRIP_H} it is worth having, at {at}",
                    l.strip.h
                );
                assert!(
                    l.viewfinder.h >= MIN_VIEWFINDER_H - 0.01,
                    "the strip left the viewfinder {} px at {at}",
                    l.viewfinder.h
                );
            }
        }
    }

    #[test]
    fn the_viewfinder_grows_and_shrinks_with_the_window() {
        // Monotonicity is the property that actually says a pane is a fraction
        // of the window rather than a constant with a `min` in front of it: a
        // wider window never has a narrower viewfinder.
        //
        // It is not true across the width where the sidebar appears -- that is
        // the sidebar being *taken*, and a step down there is the policy
        // working -- so the sweep is from the first width past it.
        let mut last_w = 0.0_f32;
        for w in [578.0_f32, 700.0, 1100.0, 1600.0, 2400.0] {
            let l = Layout::solve(w, 720.0);
            assert!(
                l.viewfinder.w >= last_w - 0.01,
                "a {w} px window has a {} px viewfinder, narrower than the {last_w} of a narrower window",
                l.viewfinder.w
            );
            last_w = l.viewfinder.w;
        }
        let mut last_h = 0.0_f32;
        for h in [480.0_f32, 600.0, 720.0, 1000.0, 1600.0] {
            let l = Layout::solve(1100.0, h);
            assert!(
                l.viewfinder.h >= last_h - 0.01,
                "a {h} px window has a {} px viewfinder, shorter than the {last_h} of a shorter one",
                l.viewfinder.h
            );
            last_h = l.viewfinder.h;
        }
    }

    #[test]
    fn a_nonsense_window_size_yields_no_layout_rather_than_a_wrong_one() {
        // A negative size is what a compositor reports mid-drag on some
        // backends, and `w - sidebar` on a negative `w` is more negative
        // still. Every pane must come back empty rather than inside-out.
        for (w, h) in [(-100.0_f32, -100.0_f32), (0.0, 720.0), (1100.0, 0.0)] {
            let l = Layout::solve(w, h);
            for (name, r) in panes(&l) {
                assert!(
                    r.w >= -0.01 && r.h >= -0.01,
                    "{name} is {r:?} for a {w}x{h} window"
                );
            }
        }
    }

    /// An app with `n` photographs behind it, the last one selected.
    fn with_photos(n: usize) -> CameraApp {
        let mut app = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        for i in 0..n {
            app.gallery.add_photo(
                Resolution::new(1920, 1080),
                1_000_000,
                ImageFilter::None,
                u64::try_from(i).unwrap_or(0).saturating_mul(1000),
            );
        }
        app
    }

    /// Every state the picture is drawn in.
    ///
    /// The states are the ones that change *what* is painted rather than only
    /// its colour: a camera with no device paints a disconnected band and no
    /// picture, a recording one paints a REC badge the idle one does not, a
    /// counting one paints an overlay over everything else, and a gallery of
    /// forty photographs paints a scrolled window onto a strip that holds six.
    fn states() -> Vec<(&'static str, CameraApp)> {
        let mut none = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        none.cameras.clear();

        let mut dead = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        if let Some(cam) = dead.active_camera_mut() {
            cam.status = CameraStatus::Disconnected;
        }

        let mut video = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        video.set_capture_mode(CaptureMode::Video);

        let mut recording = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        recording.set_capture_mode(CaptureMode::Video);
        recording.start_recording();
        recording.tick(5_000);

        let mut paused = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        paused.set_capture_mode(CaptureMode::Video);
        paused.start_recording();
        paused.pause_recording();

        let mut counting = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        counting.timer_mode = TimerMode::TenSeconds;
        counting.take_photo();

        let mut flashing = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        flashing.take_photo();

        let mut overlaid = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        overlaid.toggle_grid_overlay();
        overlaid.toggle_histogram();
        overlaid.tick(TICK_MS);

        let mut bare = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        bare.toggle_sidebar();
        bare.toggle_photo_strip();

        let mut device = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        device.set_sidebar_panel(SidebarPanel::DeviceInfo);

        let mut filters = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        filters.set_sidebar_panel(SidebarPanel::Filters);
        filters.settings.active_filter = ImageFilter::Sepia;

        let mut gallery = with_photos(40);
        gallery.set_sidebar_panel(SidebarPanel::Gallery);
        gallery.gallery.selected_idx = Some(20);

        let mut empty_gallery = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        empty_gallery.set_sidebar_panel(SidebarPanel::Gallery);

        let mut wordy = with_photos(3);
        wordy.set_status(
            "a status message long enough to run clean off the right-hand edge \
             of any window this program will ever be given, twice over",
        );
        if let Some(cam) = wordy.active_camera_mut() {
            cam.name = "a camera whose name nobody thought to keep short".to_string();
            cam.model_name = "a model name of the same unreasonable length".to_string();
        }

        let mut zoomed = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        zoomed.settings.set_zoom(7.5);
        zoomed.settings.active_filter = ImageFilter::Negative;

        let mut full = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        full.toggle_fullscreen_preview();

        vec![
            ("default", CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT)),
            ("no camera at all", none),
            ("disconnected camera", dead),
            ("video mode", video),
            ("recording", recording),
            ("paused", paused),
            ("counting down", counting),
            ("flashing", flashing),
            ("grid and histogram", overlaid),
            ("no sidebar and no strip", bare),
            ("device panel", device),
            ("filters panel", filters),
            ("gallery panel, scrolled", gallery),
            ("gallery panel, empty", empty_gallery),
            ("long names and a long status", wordy),
            ("zoomed and filtered", zoomed),
            ("fullscreen preview", full),
            ("one photograph", with_photos(1)),
            ("more photographs than the strip holds", with_photos(40)),
        ]
    }

    /// The rectangle a render command paints in, or `None` for one that has no
    /// rectangle of its own.
    fn painted_rect(c: &RenderCommand) -> Option<Rect> {
        match c {
            RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                ..
            }
            | RenderCommand::StrokeRect {
                x,
                y,
                width,
                height,
                ..
            } => Some(Rect::new(*x, *y, *width, *height)),
            RenderCommand::Line { x1, y1, x2, y2, .. } => Some(Rect::new(
                x1.min(*x2),
                y1.min(*y2),
                (x2 - x1).abs(),
                (y2 - y1).abs(),
            )),
            _ => None,
        }
    }

    #[test]
    fn nothing_is_painted_outside_the_window() {
        // The clip is what makes this hold for the strip and the sidebar,
        // whose contents are laid out from a count of rows and can overrun by
        // one when the window is between two whole rows.
        for (name, app) in states() {
            for (w, h) in sizes() {
                let window = Rect::new(0.0, 0.0, w, h);
                for c in app.frame(w, h).commands() {
                    let Some(r) = painted_rect(c) else {
                        continue;
                    };
                    assert!(inside(window, r), "{name}: {r:?} escapes a {w}x{h} window");
                }
            }
        }
    }

    #[test]
    fn no_run_of_text_is_drawn_unbounded_or_off_the_window() {
        // The old program passed raw `(x, y)` pairs with no width at all, so
        // every label was drawn at its full natural width whatever was to its
        // right: a long camera name wrote over the mode buttons and a long
        // status message wrote over the photo count.
        //
        // The bound has to be the one the command *declares*. Measuring the
        // string instead would test the font rather than the program, and
        // would call an unbounded label bounded whenever it happened to be
        // short in the one face the test machine has.
        for (name, app) in states() {
            for (w, h) in sizes() {
                for c in app.frame(w, h).commands() {
                    let RenderCommand::Text {
                        text: run,
                        x,
                        max_width,
                        ..
                    } = c
                    else {
                        continue;
                    };
                    let Some(bound) = *max_width else {
                        panic!(
                            "{name}: {run:?} is drawn unbounded in a {w}x{h} window, so a wide \
                             face paints it over whatever is beside it"
                        );
                    };
                    assert!(
                        *x >= -0.01,
                        "{name}: {run:?} starts at {x} in a {w}x{h} window"
                    );
                    assert!(
                        x + bound <= w + 0.01,
                        "{name}: {run:?} runs off a {w}x{h} window: x {x} + {bound}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_frame_balances_its_clips_at_every_size_in_every_state() {
        // An unbalanced clip is a clip left on the stack for whatever the
        // compositor draws next -- another window's problem, found nowhere
        // near here.
        for (name, app) in states() {
            for (w, h) in sizes() {
                assert!(
                    app.frame(w, h).is_balanced(),
                    "{name}: unbalanced clips in a {w}x{h} window"
                );
            }
        }
    }

    #[test]
    fn the_window_is_filled_edge_to_edge_before_anything_else() {
        // The panes are fractions of the window and the window is not a whole
        // number of anything, so a pixel or two is always left over. Filling
        // only the panes leaves that transparent -- a seam of desktop showing
        // through the camera.
        for (name, app) in states() {
            for (w, h) in sizes() {
                if w <= 0.0 || h <= 0.0 {
                    continue;
                }
                let frame = app.frame(w, h);
                let back = frame.commands().iter().find_map(painted_rect);
                let Some(back) = back else {
                    panic!("{name}: nothing at all is painted in a {w}x{h} window");
                };
                assert!(
                    back.w >= w - 0.01 && back.h >= h - 0.01,
                    "{name}: the first thing painted in a {w}x{h} window is {back:?}, which \
                     leaves an edge transparent"
                );
            }
        }
    }

    #[test]
    fn every_hit_box_is_inside_the_window_and_has_an_area() {
        // A hit box outside the window is a control the user can see nothing
        // of and click anyway; one with no area is a control that is drawn and
        // cannot be clicked at all.
        for (name, app) in states() {
            for (w, h) in sizes() {
                let window = Rect::new(0.0, 0.0, w, h);
                for (target, r) in app.frame(w, h).hits() {
                    assert!(
                        inside(window, *r),
                        "{name}: {target:?} is at {r:?}, outside a {w}x{h} window"
                    );
                    assert!(
                        r.w > 0.0 && r.h > 0.0,
                        "{name}: {target:?} is {r:?} in a {w}x{h} window -- drawn but unclickable"
                    );
                }
            }
        }
    }

    #[test]
    fn a_window_too_small_for_a_control_draws_no_hit_box_for_it() {
        // The alternative to dropping a control that does not fit is drawing
        // it anyway at a negative width, which paints nothing and hit-tests
        // nothing but still tells a coverage test the control is present.
        for (name, app) in states() {
            let frame = app.frame(3.0, 5.0);
            for (target, r) in frame.hits() {
                assert!(
                    r.w > 0.0 && r.h > 0.0 && r.right() <= 3.01 && r.bottom() <= 5.01,
                    "{name}: {target:?} kept a box of {r:?} in a 3x5 window"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // What the pointer and the keyboard reach
    // -----------------------------------------------------------------------

    #[test]
    fn every_control_the_picture_draws_is_reachable_by_a_click() {
        // A hit box that resolves to something else is a control drawn under
        // another one: visible, and unclickable. The topmost box at a point is
        // what a real click reaches, so the box a control is *drawn* in must
        // be the box that answers at its own middle.
        for (name, app) in states() {
            let frame = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
            for (target, r) in frame.hits() {
                let (cx, cy) = r.centre();
                let hit = frame.hit_test_ref(cx, cy);
                assert!(
                    hit.is_some(),
                    "{name}: {target:?} is drawn at {r:?} and nothing answers at its middle"
                );
            }
        }
    }

    #[test]
    fn the_shutter_and_the_space_bar_take_the_same_photograph() {
        // The one funnel: a control reachable by the mouse must not mean
        // something different from the same control reached any other way.
        // Before the rewrite the mouse reached nothing at all, so the two
        // could not be compared.
        let mut clicked = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        probe::click(&mut clicked, Target::Shutter);

        let mut typed = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        probe::key(&mut typed, &press(Key::Space));

        assert_eq!(clicked.gallery.count(), 1, "the shutter took no photograph");
        assert_eq!(typed.gallery.count(), 1, "the space bar took no photograph");
        assert_eq!(
            clicked.flash_remaining_ms, typed.flash_remaining_ms,
            "the shutter and the space bar leave the camera in different states"
        );
    }

    #[test]
    fn clicking_the_viewfinder_takes_the_picture_the_shutter_would() {
        // A camera's screen is its shutter. In video mode the same click has
        // to start the recording instead, which is the half a test of the
        // photo path alone would miss.
        let mut photo = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        probe::click(&mut photo, Target::Viewfinder);
        assert_eq!(photo.gallery.count(), 1);

        let mut video = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        video.set_capture_mode(CaptureMode::Video);
        probe::click(&mut video, Target::Viewfinder);
        assert!(
            video.is_recording(),
            "the viewfinder did not start recording"
        );
        assert_eq!(video.gallery.count(), 0, "video mode took a photograph");
    }

    #[test]
    fn each_toolbar_toggle_moves_its_own_switch_and_no_other() {
        // Four toggles drawn side by side, each a few pixels from the next.
        // A control whose hit box is one button out is indistinguishable from
        // a correct one until you check that the *other* switches held still.
        let switches = |a: &CameraApp| {
            (
                a.show_grid_overlay,
                a.show_histogram,
                a.sidebar_visible,
                a.photo_strip_visible,
            )
        };
        for (target, which) in [
            (Target::Grid, 0),
            (Target::Histogram, 1),
            (Target::Sidebar, 2),
            (Target::Strip, 3),
        ] {
            let mut app = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
            let before = switches(&app);
            probe::click(&mut app, target);
            let after = switches(&app);
            let moved = [
                before.0 != after.0,
                before.1 != after.1,
                before.2 != after.2,
                before.3 != after.3,
            ];
            for (i, m) in moved.iter().enumerate() {
                assert_eq!(
                    *m,
                    i == which,
                    "clicking {target:?} left the switches {before:?} -> {after:?}"
                );
            }
        }
    }

    #[test]
    fn the_filter_panel_selects_the_filter_it_names() {
        // `Filter` carries its filter, so a panel that drew every row with the
        // same payload -- or with the row index of a *different* list -- would
        // still draw the right number of rows in the right places.
        for filter in ImageFilter::all().iter().copied() {
            let mut app = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
            app.set_sidebar_panel(SidebarPanel::Filters);
            probe::click(&mut app, Target::Filter(filter));
            assert_eq!(
                app.settings.active_filter,
                filter,
                "clicking the {} row selected {:?}",
                filter.label(),
                app.settings.active_filter
            );
        }
    }

    #[test]
    fn the_strip_keeps_the_selected_photograph_reachable() {
        // The strip holds six or seven thumbnails and the gallery holds forty,
        // so the strip is a window onto the gallery. A window that does not
        // move with the selection leaves the selected photograph off the end,
        // where the arrow keys move a selection the user cannot see.
        let mut app = with_photos(40);
        for sel in [0_usize, 1, 7, 20, 38, 39] {
            app.gallery.selected_idx = Some(sel);
            let frame = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
            let found = frame.hits().iter().any(|(t, _)| *t == Target::Photo(sel));
            assert!(
                found,
                "photograph {sel} of 40 is selected and the strip does not show it"
            );
        }
    }

    #[test]
    fn a_click_on_bare_background_changes_nothing() {
        // `hit_test` answering `None` must not be mistaken for "the last
        // control clicked", which is what a handler that remembered its
        // target between clicks would do.
        let mut app = with_photos(3);
        app.gallery.selected_idx = Some(1);
        let before = format!("{:?}", app.settings);
        let sel = app.gallery.selected_idx;
        let count = app.gallery.count();
        let outcome = probe::click_background(&mut app);
        assert_eq!(outcome, EventResult::Ignored, "the background was consumed");
        assert_eq!(format!("{:?}", app.settings), before);
        assert_eq!(app.gallery.selected_idx, sel);
        assert_eq!(
            app.gallery.count(),
            count,
            "the background took a photograph"
        );
    }

    #[test]
    fn a_right_click_is_not_a_left_one() {
        // The shutter on the right button would take a photograph on the
        // gesture that opens a context menu everywhere else.
        let mut app = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let outcome = probe::click_with(&mut app, Target::Shutter, MouseButton::Right);
        assert_eq!(outcome, EventResult::Ignored);
        assert_eq!(app.gallery.count(), 0, "the right button took a photograph");
    }

    #[test]
    fn a_key_coming_back_up_is_not_a_second_keystroke() {
        // Both halves of every keystroke are delivered. Acting on both fires
        // every shortcut twice, which for the shutter is two photographs of
        // one moment.
        let mut app = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        probe::key(&mut app, &press(Key::Space));
        let outcome = probe::key(&mut app, &release(Key::Space));
        assert_eq!(outcome, EventResult::Ignored, "a release was consumed");
        assert_eq!(
            app.gallery.count(),
            1,
            "one press and one release took {} photographs",
            app.gallery.count()
        );
    }

    #[test]
    fn a_shortcut_reads_the_letter_typed_and_not_the_key_position() {
        // `Key::H` is *where H sits on a QWERTY board*; on a Dvorak one that
        // position types J. A program that switched on the key code would
        // flip the picture for a Dvorak user pressing J.
        let mut by_text = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let mut event = ctrl(Key::Unknown(0));
        event.text = "h".to_string();
        let outcome = probe::key(&mut by_text, &event);
        assert_eq!(outcome, EventResult::Consumed, "Ctrl+H was not answered");
        assert!(
            by_text.settings.flip_horizontal,
            "Ctrl+H did not flip the picture when the key code was not H"
        );

        // And the converse: the code without the letter must do nothing, since
        // that is a keyboard on which this position does not type an H.
        let mut by_code = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let mut bare = ctrl(Key::H);
        bare.text = String::new();
        probe::key(&mut by_code, &bare);
        assert!(
            !by_code.settings.flip_horizontal,
            "the H *position* flipped the picture on a layout that does not type H there"
        );
    }

    #[test]
    fn a_digit_picks_the_filter_at_that_position_in_the_list() {
        // The old program matched each digit to a filter by name in a second
        // list that had to be kept in step with the first by hand.
        for (i, filter) in ImageFilter::all().iter().enumerate() {
            let digit = char::from_digit(i.saturating_add(1) as u32, 10).unwrap_or('x');
            let mut app = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
            probe::key(&mut app, &typing(&digit.to_string()));
            assert_eq!(
                app.settings.active_filter,
                *filter,
                "digit {digit} chose {:?}, not the {} it sits over",
                app.settings.active_filter,
                filter.label()
            );
        }
    }

    #[test]
    fn each_slider_end_moves_its_own_setting_in_its_own_direction() {
        // Fourteen boxes, two per row, a couple of pixels apart. A row that
        // recorded the same `Nudge` at both ends, or its neighbour's
        // `Setting`, draws exactly the same picture as a correct one.
        // The claim is that down undoes up rather than merely that the two
        // differ, and it is made from *one step up* rather than from the
        // default, because two of these settings start at the bottom of their
        // own range: zoom defaults to 1x and cannot go below it, so a down
        // pressed on a fresh camera correctly moves nothing, and a test that
        // demanded movement there would be demanding a bug.
        for setting in Setting::all().iter().copied() {
            let mut app = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
            let start = app.setting_value(setting);

            probe::click(&mut app, Target::Setting(setting, Nudge::Up));
            let once = app.setting_value(setting);
            assert_ne!(
                start,
                once,
                "the up end of {} moved nothing",
                setting.label()
            );

            probe::click(&mut app, Target::Setting(setting, Nudge::Up));
            let twice = app.setting_value(setting);
            assert_ne!(
                once,
                twice,
                "the second press on the up end of {} moved nothing",
                setting.label()
            );

            probe::click(&mut app, Target::Setting(setting, Nudge::Down));
            assert_eq!(
                app.setting_value(setting),
                once,
                "the down end of {} does not undo the up end",
                setting.label()
            );

            // And nothing else moved with it. Only the settings can be
            // compared against a fresh camera -- the status line is *supposed*
            // to have changed, and says which setting moved.
            let fresh = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
            for other in Setting::all().iter().copied() {
                if other == setting {
                    continue;
                }
                assert_eq!(
                    app.setting_value(other),
                    fresh.setting_value(other),
                    "nudging {} moved {}",
                    setting.label(),
                    other.label()
                );
            }
        }
    }

    #[test]
    fn the_device_panel_chooses_the_resolution_and_rate_it_names() {
        for idx in 0..RESOLUTIONS.len() {
            let mut app = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
            app.set_sidebar_panel(SidebarPanel::DeviceInfo);
            let wanted = app
                .active_camera()
                .and_then(|c| c.supported_resolutions.get(idx).copied());
            let Some(wanted) = wanted else {
                continue;
            };
            if probe::rect_of(&app, Target::Resolution(idx)).is_none() {
                continue;
            }
            probe::click(&mut app, Target::Resolution(idx));
            assert_eq!(
                app.active_camera().map(CameraDevice::current_resolution),
                Some(wanted),
                "row {idx} chose the wrong resolution"
            );
        }
        for (idx, fps) in FRAME_RATES.iter().copied().enumerate() {
            let mut app = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
            app.set_sidebar_panel(SidebarPanel::DeviceInfo);
            if probe::rect_of(&app, Target::Framerate(idx)).is_none() {
                continue;
            }
            probe::click(&mut app, Target::Framerate(idx));
            assert_eq!(
                app.active_camera().map(|c| c.framerate),
                Some(fps),
                "row {idx} chose the wrong frame rate"
            );
        }
    }

    #[test]
    fn the_gallery_favourites_and_deletes_the_photograph_it_shows_as_selected() {
        let mut app = with_photos(4);
        app.set_sidebar_panel(SidebarPanel::Gallery);
        app.gallery.selected_idx = Some(2);
        let id = app.gallery.selected_photo().map(|p| p.id);

        probe::click(&mut app, Target::Favorite);
        assert_eq!(
            app.gallery.selected_photo().map(|p| p.favorite),
            Some(true),
            "the star did not mark the selected photograph"
        );

        probe::click(&mut app, Target::Delete);
        assert_eq!(app.gallery.count(), 3, "the bin deleted the wrong number");
        assert!(
            app.gallery.photos.iter().all(|p| Some(p.id) != id),
            "the bin deleted a photograph other than the selected one"
        );
    }

    #[test]
    fn every_control_the_program_has_is_drawn_in_some_state() {
        // A `Target` variant no state ever draws is a control that cannot be
        // reached at all -- which is what every one of them was before this
        // program had a window.
        let mut seen: Vec<String> = Vec::new();
        for (_, app) in states() {
            for name in probe::control_names(&app) {
                if !seen.contains(&name) {
                    seen.push(name);
                }
            }
        }
        for wanted in [
            "PhotoMode",
            "VideoMode",
            "Shutter",
            "Timer",
            "Grid",
            "Histogram",
            "Sidebar",
            "Strip",
            "NextCamera",
            "Panel",
            "Filter",
            "Resolution",
            "Framerate",
            "Photo",
            "Favorite",
            "Delete",
            "Setting",
            "Viewfinder",
        ] {
            assert!(
                seen.iter().any(|s| s == wanted),
                "no state draws a {wanted}; the controls on screen are {seen:?}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // The clock, and the entry points the platform actually calls
    // -----------------------------------------------------------------------

    #[test]
    fn the_clock_reaches_the_program_through_the_entry_point_the_platform_calls() {
        // Deliberately through `App::on_event`, not through `tick`. `tick` had
        // no caller outside `main`'s simulation and the tests: the running
        // program never advanced its own clock at all, and a suite that called
        // `tick` directly proved only that the arithmetic worked. See
        // known-issues.md lesson 102.
        let mut app = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let before = app.frame_counter;
        let response = app.on_event(&Event::Tick { elapsed_ms: 100 });
        assert_eq!(app.elapsed_ms, 100, "the tick did not reach the clock");
        assert!(
            app.frame_counter > before,
            "the tick reached the clock but produced no frame"
        );
        assert_eq!(
            response,
            Response::Redraw,
            "a new frame was captured and no repaint was asked for"
        );
        assert!(
            app.tick_interval().is_some(),
            "a camera with no tick interval is handed no clock to begin with"
        );
    }

    #[test]
    fn the_recording_clock_and_the_self_timer_age_on_the_tick() {
        let mut rec = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        rec.set_capture_mode(CaptureMode::Video);
        rec.start_recording();
        for _ in 0..30 {
            rec.on_event(&Event::Tick { elapsed_ms: 100 });
        }
        assert!(
            rec.recording.duration_ms >= 3_000,
            "three seconds of ticks made a {} ms recording",
            rec.recording.duration_ms
        );

        let mut timed = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        timed.timer_mode = TimerMode::ThreeSeconds;
        timed.take_photo();
        assert!(timed.timer_countdown.active, "the timer did not start");
        assert_eq!(timed.gallery.count(), 0, "the timer fired immediately");
        for _ in 0..40 {
            timed.on_event(&Event::Tick { elapsed_ms: 100 });
        }
        assert!(!timed.timer_countdown.active, "the timer never expired");
        assert_eq!(
            timed.gallery.count(),
            1,
            "the timer expired and took no photograph"
        );
    }

    #[test]
    fn a_tick_that_changes_nothing_asks_for_no_repaint() {
        // A camera with no live device produces no new frame, and a window
        // that repaints thirty times a second to draw the same picture is
        // thirty wakeups a second of somebody's battery.
        let mut app = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        if let Some(cam) = app.active_camera_mut() {
            cam.status = CameraStatus::Disconnected;
        }
        assert_eq!(
            app.on_event(&Event::Tick {
                elapsed_ms: TICK_MS
            }),
            Response::Idle,
            "a still camera asked for a repaint"
        );
        // ...and the moment anything is moving, it does ask.
        app.flash_remaining_ms = FLASH_MS;
        assert_eq!(
            app.on_event(&Event::Tick {
                elapsed_ms: TICK_MS
            }),
            Response::Redraw,
            "the flash faded and no repaint was asked for"
        );
    }

    #[test]
    fn the_flash_ages_out_of_the_picture() {
        // Asks the picture rather than the flag: a flash that is set, cleared
        // and never painted passes a test of `flash_remaining_ms` and shows
        // the user nothing. See known-issues.md lesson 103.
        let mut app = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let quiet = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT).commands().len();

        probe::click(&mut app, Target::Shutter);
        let lit = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT).commands().len();
        assert!(
            lit > quiet,
            "the shutter fired and the picture did not change"
        );

        let mut ms = 0;
        while app.flash_remaining_ms > 0 && ms < 10_000 {
            app.on_event(&Event::Tick {
                elapsed_ms: TICK_MS,
            });
            ms += TICK_MS;
        }
        assert!(ms < 10_000, "the flash never went out");
        let after = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT).commands().len();
        assert!(
            after < lit,
            "the flash went out of the state and stayed in the picture"
        );
    }

    #[test]
    fn the_overlays_are_painted_only_when_they_are_switched_on() {
        // The same question of the picture rather than of the flag, for the
        // two toggles that draw nothing of their own furniture: a grid that is
        // "on" and paints no lines is a switch with nothing behind it.
        let mut app = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let plain = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT).commands().len();

        probe::click(&mut app, Target::Grid);
        let gridded = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT).commands().len();
        assert!(gridded > plain, "the thirds grid painted nothing");

        probe::click(&mut app, Target::Histogram);
        let both = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT).commands().len();
        assert!(both > gridded, "the histogram painted nothing");

        probe::click(&mut app, Target::Grid);
        probe::click(&mut app, Target::Histogram);
        assert_eq!(
            app.frame(WINDOW_WIDTH, WINDOW_HEIGHT).commands().len(),
            plain,
            "switching both overlays back off left something behind"
        );
    }

    #[test]
    fn a_resize_is_the_size_the_next_click_is_measured_against() {
        // The click is answered against a freshly drawn frame, so a stale
        // remembered size means the user clicks the shutter and the program
        // tests the point against a picture nobody is looking at.
        let mut app = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let big = probe::rect_of(&app, Target::Shutter).unwrap_or(Rect::EMPTY);

        app.on_event(&Event::Resize {
            width: 420,
            height: 320,
        });
        let small = probe::rect_of_sized(&app, Target::Shutter, (420.0, 320.0));
        let Some(small) = small else {
            panic!("no shutter at all in a 420x320 window");
        };
        assert!(
            (big.x - small.x).abs() > 1.0 || (big.y - small.y).abs() > 1.0,
            "the shutter is in the same place in a 1100x720 window and a 420x320 one"
        );

        let (cx, cy) = small.centre();
        let outcome = app.handle_event(&Event::Mouse(MouseEvent {
            x: cx,
            y: cy,
            kind: MouseEventKind::Press(MouseButton::Left),
        }));
        assert_eq!(outcome, EventResult::Consumed);
        assert_eq!(
            app.gallery.count(),
            1,
            "the click landed where the resized picture drew the shutter and missed it"
        );
    }

    #[test]
    fn the_picture_is_drawn_at_the_size_render_is_given() {
        // `render` is handed the surface size by the compositor and must lay
        // out from *that*, not from whatever the last `Resize` happened to
        // say. The two disagree on the first frame after a resize.
        let mut app = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let tree = app.render(500.0, 380.0);
        assert!(!tree.commands.is_empty(), "render drew nothing");
        for c in &tree.commands {
            let Some(r) = painted_rect(c) else {
                continue;
            };
            assert!(
                inside(Rect::new(0.0, 0.0, 500.0, 380.0), r),
                "render(500, 380) painted {r:?}"
            );
        }
        assert!(
            (app.width - 500.0).abs() < 0.01 && (app.height - 380.0).abs() < 0.01,
            "render did not remember the size it drew at"
        );
    }

    #[test]
    fn the_close_button_closes_the_window() {
        // Every other event must not, or a program that answered `Exit` to the
        // wrong one would vanish under the user's hand.
        let mut app = CameraApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(app.on_event(&Event::CloseRequested), Response::Exit);
        for event in [
            Event::Tick { elapsed_ms: 1 },
            Event::Resize {
                width: 800,
                height: 600,
            },
            Event::Key(press(Key::Escape)),
            Event::Mouse(MouseEvent {
                x: 1.0,
                y: 1.0,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        ] {
            assert_ne!(
                app.on_event(&event),
                Response::Exit,
                "{event:?} closed the window"
            );
        }
    }

    #[test]
    fn rows_in_never_counts_a_row_that_does_not_fit() {
        // The count feeds `for slot in 0..rows`, so one row too many is one
        // list entry painted past the bottom of the panel it belongs to.
        let l = Layout::solve(1100.0, 720.0);
        for taken in [0.0_f32, 10.0, 400.0, 10_000.0] {
            for h in [0.0_f32, 5.0, 100.0, 720.0] {
                let r = Rect::new(0.0, 0.0, 200.0, h);
                let n = l.rows_in(r, taken);
                let used = usize_f32(n) * l.row;
                assert!(
                    used <= (h - taken).max(0.0) + 0.01,
                    "{n} rows of {} need {used} px but only {} are left",
                    l.row,
                    (h - taken).max(0.0)
                );
            }
        }
    }
}
