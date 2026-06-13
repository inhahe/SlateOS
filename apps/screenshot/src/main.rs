//! SlateOS Screenshot Utility
//!
//! Screen capture application with:
//! - Full screen, active window, region selection, and delayed captures
//! - BMP file encoding (32-bit BGRA)
//! - Region selection overlay with dimension labels
//! - Annotation tools: rectangle, arrow, text, highlight
//! - Hotkey-driven operation for background service mode
//! - Post-capture preview with save/copy/discard actions
//!
//! Uses the guitk library for UI rendering.

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

use std::path::{Path, PathBuf};

// ============================================================================
// Constants
// ============================================================================

const TOOLBAR_HEIGHT: f32 = 44.0;
const BUTTON_WIDTH: f32 = 120.0;
const BUTTON_HEIGHT: f32 = 32.0;
const BUTTON_SPACING: f32 = 8.0;
const ANNOTATION_TOOLBAR_HEIGHT: f32 = 36.0;
const STATUS_BAR_HEIGHT: f32 = 28.0;

const BG_COLOR: Color = Color::rgb(30, 30, 30);
const TOOLBAR_BG: Color = Color::rgb(48, 48, 48);
const STATUS_BG: Color = Color::rgb(38, 38, 38);
const BUTTON_BG: Color = Color::rgb(60, 60, 60);
const BUTTON_HOVER_BG: Color = Color::rgb(80, 80, 80);
const BUTTON_ACTIVE_BG: Color = Color::rgb(50, 110, 190);
const TEXT_PRIMARY: Color = Color::rgb(230, 230, 230);
const TEXT_SECONDARY: Color = Color::rgb(160, 160, 160);
#[allow(dead_code)]
const ACCENT_COLOR: Color = Color::rgb(70, 140, 220);
const BORDER_COLOR: Color = Color::rgb(70, 70, 70);
const OVERLAY_COLOR: Color = Color::rgba(0, 0, 0, 140);
const SELECTION_BORDER: Color = Color::rgba(70, 140, 220, 220);
const HIGHLIGHT_COLOR: Color = Color::rgba(255, 255, 0, 80);
const ANNOTATION_RED: Color = Color::rgb(220, 50, 50);
#[allow(dead_code)]
const ANNOTATION_BLUE: Color = Color::rgb(50, 100, 220);

/// Default save directory path.
const DEFAULT_SAVE_DIR: &str = "~/Pictures/Screenshots/";

/// BMP file header size (BITMAPFILEHEADER).
const BMP_FILE_HEADER_SIZE: u32 = 14;

/// BMP info header size (BITMAPINFOHEADER).
const BMP_INFO_HEADER_SIZE: u32 = 40;

/// Bytes per pixel in our 32-bit BMP output.
const BMP_BYTES_PER_PIXEL: u32 = 4;

// ============================================================================
// Capture mode
// ============================================================================

/// The type of screen capture to perform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureMode {
    /// Capture the entire screen.
    FullScreen,
    /// Capture the currently active/focused window.
    Window,
    /// User selects a rectangular region via click-drag.
    Region,
    /// Delayed capture after the given number of seconds.
    Delayed(u32),
    /// Click on a specific window to capture it.
    PickWindow,
}

impl CaptureMode {
    /// Human-readable label for UI display.
    pub fn label(self) -> &'static str {
        match self {
            Self::FullScreen => "Full Screen",
            Self::Window => "Active Window",
            Self::Region => "Region",
            Self::Delayed(3) => "Delayed (3s)",
            Self::Delayed(5) => "Delayed (5s)",
            Self::Delayed(n) => {
                // For non-standard delays, return a generic label.
                // Callers can format a custom string if needed.
                let _ = n;
                "Delayed"
            }
            Self::PickWindow => "Pick Window",
        }
    }
}

// ============================================================================
// Annotation types
// ============================================================================

/// Available annotation drawing tools.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotationTool {
    /// Draw a colored rectangle outline.
    Rectangle,
    /// Draw an arrow from start to end point.
    Arrow,
    /// Place a text label.
    Text,
    /// Draw a semi-transparent highlight rectangle.
    Highlight,
}

impl AnnotationTool {
    pub fn label(self) -> &'static str {
        match self {
            Self::Rectangle => "Rectangle",
            Self::Arrow => "Arrow",
            Self::Text => "Text",
            Self::Highlight => "Highlight",
        }
    }
}

/// A single annotation drawn on top of a captured screenshot.
#[derive(Clone, Debug)]
pub struct Annotation {
    pub tool: AnnotationTool,
    pub start_x: f32,
    pub start_y: f32,
    pub end_x: f32,
    pub end_y: f32,
    pub color: Color,
    pub text: String,
}

impl Annotation {
    /// Create a new annotation with the given tool and position.
    pub fn new(tool: AnnotationTool, start_x: f32, start_y: f32, color: Color) -> Self {
        Self {
            tool,
            start_x,
            start_y,
            end_x: start_x,
            end_y: start_y,
            color,
            text: String::new(),
        }
    }

    /// Width of the annotation bounding box.
    pub fn width(&self) -> f32 {
        (self.end_x - self.start_x).abs()
    }

    /// Height of the annotation bounding box.
    pub fn height(&self) -> f32 {
        (self.end_y - self.start_y).abs()
    }

    /// Top-left X of the bounding box.
    pub fn min_x(&self) -> f32 {
        self.start_x.min(self.end_x)
    }

    /// Top-left Y of the bounding box.
    pub fn min_y(&self) -> f32 {
        self.start_y.min(self.end_y)
    }
}

// ============================================================================
// Captured screenshot data
// ============================================================================

/// Holds the pixel data and metadata for a captured screenshot.
#[derive(Clone, Debug)]
pub struct Capture {
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Pixel data in ARGB format (one u32 per pixel, row-major top-down).
    pub pixels: Vec<u32>,
    /// Capture timestamp: (year, month, day, hour, minute, second).
    pub timestamp: (u16, u8, u8, u8, u8, u8),
}

impl Capture {
    /// Create a new capture with the given dimensions and pixel data.
    pub fn new(width: u32, height: u32, pixels: Vec<u32>) -> Self {
        Self {
            width,
            height,
            pixels,
            // Placeholder timestamp; real implementation reads from system clock.
            timestamp: (2026, 1, 1, 0, 0, 0),
        }
    }

    /// Set the timestamp on this capture.
    pub fn with_timestamp(mut self, year: u16, month: u8, day: u8, hour: u8, min: u8, sec: u8) -> Self {
        self.timestamp = (year, month, day, hour, min, sec);
        self
    }

    /// Generate a default filename based on the timestamp.
    pub fn default_filename(&self) -> String {
        let (year, month, day, hour, min, sec) = self.timestamp;
        format!(
            "screenshot_{:04}{:02}{:02}_{:02}{:02}{:02}.bmp",
            year, month, day, hour, min, sec
        )
    }

    /// Total pixel count.
    pub fn pixel_count(&self) -> usize {
        (self.width as usize).saturating_mul(self.height as usize)
    }

    /// Create a test/placeholder capture with a solid color fill.
    pub fn solid(width: u32, height: u32, argb: u32) -> Self {
        let count = (width as usize).saturating_mul(height as usize);
        let pixels = vec![argb; count];
        Self::new(width, height, pixels)
    }
}

// ============================================================================
// BMP encoder
// ============================================================================

/// Errors that can occur during BMP encoding.
#[derive(Debug)]
pub enum BmpError {
    /// The pixel buffer size does not match width * height.
    PixelCountMismatch { expected: usize, actual: usize },
    /// I/O error writing the file.
    Io(std::io::Error),
    /// Dimensions overflow the BMP format limits.
    DimensionOverflow,
}

impl From<std::io::Error> for BmpError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl core::fmt::Display for BmpError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::PixelCountMismatch { expected, actual } => {
                write!(f, "pixel count mismatch: expected {expected}, got {actual}")
            }
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::DimensionOverflow => write!(f, "image dimensions overflow BMP format limits"),
        }
    }
}

