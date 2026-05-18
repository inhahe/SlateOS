//! OurOS Image Viewer
//!
//! Graphical photo/image viewer with:
//! - Image display with zoom, pan, rotation, and flip transforms
//! - Directory browsing (next/prev image navigation)
//! - Slideshow mode with configurable intervals
//! - Image format detection (BMP, PNG, JPEG, GIF)
//! - Image information panel with metadata/EXIF display
//! - Toolbar and status bar
//! - Keyboard shortcuts for all operations
//!
//! Uses the guitk library for UI rendering.

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{Event, KeyEvent, Key, Modifiers, MouseEvent, MouseEventKind, MouseButton};
#[allow(unused_imports)]
use guitk::render::{RenderTree, RenderCommand, FontWeightHint};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

#[allow(dead_code)]
mod video;

use std::path::{Path, PathBuf};

// ============================================================================
// Constants
// ============================================================================

const TOOLBAR_HEIGHT: f32 = 40.0;
const STATUS_BAR_HEIGHT: f32 = 28.0;
const INFO_PANEL_WIDTH: f32 = 280.0;
const THUMBNAIL_STRIP_HEIGHT: f32 = 80.0;

const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 4.0;
const ZOOM_STEP: f32 = 0.25;

const BG_COLOR: Color = Color::rgb(30, 30, 30);
const TOOLBAR_BG: Color = Color::rgb(48, 48, 48);
const STATUS_BG: Color = Color::rgb(38, 38, 38);
const INFO_PANEL_BG: Color = Color::rgb(42, 42, 42);
const BUTTON_BG: Color = Color::rgb(60, 60, 60);
const BUTTON_HOVER_BG: Color = Color::rgb(80, 80, 80);
const TEXT_PRIMARY: Color = Color::rgb(230, 230, 230);
const TEXT_SECONDARY: Color = Color::rgb(160, 160, 160);
const ACCENT_COLOR: Color = Color::rgb(70, 140, 220);
const BORDER_COLOR: Color = Color::rgb(70, 70, 70);

/// Supported image file extensions for directory browsing.
const IMAGE_EXTENSIONS: &[&str] = &[
    "bmp", "png", "jpg", "jpeg", "gif", "webp", "ico", "tiff", "tif", "svg",
];

// ============================================================================
// Image format detection
// ============================================================================

/// Detected image format from magic bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    Bmp,
    Png,
    Jpeg,
    Gif,
    Unknown,
}

impl ImageFormat {
    /// Detect image format from the first bytes of a file.
    pub fn detect(data: &[u8]) -> Self {
        if data.len() < 8 {
            return Self::Unknown;
        }

        // BMP: starts with "BM"
        if data.get(0) == Some(&b'B') && data.get(1) == Some(&b'M') {
            return Self::Bmp;
        }

        // PNG: 8-byte signature
        let png_sig: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
        if data.len() >= 8 && data[..8] == png_sig {
            return Self::Png;
        }

        // JPEG: starts with FF D8
        if data.get(0) == Some(&0xFF) && data.get(1) == Some(&0xD8) {
            return Self::Jpeg;
        }

        // GIF: starts with "GIF87a" or "GIF89a"
        if data.len() >= 6 {
            let gif87 = b"GIF87a";
            let gif89 = b"GIF89a";
            if &data[..6] == gif87 || &data[..6] == gif89 {
                return Self::Gif;
            }
        }

        Self::Unknown
    }

    /// Human-readable name for the format.
    pub fn name(self) -> &'static str {
        match self {
            Self::Bmp => "BMP",
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Gif => "GIF",
            Self::Unknown => "Unknown",
        }
    }
}

/// Parse image dimensions from header bytes.
pub fn parse_dimensions(format: ImageFormat, data: &[u8]) -> Option<(u32, u32)> {
    match format {
        ImageFormat::Bmp => parse_bmp_dimensions(data),
        ImageFormat::Png => parse_png_dimensions(data),
        ImageFormat::Jpeg => parse_jpeg_dimensions(data),
        ImageFormat::Gif => parse_gif_dimensions(data),
        ImageFormat::Unknown => None,
    }
}

fn parse_bmp_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    // BMP header: width at offset 18 (4 bytes LE), height at offset 22 (4 bytes LE)
    if data.len() < 26 {
        return None;
    }
    let width = u32::from_le_bytes([
        *data.get(18)?,
        *data.get(19)?,
        *data.get(20)?,
        *data.get(21)?,
    ]);
    let height_raw = i32::from_le_bytes([
        *data.get(22)?,
        *data.get(23)?,
        *data.get(24)?,
        *data.get(25)?,
    ]);
    // Height can be negative (top-down bitmap)
    let height = height_raw.unsigned_abs();
    Some((width, height))
}

fn parse_png_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    // PNG IHDR chunk: width at offset 16 (4 bytes BE), height at offset 20 (4 bytes BE)
    if data.len() < 24 {
        return None;
    }
    let width = u32::from_be_bytes([
        *data.get(16)?,
        *data.get(17)?,
        *data.get(18)?,
        *data.get(19)?,
    ]);
    let height = u32::from_be_bytes([
        *data.get(20)?,
        *data.get(21)?,
        *data.get(22)?,
        *data.get(23)?,
    ]);
    Some((width, height))
}

fn parse_jpeg_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    // Scan for SOF0 marker (FF C0) to find dimensions
    let mut idx = 2; // skip FF D8
    while idx + 1 < data.len() {
        if *data.get(idx)? != 0xFF {
            idx += 1;
            continue;
        }
        let marker = *data.get(idx + 1)?;
        idx += 2;

        // SOF0 (baseline), SOF1 (extended), SOF2 (progressive)
        if marker == 0xC0 || marker == 0xC1 || marker == 0xC2 {
            // Skip length (2 bytes) and precision (1 byte)
            if idx + 7 > data.len() {
                return None;
            }
            let height = u16::from_be_bytes([*data.get(idx + 3)?, *data.get(idx + 4)?]);
            let width = u16::from_be_bytes([*data.get(idx + 5)?, *data.get(idx + 6)?]);
            return Some((width as u32, height as u32));
        }

        // Skip this segment
        if idx + 1 >= data.len() {
            break;
        }
        let seg_len = u16::from_be_bytes([*data.get(idx)?, *data.get(idx + 1)?]) as usize;
        idx += seg_len;
    }
    None
}

fn parse_gif_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    // GIF logical screen descriptor: width at offset 6 (2 bytes LE), height at offset 8 (2 bytes LE)
    if data.len() < 10 {
        return None;
    }
    let width = u16::from_le_bytes([*data.get(6)?, *data.get(7)?]);
    let height = u16::from_le_bytes([*data.get(8)?, *data.get(9)?]);
    Some((width as u32, height as u32))
}

// ============================================================================
// Image data
// ============================================================================

/// Decoded image pixel data (RGBA format).
#[derive(Clone, Debug)]
pub struct ImageData {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>, // RGBA, 4 bytes per pixel
    pub image_id: u64,   // Registered with compositor for rendering
}

impl ImageData {
    /// Create a placeholder checkerboard image for testing.
    pub fn placeholder(width: u32, height: u32, image_id: u64) -> Self {
        let pixel_count = (width as usize).saturating_mul(height as usize);
        let mut pixels = Vec::with_capacity(pixel_count.saturating_mul(4));
        for y in 0..height {
            for x in 0..width {
                let checker = ((x / 16) + (y / 16)) % 2 == 0;
                if checker {
                    pixels.extend_from_slice(&[200, 200, 200, 255]);
                } else {
                    pixels.extend_from_slice(&[100, 100, 100, 255]);
                }
            }
        }
        Self { width, height, pixels, image_id }
    }
}

