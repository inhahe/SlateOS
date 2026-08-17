//! Slate OS Screenshot Utility
//!
//! Screen capture application with:
//! - Full screen, active window, region selection, and delayed captures
//! - BMP file encoding (32-bit BGRA)
//! - Region selection overlay with dimension labels
//! - Annotation tools: rectangle, arrow, text, highlight
//! - Annotations flattened into the saved image, so the file matches the preview
//! - Hotkey-driven operation for background service mode
//! - Post-capture preview with save/copy/discard actions
//!
//! Uses the guitk library for UI rendering.

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
#[allow(unused_imports)]
use guitk::style::CornerRadii;
use guitk::text;

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

/// A clickable control in the preview view's two toolbars.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewButton {
    /// Write the annotated capture to a file.
    Save,
    /// Put the annotated capture on the system clipboard.
    Copy,
    /// Throw the capture away and return to the menu.
    Discard,
    /// Select an annotation tool.
    Tool(AnnotationTool),
    /// Remove the most recent annotation.
    Undo,
}

/// The action buttons in the preview's main toolbar, left to right.
const PREVIEW_ACTIONS: [PreviewButton; 3] = [
    PreviewButton::Save,
    PreviewButton::Copy,
    PreviewButton::Discard,
];

/// The tool buttons in the preview's annotation toolbar, left to right.
const PREVIEW_TOOLS: [AnnotationTool; 4] = [
    AnnotationTool::Rectangle,
    AnnotationTool::Arrow,
    AnnotationTool::Text,
    AnnotationTool::Highlight,
];

/// Width of an action button in the preview's main toolbar.
const PREVIEW_ACTION_WIDTH: f32 = 80.0;

/// Bounds of the `i`th preview action button, as `(x, y, w, h)`.
///
/// Rendering and hit-testing both call this. They used to compute the geometry
/// separately — except hit-testing did not compute it at all, so Save, Copy and
/// Discard were painted every frame and could never be clicked.
fn preview_action_rect(window_width: f32, i: usize) -> (f32, f32, f32, f32) {
    #[allow(clippy::cast_precision_loss)]
    let from_right = (PREVIEW_ACTIONS.len().saturating_sub(i)) as f32;
    let x = window_width - from_right * (PREVIEW_ACTION_WIDTH + BUTTON_SPACING);
    (x, 6.0, PREVIEW_ACTION_WIDTH, 30.0)
}

/// Bounds of the `i`th annotation tool button, as `(x, y, w, h)`.
fn preview_tool_rect(i: usize) -> (f32, f32, f32, f32) {
    #[allow(clippy::cast_precision_loss)]
    let x = 10.0 + i as f32 * 90.0;
    (x, TOOLBAR_HEIGHT + 4.0, 80.0, 28.0)
}