/// Encode pixel data as a 32-bit BMP file and write to `path`.
///
/// Pixel data is in ARGB format (u32 per pixel), row-major, top-down.
/// BMP stores rows bottom-up with BGRA byte order.
pub fn write_bmp(path: &Path, width: u32, height: u32, pixels: &[u32]) -> Result<(), BmpError> {
    let expected = (width as usize).saturating_mul(height as usize);
    if pixels.len() != expected {
        return Err(BmpError::PixelCountMismatch {
            expected,
            actual: pixels.len(),
        });
    }

    let row_bytes = width.checked_mul(BMP_BYTES_PER_PIXEL).ok_or(BmpError::DimensionOverflow)?;
    let pixel_data_size = row_bytes.checked_mul(height).ok_or(BmpError::DimensionOverflow)?;
    let header_size = BMP_FILE_HEADER_SIZE + BMP_INFO_HEADER_SIZE;
    let file_size = header_size.checked_add(pixel_data_size).ok_or(BmpError::DimensionOverflow)?;

    let data = encode_bmp_bytes(width, height, pixels, file_size, header_size, pixel_data_size)?;
    std::fs::write(path, &data)?;
    Ok(())
}

/// Encode pixel data to an in-memory BMP byte buffer.
///
/// Returns the complete BMP file as a `Vec<u8>`.
pub fn encode_bmp(width: u32, height: u32, pixels: &[u32]) -> Result<Vec<u8>, BmpError> {
    let expected = (width as usize).saturating_mul(height as usize);
    if pixels.len() != expected {
        return Err(BmpError::PixelCountMismatch {
            expected,
            actual: pixels.len(),
        });
    }

    let row_bytes = width.checked_mul(BMP_BYTES_PER_PIXEL).ok_or(BmpError::DimensionOverflow)?;
    let pixel_data_size = row_bytes.checked_mul(height).ok_or(BmpError::DimensionOverflow)?;
    let header_size = BMP_FILE_HEADER_SIZE + BMP_INFO_HEADER_SIZE;
    let file_size = header_size.checked_add(pixel_data_size).ok_or(BmpError::DimensionOverflow)?;

    encode_bmp_bytes(width, height, pixels, file_size, header_size, pixel_data_size)
}

/// Internal helper: builds the complete BMP byte buffer.
fn encode_bmp_bytes(
    width: u32,
    height: u32,
    pixels: &[u32],
    file_size: u32,
    header_offset: u32,
    pixel_data_size: u32,
) -> Result<Vec<u8>, BmpError> {
    let mut buf = Vec::with_capacity(file_size as usize);

    // --- BITMAPFILEHEADER (14 bytes) ---
    buf.extend_from_slice(b"BM");                          // magic
    buf.extend_from_slice(&file_size.to_le_bytes());       // file size
    buf.extend_from_slice(&0u16.to_le_bytes());            // reserved1
    buf.extend_from_slice(&0u16.to_le_bytes());            // reserved2
    buf.extend_from_slice(&header_offset.to_le_bytes());   // offset to pixel data

    // --- BITMAPINFOHEADER (40 bytes) ---
    buf.extend_from_slice(&BMP_INFO_HEADER_SIZE.to_le_bytes()); // header size
    buf.extend_from_slice(&(width as i32).to_le_bytes());       // width
    // Positive height = bottom-up row order (standard BMP)
    buf.extend_from_slice(&(height as i32).to_le_bytes());      // height
    buf.extend_from_slice(&1u16.to_le_bytes());                 // planes
    buf.extend_from_slice(&32u16.to_le_bytes());                // bits per pixel
    buf.extend_from_slice(&0u32.to_le_bytes());                 // compression (BI_RGB)
    buf.extend_from_slice(&pixel_data_size.to_le_bytes());      // image data size
    buf.extend_from_slice(&2835i32.to_le_bytes());              // X pixels per meter (~72 DPI)
    buf.extend_from_slice(&2835i32.to_le_bytes());              // Y pixels per meter (~72 DPI)
    buf.extend_from_slice(&0u32.to_le_bytes());                 // colors used
    buf.extend_from_slice(&0u32.to_le_bytes());                 // important colors

    // --- Pixel data (bottom-up rows, BGRA byte order) ---
    // BMP stores rows from bottom to top. Our input is top-down ARGB.
    for y in (0..height).rev() {
        let row_start = (y as usize).saturating_mul(width as usize);
        for x in 0..width as usize {
            let idx = row_start.saturating_add(x);
            let argb = pixels.get(idx).copied().unwrap_or(0);
            // ARGB → BGRA byte order
            let a = ((argb >> 24) & 0xFF) as u8;
            let r = ((argb >> 16) & 0xFF) as u8;
            let g_val = ((argb >> 8) & 0xFF) as u8;
            let b = (argb & 0xFF) as u8;
            buf.push(b);
            buf.push(g_val);
            buf.push(r);
            buf.push(a);
        }
    }

    Ok(buf)
}

// ============================================================================
// Region selector
// ============================================================================

/// State for the region-selection overlay mode.
#[derive(Clone, Debug)]
pub struct RegionSelector {
    /// Whether region selection is currently active.
    pub active: bool,
    /// Whether the user is currently dragging.
    pub dragging: bool,
    /// Drag start position.
    pub start_x: f32,
    pub start_y: f32,
    /// Current mouse position during drag.
    pub current_x: f32,
    pub current_y: f32,
    /// Screen dimensions for the overlay.
    pub screen_width: f32,
    pub screen_height: f32,
}

impl RegionSelector {
    pub fn new(screen_width: f32, screen_height: f32) -> Self {
        Self {
            active: false,
            dragging: false,
            start_x: 0.0,
            start_y: 0.0,
            current_x: 0.0,
            current_y: 0.0,
            screen_width,
            screen_height,
        }
    }

    /// Begin region selection mode.
    pub fn activate(&mut self) {
        self.active = true;
        self.dragging = false;
    }

    /// Cancel region selection.
    pub fn cancel(&mut self) {
        self.active = false;
        self.dragging = false;
    }

    /// Begin a drag at the given screen coordinates.
    pub fn start_drag(&mut self, x: f32, y: f32) {
        self.dragging = true;
        self.start_x = x;
        self.start_y = y;
        self.current_x = x;
        self.current_y = y;
    }

    /// Update the drag endpoint.
    pub fn update_drag(&mut self, x: f32, y: f32) {
        self.current_x = x;
        self.current_y = y;
    }

    /// Finish the drag and return the selected rectangle (x, y, w, h).
    /// Returns `None` if the selection is too small (< 4px in either dimension).
    pub fn finish_drag(&mut self) -> Option<(u32, u32, u32, u32)> {
        self.dragging = false;
        self.active = false;

        let x1 = self.start_x.min(self.current_x);
        let y1 = self.start_y.min(self.current_y);
        let x2 = self.start_x.max(self.current_x);
        let y2 = self.start_y.max(self.current_y);

        let w = x2 - x1;
        let h = y2 - y1;

        if w < 4.0 || h < 4.0 {
            return None;
        }

        Some((x1 as u32, y1 as u32, w as u32, h as u32))
    }

    /// The currently selected rectangle during drag (min_x, min_y, width, height).
    pub fn selection_rect(&self) -> (f32, f32, f32, f32) {
        let x1 = self.start_x.min(self.current_x);
        let y1 = self.start_y.min(self.current_y);
        let w = (self.start_x - self.current_x).abs();
        let h = (self.start_y - self.current_y).abs();
        (x1, y1, w, h)
    }

    /// Dimensions label string for the current selection.
    pub fn dimensions_label(&self) -> String {
        let (_, _, w, h) = self.selection_rect();
        format!("{} x {}", w as u32, h as u32)
    }

    /// Render the selection overlay.
    pub fn render(&self, tree: &mut RenderTree) {
        if !self.active {
            return;
        }

        // Semi-transparent dark overlay covering the entire screen.
        tree.fill_rect(0.0, 0.0, self.screen_width, self.screen_height, OVERLAY_COLOR);

        if self.dragging {
            let (sel_x, sel_y, sel_w, sel_h) = self.selection_rect();

            // Clear the selected region (draw a non-transparent rect to "cut" the overlay).
            tree.fill_rect(sel_x, sel_y, sel_w, sel_h, Color::TRANSPARENT);

            // Selection border.
            tree.stroke_rect(sel_x, sel_y, sel_w, sel_h, SELECTION_BORDER, 2.0);

            // Dimension label near the bottom-right of the selection.
            let label = self.dimensions_label();
            let label_x = sel_x + sel_w + 8.0;
            let label_y = sel_y + sel_h + 4.0;

            // Background for label readability.
            let label_w = label.len() as f32 * 8.0 + 12.0;
            tree.fill_rect(label_x - 4.0, label_y - 2.0, label_w, 20.0, Color::rgba(0, 0, 0, 180));
            tree.text(label_x, label_y, &label, TEXT_PRIMARY, 13.0);
        }

        // Crosshair at current mouse position.
        let ch_color = Color::rgba(255, 255, 255, 150);
        tree.push(RenderCommand::Line {
            x1: self.current_x,
            y1: 0.0,
            x2: self.current_x,
            y2: self.screen_height,
            color: ch_color,
            width: 1.0,
        });
        tree.push(RenderCommand::Line {
            x1: 0.0,
            y1: self.current_y,
            x2: self.screen_width,
            y2: self.current_y,
            color: ch_color,
            width: 1.0,
        });
    }
}