// ============================================================================
// Transform state
// ============================================================================

/// Rotation angle in 90-degree increments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rotation {
    None,
    Cw90,
    Cw180,
    Cw270,
}

impl Rotation {
    /// Rotate clockwise by 90 degrees.
    pub fn rotate_cw(self) -> Self {
        match self {
            Self::None => Self::Cw90,
            Self::Cw90 => Self::Cw180,
            Self::Cw180 => Self::Cw270,
            Self::Cw270 => Self::None,
        }
    }

    /// Rotate counter-clockwise by 90 degrees.
    pub fn rotate_ccw(self) -> Self {
        match self {
            Self::None => Self::Cw270,
            Self::Cw90 => Self::None,
            Self::Cw180 => Self::Cw90,
            Self::Cw270 => Self::Cw180,
        }
    }

    /// Angle in degrees for display purposes.
    pub fn degrees(self) -> u16 {
        match self {
            Self::None => 0,
            Self::Cw90 => 90,
            Self::Cw180 => 180,
            Self::Cw270 => 270,
        }
    }
}

/// Complete transform state for the viewed image.
#[derive(Clone, Debug)]
pub struct Transform {
    pub zoom: f32,
    pub pan_x: f32,
    pub pan_y: f32,
    pub rotation: Rotation,
    pub flip_h: bool,
    pub flip_v: bool,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan_x: 0.0,
            pan_y: 0.0,
            rotation: Rotation::None,
            flip_h: false,
            flip_v: false,
        }
    }
}

impl Transform {
    /// Reset all transforms to default.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Zoom in by one step, clamping to MAX_ZOOM.
    pub fn zoom_in(&mut self) {
        self.zoom = (self.zoom + ZOOM_STEP).min(MAX_ZOOM);
    }

    /// Zoom out by one step, clamping to MIN_ZOOM.
    pub fn zoom_out(&mut self) {
        self.zoom = (self.zoom - ZOOM_STEP).max(MIN_ZOOM);
    }
}

// ============================================================================
// Slideshow state
// ============================================================================

/// Slideshow interval options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlideshowInterval {
    ThreeSeconds,
    FiveSeconds,
    TenSeconds,
    ThirtySeconds,
}

impl SlideshowInterval {
    /// Duration in milliseconds.
    pub fn millis(self) -> u64 {
        match self {
            Self::ThreeSeconds => 3000,
            Self::FiveSeconds => 5000,
            Self::TenSeconds => 10000,
            Self::ThirtySeconds => 30000,
        }
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::ThreeSeconds => "3s",
            Self::FiveSeconds => "5s",
            Self::TenSeconds => "10s",
            Self::ThirtySeconds => "30s",
        }
    }

    /// Cycle to the next interval option.
    pub fn next(self) -> Self {
        match self {
            Self::ThreeSeconds => Self::FiveSeconds,
            Self::FiveSeconds => Self::TenSeconds,
            Self::TenSeconds => Self::ThirtySeconds,
            Self::ThirtySeconds => Self::ThreeSeconds,
        }
    }
}

/// Slideshow mode state.
#[derive(Clone, Debug)]
pub struct SlideshowState {
    pub active: bool,
    pub interval: SlideshowInterval,
    pub elapsed_ms: u64,
    pub paused: bool,
    pub random_order: bool,
}

impl Default for SlideshowState {
    fn default() -> Self {
        Self {
            active: false,
            interval: SlideshowInterval::FiveSeconds,
            elapsed_ms: 0,
            paused: false,
            random_order: false,
        }
    }
}

// ============================================================================
// Image metadata / EXIF
// ============================================================================

/// Image metadata and EXIF information.
#[derive(Clone, Debug, Default)]
pub struct ImageInfo {
    pub filename: String,
    pub file_size: u64,
    pub format: Option<ImageFormat>,
    pub width: u32,
    pub height: u32,
    pub color_depth: Option<u8>,
    pub dpi: Option<(u32, u32)>,
    pub date_modified: Option<String>,
    // EXIF fields (populated if available)
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub exposure_time: Option<String>,
    pub iso: Option<u32>,
    pub aperture: Option<String>,
    pub focal_length: Option<String>,
}

impl ImageInfo {
    /// Format the file size for display.
    pub fn file_size_display(&self) -> String {
        if self.file_size < 1024 {
            format!("{} B", self.file_size)
        } else if self.file_size < 1024 * 1024 {
            format!("{:.1} KB", self.file_size as f64 / 1024.0)
        } else {
            format!("{:.1} MB", self.file_size as f64 / (1024.0 * 1024.0))
        }
    }

    /// Dimensions as a display string.
    pub fn dimensions_display(&self) -> String {
        format!("{} x {}", self.width, self.height)
    }
}

// ============================================================================
// Directory entry for browsing
// ============================================================================

/// A single image file entry in the current directory listing.
#[derive(Clone, Debug)]
pub struct DirectoryEntry {
    pub path: PathBuf,
    pub filename: String,
    pub file_size: u64,
}

// ============================================================================
// Toolbar button
// ============================================================================

/// Toolbar button definition.
#[derive(Clone, Debug)]
#[allow(dead_code)]
struct ToolbarButton {
    label: &'static str,
    tooltip: &'static str,
    action: ViewerAction,
    x: f32,
    width: f32,
}

/// Actions triggered by toolbar buttons or keyboard shortcuts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewerAction {
    Open,
    PrevImage,
    NextImage,
    ZoomIn,
    ZoomOut,
    FitToWindow,
    ActualSize,
    RotateCw,
    RotateCcw,
    FlipHorizontal,
    FlipVertical,
    ToggleSlideshow,
    ToggleInfo,
    ToggleThumbnails,
    ToggleFullscreen,
    FirstImage,
    LastImage,
    DeleteImage,
    PauseSlideshow,
}

// ============================================================================
// Main viewer state
// ============================================================================

/// Complete state for the image viewer application.
pub struct ViewerState {
    // Window dimensions
    pub window_width: f32,
    pub window_height: f32,
    pub fullscreen: bool,

    // Current image
    pub current_image: Option<ImageData>,
    pub image_info: ImageInfo,
    pub transform: Transform,

    // Directory browsing
    pub directory: Option<PathBuf>,
    pub entries: Vec<DirectoryEntry>,
    pub current_index: usize,

    // UI panels
    pub show_info_panel: bool,
    pub show_thumbnails: bool,
    pub show_toolbar: bool,
    pub show_status_bar: bool,

    // Slideshow
    pub slideshow: SlideshowState,

    // Mouse interaction state
    pub dragging: bool,
    pub drag_start_x: f32,
    pub drag_start_y: f32,
    pub drag_start_pan_x: f32,
    pub drag_start_pan_y: f32,

    // Toolbar hover state
    pub hovered_button: Option<usize>,
}