/// Bounds of the Undo button, as `(x, y, w, h)`.
fn preview_undo_rect() -> (f32, f32, f32, f32) {
    (10.0 + 4.0 * 90.0 + 20.0, TOOLBAR_HEIGHT + 4.0, 60.0, 28.0)
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

/// Why a save did not happen.
///
/// Distinct from [`BmpError`] because "there is nothing captured to save" is
/// not an encoding failure, and reporting it as one — which this used to do,
/// as a `PixelCountMismatch` of 1-expected-0-actual — produced a message about
/// pixel counts for a user who had simply not taken a screenshot yet.
#[derive(Debug)]
pub enum SaveError {
    /// No capture is loaded, so there is nothing to write.
    NoCapture,
    /// Encoding or writing the BMP failed.
    Bmp(BmpError),
}

impl From<BmpError> for SaveError {
    fn from(err: BmpError) -> Self {
        Self::Bmp(err)
    }
}

impl core::fmt::Display for SaveError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoCapture => write!(f, "no screenshot to save"),
            Self::Bmp(err) => write!(f, "{err}"),
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
            let label_w = text::padded_width(&label, 6.0, 13.0, FontWeightHint::Regular);
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

    /// Turn the outcome of a save into the notification the user sees.
    ///
    /// Both save paths — the automatic one in [`finish_capture`](Self::finish_capture)
    /// and the explicit Ctrl+S in the preview — go through here, so they cannot
    /// drift into reporting the same event differently. A *failure* always
    /// notifies, even with `show_notification` off: that setting suppresses the
    /// routine "saved!" confirmation, and reading it as permission to hide an
    /// error is how a screenshot silently fails to exist.
    fn notify_save(&mut self, outcome: &Result<PathBuf, SaveError>) {
        match outcome {
            Ok(path) => {
                if self.settings.show_notification {
                    self.notification = Some(Notification {
                        message: format!("Screenshot saved to {}", path.display()),
                        file_path: Some(path.clone()),
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

    /// Process a completed capture: store it, save if needed, show notification.
    fn finish_capture(&mut self, capture: Capture) {
        // Save to file if that is the default action.
        if self.settings.default_action == PostCaptureAction::SaveToFile {
            let filename = capture.default_filename();
            let save_path = self.settings.save_directory.join(&filename);
            let outcome = write_bmp(&save_path, capture.width, capture.height, &capture.pixels)
                .map(|()| save_path)
                .map_err(SaveError::Bmp);
            self.notify_save(&outcome);
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

    /// Save the current capture, with annotations baked in, to a file.
    ///
    /// The annotations are flattened into a *copy* of the pixels: the capture
    /// itself stays clean, so undo still works and a second save after another
    /// arrow does not stack the first one twice.
    pub fn save_current(&mut self) -> Result<PathBuf, SaveError> {
        let capture = match &self.current_capture {
            Some(c) => c,
            None => return Err(SaveError::NoCapture),
        };

        let filename = capture.default_filename();
        let save_path = self.settings.save_directory.join(filename);
        let pixels = flatten_annotations(capture, &self.annotations);
        write_bmp(&save_path, capture.width, capture.height, &pixels)?;
        Ok(save_path)
    }

    /// Save the current capture and tell the user what happened.
    ///
    /// The user-facing operation. [`save_current`](Self::save_current) is the
    /// mechanism and returns a `Result` nobody is obliged to read; this is the
    /// one the UI calls, and it cannot fail silently.
    pub fn save_current_notifying(&mut self) -> bool {
        let outcome = self.save_current();
        let ok = outcome.is_ok();
        self.notify_save(&outcome);
        ok
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
                self.save_current_notifying();
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
                // Toolbars first: they sit above the content area, and a click
                // there is a command, not a place to start drawing.
                if event.y < content_y {
                    if let Some(button) = self.preview_button_at(event.x, event.y) {
                        self.activate_preview_button(button);
                        return true;
                    }
                    return false;
                }
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
                        // The annotation's box is the box its text will
                        // actually occupy when drawn, not one guessed from the
                        // byte count.
                        ann.end_x = draw_x
                            + text::measure(
                                &self.annotation_text_input,
                                ANNOTATION_TEXT_SIZE,
                                FontWeightHint::Regular,
                            );
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
                true
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

    /// Returns the preview toolbar control at (x, y), if any.
    #[must_use]
    pub fn preview_button_at(&self, x: f32, y: f32) -> Option<PreviewButton> {
        let hit = |r: (f32, f32, f32, f32)| x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3;

        for (i, action) in PREVIEW_ACTIONS.iter().enumerate() {
            if hit(preview_action_rect(self.window_width, i)) {
                return Some(*action);
            }
        }
        for (i, tool) in PREVIEW_TOOLS.iter().enumerate() {
            if hit(preview_tool_rect(i)) {
                return Some(PreviewButton::Tool(*tool));
            }
        }
        if hit(preview_undo_rect()) {
            return Some(PreviewButton::Undo);
        }
        None
    }

    /// Perform the action a preview toolbar control stands for.
    pub fn activate_preview_button(&mut self, button: PreviewButton) {
        match button {
            PreviewButton::Save => {
                self.save_current_notifying();
            }
            PreviewButton::Copy => {
                // The clipboard lives in a separate service reached over IPC,
                // and this app has no channel to it yet. Say so rather than
                // appearing to copy — a button that silently does nothing is
                // the bug this whole pass is about.
                self.notification = Some(Notification {
                    message: "Copy needs the clipboard service, which is not connected yet"
                        .to_string(),
                    file_path: None,
                    remaining_ms: 4000,
                });
            }
            PreviewButton::Discard => self.discard_current(),
            PreviewButton::Tool(tool) => self.annotation_tool = tool,
            PreviewButton::Undo => self.undo_annotation(),
        }
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
            overflow: TextOverflow::Clip,
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
        for (i, action) in PREVIEW_ACTIONS.iter().enumerate() {
            let (bx, by, bw, bh) = preview_action_rect(self.window_width, i);
            let label = match action {
                PreviewButton::Save => "Save",
                PreviewButton::Copy => "Copy",
                _ => "Discard",
            };
            tree.fill_rounded_rect(bx, by, bw, bh, BUTTON_BG, CornerRadii::all(4.0));
            tree.text(bx + 12.0, by + 8.0, label, TEXT_PRIMARY, 12.0);
        }

        // Annotation toolbar.
        let ann_y = TOOLBAR_HEIGHT;
        tree.fill_rect(0.0, ann_y, self.window_width, ANNOTATION_TOOLBAR_HEIGHT, Color::rgb(55, 55, 55));

        for (i, tool) in PREVIEW_TOOLS.iter().enumerate() {
            let (tx, ty, tw, th) = preview_tool_rect(i);
            let bg = if self.annotation_tool == *tool {
                BUTTON_ACTIVE_BG
            } else {
                BUTTON_BG
            };
            tree.fill_rounded_rect(tx, ty, tw, th, bg, CornerRadii::all(3.0));
            tree.text(tx + 8.0, ty + 7.0, tool.label(), TEXT_PRIMARY, 11.0);
        }

        // Undo button.
        let (ux, uy, uw, uh) = preview_undo_rect();
        tree.fill_rounded_rect(ux, uy, uw, uh, BUTTON_BG, CornerRadii::all(3.0));
        tree.text(ux + 10.0, uy + 7.0, "Undo", TEXT_PRIMARY, 11.0);

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
            tree.text(
                ann.start_x,
                ann.start_y,
                &ann.text,
                ann.color,
                ANNOTATION_TEXT_SIZE,
            );
        }
        AnnotationTool::Highlight => {
            tree.fill_rect(ann.min_x(), ann.min_y(), ann.width(), ann.height(), ann.color);
        }
    }
}

// ============================================================================
// Annotation flattening (baking annotations into the saved pixels)
// ============================================================================

/// A mutable ARGB pixel surface with alpha blending, used to bake annotations
/// into a capture before it is written to disk.
///
/// # Why this is not the compositor's code
///
/// Saving happens with no compositor in the loop: the app owns the pixels and
/// writes them straight to a file. The drawing rules here therefore deliberately
/// mirror the compositor's — 1-pixel Bresenham lines, an inward rectangle
/// stroke, source-over blending — so that what lands in the file is what the
/// user saw on screen. Where the compositor and this disagree, this is the bug.
struct Canvas<'a> {
    pixels: &'a mut [u32],
    width: u32,
    height: u32,
}

impl Canvas<'_> {
    /// Blend `color` over the pixel at `(x, y)`, ignoring out-of-bounds writes.
    fn blend(&mut self, x: i32, y: i32, color: Color) {
        if color.a == 0 || x < 0 || y < 0 {
            return;
        }
        let (Ok(ux), Ok(uy)) = (u32::try_from(x), u32::try_from(y)) else {
            return;
        };
        if ux >= self.width || uy >= self.height {
            return;
        }
        let idx = (uy as usize)
            .saturating_mul(self.width as usize)
            .saturating_add(ux as usize);
        let Some(slot) = self.pixels.get_mut(idx) else {
            return;
        };
        if color.a == 255 {
            *slot = argb_of(color);
            return;
        }
        // Source-over in 8-bit channels. `src * a + dst * (255 - a)` peaks at
        // 65_025, so a u32 intermediate cannot overflow.
        let a = u32::from(color.a);
        let inv = 255_u32.saturating_sub(a);
        let mix = |src: u8, dst: u32| -> u32 {
            (u32::from(src).saturating_mul(a).saturating_add(dst.saturating_mul(inv))) / 255
        };
        let dst = *slot;
        let r = mix(color.r, (dst >> 16) & 0xFF);
        let g = mix(color.g, (dst >> 8) & 0xFF);
        let b = mix(color.b, dst & 0xFF);
        *slot = 0xFF00_0000 | (r << 16) | (g << 8) | b;
    }

    /// Fill an axis-aligned rectangle given in float screen coordinates.
    fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        let (Some(x0), Some(y0)) = (whole(x), whole(y)) else {
            return;
        };
        let (Some(x1), Some(y1)) = (whole(x + w), whole(y + h)) else {
            return;
        };
        for py in y0..y1 {
            for px in x0..x1 {
                self.blend(px, py, color);
            }
        }
    }

    /// Stroke a rectangle outline `line_width` pixels thick, drawn *inward*
    /// from the given bounds — the same convention the compositor uses.
    fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32, line_width: f32, color: Color) {
        self.fill_rect(x, y, w, line_width, color);
        self.fill_rect(x, y + h - line_width, w, line_width, color);
        self.fill_rect(x, y, line_width, h, color);
        self.fill_rect(x + w - line_width, y, line_width, h, color);
    }

    /// Draw a 1-pixel Bresenham line.
    ///
    /// One pixel, not the annotation's nominal 2.0 width, because the
    /// compositor's `Line` handler discards the width field — so a thicker line
    /// in the file than on the screen would be a *new* discrepancy, not a fix.
    ///
    /// The segment is clipped to the canvas *before* it is walked, not by
    /// discarding out-of-bounds plots inside the loop. Discarding inside the
    /// loop is what the compositor does, and it is only safe there because its
    /// coordinates come from a framebuffer; here they come from annotations in
    /// preview space, so a line from far outside the image would spend
    /// millions of iterations plotting nothing before it arrived.
    fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, color: Color) {
        let Some(((sx, sy), (tx, ty))) = self.clip_segment(x1, y1, x2, y2) else {
            return;
        };
        let (Some(mut cx), Some(mut cy)) = (whole(sx), whole(sy)) else {
            return;
        };
        let (Some(ex), Some(ey)) = (whole(tx), whole(ty)) else {
            return;
        };
        // `unsigned_abs` rather than `abs`: the latter panics on `i32::MIN`,
        // which a saturating subtraction can produce.
        let dx = i32::try_from(ex.saturating_sub(cx).unsigned_abs()).unwrap_or(i32::MAX);
        let dy = 0_i32
            .saturating_sub(i32::try_from(ey.saturating_sub(cy).unsigned_abs()).unwrap_or(i32::MAX));
        let sx: i32 = if cx < ex { 1 } else { -1 };
        let sy: i32 = if cy < ey { 1 } else { -1 };
        let mut err = dx.saturating_add(dy);

        loop {
            self.blend(cx, cy, color);
            if cx == ex && cy == ey {
                break;
            }
            let e2 = err.saturating_mul(2);
            if e2 >= dy {
                err = err.saturating_add(dy);
                cx = cx.saturating_add(sx);
            }
            if e2 <= dx {
                err = err.saturating_add(dx);
                cy = cy.saturating_add(sy);
            }
        }
    }

    /// Liang-Barsky clip of a segment against the canvas, returning the visible
    /// portion or `None` when the segment misses the canvas entirely.
    ///
    /// The clipped endpoints round to whole pixels a little differently from
    /// the unclipped walk, so a line entering from off-image can land one pixel
    /// off what an infinite canvas would have drawn. That is the price of a
    /// bounded loop, and it is only ever visible on the one-pixel entry stub of
    /// a line the user drew mostly outside the image.
    fn clip_segment(
        &self,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
    ) -> Option<((f32, f32), (f32, f32))> {
        if !(x1.is_finite() && y1.is_finite() && x2.is_finite() && y2.is_finite()) {
            return None;
        }
        #[allow(clippy::cast_precision_loss)]
        let (max_x, max_y) = (self.width as f32 - 1.0, self.height as f32 - 1.0);
        if max_x < 0.0 || max_y < 0.0 {
            return None;
        }
        let (dx, dy) = (x2 - x1, y2 - y1);
        let (mut t0, mut t1) = (0.0_f32, 1.0_f32);

        for (p, q) in [
            (-dx, x1 - 0.0),
            (dx, max_x - x1),
            (-dy, y1 - 0.0),
            (dy, max_y - y1),
        ] {
            if p == 0.0 {
                // Parallel to this edge: outside it means the whole segment is.
                if q < 0.0 {
                    return None;
                }
                continue;
            }
            let r = q / p;
            if p < 0.0 {
                if r > t1 {
                    return None;
                }
                t0 = t0.max(r);
            } else {
                if r < t0 {
                    return None;
                }
                t1 = t1.min(r);
            }
        }

        Some((
            (dx.mul_add(t0, x1), dy.mul_add(t0, y1)),
            (dx.mul_add(t1, x1), dy.mul_add(t1, y1)),
        ))
    }
}

/// Pack a toolkit colour into the `0xAARRGGBB` word the capture buffer holds.
fn argb_of(color: Color) -> u32 {
    (u32::from(color.a) << 24)
        | (u32::from(color.r) << 16)
        | (u32::from(color.g) << 8)
        | u32::from(color.b)
}

/// Truncate a float coordinate to a whole pixel, rejecting the degenerate
/// values (`NaN`, infinities) that `as i32` would silently map to 0 or
/// saturate — either of which would stamp a stray mark on the image instead of
/// drawing nothing.
fn whole(v: f32) -> Option<i32> {
    if !v.is_finite() {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)]
    let out = (v >= -2_147_483_648.0 && v <= 2_147_483_647.0).then_some(v as i32);
    out
}

/// The font size text annotations are drawn at, on screen and in the file.
const ANNOTATION_TEXT_SIZE: f32 = 14.0;

/// Return a copy of `capture`'s pixels with `annotations` painted into them.
///
/// This is what makes a saved screenshot match the one on screen. Annotations
/// are stored relative to the preview's content area, and the image is shown
/// 1:1 filling that area from its origin, so an annotation at `(ax, ay)` is
/// image pixel `(ax, ay)` with no transform. Anything falling outside the image
/// is clipped rather than wrapping to the next row.
#[must_use]
pub fn flatten_annotations(capture: &Capture, annotations: &[Annotation]) -> Vec<u32> {
    let mut pixels = capture.pixels.clone();
    if annotations.is_empty() {
        return pixels;
    }
    let mut canvas = Canvas {
        pixels: &mut pixels,
        width: capture.width,
        height: capture.height,
    };

    for ann in annotations {
        match ann.tool {
            AnnotationTool::Rectangle => {
                canvas.stroke_rect(ann.min_x(), ann.min_y(), ann.width(), ann.height(), 2.0, ann.color);
            }
            AnnotationTool::Highlight => {
                canvas.fill_rect(ann.min_x(), ann.min_y(), ann.width(), ann.height(), ann.color);
            }
            AnnotationTool::Arrow => {
                canvas.line(ann.start_x, ann.start_y, ann.end_x, ann.end_y, ann.color);
                let dx = ann.end_x - ann.start_x;
                let dy = ann.end_y - ann.start_y;
                let len = dx.mul_add(dx, dy * dy).sqrt();
                if len > 1.0 {
                    let arrow_len = 12.0_f32.min(len * 0.3);
                    let (ux, uy) = (dx / len, dy / len);
                    let (px, py) = (-uy, ux);
                    let base_x = ann.end_x - ux * arrow_len;
                    let base_y = ann.end_y - uy * arrow_len;
                    let wing = arrow_len * 0.5;
                    canvas.line(ann.end_x, ann.end_y, base_x + px * wing, base_y + py * wing, ann.color);
                    canvas.line(ann.end_x, ann.end_y, base_x - px * wing, base_y - py * wing, ann.color);
                }
            }
            AnnotationTool::Text => {
                if ann.text.is_empty() {
                    continue;
                }
                // `RenderCommand::Text`'s `y` is the top of the line and the
                // compositor adds the ascent to reach the baseline; do the same
                // here, from the same font cache, so the glyphs land on the
                // same row in the file as they did on screen.
                let baseline =
                    ann.start_y + text::ascent(ANNOTATION_TEXT_SIZE, FontWeightHint::Regular);
                let mut surface = text::Surface {
                    pixels: canvas.pixels,
                    width: canvas.width,
                    height: canvas.height,
                };
                text::draw_into(
                    &mut surface,
                    &ann.text,
                    ann.start_x,
                    baseline,
                    ANNOTATION_TEXT_SIZE,
                    FontWeightHint::Regular,
                    ann.color,
                );
            }
        }
    }

    pixels
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

    // ---- Shared helpers ----

    /// A fresh, empty directory under the system temp dir.
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("slateos-screenshot-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    /// A directory that does not exist and whose parent does not either, so
    /// `fs::write` into it fails on every platform without touching
    /// permissions.
    fn unwritable_dir(tag: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("slateos-screenshot-missing-{tag}"))
            .join("nor-this")
    }

    fn ctrl(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
            text: None,
        }
    }

    fn click(x: f32, y: f32) -> Event {
        Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        })
    }

    /// An app sitting in the preview view with a plain blue capture loaded.
    fn app_in_preview() -> ScreenshotApp {
        let mut app = ScreenshotApp::new(800.0, 600.0);
        app.settings.default_action = PostCaptureAction::Annotate;
        app.current_capture = Some(Capture::solid(100, 80, 0xFF0000FF));
        app.view = AppView::Preview;
        app
    }

    fn annotation(tool: AnnotationTool, x1: f32, y1: f32, x2: f32, y2: f32, color: Color) -> Annotation {
        let mut ann = Annotation::new(tool, x1, y1, color);
        ann.end_x = x2;
        ann.end_y = y2;
        ann
    }

    fn differing(a: &[u32], b: &[u32]) -> usize {
        a.iter().zip(b).filter(|(l, r)| l != r).count()
    }

    // ---- Annotation flattening ----

    /// The headline bug: the user draws on a screenshot, saves it, and the
    /// annotations are not in the file. `save_current`'s own doc comment
    /// promised they would be.
    #[test]
    fn annotations_reach_the_saved_file() {
        let dir = temp_dir("baked");
        let mut app = app_in_preview();
        app.settings.save_directory = dir.clone();
        app.annotations.push(annotation(
            AnnotationTool::Rectangle,
            10.0,
            10.0,
            60.0,
            50.0,
            Color::rgb(255, 0, 0),
        ));

        let path = app.save_current().expect("save");
        let with_ann = std::fs::read(&path).expect("read back");

        app.annotations.clear();
        let path2 = app.save_current().expect("save again");
        let without = std::fs::read(&path2).expect("read back");

        assert_eq!(path, path2, "same capture, same filename");
        assert_ne!(
            with_ann, without,
            "the file was byte-identical with and without a rectangle drawn on it"
        );
    }

    #[test]
    fn flattening_does_not_modify_the_capture() {
        let capture = Capture::solid(40, 30, 0xFF0000FF);
        let original = capture.pixels.clone();
        let anns = [annotation(
            AnnotationTool::Highlight,
            0.0,
            0.0,
            40.0,
            30.0,
            HIGHLIGHT_COLOR,
        )];
        let flat = flatten_annotations(&capture, &anns);
        assert_eq!(capture.pixels, original, "the capture itself was painted on");
        assert_ne!(flat, original, "the flattened copy was not painted on");
    }

    #[test]
    fn no_annotations_means_the_pixels_are_returned_unchanged() {
        let capture = Capture::solid(16, 16, 0xFF123456);
        assert_eq!(flatten_annotations(&capture, &[]), capture.pixels);
    }

    #[test]
    fn a_rectangle_paints_its_border_and_not_its_interior() {
        let capture = Capture::solid(40, 40, 0xFF0000FF);
        let red = Color::rgb(255, 0, 0);
        let anns = [annotation(AnnotationTool::Rectangle, 10.0, 10.0, 30.0, 30.0, red)];
        let flat = flatten_annotations(&capture, &anns);
        let at = |x: usize, y: usize| flat[y * 40 + x];

        assert_eq!(at(10, 10), argb_of(red), "top-left corner of the border");
        assert_eq!(at(20, 10), argb_of(red), "middle of the top edge");
        assert_eq!(at(20, 29), argb_of(red), "middle of the bottom edge");
        assert_eq!(at(20, 20), 0xFF0000FF, "the interior must stay untouched");
        assert_eq!(at(5, 5), 0xFF0000FF, "outside the rectangle");
    }

    #[test]
    fn a_highlight_blends_instead_of_replacing() {
        let capture = Capture::solid(20, 20, 0xFF0000FF);
        let anns = [annotation(
            AnnotationTool::Highlight,
            0.0,
            0.0,
            20.0,
            20.0,
            HIGHLIGHT_COLOR,
        )];
        let flat = flatten_annotations(&capture, &anns);
        let px = flat[10 * 20 + 10];
        assert_ne!(px, 0xFF0000FF, "the highlight did not tint the pixel");
        assert_ne!(
            px,
            argb_of(HIGHLIGHT_COLOR),
            "a translucent highlight must blend, not overwrite"
        );
        assert!(px & 0xFF > 0, "the blue underneath must survive");
    }

    #[test]
    fn an_arrow_paints_a_line_and_a_head() {
        let capture = Capture::solid(60, 60, 0xFF000000);
        let anns = [annotation(
            AnnotationTool::Arrow,
            5.0,
            5.0,
            50.0,
            5.0,
            Color::rgb(255, 255, 255),
        )];
        let flat = flatten_annotations(&capture, &anns);
        let at = |x: usize, y: usize| flat[y * 60 + x];
        assert_eq!(at(25, 5), 0xFFFFFFFF, "the shaft");
        // The head fans off the shaft, so at least one pixel above it is set.
        let head_rows = (1..5).any(|dy| (38..52).any(|x| at(x, 5 - dy) == 0xFFFFFFFF));
        assert!(head_rows, "the arrowhead was not drawn");
    }

    #[test]
    fn a_text_annotation_reaches_the_pixels() {
        let capture = Capture::solid(200, 60, 0xFF000000);
        let mut ann = annotation(
            AnnotationTool::Text,
            10.0,
            10.0,
            10.0,
            26.0,
            Color::rgb(255, 255, 255),
        );
        ann.text = "Look here".to_string();
        let flat = flatten_annotations(&capture, &[ann]);
        assert!(
            differing(&capture.pixels, &flat) > 0,
            "the text label was dropped from the flattened image"
        );
    }

    #[test]
    fn empty_text_draws_nothing() {
        let capture = Capture::solid(50, 20, 0xFF000000);
        let ann = annotation(AnnotationTool::Text, 5.0, 5.0, 5.0, 21.0, Color::WHITE);
        assert_eq!(flatten_annotations(&capture, &[ann]), capture.pixels);
    }

    /// Annotations are in preview coordinates and the image may be smaller than
    /// the preview, so out-of-range ones are routine. They must clip, not wrap
    /// onto the next row and not panic.
    #[test]
    fn annotations_outside_the_image_are_clipped() {
        let capture = Capture::solid(20, 20, 0xFF0000FF);
        let anns = [
            annotation(AnnotationTool::Rectangle, 100.0, 100.0, 200.0, 200.0, Color::RED),
            annotation(AnnotationTool::Highlight, -50.0, -50.0, -10.0, -10.0, HIGHLIGHT_COLOR),
            annotation(AnnotationTool::Arrow, -30.0, 10.0, -5.0, 10.0, Color::RED),
        ];
        assert_eq!(flatten_annotations(&capture, &anns), capture.pixels);
    }

    /// An arrow whose tail is millions of pixels off-image must not walk the
    /// whole way there one pixel at a time. Bounded by the segment clip, this
    /// returns instantly; unbounded it would take minutes.
    #[test]
    fn an_arrow_from_far_off_image_is_clipped_not_walked() {
        let capture = Capture::solid(32, 32, 0xFF000000);
        let anns = [annotation(
            AnnotationTool::Arrow,
            -8_000_000.0,
            16.0,
            16.0,
            16.0,
            Color::WHITE,
        )];
        let flat = flatten_annotations(&capture, &anns);
        assert!(
            differing(&capture.pixels, &flat) > 0,
            "the visible part of the arrow was dropped"
        );
    }

    /// A segment that never touches the image draws nothing at all.
    #[test]
    fn a_line_that_misses_the_image_draws_nothing() {
        let capture = Capture::solid(32, 32, 0xFF000000);
        let anns = [annotation(
            AnnotationTool::Arrow,
            -100.0,
            -100.0,
            -50.0,
            -60.0,
            Color::WHITE,
        )];
        assert_eq!(flatten_annotations(&capture, &anns), capture.pixels);
    }

    #[test]
    fn a_degenerate_annotation_does_not_panic() {
        let capture = Capture::solid(10, 10, 0xFF000000);
        let anns = [
            annotation(AnnotationTool::Rectangle, f32::NAN, 0.0, 5.0, 5.0, Color::RED),
            annotation(AnnotationTool::Arrow, 0.0, 0.0, f32::INFINITY, 0.0, Color::RED),
            annotation(AnnotationTool::Highlight, 0.0, f32::NEG_INFINITY, 5.0, 5.0, HIGHLIGHT_COLOR),
        ];
        let _ = flatten_annotations(&capture, &anns);
    }

    // ---- Save reporting ----

    #[test]
    fn ctrl_s_reports_where_it_saved() {
        let dir = temp_dir("reports");
        let mut app = app_in_preview();
        app.settings.save_directory = dir;

        assert!(app.handle_event(&Event::Key(ctrl(Key::S))));

        let notif = app.notification.as_ref().expect("Ctrl+S said nothing at all");
        let path = notif.file_path.as_ref().expect("no path in the notification");
        assert!(path.exists(), "notification named {} which does not exist", path.display());
        assert!(notif.message.contains("saved"), "message was {:?}", notif.message);
    }

    /// The bug: `let _ = self.save_current()`. A save that failed looked
    /// exactly like one that worked — which is to say, like nothing at all.
    #[test]
    fn ctrl_s_reports_a_failure() {
        let mut app = app_in_preview();
        app.settings.save_directory = unwritable_dir("fail");

        app.handle_event(&Event::Key(ctrl(Key::S)));

        let notif = app.notification.as_ref().expect("a failed save said nothing");
        assert!(
            notif.message.contains("Failed"),
            "message was {:?}",
            notif.message
        );
        assert!(notif.file_path.is_none(), "a failed save must not claim a path");
    }

    #[test]
    fn ctrl_s_with_nothing_captured_says_so() {
        let mut app = ScreenshotApp::new(800.0, 600.0);
        app.view = AppView::Preview;
        app.handle_event(&Event::Key(ctrl(Key::S)));

        let notif = app.notification.as_ref().expect("no notification");
        assert!(
            notif.message.contains("no screenshot to save"),
            "a missing capture must not be reported as a pixel-count mismatch; got {:?}",
            notif.message
        );
    }

    /// `show_notification` suppresses the routine confirmation. It must not
    /// suppress an error, or a save that never happened is invisible.
    #[test]
    fn a_failure_is_reported_even_with_notifications_off() {
        let mut app = app_in_preview();
        app.settings.show_notification = false;
        app.settings.save_directory = unwritable_dir("quiet");

        app.save_current_notifying();
        assert!(app.notification.is_some(), "the error was suppressed");

        let dir = temp_dir("quiet-ok");
        app.settings.save_directory = dir;
        app.notification = None;
        assert!(app.save_current_notifying());
        assert!(
            app.notification.is_none(),
            "the routine confirmation must still be suppressed"
        );
    }

    // ---- Preview toolbar ----

    /// The Save / Copy / Discard buttons were painted every frame and hit-tested
    /// nowhere: clicking them did nothing.
    #[test]
    fn the_preview_action_buttons_are_clickable() {
        let app = app_in_preview();
        for (i, expected) in PREVIEW_ACTIONS.iter().enumerate() {
            let (x, y, w, h) = preview_action_rect(app.window_width, i);
            assert_eq!(
                app.preview_button_at(x + w / 2.0, y + h / 2.0),
                Some(*expected)
            );
        }
    }

    #[test]
    fn clicking_save_in_the_toolbar_saves() {
        let dir = temp_dir("toolbar-save");
        let mut app = app_in_preview();
        app.settings.save_directory = dir;

        let (x, y, w, h) = preview_action_rect(app.window_width, 0);
        assert!(app.handle_event(&click(x + w / 2.0, y + h / 2.0)));

        let notif = app.notification.as_ref().expect("the Save button did nothing");
        assert!(notif.file_path.as_ref().is_some_and(|p| p.exists()));
    }

    #[test]
    fn clicking_discard_returns_to_the_menu() {
        let mut app = app_in_preview();
        app.annotations.push(annotation(
            AnnotationTool::Rectangle,
            1.0,
            1.0,
            5.0,
            5.0,
            Color::RED,
        ));

        let (x, y, w, h) = preview_action_rect(app.window_width, 2);
        app.handle_event(&click(x + w / 2.0, y + h / 2.0));

        assert_eq!(app.view, AppView::Menu);
        assert!(app.current_capture.is_none());
        assert!(app.annotations.is_empty());
    }

    #[test]
    fn clicking_a_tool_button_selects_that_tool() {
        let mut app = app_in_preview();
        for (i, tool) in PREVIEW_TOOLS.iter().enumerate() {
            let (x, y, w, h) = preview_tool_rect(i);
            app.handle_event(&click(x + w / 2.0, y + h / 2.0));
            assert_eq!(app.annotation_tool, *tool);
        }
    }

    #[test]
    fn clicking_undo_removes_the_last_annotation() {
        let mut app = app_in_preview();
        app.annotations.push(annotation(AnnotationTool::Rectangle, 1.0, 1.0, 5.0, 5.0, Color::RED));
        app.annotations.push(annotation(AnnotationTool::Arrow, 2.0, 2.0, 9.0, 9.0, Color::RED));

        let (x, y, w, h) = preview_undo_rect();
        app.handle_event(&click(x + w / 2.0, y + h / 2.0));

        assert_eq!(app.annotations.len(), 1);
    }

    /// A click in a toolbar is a command. It must not also start drawing an
    /// annotation on the image underneath.
    #[test]
    fn a_toolbar_click_does_not_begin_an_annotation() {
        let mut app = app_in_preview();
        let (x, y, w, h) = preview_tool_rect(0);
        app.handle_event(&click(x + w / 2.0, y + h / 2.0));
        assert!(app.pending_annotation.is_none());
        assert!(app.annotations.is_empty());
    }

    #[test]
    fn copy_admits_it_cannot_copy_yet() {
        let mut app = app_in_preview();
        app.activate_preview_button(PreviewButton::Copy);
        let notif = app.notification.as_ref().expect("Copy was silent");
        assert!(
            notif.message.contains("clipboard"),
            "message was {:?}",
            notif.message
        );
    }

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