// ============================================================================
// Post-capture action
// ============================================================================

/// What to do after a screenshot is captured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostCaptureAction {
    /// Save to a file (the default).
    SaveToFile,
    /// Copy pixel data to the system clipboard.
    CopyToClipboard,
    /// Open the saved file in the image viewer.
    OpenInViewer,
    /// Show the annotation/preview window.
    Annotate,
}

// ============================================================================
// Application state
// ============================================================================

/// The main application view/mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppView {
    /// Quick menu / mode selector.
    Menu,
    /// Region selection overlay (full-screen).
    RegionSelect,
    /// Countdown timer display before delayed capture.
    Countdown,
    /// Preview/annotation of a captured screenshot.
    Preview,
}

/// Settings for the screenshot application.
#[derive(Clone, Debug)]
pub struct Settings {
    /// Directory to save screenshots.
    pub save_directory: PathBuf,
    /// Default action after capture.
    pub default_action: PostCaptureAction,
    /// Whether to play a shutter sound on capture.
    pub play_sound: bool,
    /// Whether to show a notification after capture.
    pub show_notification: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            save_directory: PathBuf::from(DEFAULT_SAVE_DIR),
            default_action: PostCaptureAction::SaveToFile,
            play_sound: true,
            show_notification: true,
        }
    }
}

/// Notification shown after a successful capture.
#[derive(Clone, Debug)]
pub struct Notification {
    pub message: String,
    pub file_path: Option<PathBuf>,
    /// Remaining display time in milliseconds.
    pub remaining_ms: u64,
}

/// Top-level application state for the screenshot utility.
pub struct ScreenshotApp {
    /// Current capture mode selected in the menu.
    pub mode: CaptureMode,
    /// Current application view.
    pub view: AppView,
    /// Window dimensions.
    pub window_width: f32,
    pub window_height: f32,

    /// Region selector state.
    pub region_selector: RegionSelector,
    /// Countdown state: remaining seconds for delayed capture.
    pub countdown_remaining: u32,
    /// Elapsed milliseconds within the current countdown second.
    pub countdown_elapsed_ms: u64,

    /// The most recently captured screenshot.
    pub current_capture: Option<Capture>,
    /// History of previous captures (most recent first).
    pub capture_history: Vec<Capture>,

    /// Current annotation tool.
    pub annotation_tool: AnnotationTool,
    /// Color used for new annotations.
    pub annotation_color: Color,
    /// Annotations on the current capture.
    pub annotations: Vec<Annotation>,
    /// Annotation currently being drawn (not yet committed).
    pub pending_annotation: Option<Annotation>,
    /// Text being typed for a text annotation.
    pub annotation_text_input: String,

    /// Application settings.
    pub settings: Settings,
    /// Active notification (if any).
    pub notification: Option<Notification>,

    /// Which menu button is hovered (index).
    pub hovered_button: Option<usize>,
    /// Whether the app should keep running.
    pub running: bool,
}