impl ViewerState {
    /// Create a new viewer state with default settings.
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            window_width: width,
            window_height: height,
            fullscreen: false,
            current_image: None,
            image_info: ImageInfo::default(),
            transform: Transform::default(),
            directory: None,
            entries: Vec::new(),
            current_index: 0,
            show_info_panel: false,
            show_thumbnails: false,
            show_toolbar: true,
            show_status_bar: true,
            slideshow: SlideshowState::default(),
            dragging: false,
            drag_start_x: 0.0,
            drag_start_y: 0.0,
            drag_start_pan_x: 0.0,
            drag_start_pan_y: 0.0,
            hovered_button: None,
        }
    }

    /// Open an image file by path.
    pub fn open_file(&mut self, path: &Path) {
        let filename = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| String::from("(unknown)"));

        // Populate basic info from path
        self.image_info.filename = filename;
        self.image_info.file_size = std::fs::metadata(path)
            .map(|m| m.len())
            .unwrap_or(0);
        self.image_info.date_modified = std::fs::metadata(path)
            .ok()
            .and_then(|m| m.modified().ok())
            .map(|_t| String::from("(available)"));

        // Try to detect format and dimensions from file header
        if let Ok(header_data) = std::fs::read(path) {
            let format = ImageFormat::detect(&header_data);
            self.image_info.format = Some(format);

            if let Some((w, h)) = parse_dimensions(format, &header_data) {
                self.image_info.width = w;
                self.image_info.height = h;
            }

            // In a real implementation, this would decode the image and
            // register it with the compositor. For now, create placeholder.
            let image_id = path_to_image_id(path);
            let img_w = self.image_info.width.max(1);
            let img_h = self.image_info.height.max(1);
            self.current_image = Some(ImageData::placeholder(img_w, img_h, image_id));
        }

        // Reset transform for new image
        self.transform.reset();

        // Update directory listing
        if let Some(parent) = path.parent() {
            self.load_directory(parent);
            // Find our index in the listing
            self.current_index = self
                .entries
                .iter()
                .position(|e| e.path == path)
                .unwrap_or(0);
        }
    }

    /// Load the image file listing for a directory.
    pub fn load_directory(&mut self, dir: &Path) {
        self.directory = Some(dir.to_path_buf());
        self.entries.clear();

        if let Ok(read_dir) = std::fs::read_dir(dir) {
            for entry_result in read_dir {
                let Ok(entry) = entry_result else { continue };
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if !IMAGE_EXTENSIONS.contains(&ext.as_str()) {
                    continue;
                }
                let filename = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let file_size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                self.entries.push(DirectoryEntry {
                    path,
                    filename,
                    file_size,
                });
            }
        }

        // Sort alphabetically by filename
        self.entries.sort_by(|a, b| a.filename.cmp(&b.filename));
    }

    /// Navigate to the next image in the directory.
    pub fn next_image(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        self.current_index = (self.current_index + 1) % self.entries.len();
        self.load_current_entry();
    }

    /// Navigate to the previous image in the directory.
    pub fn prev_image(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        if self.current_index == 0 {
            self.current_index = self.entries.len().saturating_sub(1);
        } else {
            self.current_index -= 1;
        }
        self.load_current_entry();
    }

    /// Navigate to the first image in the directory.
    pub fn first_image(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        self.current_index = 0;
        self.load_current_entry();
    }

    /// Navigate to the last image in the directory.
    pub fn last_image(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        self.current_index = self.entries.len().saturating_sub(1);
        self.load_current_entry();
    }

    /// Reload the image at the current index.
    fn load_current_entry(&mut self) {
        if let Some(entry) = self.entries.get(self.current_index) {
            let path = entry.path.clone();
            self.open_file(&path);
        }
    }

    /// Compute the fit-to-window zoom level for the current image.
    pub fn fit_zoom(&self) -> f32 {
        let Some(img) = &self.current_image else {
            return 1.0;
        };
        let available_width = self.image_area_width();
        let available_height = self.image_area_height();
        if img.width == 0 || img.height == 0 {
            return 1.0;
        }
        let zoom_x = available_width / img.width as f32;
        let zoom_y = available_height / img.height as f32;
        zoom_x.min(zoom_y).min(MAX_ZOOM).max(MIN_ZOOM)
    }

    /// Apply fit-to-window zoom.
    pub fn fit_to_window(&mut self) {
        self.transform.zoom = self.fit_zoom();
        self.transform.pan_x = 0.0;
        self.transform.pan_y = 0.0;
    }

    /// Set zoom to actual size (1:1 pixels).
    pub fn actual_size(&mut self) {
        self.transform.zoom = 1.0;
        self.transform.pan_x = 0.0;
        self.transform.pan_y = 0.0;
    }

    /// Width of the image display area.
    fn image_area_width(&self) -> f32 {
        let mut w = self.window_width;
        if self.show_info_panel {
            w -= INFO_PANEL_WIDTH;
        }
        w.max(1.0)
    }

    /// Height of the image display area.
    fn image_area_height(&self) -> f32 {
        let mut h = self.window_height;
        if self.show_toolbar && !self.fullscreen {
            h -= TOOLBAR_HEIGHT;
        }
        if self.show_status_bar && !self.fullscreen {
            h -= STATUS_BAR_HEIGHT;
        }
        if self.show_thumbnails {
            h -= THUMBNAIL_STRIP_HEIGHT;
        }
        h.max(1.0)
    }

    /// Execute a viewer action.
    pub fn execute_action(&mut self, action: ViewerAction) {
        match action {
            ViewerAction::Open => {
                // In a real implementation, this would open a file dialog.
                // For now, this is a placeholder.
            }
            ViewerAction::PrevImage => self.prev_image(),
            ViewerAction::NextImage => self.next_image(),
            ViewerAction::ZoomIn => self.transform.zoom_in(),
            ViewerAction::ZoomOut => self.transform.zoom_out(),
            ViewerAction::FitToWindow => self.fit_to_window(),
            ViewerAction::ActualSize => self.actual_size(),
            ViewerAction::RotateCw => {
                self.transform.rotation = self.transform.rotation.rotate_cw();
            }
            ViewerAction::RotateCcw => {
                self.transform.rotation = self.transform.rotation.rotate_ccw();
            }
            ViewerAction::FlipHorizontal => {
                self.transform.flip_h = !self.transform.flip_h;
            }
            ViewerAction::FlipVertical => {
                self.transform.flip_v = !self.transform.flip_v;
            }
            ViewerAction::ToggleSlideshow => {
                self.slideshow.active = !self.slideshow.active;
                self.slideshow.elapsed_ms = 0;
                self.slideshow.paused = false;
            }
            ViewerAction::PauseSlideshow => {
                if self.slideshow.active {
                    self.slideshow.paused = !self.slideshow.paused;
                }
            }
            ViewerAction::ToggleInfo => {
                self.show_info_panel = !self.show_info_panel;
            }
            ViewerAction::ToggleThumbnails => {
                self.show_thumbnails = !self.show_thumbnails;
            }
            ViewerAction::ToggleFullscreen => {
                self.fullscreen = !self.fullscreen;
            }
            ViewerAction::FirstImage => self.first_image(),
            ViewerAction::LastImage => self.last_image(),
            ViewerAction::DeleteImage => {
                // Would move to trash via OS recycle bin integration
            }
        }
    }

    /// Handle a tick event for slideshow progression.
    pub fn handle_tick(&mut self, elapsed_ms: u64) {
        if !self.slideshow.active || self.slideshow.paused {
            return;
        }
        self.slideshow.elapsed_ms += elapsed_ms;
        if self.slideshow.elapsed_ms >= self.slideshow.interval.millis() {
            self.slideshow.elapsed_ms = 0;
            self.next_image();
        }
    }

    /// Handle a keyboard event. Returns true if the event was consumed.
    pub fn handle_key_event(&mut self, event: &KeyEvent) -> bool {
        if !event.pressed {
            return false;
        }

        let ctrl = event.modifiers.ctrl;
        let shift = event.modifiers.shift;

        match event.key {
            // Navigation
            Key::Left if !ctrl => { self.execute_action(ViewerAction::PrevImage); true }
            Key::Right if !ctrl => { self.execute_action(ViewerAction::NextImage); true }
            Key::Home => { self.execute_action(ViewerAction::FirstImage); true }
            Key::End => { self.execute_action(ViewerAction::LastImage); true }

            // Zoom
            Key::Equals if ctrl => { self.execute_action(ViewerAction::ZoomIn); true }
            Key::Minus if ctrl => { self.execute_action(ViewerAction::ZoomOut); true }
            Key::Num0 if ctrl => { self.execute_action(ViewerAction::FitToWindow); true }
            Key::Num1 if ctrl => { self.execute_action(ViewerAction::ActualSize); true }

            // Rotation / flip
            Key::R if ctrl && !shift => { self.execute_action(ViewerAction::RotateCw); true }
            Key::R if ctrl && shift => { self.execute_action(ViewerAction::RotateCcw); true }
            Key::H if ctrl => { self.execute_action(ViewerAction::FlipHorizontal); true }
            Key::V if ctrl => { self.execute_action(ViewerAction::FlipVertical); true }

            // Panels
            Key::I if !ctrl => { self.execute_action(ViewerAction::ToggleInfo); true }
            Key::T if !ctrl => { self.execute_action(ViewerAction::ToggleThumbnails); true }

            // Slideshow
            Key::F5 => { self.execute_action(ViewerAction::ToggleSlideshow); true }
            Key::Space => { self.execute_action(ViewerAction::PauseSlideshow); true }

            // Fullscreen
            Key::F11 => { self.execute_action(ViewerAction::ToggleFullscreen); true }

            // Delete
            Key::Delete => { self.execute_action(ViewerAction::DeleteImage); true }

            // Escape exits fullscreen or slideshow
            Key::Escape => {
                if self.slideshow.active {
                    self.slideshow.active = false;
                    true
                } else if self.fullscreen {
                    self.fullscreen = false;
                    true
                } else {
                    false
                }
            }

            _ => false,
        }
    }

    /// Handle a mouse event. Returns true if the event was consumed.
    pub fn handle_mouse_event(&mut self, event: &MouseEvent) -> bool {
        match &event.kind {
            MouseEventKind::Scroll { dx: _, dy } => {
                // Scroll wheel zooms when cursor is over the image area
                if *dy > 0.0 {
                    self.transform.zoom_in();
                } else if *dy < 0.0 {
                    self.transform.zoom_out();
                }
                true
            }
            MouseEventKind::Press(MouseButton::Left) => {
                // Start panning
                let toolbar_y = if self.show_toolbar && !self.fullscreen {
                    TOOLBAR_HEIGHT
                } else {
                    0.0
                };
                if event.y > toolbar_y {
                    self.dragging = true;
                    self.drag_start_x = event.x;
                    self.drag_start_y = event.y;
                    self.drag_start_pan_x = self.transform.pan_x;
                    self.drag_start_pan_y = self.transform.pan_y;
                    true
                } else {
                    false
                }
            }
            MouseEventKind::Release(MouseButton::Left) => {
                self.dragging = false;
                true
            }
            MouseEventKind::Move => {
                if self.dragging {
                    let dx = event.x - self.drag_start_x;
                    let dy = event.y - self.drag_start_y;
                    self.transform.pan_x = self.drag_start_pan_x + dx;
                    self.transform.pan_y = self.drag_start_pan_y + dy;
                    true
                } else {
                    false
                }
            }
            MouseEventKind::DoubleClick(MouseButton::Left) => {
                // Double-click toggles between fit and actual size
                if (self.transform.zoom - 1.0).abs() < 0.01 {
                    self.fit_to_window();
                } else {
                    self.actual_size();
                }
                true
            }
            _ => false,
        }
    }

    /// Handle any event type dispatched to the viewer.
    pub fn handle_event(&mut self, event: &Event) -> bool {
        match event {
            Event::Key(key_event) => self.handle_key_event(key_event),
            Event::Mouse(mouse_event) => self.handle_mouse_event(mouse_event),
            Event::Resize { width, height } => {
                self.window_width = *width as f32;
                self.window_height = *height as f32;
                true
            }
            Event::Tick { elapsed_ms } => {
                self.handle_tick(*elapsed_ms);
                true
            }
            _ => false,
        }
    }
}

// ============================================================================
// Rendering
// ============================================================================

/// Render the complete viewer UI into a RenderTree.
pub fn render(state: &ViewerState) -> RenderTree {
    let mut tree = RenderTree::new();

    // Background
    tree.fill_rect(0.0, 0.0, state.window_width, state.window_height, BG_COLOR);

    let mut content_y = 0.0;

    // Toolbar (hidden in fullscreen)
    if state.show_toolbar && !state.fullscreen {
        render_toolbar(state, &mut tree, 0.0);
        content_y = TOOLBAR_HEIGHT;
    }

    // Main image area
    let status_y = if state.show_status_bar && !state.fullscreen {
        state.window_height - STATUS_BAR_HEIGHT
    } else {
        state.window_height
    };
    let thumb_y = if state.show_thumbnails {
        status_y - THUMBNAIL_STRIP_HEIGHT
    } else {
        status_y
    };
    let image_area_height = thumb_y - content_y;
    let image_area_width = if state.show_info_panel {
        state.window_width - INFO_PANEL_WIDTH
    } else {
        state.window_width
    };

    // Clip to image area and render image
    tree.clip(0.0, content_y, image_area_width, image_area_height);
    render_image(state, &mut tree, 0.0, content_y, image_area_width, image_area_height);
    tree.unclip();

    // Info panel
    if state.show_info_panel {
        render_info_panel(state, &mut tree, image_area_width, content_y, image_area_height);
    }

    // Thumbnail strip
    if state.show_thumbnails {
        render_thumbnail_strip(state, &mut tree, thumb_y);
    }

    // Status bar (hidden in fullscreen)
    if state.show_status_bar && !state.fullscreen {
        render_status_bar(state, &mut tree, status_y);
    }

    tree
}

/// Render the toolbar with action buttons.
fn render_toolbar(state: &ViewerState, tree: &mut RenderTree, y: f32) {
    // Toolbar background
    tree.fill_rect(0.0, y, state.window_width, TOOLBAR_HEIGHT, TOOLBAR_BG);
    // Bottom border
    tree.fill_rect(0.0, y + TOOLBAR_HEIGHT - 1.0, state.window_width, 1.0, BORDER_COLOR);

    let buttons = toolbar_buttons();
    let button_y = y + 6.0;
    let button_h = TOOLBAR_HEIGHT - 12.0;

    for (idx, btn) in buttons.iter().enumerate() {
        let bg = if state.hovered_button == Some(idx) {
            BUTTON_HOVER_BG
        } else {
            BUTTON_BG
        };

        tree.push(RenderCommand::FillRect {
            x: btn.x,
            y: button_y,
            width: btn.width,
            height: button_h,
            color: bg,
            corner_radii: CornerRadii {
                top_left: 3.0,
                top_right: 3.0,
                bottom_right: 3.0,
                bottom_left: 3.0,
            },
        });

        // Button label (centered)
        tree.push(RenderCommand::Text {
            x: btn.x + 4.0,
            y: button_y + 7.0,
            text: btn.label.to_string(),
            color: TEXT_PRIMARY,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(btn.width - 8.0),
        });
    }
}