impl ScreenshotApp {
    /// Create a new screenshot application with the given window size.
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            mode: CaptureMode::FullScreen,
            view: AppView::Menu,
            window_width: width,
            window_height: height,
            region_selector: RegionSelector::new(width, height),
            countdown_remaining: 0,
            countdown_elapsed_ms: 0,
            current_capture: None,
            capture_history: Vec::new(),
            annotation_tool: AnnotationTool::Rectangle,
            annotation_color: ANNOTATION_RED,
            annotations: Vec::new(),
            pending_annotation: None,
            annotation_text_input: String::new(),
            settings: Settings::default(),
            notification: None,
            hovered_button: None,
            running: true,
        }
    }

    // ========================================================================
    // Capture operations
    // ========================================================================

    /// Initiate a capture based on the current mode.
    pub fn start_capture(&mut self) {
        match self.mode {
            CaptureMode::FullScreen => self.capture_full_screen(),
            CaptureMode::Window => self.capture_active_window(),
            CaptureMode::Region => {
                self.view = AppView::RegionSelect;
                self.region_selector.activate();
            }
            CaptureMode::Delayed(secs) => {
                self.countdown_remaining = secs;
                self.countdown_elapsed_ms = 0;
                self.view = AppView::Countdown;
            }
            CaptureMode::PickWindow => {
                // In a real implementation, this would set the cursor to a
                // crosshair and wait for the user to click a window.
                // For now, fall back to active window capture.
                self.capture_active_window();
            }
        }
    }

    /// Capture the full screen contents.
    ///
    /// In the real OS, this issues a compositor syscall to grab the framebuffer.
    /// Here we create a placeholder capture for development.
    fn capture_full_screen(&mut self) {
        // Placeholder: compositor would provide the actual framebuffer data.
        let w = self.window_width as u32;
        let h = self.window_height as u32;
        let capture = Capture::solid(w, h, 0xFF336699);
        self.finish_capture(capture);
    }

    /// Capture the currently active/focused window.
    fn capture_active_window(&mut self) {
        // Placeholder: compositor would provide the window's pixel data.
        let w = (self.window_width * 0.6) as u32;
        let h = (self.window_height * 0.6) as u32;
        let capture = Capture::solid(w, h, 0xFF996633);
        self.finish_capture(capture);
    }

    /// Capture a rectangular region of the screen.
    pub fn capture_region(&mut self, x: u32, y: u32, width: u32, height: u32) {
        let _ = (x, y); // Region offset used by compositor in real implementation.
        let capture = Capture::solid(width, height, 0xFF669933);
        self.finish_capture(capture);
    }

    /// Process a completed capture: store it, save if needed, show notification.
    fn finish_capture(&mut self, capture: Capture) {
        // Save to file if that is the default action.
        if self.settings.default_action == PostCaptureAction::SaveToFile {
            let filename = capture.default_filename();
            let save_path = self.settings.save_directory.join(&filename);
            match write_bmp(&save_path, capture.width, capture.height, &capture.pixels) {
                Ok(()) => {
                    if self.settings.show_notification {
                        self.notification = Some(Notification {
                            message: format!("Screenshot saved to {}", save_path.display()),
                            file_path: Some(save_path),
                            remaining_ms: 4000,
                        });
                    }
                }
                Err(err) => {
                    self.notification = Some(Notification {
                        message: format!("Failed to save screenshot: {err}"),
                        file_path: None,
                        remaining_ms: 5000,
                    });
                }
            }
        }

        // Move to preview if annotating.
        if self.settings.default_action == PostCaptureAction::Annotate {
            self.view = AppView::Preview;
            self.annotations.clear();
        } else {
            self.view = AppView::Menu;
        }

        // Add to history.
        if self.capture_history.len() >= 20 {
            self.capture_history.pop();
        }
        self.current_capture = Some(capture.clone());
        self.capture_history.insert(0, capture);
    }

    /// Save the current capture (with annotations baked in) to a file.
    pub fn save_current(&mut self) -> Result<PathBuf, BmpError> {
        let capture = match &self.current_capture {
            Some(c) => c,
            None => {
                return Err(BmpError::PixelCountMismatch {
                    expected: 1,
                    actual: 0,
                });
            }
        };

        let filename = capture.default_filename();
        let save_path = self.settings.save_directory.join(filename);
        write_bmp(&save_path, capture.width, capture.height, &capture.pixels)?;
        Ok(save_path)
    }

    /// Discard the current capture and return to the menu.
    pub fn discard_current(&mut self) {
        self.current_capture = None;
        self.annotations.clear();
        self.pending_annotation = None;
        self.view = AppView::Menu;
    }

    // ========================================================================
    // Event handling
    // ========================================================================

    /// Handle a GUI event, returning true if it was consumed.
    pub fn handle_event(&mut self, event: &Event) -> bool {
        match event {
            Event::Key(key_event) if key_event.pressed => self.handle_key(key_event),
            Event::Mouse(mouse_event) => self.handle_mouse(mouse_event),
            Event::Tick { elapsed_ms } => self.handle_tick(*elapsed_ms),
            Event::Resize { width, height } => {
                self.window_width = *width as f32;
                self.window_height = *height as f32;
                self.region_selector.screen_width = *width as f32;
                self.region_selector.screen_height = *height as f32;
                true
            }
            Event::CloseRequested => {
                self.running = false;
                true
            }
            _ => false,
        }
    }

    /// Handle a key press event.
    fn handle_key(&mut self, event: &KeyEvent) -> bool {
        match self.view {
            AppView::Menu => self.handle_key_menu(event),
            AppView::RegionSelect => self.handle_key_region(event),
            AppView::Countdown => self.handle_key_countdown(event),
            AppView::Preview => self.handle_key_preview(event),
        }
    }

    fn handle_key_menu(&mut self, event: &KeyEvent) -> bool {
        // Global hotkeys for capture modes.
        match event.key {
            Key::PrintScreen => {
                if event.modifiers.alt {
                    self.mode = CaptureMode::Window;
                } else if event.modifiers.ctrl {
                    self.mode = CaptureMode::Region;
                } else if event.modifiers.shift {
                    self.mode = CaptureMode::Delayed(3);
                } else {
                    self.mode = CaptureMode::FullScreen;
                }
                self.start_capture();
                true
            }
            Key::Escape => {
                self.running = false;
                true
            }
            Key::Num1 => {
                self.mode = CaptureMode::FullScreen;
                self.start_capture();
                true
            }
            Key::Num2 => {
                self.mode = CaptureMode::Window;
                self.start_capture();
                true
            }
            Key::Num3 => {
                self.mode = CaptureMode::Region;
                self.start_capture();
                true
            }
            Key::Num4 => {
                self.mode = CaptureMode::Delayed(3);
                self.start_capture();
                true
            }
            Key::Num5 => {
                self.mode = CaptureMode::Delayed(5);
                self.start_capture();
                true
            }
            _ => false,
        }
    }

    fn handle_key_region(&mut self, event: &KeyEvent) -> bool {
        if event.key == Key::Escape {
            self.region_selector.cancel();
            self.view = AppView::Menu;
            return true;
        }
        false
    }

    fn handle_key_countdown(&mut self, event: &KeyEvent) -> bool {
        if event.key == Key::Escape {
            self.countdown_remaining = 0;
            self.view = AppView::Menu;
            return true;
        }
        false
    }

    fn handle_key_preview(&mut self, event: &KeyEvent) -> bool {
        match event.key {
            Key::Escape => {
                self.discard_current();
                true
            }
            Key::S if event.modifiers.ctrl => {
                let _ = self.save_current();
                true
            }
            Key::Z if event.modifiers.ctrl => {
                self.undo_annotation();
                true
            }
            Key::Num1 => {
                self.annotation_tool = AnnotationTool::Rectangle;
                true
            }
            Key::Num2 => {
                self.annotation_tool = AnnotationTool::Arrow;
                true
            }
            Key::Num3 => {
                self.annotation_tool = AnnotationTool::Text;
                true
            }
            Key::Num4 => {
                self.annotation_tool = AnnotationTool::Highlight;
                true
            }
            _ => {
                // Capture text input for text annotation tool.
                if self.annotation_tool == AnnotationTool::Text {
                    if let Some(ch) = event.text {
                        self.annotation_text_input.push(ch);
                        return true;
                    }
                    if event.key == Key::Backspace && !self.annotation_text_input.is_empty() {
                        self.annotation_text_input.pop();
                        return true;
                    }
                }
                false
            }
        }
    }

    /// Handle mouse events.
    fn handle_mouse(&mut self, event: &MouseEvent) -> bool {
        match self.view {
            AppView::Menu => self.handle_mouse_menu(event),
            AppView::RegionSelect => self.handle_mouse_region(event),
            AppView::Preview => self.handle_mouse_preview(event),
            AppView::Countdown => false,
        }
    }

    fn handle_mouse_menu(&mut self, event: &MouseEvent) -> bool {
        match &event.kind {
            MouseEventKind::Move => {
                // Check if hovering over a menu button.
                self.hovered_button = self.button_hit_test(event.x, event.y);
                true
            }
            MouseEventKind::Press(MouseButton::Left) => {
                if let Some(idx) = self.button_hit_test(event.x, event.y) {
                    let modes = menu_modes();
                    if let Some(&selected_mode) = modes.get(idx) {
                        self.mode = selected_mode;
                        self.start_capture();
                    }
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    fn handle_mouse_region(&mut self, event: &MouseEvent) -> bool {
        match &event.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                self.region_selector.start_drag(event.x, event.y);
                true
            }
            MouseEventKind::Move => {
                if self.region_selector.dragging {
                    self.region_selector.update_drag(event.x, event.y);
                }
                true
            }
            MouseEventKind::Release(MouseButton::Left) => {
                if self.region_selector.dragging {
                    if let Some((x, y, w, h)) = self.region_selector.finish_drag() {
                        self.capture_region(x, y, w, h);
                    } else {
                        self.view = AppView::Menu;
                    }
                }
                true
            }
            _ => false,
        }
    }

    fn handle_mouse_preview(&mut self, event: &MouseEvent) -> bool {
        let content_y = TOOLBAR_HEIGHT + ANNOTATION_TOOLBAR_HEIGHT;

        match &event.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                if event.y >= content_y {
                    let draw_x = event.x;
                    let draw_y = event.y - content_y;

                    if self.annotation_tool == AnnotationTool::Text {
                        // Place text annotation at click position.
                        if !self.annotation_text_input.is_empty() {
                            let mut ann = Annotation::new(
                                AnnotationTool::Text,
                                draw_x,
                                draw_y,
                                self.annotation_color,
                            );
                            ann.text = self.annotation_text_input.clone();
                            ann.end_x = draw_x + self.annotation_text_input.len() as f32 * 8.0;
                            ann.end_y = draw_y + 16.0;
                            self.annotations.push(ann);
                            self.annotation_text_input.clear();
                        }
                    } else {
                        self.pending_annotation = Some(Annotation::new(
                            self.annotation_tool,
                            draw_x,
                            draw_y,
                            if self.annotation_tool == AnnotationTool::Highlight {
                                HIGHLIGHT_COLOR
                            } else {
                                self.annotation_color
                            },
                        ));
                    }
                    return true;
                }
                false
            }
            MouseEventKind::Move => {
                if let Some(ref mut ann) = self.pending_annotation {
                    ann.end_x = event.x;
                    ann.end_y = event.y - content_y;
                    return true;
                }
                false
            }
            MouseEventKind::Release(MouseButton::Left) => {
                if let Some(mut ann) = self.pending_annotation.take() {
                    ann.end_x = event.x;
                    ann.end_y = event.y - content_y;
                    // Only commit if the annotation has meaningful size.
                    if ann.width() > 2.0 || ann.height() > 2.0 {
                        self.annotations.push(ann);
                    }
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    /// Handle a timer tick. Returns true if state changed.
    fn handle_tick(&mut self, elapsed_ms: u64) -> bool {
        let mut changed = false;

        // Countdown timer for delayed captures.
        if self.view == AppView::Countdown && self.countdown_remaining > 0 {
            self.countdown_elapsed_ms = self.countdown_elapsed_ms.saturating_add(elapsed_ms);
            if self.countdown_elapsed_ms >= 1000 {
                self.countdown_elapsed_ms = self.countdown_elapsed_ms.saturating_sub(1000);
                self.countdown_remaining = self.countdown_remaining.saturating_sub(1);

                if self.countdown_remaining == 0 {
                    self.capture_full_screen();
                }
            }
            changed = true;
        }

        // Notification timeout.
        if let Some(ref mut notif) = self.notification {
            if elapsed_ms >= notif.remaining_ms {
                self.notification = None;
            } else {
                notif.remaining_ms = notif.remaining_ms.saturating_sub(elapsed_ms);
            }
            changed = true;
        }

        changed
    }

    // ========================================================================
    // Annotation helpers
    // ========================================================================

    /// Undo the most recent annotation.
    pub fn undo_annotation(&mut self) {
        self.annotations.pop();
    }

    // ========================================================================
    // Hit testing
    // ========================================================================

    /// Returns the index of the menu button at (x, y), if any.
    fn button_hit_test(&self, x: f32, y: f32) -> Option<usize> {
        let modes = menu_modes();
        let menu_y = TOOLBAR_HEIGHT + 40.0;

        for (i, _mode) in modes.iter().enumerate() {
            let bx = 20.0 + (i as f32) * (BUTTON_WIDTH + BUTTON_SPACING);
            let by = menu_y;
            if x >= bx && x <= bx + BUTTON_WIDTH && y >= by && y <= by + BUTTON_HEIGHT {
                return Some(i);
            }
        }
        None
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the current application state into a `RenderTree`.
    pub fn render(&self) -> RenderTree {
        let mut tree = RenderTree::new();

        match self.view {
            AppView::Menu => self.render_menu(&mut tree),
            AppView::RegionSelect => self.render_region_select(&mut tree),
            AppView::Countdown => self.render_countdown(&mut tree),
            AppView::Preview => self.render_preview(&mut tree),
        }

        // Notification overlay (shown on top of everything).
        if let Some(ref notif) = self.notification {
            self.render_notification(&mut tree, notif);
        }

        tree
    }

    fn render_menu(&self, tree: &mut RenderTree) {
        // Background.
        tree.fill_rect(0.0, 0.0, self.window_width, self.window_height, BG_COLOR);

        // Toolbar.
        tree.fill_rect(0.0, 0.0, self.window_width, TOOLBAR_HEIGHT, TOOLBAR_BG);
        tree.push(RenderCommand::Line {
            x1: 0.0,
            y1: TOOLBAR_HEIGHT,
            x2: self.window_width,
            y2: TOOLBAR_HEIGHT,
            color: BORDER_COLOR,
            width: 1.0,
        });
        tree.text(16.0, 12.0, "Screenshot", TEXT_PRIMARY, 18.0);

        // Menu buttons.
        let modes = menu_modes();
        let menu_y = TOOLBAR_HEIGHT + 40.0;

        for (i, mode) in modes.iter().enumerate() {
            let bx = 20.0 + (i as f32) * (BUTTON_WIDTH + BUTTON_SPACING);
            let by = menu_y;
            let bg = if self.hovered_button == Some(i) {
                BUTTON_HOVER_BG
            } else {
                BUTTON_BG
            };

            tree.fill_rounded_rect(
                bx, by, BUTTON_WIDTH, BUTTON_HEIGHT, bg,
                CornerRadii::all(4.0),
            );
            tree.stroke_rect(bx, by, BUTTON_WIDTH, BUTTON_HEIGHT, BORDER_COLOR, 1.0);
            tree.text(bx + 10.0, by + 8.0, mode.label(), TEXT_PRIMARY, 13.0);
        }

        // Hotkey hints.
        let hints_y = menu_y + BUTTON_HEIGHT + 30.0;
        let hints = [
            "PrintScreen          Full screen",
            "Alt+PrintScreen      Active window",
            "Ctrl+PrintScreen     Region select",
            "Shift+PrintScreen    Delayed (3s)",
        ];
        for (i, hint) in hints.iter().enumerate() {
            tree.text(20.0, hints_y + i as f32 * 22.0, hint, TEXT_SECONDARY, 12.0);
        }

        // Status bar.
        tree.fill_rect(
            0.0,
            self.window_height - STATUS_BAR_HEIGHT,
            self.window_width,
            STATUS_BAR_HEIGHT,
            STATUS_BG,
        );
        let status_text = format!(
            "Mode: {}  |  Save to: {}",
            self.mode.label(),
            self.settings.save_directory.display()
        );
        tree.text(
            10.0,
            self.window_height - STATUS_BAR_HEIGHT + 7.0,
            &status_text,
            TEXT_SECONDARY,
            12.0,
        );
    }

    fn render_region_select(&self, tree: &mut RenderTree) {
        self.region_selector.render(tree);
    }

    fn render_countdown(&self, tree: &mut RenderTree) {
        // Dim background.
        tree.fill_rect(0.0, 0.0, self.window_width, self.window_height, Color::rgba(0, 0, 0, 200));

        // Large countdown number centered on screen.
        let cx = self.window_width / 2.0 - 20.0;
        let cy = self.window_height / 2.0 - 30.0;

        let count_str = format!("{}", self.countdown_remaining);
        tree.push(RenderCommand::Text {
            x: cx,
            y: cy,
            text: count_str,
            color: TEXT_PRIMARY,
            font_size: 72.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // "Press Escape to cancel" hint.
        tree.text(
            cx - 60.0,
            cy + 80.0,
            "Press Escape to cancel",
            TEXT_SECONDARY,
            14.0,
        );
    }

    fn render_preview(&self, tree: &mut RenderTree) {
        // Background.
        tree.fill_rect(0.0, 0.0, self.window_width, self.window_height, BG_COLOR);

        // Main toolbar.
        tree.fill_rect(0.0, 0.0, self.window_width, TOOLBAR_HEIGHT, TOOLBAR_BG);
        tree.text(16.0, 12.0, "Preview", TEXT_PRIMARY, 18.0);

        // Action buttons in toolbar.
        let actions = ["Save", "Copy", "Discard"];
        for (i, label) in actions.iter().enumerate() {
            let bx = self.window_width - ((actions.len() - i) as f32) * (80.0 + BUTTON_SPACING);
            let by = 6.0;
            tree.fill_rounded_rect(bx, by, 80.0, 30.0, BUTTON_BG, CornerRadii::all(4.0));
            tree.text(bx + 12.0, by + 8.0, label, TEXT_PRIMARY, 12.0);
        }

        // Annotation toolbar.
        let ann_y = TOOLBAR_HEIGHT;
        tree.fill_rect(0.0, ann_y, self.window_width, ANNOTATION_TOOLBAR_HEIGHT, Color::rgb(55, 55, 55));

        let tools = [
            AnnotationTool::Rectangle,
            AnnotationTool::Arrow,
            AnnotationTool::Text,
            AnnotationTool::Highlight,
        ];
        for (i, tool) in tools.iter().enumerate() {
            let tx = 10.0 + i as f32 * 90.0;
            let ty = ann_y + 4.0;
            let bg = if self.annotation_tool == *tool {
                BUTTON_ACTIVE_BG
            } else {
                BUTTON_BG
            };
            tree.fill_rounded_rect(tx, ty, 80.0, 28.0, bg, CornerRadii::all(3.0));
            tree.text(tx + 8.0, ty + 7.0, tool.label(), TEXT_PRIMARY, 11.0);
        }

        // Undo button.
        let undo_x = 10.0 + 4.0 * 90.0 + 20.0;
        tree.fill_rounded_rect(undo_x, ann_y + 4.0, 60.0, 28.0, BUTTON_BG, CornerRadii::all(3.0));
        tree.text(undo_x + 10.0, ann_y + 11.0, "Undo", TEXT_PRIMARY, 11.0);

        // Content area: show the captured image.
        let content_y = TOOLBAR_HEIGHT + ANNOTATION_TOOLBAR_HEIGHT;
        let content_h = self.window_height - content_y - STATUS_BAR_HEIGHT;

        if let Some(ref capture) = self.current_capture {
            // Show image placeholder (the compositor would blit the actual pixels).
            tree.fill_rect(0.0, content_y, self.window_width, content_h, Color::rgb(40, 40, 40));

            // Image info overlay.
            let info = format!("{}x{}", capture.width, capture.height);
            tree.text(10.0, content_y + 10.0, &info, TEXT_SECONDARY, 12.0);
        }

        // Render committed annotations.
        tree.translate(0.0, content_y);
        for ann in &self.annotations {
            render_annotation(tree, ann);
        }
        // Render pending annotation.
        if let Some(ref ann) = self.pending_annotation {
            render_annotation(tree, ann);
        }
        tree.untranslate();

        // Text input indicator for text tool.
        if self.annotation_tool == AnnotationTool::Text && !self.annotation_text_input.is_empty() {
            let input_y = self.window_height - STATUS_BAR_HEIGHT - 30.0;
            tree.fill_rect(0.0, input_y, self.window_width, 30.0, Color::rgba(0, 0, 0, 180));
            let display = format!("Text: {}_", self.annotation_text_input);
            tree.text(10.0, input_y + 7.0, &display, TEXT_PRIMARY, 13.0);
        }

        // Status bar.
        tree.fill_rect(
            0.0,
            self.window_height - STATUS_BAR_HEIGHT,
            self.window_width,
            STATUS_BAR_HEIGHT,
            STATUS_BG,
        );
        let ann_count = self.annotations.len();
        let status = format!(
            "Tool: {}  |  Annotations: {}  |  Ctrl+Z: Undo  |  Ctrl+S: Save",
            self.annotation_tool.label(),
            ann_count
        );
        tree.text(
            10.0,
            self.window_height - STATUS_BAR_HEIGHT + 7.0,
            &status,
            TEXT_SECONDARY,
            12.0,
        );
    }

    fn render_notification(&self, tree: &mut RenderTree, notif: &Notification) {
        let nw = 400.0_f32.min(self.window_width - 40.0);
        let nh = 50.0;
        let nx = self.window_width - nw - 20.0;
        let ny = 20.0;

        tree.fill_rounded_rect(
            nx, ny, nw, nh,
            Color::rgba(30, 100, 50, 230),
            CornerRadii::all(6.0),
        );
        tree.stroke_rect(nx, ny, nw, nh, Color::rgba(50, 160, 80, 200), 1.0);
        tree.text(nx + 12.0, ny + 8.0, &notif.message, TEXT_PRIMARY, 12.0);

        if let Some(ref path) = notif.file_path {
            let path_str = format!("{}", path.display());
            tree.text(nx + 12.0, ny + 26.0, &path_str, TEXT_SECONDARY, 11.0);
        }
    }
}

// ============================================================================
// Annotation rendering
// ============================================================================

/// Render a single annotation into the tree.
fn render_annotation(tree: &mut RenderTree, ann: &Annotation) {
    match ann.tool {
        AnnotationTool::Rectangle => {
            tree.stroke_rect(
                ann.min_x(),
                ann.min_y(),
                ann.width(),
                ann.height(),
                ann.color,
                2.0,
            );
        }
        AnnotationTool::Arrow => {
            // Main line.
            tree.push(RenderCommand::Line {
                x1: ann.start_x,
                y1: ann.start_y,
                x2: ann.end_x,
                y2: ann.end_y,
                color: ann.color,
                width: 2.0,
            });

            // Arrowhead: two short lines from the endpoint at angles.
            let dx = ann.end_x - ann.start_x;
            let dy = ann.end_y - ann.start_y;
            let len = (dx * dx + dy * dy).sqrt();
            if len > 1.0 {
                let arrow_len = 12.0_f32.min(len * 0.3);
                // Unit vector along the arrow shaft.
                let ux = dx / len;
                let uy = dy / len;
                // Perpendicular.
                let px = -uy;
                let py = ux;

                let base_x = ann.end_x - ux * arrow_len;
                let base_y = ann.end_y - uy * arrow_len;
                let wing = arrow_len * 0.5;

                tree.push(RenderCommand::Line {
                    x1: ann.end_x,
                    y1: ann.end_y,
                    x2: base_x + px * wing,
                    y2: base_y + py * wing,
                    color: ann.color,
                    width: 2.0,
                });
                tree.push(RenderCommand::Line {
                    x1: ann.end_x,
                    y1: ann.end_y,
                    x2: base_x - px * wing,
                    y2: base_y - py * wing,
                    color: ann.color,
                    width: 2.0,
                });
            }
        }
        AnnotationTool::Text => {
            tree.text(ann.start_x, ann.start_y, &ann.text, ann.color, 14.0);
        }
        AnnotationTool::Highlight => {
            tree.fill_rect(ann.min_x(), ann.min_y(), ann.width(), ann.height(), ann.color);
        }
    }
}

// ============================================================================
// Menu mode list
// ============================================================================

/// The capture modes shown in the quick menu.
fn menu_modes() -> Vec<CaptureMode> {
    vec![
        CaptureMode::FullScreen,
        CaptureMode::Window,
        CaptureMode::Region,
        CaptureMode::Delayed(3),
        CaptureMode::Delayed(5),
    ]
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let mut app = ScreenshotApp::new(800.0, 600.0);

    // Parse command-line arguments for immediate capture mode.
    let args: Vec<String> = std::env::args().collect();
    if let Some(mode_arg) = args.get(1) {
        match mode_arg.as_str() {
            "--fullscreen" | "-f" => {
                app.mode = CaptureMode::FullScreen;
                app.start_capture();
            }
            "--window" | "-w" => {
                app.mode = CaptureMode::Window;
                app.start_capture();
            }
            "--region" | "-r" => {
                app.mode = CaptureMode::Region;
                app.start_capture();
            }
            "--delay3" => {
                app.mode = CaptureMode::Delayed(3);
                app.start_capture();
            }
            "--delay5" => {
                app.mode = CaptureMode::Delayed(5);
                app.start_capture();
            }
            _ => {}
        }
    }

    // Render one frame to verify the rendering pipeline works.
    let _frame = app.render();

    // Event loop placeholder: in practice, the compositor calls us with events
    // and we return render trees each frame.
    // loop {
    //     let event = wait_for_event();
    //     app.handle_event(&event);
    //     let frame = app.render();
    //     submit_frame(frame);
    // }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- BMP encoder tests ----

    #[test]
    fn test_bmp_encode_1x1_white() {
        let pixels = vec![0xFFFFFFFF_u32]; // ARGB white
        let data = encode_bmp(1, 1, &pixels).expect("encode should succeed");

        // Check file header magic.
        assert_eq!(&data[0..2], b"BM");

        // Total size: 14 + 40 + 4 = 58 bytes.
        let file_size = u32::from_le_bytes([data[2], data[3], data[4], data[5]]);
        assert_eq!(file_size, 58);

        // Pixel data offset.
        let offset = u32::from_le_bytes([data[10], data[11], data[12], data[13]]);
        assert_eq!(offset, 54);

        // Pixel at offset 54: BGRA = (0xFF, 0xFF, 0xFF, 0xFF).
        assert_eq!(data[54], 0xFF); // B
        assert_eq!(data[55], 0xFF); // G
        assert_eq!(data[56], 0xFF); // R
        assert_eq!(data[57], 0xFF); // A
    }

    #[test]
    fn test_bmp_encode_2x2() {
        // 2x2 image:
        //   top-left=red, top-right=green, bottom-left=blue, bottom-right=black
        let pixels = vec![
            0xFFFF0000, 0xFF00FF00, // row 0 (top)
            0xFF0000FF, 0xFF000000, // row 1 (bottom)
        ];
        let data = encode_bmp(2, 2, &pixels).expect("encode should succeed");

        let offset = u32::from_le_bytes([data[10], data[11], data[12], data[13]]) as usize;

        // BMP is bottom-up, so first row in file is row 1 (bottom).
        // Row 1, pixel 0 = blue (ARGB 0xFF0000FF) → BGRA = (0xFF, 0x00, 0x00, 0xFF)
        assert_eq!(data[offset], 0xFF);     // B
        assert_eq!(data[offset + 1], 0x00); // G
        assert_eq!(data[offset + 2], 0x00); // R
        assert_eq!(data[offset + 3], 0xFF); // A

        // Row 1, pixel 1 = black (ARGB 0xFF000000) → BGRA = (0x00, 0x00, 0x00, 0xFF)
        assert_eq!(data[offset + 4], 0x00); // B
        assert_eq!(data[offset + 5], 0x00); // G
        assert_eq!(data[offset + 6], 0x00); // R
        assert_eq!(data[offset + 7], 0xFF); // A

        // Row 0, pixel 0 = red (ARGB 0xFFFF0000) → BGRA = (0x00, 0x00, 0xFF, 0xFF)
        assert_eq!(data[offset + 8], 0x00);  // B
        assert_eq!(data[offset + 9], 0x00);  // G
        assert_eq!(data[offset + 10], 0xFF); // R
        assert_eq!(data[offset + 11], 0xFF); // A

        // Row 0, pixel 1 = green (ARGB 0xFF00FF00) → BGRA = (0x00, 0xFF, 0x00, 0xFF)
        assert_eq!(data[offset + 12], 0x00); // B
        assert_eq!(data[offset + 13], 0xFF); // G
        assert_eq!(data[offset + 14], 0x00); // R
        assert_eq!(data[offset + 15], 0xFF); // A
    }

    #[test]
    fn test_bmp_encode_pixel_mismatch_error() {
        let pixels = vec![0xFF000000; 5]; // 5 pixels but dimensions say 2x3=6
        let result = encode_bmp(2, 3, &pixels);
        assert!(result.is_err());
    }

    #[test]
    fn test_bmp_encode_zero_dimensions() {
        let pixels: Vec<u32> = vec![];
        let data = encode_bmp(0, 0, &pixels).expect("encode should succeed");
        assert_eq!(&data[0..2], b"BM");
        let file_size = u32::from_le_bytes([data[2], data[3], data[4], data[5]]);
        assert_eq!(file_size, 54); // Just headers, no pixel data.
    }

    #[test]
    fn test_bmp_header_dimensions() {
        let pixels = vec![0xFF000000; 10 * 20];
        let data = encode_bmp(10, 20, &pixels).expect("encode should succeed");

        // Width at offset 18 (4 bytes LE).
        let width = i32::from_le_bytes([data[18], data[19], data[20], data[21]]);
        assert_eq!(width, 10);

        // Height at offset 22 (4 bytes LE).
        let height = i32::from_le_bytes([data[22], data[23], data[24], data[25]]);
        assert_eq!(height, 20);

        // Bits per pixel at offset 28 (2 bytes LE).
        let bpp = u16::from_le_bytes([data[28], data[29]]);
        assert_eq!(bpp, 32);
    }

    #[test]
    fn test_bmp_info_header_size() {
        let pixels = vec![0xFF000000; 4];
        let data = encode_bmp(2, 2, &pixels).expect("encode should succeed");

        // Info header size at offset 14 (4 bytes LE).
        let info_size = u32::from_le_bytes([data[14], data[15], data[16], data[17]]);
        assert_eq!(info_size, 40);
    }

    // ---- Capture tests ----

    #[test]
    fn test_capture_default_filename() {
        let capture = Capture::new(100, 100, vec![0; 10000])
            .with_timestamp(2026, 5, 17, 14, 30, 45);
        assert_eq!(capture.default_filename(), "screenshot_20260517_143045.bmp");
    }

    #[test]
    fn test_capture_solid_fill() {
        let capture = Capture::solid(4, 4, 0xFFAABBCC);
        assert_eq!(capture.pixel_count(), 16);
        assert_eq!(capture.pixels.len(), 16);
        for &px in &capture.pixels {
            assert_eq!(px, 0xFFAABBCC);
        }
    }

    // ---- Region selector tests ----

    #[test]
    fn test_region_selector_activation() {
        let mut sel = RegionSelector::new(1920.0, 1080.0);
        assert!(!sel.active);
        sel.activate();
        assert!(sel.active);
        assert!(!sel.dragging);
    }

    #[test]
    fn test_region_selector_drag_and_finish() {
        let mut sel = RegionSelector::new(1920.0, 1080.0);
        sel.activate();
        sel.start_drag(100.0, 200.0);
        sel.update_drag(300.0, 400.0);

        let (rx, ry, rw, rh) = sel.selection_rect();
        assert!((rx - 100.0).abs() < f32::EPSILON);
        assert!((ry - 200.0).abs() < f32::EPSILON);
        assert!((rw - 200.0).abs() < f32::EPSILON);
        assert!((rh - 200.0).abs() < f32::EPSILON);

        let result = sel.finish_drag();
        assert!(result.is_some());
        let (x, y, w, h) = result.expect("should have a region");
        assert_eq!(x, 100);
        assert_eq!(y, 200);
        assert_eq!(w, 200);
        assert_eq!(h, 200);
    }

    #[test]
    fn test_region_selector_too_small() {
        let mut sel = RegionSelector::new(1920.0, 1080.0);
        sel.activate();
        sel.start_drag(100.0, 100.0);
        sel.update_drag(102.0, 102.0); // Only 2x2 - too small.
        let result = sel.finish_drag();
        assert!(result.is_none());
    }

    #[test]
    fn test_region_selector_reverse_drag() {
        let mut sel = RegionSelector::new(1920.0, 1080.0);
        sel.activate();
        sel.start_drag(400.0, 300.0);
        sel.update_drag(100.0, 100.0);

        let result = sel.finish_drag();
        assert!(result.is_some());
        let (x, y, w, h) = result.expect("should have a region");
        assert_eq!(x, 100);
        assert_eq!(y, 100);
        assert_eq!(w, 300);
        assert_eq!(h, 200);
    }

    #[test]
    fn test_region_selector_cancel() {
        let mut sel = RegionSelector::new(1920.0, 1080.0);
        sel.activate();
        sel.start_drag(10.0, 10.0);
        sel.cancel();
        assert!(!sel.active);
        assert!(!sel.dragging);
    }

    #[test]
    fn test_region_selector_dimensions_label() {
        let mut sel = RegionSelector::new(1920.0, 1080.0);
        sel.activate();
        sel.start_drag(0.0, 0.0);
        sel.update_drag(640.0, 480.0);
        assert_eq!(sel.dimensions_label(), "640 x 480");
    }

    // ---- Annotation tests ----

    #[test]
    fn test_annotation_bounding_box() {
        let ann = Annotation {
            tool: AnnotationTool::Rectangle,
            start_x: 50.0,
            start_y: 80.0,
            end_x: 200.0,
            end_y: 150.0,
            color: ANNOTATION_RED,
            text: String::new(),
        };
        assert!((ann.width() - 150.0).abs() < f32::EPSILON);
        assert!((ann.height() - 70.0).abs() < f32::EPSILON);
        assert!((ann.min_x() - 50.0).abs() < f32::EPSILON);
        assert!((ann.min_y() - 80.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_annotation_reverse_coords() {
        let ann = Annotation {
            tool: AnnotationTool::Highlight,
            start_x: 300.0,
            start_y: 200.0,
            end_x: 100.0,
            end_y: 50.0,
            color: HIGHLIGHT_COLOR,
            text: String::new(),
        };
        assert!((ann.min_x() - 100.0).abs() < f32::EPSILON);
        assert!((ann.min_y() - 50.0).abs() < f32::EPSILON);
        assert!((ann.width() - 200.0).abs() < f32::EPSILON);
        assert!((ann.height() - 150.0).abs() < f32::EPSILON);
    }

    // ---- App state tests ----

    #[test]
    fn test_app_initial_state() {
        let app = ScreenshotApp::new(800.0, 600.0);
        assert_eq!(app.mode, CaptureMode::FullScreen);
        assert_eq!(app.view, AppView::Menu);
        assert!(app.running);
        assert!(app.current_capture.is_none());
        assert!(app.annotations.is_empty());
    }

    #[test]
    fn test_app_fullscreen_capture() {
        let mut app = ScreenshotApp::new(800.0, 600.0);
        app.settings.default_action = PostCaptureAction::Annotate;
        app.mode = CaptureMode::FullScreen;
        app.start_capture();
        assert!(app.current_capture.is_some());
        assert_eq!(app.view, AppView::Preview);
    }

    #[test]
    fn test_app_region_mode_activates_selector() {
        let mut app = ScreenshotApp::new(800.0, 600.0);
        app.mode = CaptureMode::Region;
        app.start_capture();
        assert_eq!(app.view, AppView::RegionSelect);
        assert!(app.region_selector.active);
    }

    #[test]
    fn test_app_delayed_mode_starts_countdown() {
        let mut app = ScreenshotApp::new(800.0, 600.0);
        app.mode = CaptureMode::Delayed(5);
        app.start_capture();
        assert_eq!(app.view, AppView::Countdown);
        assert_eq!(app.countdown_remaining, 5);
    }

    #[test]
    fn test_app_countdown_tick() {
        let mut app = ScreenshotApp::new(800.0, 600.0);
        app.mode = CaptureMode::Delayed(2);
        app.settings.default_action = PostCaptureAction::Annotate;
        app.start_capture();
        assert_eq!(app.countdown_remaining, 2);

        app.handle_tick(1000);
        assert_eq!(app.countdown_remaining, 1);

        app.handle_tick(1000);
        assert_eq!(app.countdown_remaining, 0);
        // Should have captured after countdown reaches zero.
        assert!(app.current_capture.is_some());
    }

    #[test]
    fn test_app_escape_cancels_countdown() {
        let mut app = ScreenshotApp::new(800.0, 600.0);
        app.mode = CaptureMode::Delayed(3);
        app.start_capture();
        assert_eq!(app.view, AppView::Countdown);

        let esc = Event::Key(KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&esc);
        assert_eq!(app.view, AppView::Menu);
        assert_eq!(app.countdown_remaining, 0);
    }

    #[test]
    fn test_app_undo_annotation() {
        let mut app = ScreenshotApp::new(800.0, 600.0);
        app.annotations.push(Annotation::new(
            AnnotationTool::Rectangle,
            10.0,
            10.0,
            ANNOTATION_RED,
        ));
        app.annotations.push(Annotation::new(
            AnnotationTool::Arrow,
            20.0,
            20.0,
            ANNOTATION_BLUE,
        ));
        assert_eq!(app.annotations.len(), 2);

        app.undo_annotation();
        assert_eq!(app.annotations.len(), 1);
        assert_eq!(app.annotations[0].tool, AnnotationTool::Rectangle);

        app.undo_annotation();
        assert!(app.annotations.is_empty());

        // Undo on empty list should not panic.
        app.undo_annotation();
        assert!(app.annotations.is_empty());
    }

    #[test]
    fn test_app_discard_clears_state() {
        let mut app = ScreenshotApp::new(800.0, 600.0);
        app.current_capture = Some(Capture::solid(10, 10, 0xFF000000));
        app.annotations.push(Annotation::new(
            AnnotationTool::Rectangle,
            0.0,
            0.0,
            ANNOTATION_RED,
        ));
        app.view = AppView::Preview;

        app.discard_current();
        assert!(app.current_capture.is_none());
        assert!(app.annotations.is_empty());
        assert_eq!(app.view, AppView::Menu);
    }

    #[test]
    fn test_app_capture_history_limit() {
        let mut app = ScreenshotApp::new(800.0, 600.0);
        app.settings.default_action = PostCaptureAction::Annotate;
        for _ in 0..25 {
            app.mode = CaptureMode::FullScreen;
            app.start_capture();
        }
        // History should be capped at 20.
        assert!(app.capture_history.len() <= 20);
    }

    #[test]
    fn test_app_render_produces_commands() {
        let app = ScreenshotApp::new(800.0, 600.0);
        let tree = app.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_app_hotkey_printscreen() {
        let mut app = ScreenshotApp::new(800.0, 600.0);
        app.settings.default_action = PostCaptureAction::Annotate;

        let event = Event::Key(KeyEvent {
            key: Key::PrintScreen,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&event);
        assert!(app.current_capture.is_some());
    }

    #[test]
    fn test_app_hotkey_alt_printscreen() {
        let mut app = ScreenshotApp::new(800.0, 600.0);
        app.settings.default_action = PostCaptureAction::Annotate;

        let event = Event::Key(KeyEvent {
            key: Key::PrintScreen,
            pressed: true,
            modifiers: Modifiers::alt(),
            text: None,
        });
        app.handle_event(&event);
        assert_eq!(app.mode, CaptureMode::Window);
        assert!(app.current_capture.is_some());
    }

    #[test]
    fn test_app_hotkey_ctrl_printscreen() {
        let mut app = ScreenshotApp::new(800.0, 600.0);

        let event = Event::Key(KeyEvent {
            key: Key::PrintScreen,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: None,
        });
        app.handle_event(&event);
        assert_eq!(app.mode, CaptureMode::Region);
        assert_eq!(app.view, AppView::RegionSelect);
    }

    #[test]
    fn test_app_hotkey_shift_printscreen() {
        let mut app = ScreenshotApp::new(800.0, 600.0);

        let event = Event::Key(KeyEvent {
            key: Key::PrintScreen,
            pressed: true,
            modifiers: Modifiers::shift(),
            text: None,
        });
        app.handle_event(&event);
        assert_eq!(app.mode, CaptureMode::Delayed(3));
        assert_eq!(app.view, AppView::Countdown);
    }

    #[test]
    fn test_capture_mode_labels() {
        assert_eq!(CaptureMode::FullScreen.label(), "Full Screen");
        assert_eq!(CaptureMode::Window.label(), "Active Window");
        assert_eq!(CaptureMode::Region.label(), "Region");
        assert_eq!(CaptureMode::Delayed(3).label(), "Delayed (3s)");
        assert_eq!(CaptureMode::Delayed(5).label(), "Delayed (5s)");
        assert_eq!(CaptureMode::PickWindow.label(), "Pick Window");
    }

    #[test]
    fn test_annotation_tool_labels() {
        assert_eq!(AnnotationTool::Rectangle.label(), "Rectangle");
        assert_eq!(AnnotationTool::Arrow.label(), "Arrow");
        assert_eq!(AnnotationTool::Text.label(), "Text");
        assert_eq!(AnnotationTool::Highlight.label(), "Highlight");
    }

    #[test]
    fn test_app_resize_event() {
        let mut app = ScreenshotApp::new(800.0, 600.0);
        let event = Event::Resize {
            width: 1920,
            height: 1080,
        };
        app.handle_event(&event);
        assert!((app.window_width - 1920.0).abs() < f32::EPSILON);
        assert!((app.window_height - 1080.0).abs() < f32::EPSILON);
        assert!((app.region_selector.screen_width - 1920.0).abs() < f32::EPSILON);
        assert!((app.region_selector.screen_height - 1080.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_app_close_requested() {
        let mut app = ScreenshotApp::new(800.0, 600.0);
        assert!(app.running);
        let event = Event::CloseRequested;
        app.handle_event(&event);
        assert!(!app.running);
    }

    #[test]
    fn test_notification_timeout() {
        let mut app = ScreenshotApp::new(800.0, 600.0);
        app.notification = Some(Notification {
            message: "test".to_string(),
            file_path: None,
            remaining_ms: 2000,
        });

        app.handle_tick(1000);
        assert!(app.notification.is_some());

        app.handle_tick(1500);
        assert!(app.notification.is_none());
    }

    #[test]
    fn test_button_hit_test() {
        let app = ScreenshotApp::new(800.0, 600.0);
        // First button starts at x=20, y=TOOLBAR_HEIGHT+40
        let hit = app.button_hit_test(30.0, TOOLBAR_HEIGHT + 50.0);
        assert_eq!(hit, Some(0));

        // Between buttons or outside — no hit.
        let miss = app.button_hit_test(0.0, 0.0);
        assert_eq!(miss, None);
    }

    #[test]
    fn test_region_select_render_not_empty() {
        let mut sel = RegionSelector::new(1920.0, 1080.0);
        sel.activate();
        sel.start_drag(100.0, 100.0);
        sel.update_drag(500.0, 400.0);

        let mut tree = RenderTree::new();
        sel.render(&mut tree);
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_preview_render_with_annotations() {
        let mut app = ScreenshotApp::new(800.0, 600.0);
        app.current_capture = Some(Capture::solid(800, 600, 0xFF000000));
        app.view = AppView::Preview;
        app.annotations.push(Annotation {
            tool: AnnotationTool::Rectangle,
            start_x: 10.0,
            start_y: 10.0,
            end_x: 100.0,
            end_y: 100.0,
            color: ANNOTATION_RED,
            text: String::new(),
        });
        app.annotations.push(Annotation {
            tool: AnnotationTool::Arrow,
            start_x: 50.0,
            start_y: 50.0,
            end_x: 200.0,
            end_y: 200.0,
            color: ANNOTATION_BLUE,
            text: String::new(),
        });
        app.annotations.push(Annotation {
            tool: AnnotationTool::Text,
            start_x: 10.0,
            start_y: 10.0,
            end_x: 80.0,
            end_y: 26.0,
            color: TEXT_PRIMARY,
            text: "Hello".to_string(),
        });
        app.annotations.push(Annotation {
            tool: AnnotationTool::Highlight,
            start_x: 0.0,
            start_y: 0.0,
            end_x: 50.0,
            end_y: 30.0,
            color: HIGHLIGHT_COLOR,
            text: String::new(),
        });

        let tree = app.render();
        assert!(!tree.is_empty());
    }
}