/// Render the image in the display area with current transforms.
fn render_image(
    state: &ViewerState,
    tree: &mut RenderTree,
    area_x: f32,
    area_y: f32,
    area_w: f32,
    area_h: f32,
) {
    let Some(img) = &state.current_image else {
        // No image loaded — show placeholder text
        tree.push(RenderCommand::Text {
            x: area_x + area_w / 2.0 - 80.0,
            y: area_y + area_h / 2.0 - 8.0,
            text: String::from("No image loaded"),
            color: TEXT_SECONDARY,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        tree.push(RenderCommand::Text {
            x: area_x + area_w / 2.0 - 100.0,
            y: area_y + area_h / 2.0 + 12.0,
            text: String::from("Open a file or drag an image here"),
            color: TEXT_SECONDARY,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        return;
    };

    let zoom = state.transform.zoom;
    let display_w = img.width as f32 * zoom;
    let display_h = img.height as f32 * zoom;

    // Center the image in the available area, then apply pan offset
    let center_x = area_x + (area_w - display_w) / 2.0 + state.transform.pan_x;
    let center_y = area_y + (area_h - display_h) / 2.0 + state.transform.pan_y;

    // Apply translation for pan
    tree.translate(center_x, center_y);

    // Render the image command
    tree.push(RenderCommand::Image {
        x: 0.0,
        y: 0.0,
        width: display_w,
        height: display_h,
        image_id: img.image_id,
    });

    tree.untranslate();

    // Slideshow overlay indicator
    if state.slideshow.active {
        let indicator_text = if state.slideshow.paused {
            "PAUSED"
        } else {
            "SLIDESHOW"
        };
        let indicator_color = if state.slideshow.paused {
            Color::rgba(220, 180, 50, 200)
        } else {
            Color::rgba(70, 180, 70, 200)
        };

        // Small badge in top-right of image area
        tree.push(RenderCommand::FillRect {
            x: area_x + area_w - 100.0,
            y: area_y + 8.0,
            width: 92.0,
            height: 24.0,
            color: Color::rgba(0, 0, 0, 160),
            corner_radii: CornerRadii {
                top_left: 4.0,
                top_right: 4.0,
                bottom_right: 4.0,
                bottom_left: 4.0,
            },
        });
        tree.push(RenderCommand::Text {
            x: area_x + area_w - 92.0,
            y: area_y + 14.0,
            text: String::from(indicator_text),
            color: indicator_color,
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }
}

/// Render the image information panel on the right side.
fn render_info_panel(
    state: &ViewerState,
    tree: &mut RenderTree,
    x: f32,
    y: f32,
    height: f32,
) {
    // Panel background
    tree.fill_rect(x, y, INFO_PANEL_WIDTH, height, INFO_PANEL_BG);
    // Left border
    tree.fill_rect(x, y, 1.0, height, BORDER_COLOR);

    let pad = 12.0;
    let mut text_y = y + pad;
    let label_x = x + pad;
    let value_x = x + pad + 80.0;
    let line_height = 20.0;

    // Panel title
    tree.push(RenderCommand::Text {
        x: label_x,
        y: text_y,
        text: String::from("Image Information"),
        color: TEXT_PRIMARY,
        font_size: 13.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(INFO_PANEL_WIDTH - pad * 2.0),
    });
    text_y += line_height + 8.0;

    // Separator
    tree.fill_rect(
        label_x,
        text_y,
        INFO_PANEL_WIDTH - pad * 2.0,
        1.0,
        BORDER_COLOR,
    );
    text_y += 8.0;

    let info = &state.image_info;

    // File info section
    let fields: Vec<(&str, String)> = vec![
        ("File:", info.filename.clone()),
        ("Size:", info.file_size_display()),
        ("Dimensions:", info.dimensions_display()),
        (
            "Format:",
            info.format.map(|f| f.name().to_string()).unwrap_or_else(|| String::from("—")),
        ),
        (
            "Depth:",
            info.color_depth
                .map(|d| format!("{} bpp", d))
                .unwrap_or_else(|| String::from("—")),
        ),
        (
            "DPI:",
            info.dpi
                .map(|(x, y)| format!("{} x {}", x, y))
                .unwrap_or_else(|| String::from("—")),
        ),
        (
            "Modified:",
            info.date_modified.clone().unwrap_or_else(|| String::from("—")),
        ),
    ];

    for (label, value) in &fields {
        tree.push(RenderCommand::Text {
            x: label_x,
            y: text_y,
            text: String::from(*label),
            color: TEXT_SECONDARY,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        tree.push(RenderCommand::Text {
            x: value_x,
            y: text_y,
            text: value.clone(),
            color: TEXT_PRIMARY,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(INFO_PANEL_WIDTH - 80.0 - pad * 2.0),
        });
        text_y += line_height;
    }

    // EXIF section (if any data available)
    let has_exif = info.camera_make.is_some()
        || info.camera_model.is_some()
        || info.exposure_time.is_some()
        || info.iso.is_some()
        || info.aperture.is_some()
        || info.focal_length.is_some();

    if has_exif {
        text_y += 8.0;
        tree.fill_rect(
            label_x,
            text_y,
            INFO_PANEL_WIDTH - pad * 2.0,
            1.0,
            BORDER_COLOR,
        );
        text_y += 8.0;

        tree.push(RenderCommand::Text {
            x: label_x,
            y: text_y,
            text: String::from("EXIF Data"),
            color: TEXT_PRIMARY,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        text_y += line_height + 4.0;

        let exif_fields: Vec<(&str, Option<String>)> = vec![
            ("Camera:", info.camera_make.clone()),
            ("Model:", info.camera_model.clone()),
            ("Exposure:", info.exposure_time.clone()),
            ("ISO:", info.iso.map(|v| format!("{}", v))),
            ("Aperture:", info.aperture.clone()),
            ("Focal:", info.focal_length.clone()),
        ];

        for (label, value_opt) in &exif_fields {
            if let Some(value) = value_opt {
                tree.push(RenderCommand::Text {
                    x: label_x,
                    y: text_y,
                    text: String::from(*label),
                    color: TEXT_SECONDARY,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
                tree.push(RenderCommand::Text {
                    x: value_x,
                    y: text_y,
                    text: value.clone(),
                    color: TEXT_PRIMARY,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(INFO_PANEL_WIDTH - 80.0 - pad * 2.0),
                });
                text_y += line_height;
            }
        }
    }

    // Transform info
    text_y += 8.0;
    tree.fill_rect(
        label_x,
        text_y,
        INFO_PANEL_WIDTH - pad * 2.0,
        1.0,
        BORDER_COLOR,
    );
    text_y += 8.0;

    tree.push(RenderCommand::Text {
        x: label_x,
        y: text_y,
        text: String::from("View"),
        color: TEXT_PRIMARY,
        font_size: 12.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });
    text_y += line_height + 4.0;

    let zoom_pct = (state.transform.zoom * 100.0) as u32;
    let view_fields: Vec<(&str, String)> = vec![
        ("Zoom:", format!("{}%", zoom_pct)),
        ("Rotation:", format!("{}deg", state.transform.rotation.degrees())),
        (
            "Flip:",
            match (state.transform.flip_h, state.transform.flip_v) {
                (false, false) => String::from("None"),
                (true, false) => String::from("Horizontal"),
                (false, true) => String::from("Vertical"),
                (true, true) => String::from("Both"),
            },
        ),
    ];

    for (label, value) in &view_fields {
        tree.push(RenderCommand::Text {
            x: label_x,
            y: text_y,
            text: String::from(*label),
            color: TEXT_SECONDARY,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        tree.push(RenderCommand::Text {
            x: value_x,
            y: text_y,
            text: value.clone(),
            color: TEXT_PRIMARY,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(INFO_PANEL_WIDTH - 80.0 - pad * 2.0),
        });
        text_y += line_height;
    }
}

/// Render the thumbnail strip at the bottom.
fn render_thumbnail_strip(state: &ViewerState, tree: &mut RenderTree, y: f32) {
    // Background
    tree.fill_rect(0.0, y, state.window_width, THUMBNAIL_STRIP_HEIGHT, TOOLBAR_BG);
    // Top border
    tree.fill_rect(0.0, y, state.window_width, 1.0, BORDER_COLOR);

    if state.entries.is_empty() {
        return;
    }

    let thumb_size = 60.0;
    let thumb_pad = 4.0;
    let thumb_y = y + (THUMBNAIL_STRIP_HEIGHT - thumb_size) / 2.0;
    let total_thumb_width = thumb_size + thumb_pad;

    // Calculate visible range centered on current image
    let visible_count = (state.window_width / total_thumb_width) as usize;
    let half_visible = visible_count / 2;
    let start_idx = state.current_index.saturating_sub(half_visible);
    let end_idx = (start_idx + visible_count).min(state.entries.len());

    for (rel_idx, abs_idx) in (start_idx..end_idx).enumerate() {
        let thumb_x = (rel_idx as f32) * total_thumb_width + thumb_pad;
        let is_current = abs_idx == state.current_index;

        // Thumbnail border (highlight current)
        let border_color = if is_current { ACCENT_COLOR } else { BORDER_COLOR };
        tree.push(RenderCommand::StrokeRect {
            x: thumb_x,
            y: thumb_y,
            width: thumb_size,
            height: thumb_size,
            color: border_color,
            line_width: if is_current { 2.0 } else { 1.0 },
            corner_radii: CornerRadii::ZERO,
        });

        // Thumbnail placeholder (would use actual thumbnails)
        tree.push(RenderCommand::FillRect {
            x: thumb_x + 1.0,
            y: thumb_y + 1.0,
            width: thumb_size - 2.0,
            height: thumb_size - 2.0,
            color: Color::rgb(50, 50, 50),
            corner_radii: CornerRadii::ZERO,
        });

        // Filename label below (truncated)
        if let Some(entry) = state.entries.get(abs_idx) {
            let display_name = if entry.filename.len() > 8 {
                let truncated: String = entry.filename.chars().take(7).collect();
                format!("{}~", truncated)
            } else {
                entry.filename.clone()
            };
            tree.push(RenderCommand::Text {
                x: thumb_x + 2.0,
                y: thumb_y + thumb_size - 12.0,
                text: display_name,
                color: if is_current { TEXT_PRIMARY } else { TEXT_SECONDARY },
                font_size: 9.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(thumb_size - 4.0),
            });
        }
    }
}

/// Render the status bar at the bottom.
fn render_status_bar(state: &ViewerState, tree: &mut RenderTree, y: f32) {
    // Background
    tree.fill_rect(0.0, y, state.window_width, STATUS_BAR_HEIGHT, STATUS_BG);
    // Top border
    tree.fill_rect(0.0, y, state.window_width, 1.0, BORDER_COLOR);

    let text_y = y + 8.0;
    let pad = 10.0;

    // Left: filename
    tree.push(RenderCommand::Text {
        x: pad,
        y: text_y,
        text: state.image_info.filename.clone(),
        color: TEXT_PRIMARY,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(state.window_width * 0.4),
    });

    // Center: dimensions
    let dims = state.image_info.dimensions_display();
    tree.push(RenderCommand::Text {
        x: state.window_width * 0.4,
        y: text_y,
        text: dims,
        color: TEXT_SECONDARY,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });

    // Right side: zoom level + image position
    let zoom_pct = (state.transform.zoom * 100.0) as u32;
    let zoom_text = format!("{}%", zoom_pct);
    tree.push(RenderCommand::Text {
        x: state.window_width - 160.0,
        y: text_y,
        text: zoom_text,
        color: TEXT_SECONDARY,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });

    // Image N of M
    if !state.entries.is_empty() {
        let pos_text = format!(
            "{} / {}",
            state.current_index + 1,
            state.entries.len()
        );
        tree.push(RenderCommand::Text {
            x: state.window_width - 80.0,
            y: text_y,
            text: pos_text,
            color: TEXT_SECONDARY,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }
}

/// Build the toolbar button definitions with positions.
fn toolbar_buttons() -> Vec<ToolbarButton> {
    let mut buttons = Vec::new();
    let mut x = 8.0;
    let gap = 4.0;

    let defs: &[(&str, &str, ViewerAction, f32)] = &[
        ("Open", "Open file (Ctrl+O)", ViewerAction::Open, 44.0),
        ("|<", "Previous (Left)", ViewerAction::PrevImage, 28.0),
        (">|", "Next (Right)", ViewerAction::NextImage, 28.0),
        ("+", "Zoom in (Ctrl++)", ViewerAction::ZoomIn, 24.0),
        ("-", "Zoom out (Ctrl+-)", ViewerAction::ZoomOut, 24.0),
        ("Fit", "Fit to window (Ctrl+0)", ViewerAction::FitToWindow, 32.0),
        ("1:1", "Actual size (Ctrl+1)", ViewerAction::ActualSize, 32.0),
        ("CW", "Rotate CW (Ctrl+R)", ViewerAction::RotateCw, 30.0),
        ("CCW", "Rotate CCW (Ctrl+Shift+R)", ViewerAction::RotateCcw, 36.0),
        ("H", "Flip H (Ctrl+H)", ViewerAction::FlipHorizontal, 24.0),
        ("V", "Flip V (Ctrl+V)", ViewerAction::FlipVertical, 24.0),
        ("Show", "Slideshow (F5)", ViewerAction::ToggleSlideshow, 42.0),
        ("Info", "Info panel (I)", ViewerAction::ToggleInfo, 36.0),
    ];

    for &(label, tooltip, action, width) in defs {
        buttons.push(ToolbarButton {
            label,
            tooltip,
            action,
            x,
            width,
        });
        x += width + gap;
    }

    buttons
}

// ============================================================================
// Utility functions
// ============================================================================

/// Generate a stable image ID from a file path (simple hash).
fn path_to_image_id(path: &Path) -> u64 {
    let path_str = path.to_string_lossy();
    let mut hash: u64 = 5381;
    for byte in path_str.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
    }
    hash
}

/// Check if a file extension represents a supported image format.
pub fn is_image_extension(ext: &str) -> bool {
    IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str())
}

// ============================================================================
// Application entry point
// ============================================================================

fn main() {
    // Initialize viewer with a default window size
    let mut state = ViewerState::new(1024.0, 768.0);

    // Parse command-line arguments for initial file
    let args: Vec<String> = std::env::args().collect();
    if let Some(file_path) = args.get(1) {
        let path = PathBuf::from(file_path);
        if path.exists() {
            state.open_file(&path);
            // Auto-fit the first image
            state.fit_to_window();
        }
    }

    // In a real windowing environment, this would enter the event loop
    // managed by the compositor/window manager. For now, render one frame
    // to verify the rendering pipeline works.
    let _frame = render(&state);

    // Event loop placeholder — in practice, the compositor calls us with events
    // and we return render trees each frame.
    // loop {
    //     let event = wait_for_event();
    //     state.handle_event(&event);
    //     let frame = render(&state);
    //     submit_frame(frame);
    // }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_format_detection_bmp() {
        let data = b"BM\x00\x00\x00\x00\x00\x00\x00\x00";
        assert_eq!(ImageFormat::detect(data), ImageFormat::Bmp);
    }

    #[test]
    fn test_image_format_detection_png() {
        let data: &[u8] = &[137, 80, 78, 71, 13, 10, 26, 10, 0, 0];
        assert_eq!(ImageFormat::detect(data), ImageFormat::Png);
    }

    #[test]
    fn test_image_format_detection_jpeg() {
        let data: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0, 0, 0, 0];
        assert_eq!(ImageFormat::detect(data), ImageFormat::Jpeg);
    }

    #[test]
    fn test_image_format_detection_gif87a() {
        let data = b"GIF87a\x00\x00\x00\x00";
        assert_eq!(ImageFormat::detect(data), ImageFormat::Gif);
    }

    #[test]
    fn test_image_format_detection_gif89a() {
        let data = b"GIF89a\x00\x00\x00\x00";
        assert_eq!(ImageFormat::detect(data), ImageFormat::Gif);
    }

    #[test]
    fn test_image_format_detection_unknown() {
        let data = b"RIFF\x00\x00\x00\x00\x00\x00";
        assert_eq!(ImageFormat::detect(data), ImageFormat::Unknown);
    }

    #[test]
    fn test_image_format_detection_too_short() {
        let data = b"BM";
        assert_eq!(ImageFormat::detect(data), ImageFormat::Unknown);
    }

    #[test]
    fn test_bmp_dimensions() {
        // Minimal BMP header with width=100, height=200
        let mut data = vec![0u8; 30];
        data[0] = b'B';
        data[1] = b'M';
        // Width at offset 18 (LE)
        let w: u32 = 100;
        data[18..22].copy_from_slice(&w.to_le_bytes());
        // Height at offset 22 (LE, signed)
        let h: i32 = 200;
        data[22..26].copy_from_slice(&h.to_le_bytes());

        assert_eq!(parse_bmp_dimensions(&data), Some((100, 200)));
    }

    #[test]
    fn test_bmp_dimensions_negative_height() {
        let mut data = vec![0u8; 30];
        data[0] = b'B';
        data[1] = b'M';
        let w: u32 = 640;
        data[18..22].copy_from_slice(&w.to_le_bytes());
        let h: i32 = -480; // top-down bitmap
        data[22..26].copy_from_slice(&h.to_le_bytes());

        assert_eq!(parse_bmp_dimensions(&data), Some((640, 480)));
    }

    #[test]
    fn test_png_dimensions() {
        // Minimal PNG with IHDR: width=800, height=600
        let mut data = vec![0u8; 30];
        // PNG signature
        data[..8].copy_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
        // IHDR length (13 bytes)
        data[8..12].copy_from_slice(&13u32.to_be_bytes());
        // "IHDR" chunk type
        data[12..16].copy_from_slice(b"IHDR");
        // Width at offset 16
        data[16..20].copy_from_slice(&800u32.to_be_bytes());
        // Height at offset 20
        data[20..24].copy_from_slice(&600u32.to_be_bytes());

        assert_eq!(parse_png_dimensions(&data), Some((800, 600)));
    }

    #[test]
    fn test_gif_dimensions() {
        let mut data = vec![0u8; 13];
        data[..6].copy_from_slice(b"GIF89a");
        // Width at offset 6 (LE 16-bit)
        data[6..8].copy_from_slice(&320u16.to_le_bytes());
        // Height at offset 8 (LE 16-bit)
        data[8..10].copy_from_slice(&240u16.to_le_bytes());

        assert_eq!(parse_gif_dimensions(&data), Some((320, 240)));
    }

    #[test]
    fn test_rotation() {
        let r = Rotation::None;
        assert_eq!(r.rotate_cw(), Rotation::Cw90);
        assert_eq!(r.rotate_cw().rotate_cw(), Rotation::Cw180);
        assert_eq!(r.rotate_cw().rotate_cw().rotate_cw(), Rotation::Cw270);
        assert_eq!(r.rotate_cw().rotate_cw().rotate_cw().rotate_cw(), Rotation::None);
    }

    #[test]
    fn test_rotation_ccw() {
        let r = Rotation::None;
        assert_eq!(r.rotate_ccw(), Rotation::Cw270);
        assert_eq!(r.rotate_ccw().rotate_ccw(), Rotation::Cw180);
    }

    #[test]
    fn test_rotation_degrees() {
        assert_eq!(Rotation::None.degrees(), 0);
        assert_eq!(Rotation::Cw90.degrees(), 90);
        assert_eq!(Rotation::Cw180.degrees(), 180);
        assert_eq!(Rotation::Cw270.degrees(), 270);
    }

    #[test]
    fn test_transform_zoom_clamp() {
        let mut t = Transform::default();
        // Zoom in repeatedly — should clamp at MAX_ZOOM
        for _ in 0..100 {
            t.zoom_in();
        }
        assert!((t.zoom - MAX_ZOOM).abs() < f32::EPSILON);

        // Zoom out repeatedly — should clamp at MIN_ZOOM
        for _ in 0..100 {
            t.zoom_out();
        }
        assert!((t.zoom - MIN_ZOOM).abs() < f32::EPSILON);
    }

    #[test]
    fn test_transform_reset() {
        let mut t = Transform {
            zoom: 2.5,
            pan_x: 100.0,
            pan_y: -50.0,
            rotation: Rotation::Cw180,
            flip_h: true,
            flip_v: true,
        };
        t.reset();
        assert!((t.zoom - 1.0).abs() < f32::EPSILON);
        assert!((t.pan_x).abs() < f32::EPSILON);
        assert!((t.pan_y).abs() < f32::EPSILON);
        assert_eq!(t.rotation, Rotation::None);
        assert!(!t.flip_h);
        assert!(!t.flip_v);
    }

    #[test]
    fn test_slideshow_interval_cycle() {
        let i = SlideshowInterval::ThreeSeconds;
        assert_eq!(i.next(), SlideshowInterval::FiveSeconds);
        assert_eq!(i.next().next(), SlideshowInterval::TenSeconds);
        assert_eq!(i.next().next().next(), SlideshowInterval::ThirtySeconds);
        assert_eq!(i.next().next().next().next(), SlideshowInterval::ThreeSeconds);
    }

    #[test]
    fn test_slideshow_interval_millis() {
        assert_eq!(SlideshowInterval::ThreeSeconds.millis(), 3000);
        assert_eq!(SlideshowInterval::FiveSeconds.millis(), 5000);
        assert_eq!(SlideshowInterval::TenSeconds.millis(), 10000);
        assert_eq!(SlideshowInterval::ThirtySeconds.millis(), 30000);
    }

    #[test]
    fn test_viewer_state_default() {
        let state = ViewerState::new(1024.0, 768.0);
        assert!((state.window_width - 1024.0).abs() < f32::EPSILON);
        assert!((state.window_height - 768.0).abs() < f32::EPSILON);
        assert!(state.current_image.is_none());
        assert!(!state.fullscreen);
        assert!(!state.slideshow.active);
        assert!(state.show_toolbar);
        assert!(state.show_status_bar);
        assert!(!state.show_info_panel);
    }

    #[test]
    fn test_viewer_fit_zoom_no_image() {
        let state = ViewerState::new(800.0, 600.0);
        assert!((state.fit_zoom() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_is_image_extension() {
        assert!(is_image_extension("png"));
        assert!(is_image_extension("PNG"));
        assert!(is_image_extension("jpg"));
        assert!(is_image_extension("jpeg"));
        assert!(is_image_extension("bmp"));
        assert!(is_image_extension("gif"));
        assert!(!is_image_extension("txt"));
        assert!(!is_image_extension("rs"));
        assert!(!is_image_extension("exe"));
    }

    #[test]
    fn test_image_info_file_size_display() {
        let mut info = ImageInfo::default();
        info.file_size = 500;
        assert_eq!(info.file_size_display(), "500 B");

        info.file_size = 2048;
        assert_eq!(info.file_size_display(), "2.0 KB");

        info.file_size = 1_500_000;
        assert_eq!(info.file_size_display(), "1.4 MB");
    }

    #[test]
    fn test_render_produces_commands() {
        let state = ViewerState::new(800.0, 600.0);
        let tree = render(&state);
        // Should have background + toolbar + status bar at minimum
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_with_image() {
        let mut state = ViewerState::new(800.0, 600.0);
        state.current_image = Some(ImageData::placeholder(200, 150, 42));
        state.image_info.width = 200;
        state.image_info.height = 150;
        state.image_info.filename = String::from("test.png");

        let tree = render(&state);
        assert!(!tree.is_empty());
        // Should contain an Image command
        let has_image = tree.commands.iter().any(|cmd| matches!(cmd, RenderCommand::Image { .. }));
        assert!(has_image);
    }

    #[test]
    fn test_handle_tick_advances_slideshow() {
        let mut state = ViewerState::new(800.0, 600.0);
        state.entries = vec![
            DirectoryEntry { path: PathBuf::from("/a.png"), filename: String::from("a.png"), file_size: 100 },
            DirectoryEntry { path: PathBuf::from("/b.png"), filename: String::from("b.png"), file_size: 200 },
            DirectoryEntry { path: PathBuf::from("/c.png"), filename: String::from("c.png"), file_size: 300 },
        ];
        state.current_index = 0;
        state.slideshow.active = true;
        state.slideshow.interval = SlideshowInterval::ThreeSeconds;

        // Not enough time elapsed
        state.handle_tick(2000);
        assert_eq!(state.current_index, 0);

        // Now enough time
        state.handle_tick(1500);
        assert_eq!(state.current_index, 1);
    }

    #[test]
    fn test_handle_tick_paused() {
        let mut state = ViewerState::new(800.0, 600.0);
        state.entries = vec![
            DirectoryEntry { path: PathBuf::from("/a.png"), filename: String::from("a.png"), file_size: 100 },
            DirectoryEntry { path: PathBuf::from("/b.png"), filename: String::from("b.png"), file_size: 200 },
        ];
        state.current_index = 0;
        state.slideshow.active = true;
        state.slideshow.paused = true;
        state.slideshow.interval = SlideshowInterval::ThreeSeconds;

        state.handle_tick(10000);
        // Should not advance when paused
        assert_eq!(state.current_index, 0);
    }

    #[test]
    fn test_path_to_image_id_deterministic() {
        let path = Path::new("/home/user/photos/sunset.jpg");
        let id1 = path_to_image_id(path);
        let id2 = path_to_image_id(path);
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_path_to_image_id_different_paths() {
        let p1 = Path::new("/a.png");
        let p2 = Path::new("/b.png");
        assert_ne!(path_to_image_id(p1), path_to_image_id(p2));
    }

    #[test]
    fn test_placeholder_image() {
        let img = ImageData::placeholder(16, 16, 1);
        assert_eq!(img.width, 16);
        assert_eq!(img.height, 16);
        assert_eq!(img.pixels.len(), 16 * 16 * 4);
        assert_eq!(img.image_id, 1);
    }

    #[test]
    fn test_key_event_zoom_in() {
        let mut state = ViewerState::new(800.0, 600.0);
        let initial_zoom = state.transform.zoom;
        let event = KeyEvent {
            key: Key::Equals,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: None,
        };
        assert!(state.handle_key_event(&event));
        assert!(state.transform.zoom > initial_zoom);
    }

    #[test]
    fn test_key_event_zoom_out() {
        let mut state = ViewerState::new(800.0, 600.0);
        state.transform.zoom = 2.0;
        let event = KeyEvent {
            key: Key::Minus,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: None,
        };
        assert!(state.handle_key_event(&event));
        assert!(state.transform.zoom < 2.0);
    }

    #[test]
    fn test_key_event_not_consumed_on_release() {
        let mut state = ViewerState::new(800.0, 600.0);
        let event = KeyEvent {
            key: Key::Left,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: None,
        };
        assert!(!state.handle_key_event(&event));
    }

    #[test]
    fn test_mouse_scroll_zoom() {
        let mut state = ViewerState::new(800.0, 600.0);
        let initial_zoom = state.transform.zoom;
        let event = MouseEvent {
            x: 400.0,
            y: 300.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy: 1.0 },
        };
        assert!(state.handle_mouse_event(&event));
        assert!(state.transform.zoom > initial_zoom);
    }

    #[test]
    fn test_fullscreen_toggle() {
        let mut state = ViewerState::new(800.0, 600.0);
        assert!(!state.fullscreen);
        state.execute_action(ViewerAction::ToggleFullscreen);
        assert!(state.fullscreen);
        state.execute_action(ViewerAction::ToggleFullscreen);
        assert!(!state.fullscreen);
    }

    #[test]
    fn test_escape_exits_slideshow_first() {
        let mut state = ViewerState::new(800.0, 600.0);
        state.slideshow.active = true;
        state.fullscreen = true;
        let event = KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        // First escape should stop slideshow
        state.handle_key_event(&event);
        assert!(!state.slideshow.active);
        assert!(state.fullscreen);
        // Second escape should exit fullscreen
        state.handle_key_event(&event);
        assert!(!state.fullscreen);
    }

    #[test]
    fn test_navigation_wraps() {
        let mut state = ViewerState::new(800.0, 600.0);
        state.entries = vec![
            DirectoryEntry { path: PathBuf::from("/a.png"), filename: String::from("a.png"), file_size: 100 },
            DirectoryEntry { path: PathBuf::from("/b.png"), filename: String::from("b.png"), file_size: 200 },
            DirectoryEntry { path: PathBuf::from("/c.png"), filename: String::from("c.png"), file_size: 300 },
        ];
        state.current_index = 2;
        state.next_image();
        assert_eq!(state.current_index, 0); // wraps around

        state.current_index = 0;
        state.prev_image();
        assert_eq!(state.current_index, 2); // wraps around
    }
}
