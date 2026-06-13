//! SlateOS Compositor — Display Server
//!
//! The central display server for SlateOS. Manages windows, composites their
//! contents onto a framebuffer, and routes input events to the appropriate
//! client windows.
//!
//! # Architecture
//!
//! ```text
//! Client Applications
//!     │ (submit RenderTree via IPC)
//!     ▼
//! Compositor Server
//!     ├── Window Manager (z-order, focus, decorations)
//!     ├── Compositing Pipeline (damage tracking, alpha blending, double buffer)
//!     ├── Input Router (hit testing, event dispatch)
//!     └── Rendering Engine (rasterize RenderCommands to pixels)
//!     │
//!     ▼
//! Framebuffer (presented to display hardware)
//! ```
//!
//! # Design Decisions
//!
//! - Double-buffered compositing with damage tracking to minimize GPU writes.
//! - Window decorations drawn server-side (consistent look, secure close button).
//! - Input routed by Z-order hit testing; keyboard follows focus, mouse follows cursor.
//! - VSync-aware frame scheduling: target refresh rate, skip frames if behind.
//! - Stub IPC layer ready for real SlateOS channel-based IPC when available.

// Drawing primitives (fill_rect, stroke_rect, draw_text, draw_line) and the
// renderer execute() pump take 8-9 args (framebuffer + geometry + color +
// optional clip / font / weight / stroke-width). Grouping into a struct
// would help marginally but obscures the per-call clarity at the call site
// — every primitive needs every arg.
#![allow(clippy::too_many_arguments)]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Height of the window title bar in pixels.
const TITLE_BAR_HEIGHT: u32 = 30;

/// Width of the window border in pixels.
const BORDER_WIDTH: u32 = 1;

/// Size of the window shadow in pixels.
const SHADOW_SIZE: u32 = 8;

/// Width/height of title bar buttons (close, maximize, minimize).
const TITLE_BUTTON_SIZE: u32 = 20;

/// Spacing between title bar buttons.
const TITLE_BUTTON_SPACING: u32 = 4;

/// Default window opacity (fully opaque).
const DEFAULT_OPACITY: f32 = 1.0;

/// Monospace font character width in pixels (for basic text rendering).
const CHAR_WIDTH: u32 = 8;

/// Monospace font character height in pixels.
const CHAR_HEIGHT: u32 = 14;

/// Maximum framebuffer width supported.
const MAX_FB_WIDTH: u32 = 7680;

/// Maximum framebuffer height supported.
const MAX_FB_HEIGHT: u32 = 4320;

// ---------------------------------------------------------------------------
// Window ID generation
// ---------------------------------------------------------------------------

/// Global atomic counter for generating unique window IDs.
static NEXT_WINDOW_ID: AtomicU64 = AtomicU64::new(1);

/// Unique identifier for a window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WindowId(u64);

impl WindowId {
    /// Allocate a new unique window ID.
    fn allocate() -> Self {
        Self(NEXT_WINDOW_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// Get the raw numeric value.
    pub fn raw(self) -> u64 {
        self.0
    }
}

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Compositor error type.
#[derive(Clone, Debug)]
pub enum CompositorError {
    /// The specified window does not exist.
    WindowNotFound(WindowId),
    /// Invalid framebuffer dimensions.
    InvalidDimensions { width: u32, height: u32 },
    /// The framebuffer exceeds maximum supported size.
    FramebufferTooLarge { width: u32, height: u32 },
    /// A rendering operation failed.
    RenderError(String),
    /// IPC communication error.
    IpcError(String),
    /// Display configuration error.
    DisplayError(String),
}

impl std::fmt::Display for CompositorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WindowNotFound(id) => write!(f, "window not found: {}", id.raw()),
            Self::InvalidDimensions { width, height } => {
                write!(f, "invalid dimensions: {}x{}", width, height)
            }
            Self::FramebufferTooLarge { width, height } => {
                write!(
                    f,
                    "framebuffer too large: {}x{} (max {}x{})",
                    width, height, MAX_FB_WIDTH, MAX_FB_HEIGHT
                )
            }
            Self::RenderError(msg) => write!(f, "render error: {}", msg),
            Self::IpcError(msg) => write!(f, "ipc error: {}", msg),
            Self::DisplayError(msg) => write!(f, "display error: {}", msg),
        }
    }
}

pub type CompositorResult<T> = Result<T, CompositorError>;

// ---------------------------------------------------------------------------
// Geometry types
// ---------------------------------------------------------------------------

/// A 2D point (screen coordinates).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// A 2D rectangle (screen coordinates).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Check if a point is inside this rectangle.
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x
            && py >= self.y
            && px < self.x.saturating_add(self.width as i32)
            && py < self.y.saturating_add(self.height as i32)
    }

    /// Compute the intersection of two rectangles. Returns None if they don't overlap.
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let x1 = self.x.max(other.x);
        let y1 = self.y.max(other.y);
        let x2 = self
            .x
            .saturating_add(self.width as i32)
            .min(other.x.saturating_add(other.width as i32));
        let y2 = self
            .y
            .saturating_add(self.height as i32)
            .min(other.y.saturating_add(other.height as i32));

        if x2 > x1 && y2 > y1 {
            Some(Rect::new(x1, y1, (x2 - x1) as u32, (y2 - y1) as u32))
        } else {
            None
        }
    }

    /// Compute the bounding box that contains both rectangles.
    pub fn union(&self, other: &Rect) -> Rect {
        let x1 = self.x.min(other.x);
        let y1 = self.y.min(other.y);
        let x2 = self
            .x
            .saturating_add(self.width as i32)
            .max(other.x.saturating_add(other.width as i32));
        let y2 = self
            .y
            .saturating_add(self.height as i32)
            .max(other.y.saturating_add(other.height as i32));

        Rect::new(x1, y1, (x2 - x1) as u32, (y2 - y1) as u32)
    }
}

// ---------------------------------------------------------------------------
// Input events (compositor-level)
// ---------------------------------------------------------------------------

/// Mouse button identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

/// Input event received from the input subsystem.
#[derive(Clone, Debug)]
pub enum InputEvent {
    /// Mouse moved to absolute position.
    MouseMove { x: i32, y: i32 },
    /// Mouse button pressed or released.
    MouseButton {
        button: MouseButton,
        pressed: bool,
        x: i32,
        y: i32,
    },
    /// Mouse scroll wheel.
    MouseScroll { dx: f32, dy: f32, x: i32, y: i32 },
    /// Key pressed.
    KeyDown { scancode: u32, character: Option<char> },
    /// Key released.
    KeyUp { scancode: u32 },
    /// Text input (after IME processing).
    TextInput { text: String },
}

/// Cursor shape the compositor should display.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorShape {
    #[default]
    Arrow,
    ResizeNS,
    ResizeEW,
    ResizeNESW,
    ResizeNWSE,
    Text,
    Hand,
    Move,
    Wait,
    Crosshair,
    NotAllowed,
    Hidden,
}

// ---------------------------------------------------------------------------
// Window
// ---------------------------------------------------------------------------

/// A managed window in the compositor.
#[derive(Clone, Debug)]
pub struct Window {
    /// Unique identifier for this window.
    pub id: WindowId,
    /// Window title (displayed in title bar).
    pub title: String,
    /// Position of the window's top-left corner (including decorations).
    pub x: i32,
    /// Position of the window's top-left corner (including decorations).
    pub y: i32,
    /// Width of the client area in pixels.
    pub width: u32,
    /// Height of the client area in pixels.
    pub height: u32,
    /// Whether the window is visible.
    pub visible: bool,
    /// Whether the window is minimized to the taskbar.
    pub minimized: bool,
    /// Whether the window is maximized (fills the screen).
    pub maximized: bool,
    /// Whether this window currently has keyboard focus.
    pub focused: bool,
    /// Z-order index (higher = more in front).
    pub z_order: u32,
    /// Window opacity (0.0 = fully transparent, 1.0 = fully opaque).
    pub opacity: f32,
    /// Process ID of the client that owns this window.
    pub client_pid: u64,
    /// The most recently submitted render tree from the client.
    pub render_tree: RenderTree,
    /// Position and size before maximizing (for restore).
    pub restore_rect: Option<Rect>,
    /// Whether the window needs to be redrawn.
    pub dirty: bool,
}

impl Window {
    /// Create a new window with the given parameters.
    fn new(title: String, x: i32, y: i32, width: u32, height: u32, client_pid: u64) -> Self {
        Self {
            id: WindowId::allocate(),
            title,
            x,
            y,
            width,
            height,
            visible: true,
            minimized: false,
            maximized: false,
            focused: false,
            z_order: 0,
            opacity: DEFAULT_OPACITY,
            client_pid,
            render_tree: RenderTree::new(),
            restore_rect: None,
            dirty: true,
        }
    }

    /// Get the total bounds including decorations (title bar, borders, shadow).
    pub fn outer_rect(&self) -> Rect {
        let total_width =
            self.width + (BORDER_WIDTH * 2) + (SHADOW_SIZE * 2);
        let total_height =
            self.height + TITLE_BAR_HEIGHT + BORDER_WIDTH + (SHADOW_SIZE * 2);
        Rect::new(
            self.x - SHADOW_SIZE as i32 - BORDER_WIDTH as i32,
            self.y - SHADOW_SIZE as i32 - TITLE_BAR_HEIGHT as i32,
            total_width,
            total_height,
        )
    }

    /// Get the client area rectangle (where the application draws).
    pub fn client_rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.width, self.height)
    }

    /// Get the title bar rectangle (for drag and button hit testing).
    pub fn title_bar_rect(&self) -> Rect {
        Rect::new(
            self.x - BORDER_WIDTH as i32,
            self.y - TITLE_BAR_HEIGHT as i32,
            self.width + (BORDER_WIDTH * 2),
            TITLE_BAR_HEIGHT,
        )
    }

    /// Get the close button rectangle.
    pub fn close_button_rect(&self) -> Rect {
        let title_rect = self.title_bar_rect();
        let btn_x = title_rect.x + title_rect.width as i32
            - TITLE_BUTTON_SIZE as i32
            - TITLE_BUTTON_SPACING as i32;
        let btn_y = title_rect.y + (TITLE_BAR_HEIGHT as i32 - TITLE_BUTTON_SIZE as i32) / 2;
        Rect::new(btn_x, btn_y, TITLE_BUTTON_SIZE, TITLE_BUTTON_SIZE)
    }

    /// Get the maximize button rectangle.
    pub fn maximize_button_rect(&self) -> Rect {
        let close_rect = self.close_button_rect();
        Rect::new(
            close_rect.x - TITLE_BUTTON_SIZE as i32 - TITLE_BUTTON_SPACING as i32,
            close_rect.y,
            TITLE_BUTTON_SIZE,
            TITLE_BUTTON_SIZE,
        )
    }

    /// Get the minimize button rectangle.
    pub fn minimize_button_rect(&self) -> Rect {
        let max_rect = self.maximize_button_rect();
        Rect::new(
            max_rect.x - TITLE_BUTTON_SIZE as i32 - TITLE_BUTTON_SPACING as i32,
            max_rect.y,
            TITLE_BUTTON_SIZE,
            TITLE_BUTTON_SIZE,
        )
    }
}

// ---------------------------------------------------------------------------
// Damage tracking
// ---------------------------------------------------------------------------

/// A region of the screen that needs to be redrawn.
#[derive(Clone, Debug, Default)]
pub struct DamageRegion {
    /// List of dirty rectangles.
    rects: Vec<Rect>,
}

impl DamageRegion {
    /// Create an empty damage region.
    pub fn new() -> Self {
        Self { rects: Vec::new() }
    }

    /// Mark a rectangle as damaged (needing redraw).
    pub fn add(&mut self, rect: Rect) {
        // Merge with existing rects if they overlap to avoid excessive redraw regions.
        for existing in &mut self.rects {
            if existing.intersect(&rect).is_some() {
                *existing = existing.union(&rect);
                return;
            }
        }
        self.rects.push(rect);
    }

    /// Mark the entire screen as damaged.
    pub fn mark_full(&mut self, width: u32, height: u32) {
        self.rects.clear();
        self.rects.push(Rect::new(0, 0, width, height));
    }

    /// Check if there is any damage to process.
    pub fn has_damage(&self) -> bool {
        !self.rects.is_empty()
    }

    /// Get all damaged rectangles.
    pub fn rects(&self) -> &[Rect] {
        &self.rects
    }

    /// Clear all damage (after compositing).
    pub fn clear(&mut self) {
        self.rects.clear();
    }
}

// ---------------------------------------------------------------------------
// Framebuffer
// ---------------------------------------------------------------------------

/// Double-buffered framebuffer for compositing.
pub struct Framebuffer {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Back buffer (being composited into).
    back: Vec<u32>,
    /// Front buffer (currently being displayed).
    front: Vec<u32>,
}

impl Framebuffer {
    /// Create a new framebuffer with the given dimensions.
    pub fn new(width: u32, height: u32) -> CompositorResult<Self> {
        if width == 0 || height == 0 {
            return Err(CompositorError::InvalidDimensions { width, height });
        }
        if width > MAX_FB_WIDTH || height > MAX_FB_HEIGHT {
            return Err(CompositorError::FramebufferTooLarge { width, height });
        }

        let size = width as usize * height as usize;
        Ok(Self {
            width,
            height,
            back: vec![0xFF_00_00_00; size], // Opaque black
            front: vec![0xFF_00_00_00; size],
        })
    }

    /// Swap front and back buffers.
    pub fn swap(&mut self) {
        std::mem::swap(&mut self.front, &mut self.back);
    }

    /// Clear the back buffer to a solid color.
    pub fn clear(&mut self, color: u32) {
        self.back.fill(color);
    }

    /// Clear a specific rectangle in the back buffer.
    pub fn clear_rect(&mut self, rect: &Rect, color: u32) {
        let x_start = rect.x.max(0) as u32;
        let y_start = rect.y.max(0) as u32;
        let x_end = (rect.x.saturating_add(rect.width as i32) as u32).min(self.width);
        let y_end = (rect.y.saturating_add(rect.height as i32) as u32).min(self.height);

        for row in y_start..y_end {
            let row_offset = row as usize * self.width as usize;
            for col in x_start..x_end {
                if let Some(pixel) = self.back.get_mut(row_offset + col as usize) {
                    *pixel = color;
                }
            }
        }
    }

    /// Set a pixel in the back buffer (bounds-checked).
    #[inline]
    pub fn set_pixel(&mut self, x: u32, y: u32, color: u32) {
        if x < self.width && y < self.height {
            let idx = y as usize * self.width as usize + x as usize;
            if let Some(pixel) = self.back.get_mut(idx) {
                *pixel = color;
            }
        }
    }

    /// Get a pixel from the back buffer (bounds-checked).
    #[inline]
    pub fn get_pixel(&self, x: u32, y: u32) -> Option<u32> {
        if x < self.width && y < self.height {
            let idx = y as usize * self.width as usize + x as usize;
            self.back.get(idx).copied()
        } else {
            None
        }
    }

    /// Blend a pixel with alpha onto the back buffer at the given position.
    #[inline]
    pub fn blend_pixel(&mut self, x: u32, y: u32, src_color: u32, window_opacity: f32) {
        if x >= self.width || y >= self.height {
            return;
        }

        let idx = y as usize * self.width as usize + x as usize;
        let dst = match self.back.get(idx) {
            Some(&val) => val,
            None => return,
        };

        let src_a_raw = ((src_color >> 24) & 0xFF) as f32;
        let src_a = ((src_a_raw * window_opacity) as u32).min(255);

        if src_a == 255 {
            // Fully opaque — just write.
            if let Some(pixel) = self.back.get_mut(idx) {
                *pixel = src_color | 0xFF_00_00_00;
            }
            return;
        }
        if src_a == 0 {
            return;
        }

        let inv_a = 255 - src_a;

        let src_r = (src_color >> 16) & 0xFF;
        let src_g = (src_color >> 8) & 0xFF;
        let src_b = src_color & 0xFF;

        let dst_r = (dst >> 16) & 0xFF;
        let dst_g = (dst >> 8) & 0xFF;
        let dst_b = dst & 0xFF;

        let out_r = (src_r * src_a + dst_r * inv_a) / 255;
        let out_g = (src_g * src_a + dst_g * inv_a) / 255;
        let out_b = (src_b * src_a + dst_b * inv_a) / 255;

        if let Some(pixel) = self.back.get_mut(idx) {
            *pixel = 0xFF_00_00_00 | (out_r << 16) | (out_g << 8) | out_b;
        }
    }

    /// Get a reference to the front buffer for display.
    pub fn front_buffer(&self) -> &[u32] {
        &self.front
    }

    /// Resize the framebuffer. Clears all contents.
    pub fn resize(&mut self, width: u32, height: u32) -> CompositorResult<()> {
        if width == 0 || height == 0 {
            return Err(CompositorError::InvalidDimensions { width, height });
        }
        if width > MAX_FB_WIDTH || height > MAX_FB_HEIGHT {
            return Err(CompositorError::FramebufferTooLarge { width, height });
        }

        let size = width as usize * height as usize;
        self.width = width;
        self.height = height;
        self.back = vec![0xFF_00_00_00; size];
        self.front = vec![0xFF_00_00_00; size];
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Display management
// ---------------------------------------------------------------------------

/// A physical or virtual display.
#[derive(Clone, Debug)]
pub struct Display {
    /// Display identifier.
    pub id: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Refresh rate in Hz.
    pub refresh_rate: u32,
    /// DPI scale factor (1.0 = 96dpi, 2.0 = 192dpi).
    pub scale_factor: f32,
    /// Offset in the virtual display space (for multi-monitor).
    pub offset_x: i32,
    /// Offset in the virtual display space (for multi-monitor).
    pub offset_y: i32,
    /// Whether this is the primary display.
    pub primary: bool,
}

impl Display {
    /// Create a new display with the given parameters.
    pub fn new(
        id: u32,
        width: u32,
        height: u32,
        refresh_rate: u32,
        scale_factor: f32,
        primary: bool,
    ) -> Self {
        Self {
            id,
            width,
            height,
            refresh_rate,
            scale_factor,
            offset_x: 0,
            offset_y: 0,
            primary,
        }
    }

    /// Get the frame interval for this display's refresh rate.
    pub fn frame_interval(&self) -> Duration {
        if self.refresh_rate == 0 {
            Duration::from_millis(16) // Default to ~60fps
        } else {
            Duration::from_micros(1_000_000 / self.refresh_rate as u64)
        }
    }

    /// Get the display's bounding rectangle in virtual space.
    pub fn bounds(&self) -> Rect {
        Rect::new(self.offset_x, self.offset_y, self.width, self.height)
    }
}

/// Multi-monitor display layout.
pub struct DisplayManager {
    /// All connected displays.
    displays: Vec<Display>,
}

impl DisplayManager {
    /// Create a display manager with a single primary display.
    pub fn new(width: u32, height: u32, refresh_rate: u32) -> Self {
        Self {
            displays: vec![Display::new(0, width, height, refresh_rate, 1.0, true)],
        }
    }

    /// Get the primary display.
    pub fn primary(&self) -> Option<&Display> {
        self.displays.iter().find(|d| d.primary)
    }

    /// Add an additional display to the layout.
    pub fn add_display(&mut self, mut display: Display) {
        // Place it to the right of the existing displays by default.
        if self.displays.is_empty() {
            display.primary = true;
        } else {
            let rightmost = self
                .displays
                .iter()
                .map(|d| d.offset_x + d.width as i32)
                .max()
                .unwrap_or(0);
            display.offset_x = rightmost;
        }
        self.displays.push(display);
    }

    /// Get the total virtual desktop bounds (union of all displays).
    pub fn virtual_bounds(&self) -> Rect {
        if self.displays.is_empty() {
            return Rect::new(0, 0, 0, 0);
        }
        let mut bounds = self.displays[0].bounds();
        for display in self.displays.iter().skip(1) {
            bounds = bounds.union(&display.bounds());
        }
        bounds
    }

    /// Get all displays.
    pub fn displays(&self) -> &[Display] {
        &self.displays
    }

    /// Get the refresh rate of the primary display.
    pub fn primary_refresh_rate(&self) -> u32 {
        self.primary().map_or(60, |d| d.refresh_rate)
    }
}

// ---------------------------------------------------------------------------
// Frame timing / stats
// ---------------------------------------------------------------------------

/// Frame timing and performance statistics.
#[derive(Clone, Debug)]
pub struct FrameStats {
    /// Time taken to composite the last frame (microseconds).
    pub last_frame_time_us: u64,
    /// Total frames composited since startup.
    pub frames_composited: u64,
    /// Frames dropped (compose took longer than frame interval).
    pub dropped_frames: u64,
    /// Target frame interval based on display refresh rate.
    pub target_interval: Duration,
    /// Timestamp of the last frame start.
    last_frame_start: Option<Instant>,
}

impl FrameStats {
    /// Create new frame stats with the given target interval.
    pub fn new(target_interval: Duration) -> Self {
        Self {
            last_frame_time_us: 0,
            frames_composited: 0,
            dropped_frames: 0,
            target_interval,
            last_frame_start: None,
        }
    }

    /// Mark the start of a new frame.
    pub fn begin_frame(&mut self) {
        self.last_frame_start = Some(Instant::now());
    }

    /// Mark the end of a frame. Returns true if the frame was within budget.
    pub fn end_frame(&mut self) -> bool {
        let elapsed = self
            .last_frame_start
            .map(|start| start.elapsed())
            .unwrap_or(Duration::ZERO);

        self.last_frame_time_us = elapsed.as_micros() as u64;
        self.frames_composited = self.frames_composited.saturating_add(1);

        if elapsed > self.target_interval {
            self.dropped_frames = self.dropped_frames.saturating_add(1);
            false
        } else {
            true
        }
    }

    /// Check if enough time has passed since the last frame to start a new one.
    pub fn should_compose(&self) -> bool {
        match self.last_frame_start {
            Some(start) => start.elapsed() >= self.target_interval,
            None => true,
        }
    }
}

// ---------------------------------------------------------------------------
// Compositor protocol (stub IPC)
// ---------------------------------------------------------------------------

/// Requests from clients to the compositor.
#[derive(Clone, Debug)]
pub enum CompositorRequest {
    /// Create a new window.
    CreateWindow {
        title: String,
        width: u32,
        height: u32,
        client_pid: u64,
    },
    /// Destroy an existing window.
    DestroyWindow { window_id: WindowId },
    /// Set the window title.
    SetTitle { window_id: WindowId, title: String },
    /// Submit render commands for a window's client area.
    Submit {
        window_id: WindowId,
        commands: Vec<RenderCommand>,
    },
    /// Move a window to a new position.
    Move { window_id: WindowId, x: i32, y: i32 },
    /// Resize a window's client area.
    Resize {
        window_id: WindowId,
        width: u32,
        height: u32,
    },
    /// Minimize a window.
    Minimize { window_id: WindowId },
    /// Maximize a window.
    Maximize { window_id: WindowId },
    /// Restore a window from minimized/maximized state.
    Restore { window_id: WindowId },
    /// Set the cursor shape for a window.
    SetCursor {
        window_id: WindowId,
        cursor: CursorShape,
    },
    /// Set window opacity.
    SetOpacity { window_id: WindowId, opacity: f32 },
    /// Query display information.
    GetDisplayInfo,
}

/// Responses from the compositor to clients.
#[derive(Clone, Debug)]
pub enum CompositorResponse {
    /// A window was created successfully.
    WindowCreated { window_id: WindowId },
    /// Operation completed successfully.
    Ok,
    /// Operation failed.
    Error { message: String },
    /// Display information response.
    DisplayInfo {
        width: u32,
        height: u32,
        refresh_rate: u32,
        scale_factor: f32,
    },
}

/// Notifications sent from the compositor to clients (events).
#[derive(Clone, Debug)]
pub enum EventNotification {
    /// Keyboard event for the focused window.
    KeyEvent {
        window_id: WindowId,
        scancode: u32,
        pressed: bool,
        character: Option<char>,
    },
    /// Mouse event for a window.
    MouseEvent {
        window_id: WindowId,
        x: i32,
        y: i32,
        kind: MouseEventKind,
    },
    /// Window close was requested (close button clicked).
    WindowClose { window_id: WindowId },
    /// Window was resized by the user.
    WindowResized {
        window_id: WindowId,
        width: u32,
        height: u32,
    },
    /// Window gained keyboard focus.
    FocusGained { window_id: WindowId },
    /// Window lost keyboard focus.
    FocusLost { window_id: WindowId },
}

/// Mouse event kind for notifications.
#[derive(Clone, Copy, Debug)]
pub enum MouseEventKind {
    Move,
    ButtonPress(MouseButton),
    ButtonRelease(MouseButton),
    Scroll { dx: f32, dy: f32 },
}

// ---------------------------------------------------------------------------
// Drag state
// ---------------------------------------------------------------------------

/// What kind of drag operation is in progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DragMode {
    /// Dragging the window by its title bar.
    MoveWindow,
    /// Resizing from the left edge.
    ResizeLeft,
    /// Resizing from the right edge.
    ResizeRight,
    /// Resizing from the top edge.
    ResizeTop,
    /// Resizing from the bottom edge.
    ResizeBottom,
    /// Resizing from the top-left corner.
    ResizeTopLeft,
    /// Resizing from the top-right corner.
    ResizeTopRight,
    /// Resizing from the bottom-left corner.
    ResizeBottomLeft,
    /// Resizing from the bottom-right corner.
    ResizeBottomRight,
}

/// Active drag state.
#[derive(Clone, Debug)]
struct DragState {
    /// The window being dragged.
    window_id: WindowId,
    /// The kind of drag.
    mode: DragMode,
    /// Mouse position at drag start.
    start_mouse: Point,
    /// Window position at drag start.
    start_window_pos: Point,
    /// Window size at drag start.
    start_window_size: (u32, u32),
}

// ---------------------------------------------------------------------------
// Clip and translation stacks (for rendering)
// ---------------------------------------------------------------------------

/// Clipping rectangle stack for the rendering engine.
#[derive(Clone, Debug, Default)]
struct ClipStack {
    stack: Vec<Rect>,
}

impl ClipStack {
    fn push(&mut self, rect: Rect) {
        // Intersect with current clip (if any) to narrow the visible area.
        let effective = if let Some(current) = self.stack.last() {
            current.intersect(&rect).unwrap_or(Rect::new(0, 0, 0, 0))
        } else {
            rect
        };
        self.stack.push(effective);
    }

    fn pop(&mut self) {
        self.stack.pop();
    }

    fn current(&self) -> Option<&Rect> {
        self.stack.last()
    }

    fn clear(&mut self) {
        self.stack.clear();
    }
}

/// Translation offset stack for the rendering engine.
#[derive(Clone, Debug, Default)]
struct TranslateStack {
    stack: Vec<(f32, f32)>,
    /// Cumulative offset (sum of all pushed translations).
    total_dx: f32,
    total_dy: f32,
}

impl TranslateStack {
    fn push(&mut self, dx: f32, dy: f32) {
        self.stack.push((dx, dy));
        self.total_dx += dx;
        self.total_dy += dy;
    }

    fn pop(&mut self) {
        if let Some((dx, dy)) = self.stack.pop() {
            self.total_dx -= dx;
            self.total_dy -= dy;
        }
    }

    fn offset(&self) -> (f32, f32) {
        (self.total_dx, self.total_dy)
    }

    fn clear(&mut self) {
        self.stack.clear();
        self.total_dx = 0.0;
        self.total_dy = 0.0;
    }
}

// ---------------------------------------------------------------------------
// Basic bitmap font (8x14 monospace, ASCII subset)
// ---------------------------------------------------------------------------

/// Simple 8x14 monospace bitmap font for basic text rendering.
/// Stores 1-bit-per-pixel glyph data for printable ASCII (32..127).
struct BitmapFont;

impl BitmapFont {
    /// Render a character at (px, py) in the framebuffer.
    /// Returns the advance width in pixels.
    fn draw_char(
        fb: &mut Framebuffer,
        ch: char,
        px: u32,
        py: u32,
        color: u32,
        opacity: f32,
        clip: Option<&Rect>,
    ) -> u32 {
        let code = ch as u32;
        // Only render printable ASCII for now.
        if !(32..=126).contains(&code) {
            return CHAR_WIDTH;
        }

        // Simple procedural glyph rendering: for each character, we generate
        // a basic pattern. A real font would use stored bitmap data.
        let glyph = Self::glyph_data(ch);

        for row in 0..CHAR_HEIGHT {
            let bits = glyph.get(row as usize).copied().unwrap_or(0u8);
            for col in 0..CHAR_WIDTH {
                if bits & (0x80 >> col) != 0 {
                    let fx = px + col;
                    let fy = py + row;

                    // Clip check
                    if let Some(clip_rect) = clip
                        && !clip_rect.contains(fx as i32, fy as i32) {
                            continue;
                        }

                    fb.blend_pixel(fx, fy, color, opacity);
                }
            }
        }

        CHAR_WIDTH
    }

    /// Get the glyph bitmap data for a character (8 bits wide, 14 rows).
    /// This is a simplified procedural font — enough for basic compositor text.
    fn glyph_data(ch: char) -> [u8; 14] {
        // Provide basic glyphs for common characters used in window titles
        // and decorations. A production compositor would load a proper font file.
        match ch {
            ' ' => [0x00; 14],
            'A' => [
                0x00, 0x18, 0x3C, 0x66, 0x66, 0xC3, 0xC3, 0xFF, 0xC3, 0xC3, 0xC3, 0xC3, 0x00,
                0x00,
            ],
            'B' => [
                0x00, 0xFC, 0x66, 0x66, 0x66, 0x7C, 0x66, 0x66, 0x66, 0x66, 0x66, 0xFC, 0x00,
                0x00,
            ],
            'C' => [
                0x00, 0x3C, 0x66, 0xC2, 0xC0, 0xC0, 0xC0, 0xC0, 0xC0, 0xC2, 0x66, 0x3C, 0x00,
                0x00,
            ],
            'D' => [
                0x00, 0xF8, 0x6C, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x6C, 0xF8, 0x00,
                0x00,
            ],
            'E' => [
                0x00, 0xFE, 0x66, 0x62, 0x68, 0x78, 0x68, 0x60, 0x60, 0x62, 0x66, 0xFE, 0x00,
                0x00,
            ],
            'F' => [
                0x00, 0xFE, 0x66, 0x62, 0x68, 0x78, 0x68, 0x60, 0x60, 0x60, 0x60, 0xF0, 0x00,
                0x00,
            ],
            'G' => [
                0x00, 0x3C, 0x66, 0xC2, 0xC0, 0xC0, 0xDE, 0xC6, 0xC6, 0xC6, 0x66, 0x3A, 0x00,
                0x00,
            ],
            'H' => [
                0x00, 0xC6, 0xC6, 0xC6, 0xC6, 0xFE, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x00,
                0x00,
            ],
            'I' => [
                0x00, 0x3C, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00,
                0x00,
            ],
            'J' => [
                0x00, 0x1E, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0xCC, 0xCC, 0xCC, 0x78, 0x00,
                0x00,
            ],
            'K' => [
                0x00, 0xE6, 0x66, 0x6C, 0x6C, 0x78, 0x78, 0x6C, 0x6C, 0x66, 0x66, 0xE6, 0x00,
                0x00,
            ],
            'L' => [
                0x00, 0xF0, 0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x62, 0x66, 0xFE, 0x00,
                0x00,
            ],
            'M' => [
                0x00, 0xC6, 0xEE, 0xFE, 0xD6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x00,
                0x00,
            ],
            'N' => [
                0x00, 0xC6, 0xE6, 0xF6, 0xFE, 0xDE, 0xCE, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x00,
                0x00,
            ],
            'O' => [
                0x00, 0x38, 0x6C, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x6C, 0x38, 0x00,
                0x00,
            ],
            'P' => [
                0x00, 0xFC, 0x66, 0x66, 0x66, 0x7C, 0x60, 0x60, 0x60, 0x60, 0x60, 0xF0, 0x00,
                0x00,
            ],
            'Q' => [
                0x00, 0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xD6, 0xDE, 0x7C, 0x0C, 0x00,
                0x00,
            ],
            'R' => [
                0x00, 0xFC, 0x66, 0x66, 0x66, 0x7C, 0x6C, 0x66, 0x66, 0x66, 0x66, 0xE6, 0x00,
                0x00,
            ],
            'S' => [
                0x00, 0x7C, 0xC6, 0xC6, 0x60, 0x38, 0x0C, 0x06, 0xC6, 0xC6, 0xC6, 0x7C, 0x00,
                0x00,
            ],
            'T' => [
                0x00, 0x7E, 0x7E, 0x5A, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00,
                0x00,
            ],
            'U' => [
                0x00, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00,
                0x00,
            ],
            'V' => [
                0x00, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x6C, 0x38, 0x10, 0x10, 0x00,
                0x00,
            ],
            'W' => [
                0x00, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xD6, 0xD6, 0xFE, 0xEE, 0xC6, 0xC6, 0x00,
                0x00,
            ],
            'X' => [
                0x00, 0xC6, 0xC6, 0x6C, 0x38, 0x38, 0x38, 0x38, 0x6C, 0xC6, 0xC6, 0xC6, 0x00,
                0x00,
            ],
            'Y' => [
                0x00, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00,
                0x00,
            ],
            'Z' => [
                0x00, 0xFE, 0xC6, 0x86, 0x0C, 0x18, 0x30, 0x60, 0xC0, 0xC2, 0xC6, 0xFE, 0x00,
                0x00,
            ],
            'a' => [
                0x00, 0x00, 0x00, 0x00, 0x78, 0x0C, 0x7C, 0xCC, 0xCC, 0xCC, 0xCC, 0x76, 0x00,
                0x00,
            ],
            'b' => [
                0x00, 0xE0, 0x60, 0x60, 0x78, 0x6C, 0x66, 0x66, 0x66, 0x66, 0x6C, 0x78, 0x00,
                0x00,
            ],
            'c' => [
                0x00, 0x00, 0x00, 0x00, 0x7C, 0xC6, 0xC0, 0xC0, 0xC0, 0xC0, 0xC6, 0x7C, 0x00,
                0x00,
            ],
            'd' => [
                0x00, 0x1C, 0x0C, 0x0C, 0x3C, 0x6C, 0xCC, 0xCC, 0xCC, 0xCC, 0x6C, 0x3C, 0x00,
                0x00,
            ],
            'e' => [
                0x00, 0x00, 0x00, 0x00, 0x7C, 0xC6, 0xFE, 0xC0, 0xC0, 0xC0, 0xC6, 0x7C, 0x00,
                0x00,
            ],
            'f' => [
                0x00, 0x1C, 0x36, 0x32, 0x30, 0x7C, 0x30, 0x30, 0x30, 0x30, 0x30, 0x78, 0x00,
                0x00,
            ],
            'g' => [
                0x00, 0x00, 0x00, 0x00, 0x76, 0xCC, 0xCC, 0xCC, 0xCC, 0x7C, 0x0C, 0xCC, 0x78,
                0x00,
            ],
            'h' => [
                0x00, 0xE0, 0x60, 0x60, 0x6C, 0x76, 0x66, 0x66, 0x66, 0x66, 0x66, 0xE6, 0x00,
                0x00,
            ],
            'i' => [
                0x00, 0x18, 0x18, 0x00, 0x38, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00,
                0x00,
            ],
            'j' => [
                0x00, 0x06, 0x06, 0x00, 0x0E, 0x06, 0x06, 0x06, 0x06, 0x06, 0x66, 0x66, 0x3C,
                0x00,
            ],
            'k' => [
                0x00, 0xE0, 0x60, 0x60, 0x66, 0x6C, 0x78, 0x78, 0x6C, 0x66, 0x66, 0xE6, 0x00,
                0x00,
            ],
            'l' => [
                0x00, 0x38, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00,
                0x00,
            ],
            'm' => [
                0x00, 0x00, 0x00, 0x00, 0xEC, 0xFE, 0xD6, 0xD6, 0xD6, 0xD6, 0xC6, 0xC6, 0x00,
                0x00,
            ],
            'n' => [
                0x00, 0x00, 0x00, 0x00, 0xDC, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x00,
                0x00,
            ],
            'o' => [
                0x00, 0x00, 0x00, 0x00, 0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00,
                0x00,
            ],
            'p' => [
                0x00, 0x00, 0x00, 0x00, 0xDC, 0x66, 0x66, 0x66, 0x66, 0x7C, 0x60, 0x60, 0xF0,
                0x00,
            ],
            'q' => [
                0x00, 0x00, 0x00, 0x00, 0x76, 0xCC, 0xCC, 0xCC, 0xCC, 0x7C, 0x0C, 0x0C, 0x1E,
                0x00,
            ],
            'r' => [
                0x00, 0x00, 0x00, 0x00, 0xDC, 0x76, 0x66, 0x60, 0x60, 0x60, 0x60, 0xF0, 0x00,
                0x00,
            ],
            's' => [
                0x00, 0x00, 0x00, 0x00, 0x7C, 0xC6, 0x60, 0x38, 0x0C, 0x06, 0xC6, 0x7C, 0x00,
                0x00,
            ],
            't' => [
                0x00, 0x10, 0x30, 0x30, 0xFC, 0x30, 0x30, 0x30, 0x30, 0x30, 0x36, 0x1C, 0x00,
                0x00,
            ],
            'u' => [
                0x00, 0x00, 0x00, 0x00, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0x76, 0x00,
                0x00,
            ],
            'v' => [
                0x00, 0x00, 0x00, 0x00, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x6C, 0x38, 0x10, 0x00,
                0x00,
            ],
            'w' => [
                0x00, 0x00, 0x00, 0x00, 0xC6, 0xC6, 0xD6, 0xD6, 0xD6, 0xFE, 0xEE, 0x6C, 0x00,
                0x00,
            ],
            'x' => [
                0x00, 0x00, 0x00, 0x00, 0xC6, 0x6C, 0x38, 0x38, 0x38, 0x6C, 0xC6, 0xC6, 0x00,
                0x00,
            ],
            'y' => [
                0x00, 0x00, 0x00, 0x00, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7E, 0x06, 0x0C, 0xF8,
                0x00,
            ],
            'z' => [
                0x00, 0x00, 0x00, 0x00, 0xFE, 0xCC, 0x18, 0x30, 0x60, 0xC0, 0xC6, 0xFE, 0x00,
                0x00,
            ],
            '0' => [
                0x00, 0x7C, 0xC6, 0xCE, 0xDE, 0xF6, 0xE6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00,
                0x00,
            ],
            '1' => [
                0x00, 0x18, 0x38, 0x78, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00,
                0x00,
            ],
            '2' => [
                0x00, 0x7C, 0xC6, 0x06, 0x0C, 0x18, 0x30, 0x60, 0xC0, 0xC0, 0xC6, 0xFE, 0x00,
                0x00,
            ],
            '3' => [
                0x00, 0x7C, 0xC6, 0x06, 0x06, 0x3C, 0x06, 0x06, 0x06, 0x06, 0xC6, 0x7C, 0x00,
                0x00,
            ],
            '4' => [
                0x00, 0x0C, 0x1C, 0x3C, 0x6C, 0xCC, 0xFE, 0x0C, 0x0C, 0x0C, 0x0C, 0x1E, 0x00,
                0x00,
            ],
            '5' => [
                0x00, 0xFE, 0xC0, 0xC0, 0xC0, 0xFC, 0x06, 0x06, 0x06, 0x06, 0xC6, 0x7C, 0x00,
                0x00,
            ],
            '6' => [
                0x00, 0x38, 0x60, 0xC0, 0xC0, 0xFC, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00,
                0x00,
            ],
            '7' => [
                0x00, 0xFE, 0xC6, 0x06, 0x0C, 0x18, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x00,
                0x00,
            ],
            '8' => [
                0x00, 0x7C, 0xC6, 0xC6, 0xC6, 0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00,
                0x00,
            ],
            '9' => [
                0x00, 0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0x7E, 0x06, 0x06, 0x06, 0x0C, 0x78, 0x00,
                0x00,
            ],
            '-' => [
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFE, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00,
            ],
            '_' => [
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF,
                0x00,
            ],
            '.' => [
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00,
                0x00,
            ],
            ':' => [
                0x00, 0x00, 0x00, 0x18, 0x18, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00, 0x00, 0x00,
                0x00,
            ],
            '/' => [
                0x00, 0x02, 0x06, 0x0C, 0x18, 0x30, 0x60, 0xC0, 0x80, 0x00, 0x00, 0x00, 0x00,
                0x00,
            ],
            '(' => [
                0x00, 0x0C, 0x18, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x18, 0x0C, 0x00,
                0x00,
            ],
            ')' => [
                0x00, 0x30, 0x18, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x18, 0x30, 0x00,
                0x00,
            ],
            // For any character without a specific glyph, render a filled box.
            _ => [
                0x00, 0x00, 0xFE, 0xFE, 0xFE, 0xFE, 0xFE, 0xFE, 0xFE, 0xFE, 0xFE, 0x00, 0x00,
                0x00,
            ],
        }
    }
}

// ---------------------------------------------------------------------------
// Rendering engine
// ---------------------------------------------------------------------------

/// The rendering engine rasterizes RenderCommands to the framebuffer.
struct RenderEngine {
    clip_stack: ClipStack,
    translate_stack: TranslateStack,
}

impl RenderEngine {
    fn new() -> Self {
        Self {
            clip_stack: ClipStack::default(),
            translate_stack: TranslateStack::default(),
        }
    }

    /// Execute a list of render commands, drawing into the framebuffer within
    /// the given window region.
    fn execute(
        &mut self,
        fb: &mut Framebuffer,
        commands: &[RenderCommand],
        window_x: i32,
        window_y: i32,
        window_width: u32,
        window_height: u32,
        opacity: f32,
    ) {
        // Set up initial clip to the window's client area.
        self.clip_stack.clear();
        self.translate_stack.clear();
        self.clip_stack.push(Rect::new(
            window_x,
            window_y,
            window_width,
            window_height,
        ));
        // Push the window origin as the base translation.
        self.translate_stack
            .push(window_x as f32, window_y as f32);

        for cmd in commands {
            self.execute_command(fb, cmd, opacity);
        }

        self.clip_stack.clear();
        self.translate_stack.clear();
    }

    fn execute_command(&mut self, fb: &mut Framebuffer, cmd: &RenderCommand, opacity: f32) {
        let (tx, ty) = self.translate_stack.offset();

        match cmd {
            RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                color,
                corner_radii: _,
            } => {
                let px = (*x + tx) as i32;
                let py = (*y + ty) as i32;
                let w = *width as u32;
                let h = *height as u32;
                self.fill_rect(fb, px, py, w, h, color_to_argb(color), opacity);
            }
            RenderCommand::StrokeRect {
                x,
                y,
                width,
                height,
                color,
                line_width,
                corner_radii: _,
            } => {
                let px = (*x + tx) as i32;
                let py = (*y + ty) as i32;
                let w = *width as u32;
                let h = *height as u32;
                let lw = (*line_width).max(1.0) as u32;
                self.stroke_rect(fb, px, py, w, h, lw, color_to_argb(color), opacity);
            }
            RenderCommand::Text {
                x,
                y,
                text,
                color,
                font_size: _,
                font_weight: _,
                max_width,
            } => {
                let px = (*x + tx) as i32;
                let py = (*y + ty) as i32;
                let max_w = max_width.map(|w| w as u32);
                self.draw_text(fb, px, py, text, color_to_argb(color), opacity, max_w);
            }
            RenderCommand::Line {
                x1,
                y1,
                x2,
                y2,
                color,
                width: _,
            } => {
                let px1 = (*x1 + tx) as i32;
                let py1 = (*y1 + ty) as i32;
                let px2 = (*x2 + tx) as i32;
                let py2 = (*y2 + ty) as i32;
                self.draw_line(fb, px1, py1, px2, py2, color_to_argb(color), opacity);
            }
            RenderCommand::PushClip {
                x,
                y,
                width,
                height,
            } => {
                let px = (*x + tx) as i32;
                let py = (*y + ty) as i32;
                self.clip_stack
                    .push(Rect::new(px, py, *width as u32, *height as u32));
            }
            RenderCommand::PopClip => {
                self.clip_stack.pop();
            }
            RenderCommand::PushTranslate { dx, dy } => {
                self.translate_stack.push(*dx, *dy);
            }
            RenderCommand::PopTranslate => {
                self.translate_stack.pop();
            }
            RenderCommand::Image { .. } => {
                // Image rendering requires an asset store — stub for now.
            }
            RenderCommand::BoxShadow {
                x,
                y,
                width,
                height,
                offset_x,
                offset_y,
                blur,
                spread,
                color,
                corner_radii: _,
            } => {
                // Simplified shadow: draw a semi-transparent rectangle expanded by spread+blur.
                let expand = (*spread + *blur) as i32;
                let px = (*x + tx + *offset_x) as i32 - expand;
                let py = (*y + ty + *offset_y) as i32 - expand;
                let w = (*width as i32 + expand * 2) as u32;
                let h = (*height as i32 + expand * 2) as u32;
                self.fill_rect(fb, px, py, w, h, color_to_argb(color), opacity);
            }
        }
    }

    /// Fill a rectangle with bounds checking and clipping.
    fn fill_rect(
        &self,
        fb: &mut Framebuffer,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        color: u32,
        opacity: f32,
    ) {
        let draw_rect = Rect::new(x, y, width, height);
        let clipped = match self.effective_clip(&draw_rect) {
            Some(r) => r,
            None => return,
        };

        let x_start = clipped.x.max(0) as u32;
        let y_start = clipped.y.max(0) as u32;
        let x_end = (clipped.x + clipped.width as i32).max(0) as u32;
        let y_end = (clipped.y + clipped.height as i32).max(0) as u32;

        for row in y_start..y_end {
            for col in x_start..x_end {
                fb.blend_pixel(col, row, color, opacity);
            }
        }
    }

    /// Stroke (outline) a rectangle.
    fn stroke_rect(
        &self,
        fb: &mut Framebuffer,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        line_width: u32,
        color: u32,
        opacity: f32,
    ) {
        // Top edge
        self.fill_rect(fb, x, y, width, line_width, color, opacity);
        // Bottom edge
        self.fill_rect(
            fb,
            x,
            y + height as i32 - line_width as i32,
            width,
            line_width,
            color,
            opacity,
        );
        // Left edge
        self.fill_rect(fb, x, y, line_width, height, color, opacity);
        // Right edge
        self.fill_rect(
            fb,
            x + width as i32 - line_width as i32,
            y,
            line_width,
            height,
            color,
            opacity,
        );
    }

    /// Draw text using the bitmap font.
    fn draw_text(
        &self,
        fb: &mut Framebuffer,
        x: i32,
        y: i32,
        text: &str,
        color: u32,
        opacity: f32,
        max_width: Option<u32>,
    ) {
        let clip = self.clip_stack.current().copied();
        let mut cursor_x = x;
        let max_x = max_width.map(|w| x + w as i32);

        for ch in text.chars() {
            if let Some(mx) = max_x
                && cursor_x + CHAR_WIDTH as i32 > mx {
                    break;
                }

            if cursor_x >= 0 && y >= 0 {
                BitmapFont::draw_char(
                    fb,
                    ch,
                    cursor_x as u32,
                    y as u32,
                    color,
                    opacity,
                    clip.as_ref(),
                );
            }
            cursor_x += CHAR_WIDTH as i32;
        }
    }

    /// Draw a line using Bresenham's algorithm.
    fn draw_line(
        &self,
        fb: &mut Framebuffer,
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        color: u32,
        opacity: f32,
    ) {
        let clip = self.clip_stack.current().copied();

        let dx = (x2 - x1).abs();
        let dy = -(y2 - y1).abs();
        let sx: i32 = if x1 < x2 { 1 } else { -1 };
        let sy: i32 = if y1 < y2 { 1 } else { -1 };
        let mut err = dx + dy;

        let mut cx = x1;
        let mut cy = y1;

        loop {
            // Plot pixel if within clip bounds.
            if cx >= 0 && cy >= 0 {
                let in_clip = match &clip {
                    Some(c) => c.contains(cx, cy),
                    None => true,
                };
                if in_clip {
                    fb.blend_pixel(cx as u32, cy as u32, color, opacity);
                }
            }

            if cx == x2 && cy == y2 {
                break;
            }

            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                cx += sx;
            }
            if e2 <= dx {
                err += dx;
                cy += sy;
            }
        }
    }

    /// Compute the effective clip rectangle by intersecting the draw area
    /// with the current clip stack.
    fn effective_clip(&self, draw_rect: &Rect) -> Option<Rect> {
        match self.clip_stack.current() {
            Some(clip) => clip.intersect(draw_rect),
            None => Some(*draw_rect),
        }
    }
}

/// Convert a guitk Color to ARGB u32 format.
#[inline]
fn color_to_argb(color: &Color) -> u32 {
    (color.a as u32) << 24 | (color.r as u32) << 16 | (color.g as u32) << 8 | color.b as u32
}

// ---------------------------------------------------------------------------
// Theme colors for window decorations
// ---------------------------------------------------------------------------

/// Colors used for window decoration rendering.
#[allow(dead_code)]
struct DecorationTheme {
    /// Title bar background when focused.
    title_bar_focused: u32,
    /// Title bar background when unfocused.
    title_bar_unfocused: u32,
    /// Title text color when focused.
    title_text_focused: u32,
    /// Title text color when unfocused.
    title_text_unfocused: u32,
    /// Close button color.
    close_button: u32,
    /// Close button hover color.
    close_button_hover: u32,
    /// Maximize button color.
    maximize_button: u32,
    /// Minimize button color.
    minimize_button: u32,
    /// Window border color when focused.
    border_focused: u32,
    /// Window border color when unfocused.
    border_unfocused: u32,
    /// Shadow color (semi-transparent black).
    shadow_color: u32,
    /// Desktop background color.
    desktop_background: u32,
}

impl Default for DecorationTheme {
    fn default() -> Self {
        Self {
            title_bar_focused: 0xFF_2B_2B_3D,    // Dark blue-gray
            title_bar_unfocused: 0xFF_3C_3C_4A,  // Lighter gray
            title_text_focused: 0xFF_FF_FF_FF,    // White
            title_text_unfocused: 0xFF_A0_A0_A0,  // Gray text
            close_button: 0xFF_E8_4D_4D,          // Red
            close_button_hover: 0xFF_FF_60_60,    // Bright red
            maximize_button: 0xFF_4D_C8_4D,       // Green
            minimize_button: 0xFF_E8_C8_4D,       // Yellow
            border_focused: 0xFF_50_50_70,        // Subtle border
            border_unfocused: 0xFF_40_40_50,      // Dimmer border
            shadow_color: 0x40_00_00_00,          // Semi-transparent black
            desktop_background: 0xFF_1A_1A_2E,    // Dark navy
        }
    }
}

// ---------------------------------------------------------------------------
// Compositor
// ---------------------------------------------------------------------------

/// The main compositor state machine.
pub struct Compositor {
    /// All managed windows (ordered by creation, z_order field determines draw order).
    windows: Vec<Window>,
    /// Z-order stack (WindowIds from bottom to top).
    z_stack: Vec<WindowId>,
    /// The currently focused window (receives keyboard input).
    focused_window: Option<WindowId>,
    /// The framebuffer we composite into.
    framebuffer: Framebuffer,
    /// Display configuration.
    display_manager: DisplayManager,
    /// Damage tracking for the current frame.
    damage: DamageRegion,
    /// Frame timing statistics.
    frame_stats: FrameStats,
    /// Current mouse cursor position.
    cursor_x: i32,
    /// Current mouse cursor position.
    cursor_y: i32,
    /// Current cursor shape.
    cursor_shape: CursorShape,
    /// Active drag operation (if any).
    drag: Option<DragState>,
    /// Rendering engine instance.
    render_engine: RenderEngine,
    /// Decoration theme.
    theme: DecorationTheme,
    /// Outbound event notifications for clients (stub queue).
    pending_notifications: VecDeque<EventNotification>,
    /// Whether a full recomposite is needed (e.g., after display resize).
    full_recomposite: bool,
}

impl Compositor {
    /// Create a new compositor with the given display dimensions.
    pub fn new(width: u32, height: u32, refresh_rate: u32) -> CompositorResult<Self> {
        let framebuffer = Framebuffer::new(width, height)?;
        let display_manager = DisplayManager::new(width, height, refresh_rate);
        let frame_interval = Duration::from_micros(
            if refresh_rate == 0 {
                16_667
            } else {
                1_000_000 / refresh_rate as u64
            },
        );

        Ok(Self {
            windows: Vec::new(),
            z_stack: Vec::new(),
            focused_window: None,
            framebuffer,
            display_manager,
            damage: DamageRegion::new(),
            frame_stats: FrameStats::new(frame_interval),
            cursor_x: width as i32 / 2,
            cursor_y: height as i32 / 2,
            cursor_shape: CursorShape::Arrow,
            drag: None,
            render_engine: RenderEngine::new(),
            theme: DecorationTheme::default(),
            pending_notifications: VecDeque::new(),
            full_recomposite: true,
        })
    }

    // -----------------------------------------------------------------------
    // Window management
    // -----------------------------------------------------------------------

    /// Create a new window and return its ID.
    pub fn create_window(
        &mut self,
        title: String,
        width: u32,
        height: u32,
        client_pid: u64,
    ) -> WindowId {
        // Place the window at a slightly offset position from existing windows.
        let offset = (self.windows.len() as i32 * 30) % 300;
        let x = 100 + offset;
        let y = 80 + offset;

        let window = Window::new(title, x, y, width, height, client_pid);
        let id = window.id;

        self.windows.push(window);
        self.z_stack.push(id);
        self.update_z_orders();

        // Focus the new window.
        self.focus_window(id);

        // Mark damage for the new window's area.
        self.damage_window(id);
        self.full_recomposite = true;

        id
    }

    /// Destroy a window.
    pub fn destroy_window(&mut self, window_id: WindowId) -> CompositorResult<()> {
        let idx = self
            .window_index(window_id)
            .ok_or(CompositorError::WindowNotFound(window_id))?;

        // Mark the old area as damaged before removing.
        self.damage_window(window_id);

        self.windows.remove(idx);
        self.z_stack.retain(|&id| id != window_id);
        self.update_z_orders();

        // If this was the focused window, focus the topmost remaining window.
        if self.focused_window == Some(window_id) {
            self.focused_window = None;
            if let Some(&top_id) = self.z_stack.last() {
                self.focus_window(top_id);
            }
        }

        self.full_recomposite = true;
        Ok(())
    }

    /// Move a window to a new position.
    pub fn move_window(&mut self, window_id: WindowId, x: i32, y: i32) -> CompositorResult<()> {
        // Damage old position.
        self.damage_window(window_id);

        let window = self
            .window_mut(window_id)
            .ok_or(CompositorError::WindowNotFound(window_id))?;
        window.x = x;
        window.y = y;
        window.dirty = true;

        // Damage new position.
        self.damage_window(window_id);
        Ok(())
    }

    /// Resize a window's client area.
    pub fn resize_window(
        &mut self,
        window_id: WindowId,
        width: u32,
        height: u32,
    ) -> CompositorResult<()> {
        // Damage old area.
        self.damage_window(window_id);

        let (final_w, final_h) = {
            let window = self
                .window_mut(window_id)
                .ok_or(CompositorError::WindowNotFound(window_id))?;
            window.width = width.max(100); // Minimum window size
            window.height = height.max(50);
            window.dirty = true;
            (window.width, window.height)
        };

        // Damage new area.
        self.damage_window(window_id);

        // Notify client of resize.
        self.pending_notifications.push_back(EventNotification::WindowResized {
            window_id,
            width: final_w,
            height: final_h,
        });

        Ok(())
    }

    /// Minimize a window.
    pub fn minimize_window(&mut self, window_id: WindowId) -> CompositorResult<()> {
        self.damage_window(window_id);

        let window = self
            .window_mut(window_id)
            .ok_or(CompositorError::WindowNotFound(window_id))?;
        window.minimized = true;
        window.visible = false;
        window.dirty = true;

        // Focus next window if this was focused.
        if self.focused_window == Some(window_id) {
            self.focused_window = None;
            self.focus_topmost_visible();
        }

        self.full_recomposite = true;
        Ok(())
    }

    /// Maximize a window to fill the display.
    pub fn maximize_window(&mut self, window_id: WindowId) -> CompositorResult<()> {
        self.damage_window(window_id);

        let display_bounds = self.display_manager.virtual_bounds();

        let (final_w, final_h) = {
            let window = self
                .window_mut(window_id)
                .ok_or(CompositorError::WindowNotFound(window_id))?;

            if !window.maximized {
                // Save current geometry for restore.
                window.restore_rect = Some(Rect::new(
                    window.x,
                    window.y,
                    window.width,
                    window.height,
                ));
            }

            window.maximized = true;
            window.x = display_bounds.x + BORDER_WIDTH as i32;
            window.y = display_bounds.y + TITLE_BAR_HEIGHT as i32;
            window.width = display_bounds
                .width
                .saturating_sub(BORDER_WIDTH * 2);
            window.height = display_bounds
                .height
                .saturating_sub(TITLE_BAR_HEIGHT + BORDER_WIDTH);
            window.dirty = true;
            (window.width, window.height)
        };

        self.damage_window(window_id);
        self.full_recomposite = true;

        // Notify client of resize.
        self.pending_notifications.push_back(EventNotification::WindowResized {
            window_id,
            width: final_w,
            height: final_h,
        });

        Ok(())
    }

    /// Restore a window from minimized or maximized state.
    pub fn restore_window(&mut self, window_id: WindowId) -> CompositorResult<()> {
        self.damage_window(window_id);

        let window = self
            .window_mut(window_id)
            .ok_or(CompositorError::WindowNotFound(window_id))?;

        if window.minimized {
            window.minimized = false;
            window.visible = true;
        }

        if window.maximized {
            window.maximized = false;
            if let Some(restore) = window.restore_rect.take() {
                window.x = restore.x;
                window.y = restore.y;
                window.width = restore.width;
                window.height = restore.height;
            }
        }

        window.dirty = true;
        self.damage_window(window_id);
        self.focus_window(window_id);
        self.full_recomposite = true;

        Ok(())
    }

    /// Set focus to a specific window.
    pub fn focus_window(&mut self, window_id: WindowId) {
        let old_focused = self.focused_window;

        // Unfocus the previously focused window.
        if let Some(old_id) = old_focused
            && old_id != window_id {
                if let Some(win) = self.window_mut(old_id) {
                    win.focused = false;
                    win.dirty = true;
                }
                self.damage_window(old_id);
                self.pending_notifications
                    .push_back(EventNotification::FocusLost { window_id: old_id });
            }

        // Focus the new window.
        if let Some(win) = self.window_mut(window_id)
            && !win.minimized {
                win.focused = true;
                win.dirty = true;
                self.focused_window = Some(window_id);

                // Bring to top of z-stack.
                self.z_stack.retain(|&id| id != window_id);
                self.z_stack.push(window_id);
                self.update_z_orders();

                self.damage_window(window_id);
                self.pending_notifications
                    .push_back(EventNotification::FocusGained { window_id });
            }
    }

    /// Set a window's title.
    pub fn set_title(&mut self, window_id: WindowId, title: String) -> CompositorResult<()> {
        let window = self
            .window_mut(window_id)
            .ok_or(CompositorError::WindowNotFound(window_id))?;
        window.title = title;
        window.dirty = true;
        self.damage_window(window_id);
        Ok(())
    }

    /// Set a window's opacity.
    pub fn set_opacity(&mut self, window_id: WindowId, opacity: f32) -> CompositorResult<()> {
        let window = self
            .window_mut(window_id)
            .ok_or(CompositorError::WindowNotFound(window_id))?;
        window.opacity = opacity.clamp(0.0, 1.0);
        window.dirty = true;
        self.damage_window(window_id);
        self.full_recomposite = true;
        Ok(())
    }

    /// Submit render commands from a client for its window.
    pub fn submit_render(
        &mut self,
        window_id: WindowId,
        commands: Vec<RenderCommand>,
    ) -> CompositorResult<()> {
        let window = self
            .window_mut(window_id)
            .ok_or(CompositorError::WindowNotFound(window_id))?;
        window.render_tree = RenderTree { commands };
        window.dirty = true;
        self.damage_window(window_id);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Input routing
    // -----------------------------------------------------------------------

    /// Process an input event and route it to the appropriate window.
    pub fn handle_input(&mut self, event: InputEvent) {
        match event {
            InputEvent::MouseMove { x, y } => self.handle_mouse_move(x, y),
            InputEvent::MouseButton {
                button,
                pressed,
                x,
                y,
            } => self.handle_mouse_button(button, pressed, x, y),
            InputEvent::MouseScroll { dx, dy, x, y } => self.handle_mouse_scroll(dx, dy, x, y),
            InputEvent::KeyDown {
                scancode,
                character,
            } => self.handle_key(scancode, true, character),
            InputEvent::KeyUp { scancode } => self.handle_key(scancode, false, None),
            InputEvent::TextInput { text } => self.handle_text_input(&text),
        }
    }

    fn handle_mouse_move(&mut self, x: i32, y: i32) {
        self.cursor_x = x;
        self.cursor_y = y;

        // Handle active drag.
        if let Some(drag) = self.drag.clone() {
            let dx = x - drag.start_mouse.x;
            let dy = y - drag.start_mouse.y;

            match drag.mode {
                DragMode::MoveWindow => {
                    let new_x = drag.start_window_pos.x + dx;
                    let new_y = drag.start_window_pos.y + dy;
                    let _ = self.move_window(drag.window_id, new_x, new_y);
                }
                DragMode::ResizeRight => {
                    let new_w = (drag.start_window_size.0 as i32 + dx).max(100) as u32;
                    let _ = self.resize_window(drag.window_id, new_w, drag.start_window_size.1);
                }
                DragMode::ResizeBottom => {
                    let new_h = (drag.start_window_size.1 as i32 + dy).max(50) as u32;
                    let _ = self.resize_window(drag.window_id, drag.start_window_size.0, new_h);
                }
                DragMode::ResizeLeft => {
                    let new_w = (drag.start_window_size.0 as i32 - dx).max(100) as u32;
                    let new_x = drag.start_window_pos.x + (drag.start_window_size.0 as i32 - new_w as i32);
                    let _ = self.move_window(drag.window_id, new_x, drag.start_window_pos.y);
                    let _ = self.resize_window(drag.window_id, new_w, drag.start_window_size.1);
                }
                DragMode::ResizeTop => {
                    let new_h = (drag.start_window_size.1 as i32 - dy).max(50) as u32;
                    let new_y = drag.start_window_pos.y + (drag.start_window_size.1 as i32 - new_h as i32);
                    let _ = self.move_window(drag.window_id, drag.start_window_pos.x, new_y);
                    let _ = self.resize_window(drag.window_id, drag.start_window_size.0, new_h);
                }
                DragMode::ResizeTopLeft => {
                    let new_w = (drag.start_window_size.0 as i32 - dx).max(100) as u32;
                    let new_h = (drag.start_window_size.1 as i32 - dy).max(50) as u32;
                    let new_x = drag.start_window_pos.x + (drag.start_window_size.0 as i32 - new_w as i32);
                    let new_y = drag.start_window_pos.y + (drag.start_window_size.1 as i32 - new_h as i32);
                    let _ = self.move_window(drag.window_id, new_x, new_y);
                    let _ = self.resize_window(drag.window_id, new_w, new_h);
                }
                DragMode::ResizeTopRight => {
                    let new_w = (drag.start_window_size.0 as i32 + dx).max(100) as u32;
                    let new_h = (drag.start_window_size.1 as i32 - dy).max(50) as u32;
                    let new_y = drag.start_window_pos.y + (drag.start_window_size.1 as i32 - new_h as i32);
                    let _ = self.move_window(drag.window_id, drag.start_window_pos.x, new_y);
                    let _ = self.resize_window(drag.window_id, new_w, new_h);
                }
                DragMode::ResizeBottomLeft => {
                    let new_w = (drag.start_window_size.0 as i32 - dx).max(100) as u32;
                    let new_h = (drag.start_window_size.1 as i32 + dy).max(50) as u32;
                    let new_x = drag.start_window_pos.x + (drag.start_window_size.0 as i32 - new_w as i32);
                    let _ = self.move_window(drag.window_id, new_x, drag.start_window_pos.y);
                    let _ = self.resize_window(drag.window_id, new_w, new_h);
                }
                DragMode::ResizeBottomRight => {
                    let new_w = (drag.start_window_size.0 as i32 + dx).max(100) as u32;
                    let new_h = (drag.start_window_size.1 as i32 + dy).max(50) as u32;
                    let _ = self.resize_window(drag.window_id, new_w, new_h);
                }
            }
            return;
        }

        // Update cursor shape based on what's under the cursor.
        self.update_cursor_shape(x, y);

        // Route mouse move to the window under the cursor.
        if let Some(window_id) = self.window_at(x, y)
            && let Some(win) = self.window_ref(window_id) {
                let local_x = x - win.x;
                let local_y = y - win.y;
                self.pending_notifications
                    .push_back(EventNotification::MouseEvent {
                        window_id,
                        x: local_x,
                        y: local_y,
                        kind: MouseEventKind::Move,
                    });
            }
    }

    fn handle_mouse_button(&mut self, button: MouseButton, pressed: bool, x: i32, y: i32) {
        self.cursor_x = x;
        self.cursor_y = y;

        // Release ends any active drag.
        if !pressed && button == MouseButton::Left {
            self.drag = None;
            return;
        }

        if !pressed {
            // Route release to focused window.
            if let Some(window_id) = self.focused_window
                && let Some(win) = self.window_ref(window_id) {
                    let local_x = x - win.x;
                    let local_y = y - win.y;
                    self.pending_notifications
                        .push_back(EventNotification::MouseEvent {
                            window_id,
                            x: local_x,
                            y: local_y,
                            kind: MouseEventKind::ButtonRelease(button),
                        });
                }
            return;
        }

        // Left button press: check window decorations first, then client area.
        if button == MouseButton::Left {
            // Check windows from top to bottom z-order.
            let hit_window = self.window_at_with_decorations(x, y);

            if let Some(window_id) = hit_window {
                // Focus the window.
                self.focus_window(window_id);

                // Check if we hit a decoration element.
                if let Some(win) = self.window_ref(window_id) {
                    // Close button?
                    if win.close_button_rect().contains(x, y) {
                        self.pending_notifications
                            .push_back(EventNotification::WindowClose { window_id });
                        return;
                    }
                    // Maximize button?
                    if win.maximize_button_rect().contains(x, y) {
                        if win.maximized {
                            let _ = self.restore_window(window_id);
                        } else {
                            let _ = self.maximize_window(window_id);
                        }
                        return;
                    }
                    // Minimize button?
                    if win.minimize_button_rect().contains(x, y) {
                        let _ = self.minimize_window(window_id);
                        return;
                    }
                    // Title bar drag?
                    if win.title_bar_rect().contains(x, y) {
                        self.drag = Some(DragState {
                            window_id,
                            mode: DragMode::MoveWindow,
                            start_mouse: Point::new(x, y),
                            start_window_pos: Point::new(win.x, win.y),
                            start_window_size: (win.width, win.height),
                        });
                        return;
                    }
                    // Border resize?
                    if let Some(mode) = self.detect_border_drag(win, x, y) {
                        self.drag = Some(DragState {
                            window_id,
                            mode,
                            start_mouse: Point::new(x, y),
                            start_window_pos: Point::new(win.x, win.y),
                            start_window_size: (win.width, win.height),
                        });
                        return;
                    }
                    // Client area click.
                    let local_x = x - win.x;
                    let local_y = y - win.y;
                    self.pending_notifications
                        .push_back(EventNotification::MouseEvent {
                            window_id,
                            x: local_x,
                            y: local_y,
                            kind: MouseEventKind::ButtonPress(button),
                        });
                }
            }
        } else {
            // Non-left button: route to window under cursor.
            if let Some(window_id) = self.window_at(x, y)
                && let Some(win) = self.window_ref(window_id) {
                    let local_x = x - win.x;
                    let local_y = y - win.y;
                    self.pending_notifications
                        .push_back(EventNotification::MouseEvent {
                            window_id,
                            x: local_x,
                            y: local_y,
                            kind: MouseEventKind::ButtonPress(button),
                        });
                }
        }
    }

    fn handle_mouse_scroll(&mut self, dx: f32, dy: f32, x: i32, y: i32) {
        if let Some(window_id) = self.window_at(x, y)
            && let Some(win) = self.window_ref(window_id) {
                let local_x = x - win.x;
                let local_y = y - win.y;
                self.pending_notifications
                    .push_back(EventNotification::MouseEvent {
                        window_id,
                        x: local_x,
                        y: local_y,
                        kind: MouseEventKind::Scroll { dx, dy },
                    });
            }
    }

    fn handle_key(&mut self, scancode: u32, pressed: bool, character: Option<char>) {
        // Keyboard events go to the focused window.
        if let Some(window_id) = self.focused_window {
            self.pending_notifications
                .push_back(EventNotification::KeyEvent {
                    window_id,
                    scancode,
                    pressed,
                    character,
                });
        }
    }

    fn handle_text_input(&mut self, _text: &str) {
        // Text input is delivered as key events with characters for now.
        // A full IME system would handle this separately.
    }

    // -----------------------------------------------------------------------
    // Hit testing
    // -----------------------------------------------------------------------

    /// Find the topmost window whose client area contains the point.
    fn window_at(&self, x: i32, y: i32) -> Option<WindowId> {
        // Iterate z_stack from top to bottom.
        for &window_id in self.z_stack.iter().rev() {
            if let Some(win) = self.window_ref(window_id)
                && win.visible && !win.minimized && win.client_rect().contains(x, y) {
                    return Some(window_id);
                }
        }
        None
    }

    /// Find the topmost window whose full area (including decorations) contains the point.
    fn window_at_with_decorations(&self, x: i32, y: i32) -> Option<WindowId> {
        for &window_id in self.z_stack.iter().rev() {
            if let Some(win) = self.window_ref(window_id)
                && win.visible && !win.minimized && win.outer_rect().contains(x, y) {
                    return Some(window_id);
                }
        }
        None
    }

    /// Detect which border edge the cursor is on (for resize drag detection).
    fn detect_border_drag(&self, win: &Window, x: i32, y: i32) -> Option<DragMode> {
        let grab_size = BORDER_WIDTH as i32 + SHADOW_SIZE as i32;
        let outer = win.outer_rect();

        // Don't detect border drag if the point is inside the client area or title bar.
        if win.client_rect().contains(x, y) || win.title_bar_rect().contains(x, y) {
            return None;
        }

        if !outer.contains(x, y) {
            return None;
        }

        let at_left = x < win.x - BORDER_WIDTH as i32 + grab_size;
        let at_right = x >= win.x + win.width as i32 + BORDER_WIDTH as i32 - grab_size;
        let at_top = y < win.y - TITLE_BAR_HEIGHT as i32 + grab_size;
        let at_bottom = y >= win.y + win.height as i32 + BORDER_WIDTH as i32 - grab_size;

        match (at_left, at_right, at_top, at_bottom) {
            (true, false, true, false) => Some(DragMode::ResizeTopLeft),
            (false, true, true, false) => Some(DragMode::ResizeTopRight),
            (true, false, false, true) => Some(DragMode::ResizeBottomLeft),
            (false, true, false, true) => Some(DragMode::ResizeBottomRight),
            (true, false, _, _) => Some(DragMode::ResizeLeft),
            (false, true, _, _) => Some(DragMode::ResizeRight),
            (_, _, true, false) => Some(DragMode::ResizeTop),
            (_, _, false, true) => Some(DragMode::ResizeBottom),
            _ => None,
        }
    }

    /// Update the cursor shape based on what's under the cursor.
    fn update_cursor_shape(&mut self, x: i32, y: i32) {
        // Check if we're on a window border (resize cursor).
        for &window_id in self.z_stack.iter().rev() {
            if let Some(win) = self.window_ref(window_id) {
                if !win.visible || win.minimized {
                    continue;
                }
                if let Some(mode) = self.detect_border_drag(win, x, y) {
                    self.cursor_shape = match mode {
                        DragMode::ResizeLeft | DragMode::ResizeRight => CursorShape::ResizeEW,
                        DragMode::ResizeTop | DragMode::ResizeBottom => CursorShape::ResizeNS,
                        DragMode::ResizeTopLeft | DragMode::ResizeBottomRight => {
                            CursorShape::ResizeNWSE
                        }
                        DragMode::ResizeTopRight | DragMode::ResizeBottomLeft => {
                            CursorShape::ResizeNESW
                        }
                        DragMode::MoveWindow => CursorShape::Move,
                    };
                    return;
                }
                if win.outer_rect().contains(x, y) {
                    // Inside a window area but not on a border.
                    self.cursor_shape = CursorShape::Arrow;
                    return;
                }
            }
        }
        // Over the desktop background.
        self.cursor_shape = CursorShape::Arrow;
    }

    // -----------------------------------------------------------------------
    // Compositing pipeline
    // -----------------------------------------------------------------------

    /// Composite a frame. Returns true if a frame was actually composited
    /// (false if skipped due to no damage or frame budget).
    pub fn compose_frame(&mut self) -> bool {
        // Check if we should compose (frame timing).
        if !self.frame_stats.should_compose() {
            return false;
        }

        // Check if there's anything to composite.
        if !self.full_recomposite && !self.damage.has_damage() {
            return false;
        }

        self.frame_stats.begin_frame();

        if self.full_recomposite {
            // Full recomposite: clear and redraw everything.
            self.framebuffer.clear(self.theme.desktop_background);
            self.render_all_windows();
            self.full_recomposite = false;
            self.damage.clear();
        } else {
            // Partial recomposite: only redraw damaged areas.
            let damaged_rects: Vec<Rect> = self.damage.rects().to_vec();
            for rect in &damaged_rects {
                self.framebuffer.clear_rect(rect, self.theme.desktop_background);
            }
            // Re-render windows that overlap with damaged areas.
            self.render_damaged_windows(&damaged_rects);
            self.damage.clear();
        }

        // Swap buffers.
        self.framebuffer.swap();

        self.frame_stats.end_frame();
        true
    }

    /// Render all visible windows from bottom to top z-order.
    fn render_all_windows(&mut self) {
        let z_stack_copy: Vec<WindowId> = self.z_stack.clone();
        for &window_id in &z_stack_copy {
            self.render_window(window_id);
        }
    }

    /// Render only windows that overlap with the given damaged rects.
    fn render_damaged_windows(&mut self, damaged_rects: &[Rect]) {
        let z_stack_copy: Vec<WindowId> = self.z_stack.clone();
        for &window_id in &z_stack_copy {
            if let Some(win) = self.window_ref(window_id) {
                if !win.visible || win.minimized {
                    continue;
                }
                let outer = win.outer_rect();
                let overlaps = damaged_rects.iter().any(|r| r.intersect(&outer).is_some());
                if overlaps {
                    self.render_window(window_id);
                }
            }
        }
    }

    /// Render a single window (shadow, decorations, client content).
    fn render_window(&mut self, window_id: WindowId) {
        // Gather window data we need (avoiding borrow conflicts with self).
        let win_data = match self.window_ref(window_id) {
            Some(win) if win.visible && !win.minimized => {
                (
                    win.x,
                    win.y,
                    win.width,
                    win.height,
                    win.opacity,
                    win.focused,
                    win.title.clone(),
                    win.render_tree.commands.clone(),
                )
            }
            _ => return,
        };

        let (win_x, win_y, win_width, win_height, opacity, focused, title, commands) = win_data;

        // 1. Draw window shadow.
        self.render_shadow(win_x, win_y, win_width, win_height, opacity);

        // 2. Draw window border.
        let border_color = if focused {
            self.theme.border_focused
        } else {
            self.theme.border_unfocused
        };
        self.render_border(win_x, win_y, win_width, win_height, border_color, opacity);

        // 3. Draw title bar.
        self.render_title_bar(
            win_x, win_y, win_width, focused, &title, opacity,
        );

        // 4. Fill client area background (white).
        self.render_engine.fill_rect(
            &mut self.framebuffer,
            win_x,
            win_y,
            win_width,
            win_height,
            0xFF_FF_FF_FF,
            opacity,
        );

        // 5. Execute client render commands.
        self.render_engine.execute(
            &mut self.framebuffer,
            &commands,
            win_x,
            win_y,
            win_width,
            win_height,
            opacity,
        );

        // Mark window as no longer dirty.
        if let Some(win) = self.window_mut(window_id) {
            win.dirty = false;
        }
    }

    /// Render the window shadow.
    fn render_shadow(&mut self, x: i32, y: i32, width: u32, height: u32, opacity: f32) {
        let shadow_offset = 3_i32;
        let total_width = width + (BORDER_WIDTH * 2);
        let total_height = height + TITLE_BAR_HEIGHT + BORDER_WIDTH;

        // Draw shadow layers (progressively more transparent).
        for layer in 0..SHADOW_SIZE {
            let alpha = (40u32.saturating_sub(layer * 5)).min(255);
            let shadow_color = alpha << 24;
            let expand = layer as i32;

            let sx = x - BORDER_WIDTH as i32 - expand + shadow_offset;
            let sy = y - TITLE_BAR_HEIGHT as i32 - expand + shadow_offset;
            let sw = total_width + (expand as u32 * 2);
            let sh = total_height + (expand as u32 * 2);

            // Draw only the outline of each shadow layer for performance.
            self.render_engine
                .stroke_rect(&mut self.framebuffer, sx, sy, sw, sh, 1, shadow_color, opacity);
        }
    }

    /// Render the window border.
    fn render_border(
        &mut self,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        color: u32,
        opacity: f32,
    ) {
        let border_x = x - BORDER_WIDTH as i32;
        let border_y = y - TITLE_BAR_HEIGHT as i32 - BORDER_WIDTH as i32;
        let border_w = width + (BORDER_WIDTH * 2);
        let border_h = height + TITLE_BAR_HEIGHT + (BORDER_WIDTH * 2);

        self.render_engine.stroke_rect(
            &mut self.framebuffer,
            border_x,
            border_y,
            border_w,
            border_h,
            BORDER_WIDTH,
            color,
            opacity,
        );
    }

    /// Render the title bar with title text and buttons.
    fn render_title_bar(
        &mut self,
        win_x: i32,
        win_y: i32,
        win_width: u32,
        focused: bool,
        title: &str,
        opacity: f32,
    ) {
        let tb_x = win_x - BORDER_WIDTH as i32;
        let tb_y = win_y - TITLE_BAR_HEIGHT as i32;
        let tb_width = win_width + (BORDER_WIDTH * 2);

        // Title bar background.
        let bg_color = if focused {
            self.theme.title_bar_focused
        } else {
            self.theme.title_bar_unfocused
        };
        self.render_engine.fill_rect(
            &mut self.framebuffer,
            tb_x,
            tb_y,
            tb_width,
            TITLE_BAR_HEIGHT,
            bg_color,
            opacity,
        );

        // Title text.
        let text_color = if focused {
            self.theme.title_text_focused
        } else {
            self.theme.title_text_unfocused
        };
        let text_x = tb_x + 8;
        let text_y = tb_y + (TITLE_BAR_HEIGHT as i32 - CHAR_HEIGHT as i32) / 2;
        let max_text_width = tb_width.saturating_sub(
            (TITLE_BUTTON_SIZE + TITLE_BUTTON_SPACING) * 3 + 16,
        );
        self.render_engine.draw_text(
            &mut self.framebuffer,
            text_x,
            text_y,
            title,
            text_color,
            opacity,
            Some(max_text_width),
        );

        // Close button (red circle/square).
        let close_x = tb_x + tb_width as i32
            - TITLE_BUTTON_SIZE as i32
            - TITLE_BUTTON_SPACING as i32;
        let close_y = tb_y + (TITLE_BAR_HEIGHT as i32 - TITLE_BUTTON_SIZE as i32) / 2;
        self.render_engine.fill_rect(
            &mut self.framebuffer,
            close_x,
            close_y,
            TITLE_BUTTON_SIZE,
            TITLE_BUTTON_SIZE,
            self.theme.close_button,
            opacity,
        );

        // Maximize button (green).
        let max_x = close_x - TITLE_BUTTON_SIZE as i32 - TITLE_BUTTON_SPACING as i32;
        self.render_engine.fill_rect(
            &mut self.framebuffer,
            max_x,
            close_y,
            TITLE_BUTTON_SIZE,
            TITLE_BUTTON_SIZE,
            self.theme.maximize_button,
            opacity,
        );

        // Minimize button (yellow).
        let min_x = max_x - TITLE_BUTTON_SIZE as i32 - TITLE_BUTTON_SPACING as i32;
        self.render_engine.fill_rect(
            &mut self.framebuffer,
            min_x,
            close_y,
            TITLE_BUTTON_SIZE,
            TITLE_BUTTON_SIZE,
            self.theme.minimize_button,
            opacity,
        );
    }

    // -----------------------------------------------------------------------
    // Protocol handling (stub IPC)
    // -----------------------------------------------------------------------

    /// Handle a compositor request from a client.
    pub fn handle_request(&mut self, request: CompositorRequest) -> CompositorResponse {
        match request {
            CompositorRequest::CreateWindow {
                title,
                width,
                height,
                client_pid,
            } => {
                let id = self.create_window(title, width, height, client_pid);
                CompositorResponse::WindowCreated { window_id: id }
            }
            CompositorRequest::DestroyWindow { window_id } => {
                match self.destroy_window(window_id) {
                    Ok(()) => CompositorResponse::Ok,
                    Err(e) => CompositorResponse::Error {
                        message: e.to_string(),
                    },
                }
            }
            CompositorRequest::SetTitle { window_id, title } => {
                match self.set_title(window_id, title) {
                    Ok(()) => CompositorResponse::Ok,
                    Err(e) => CompositorResponse::Error {
                        message: e.to_string(),
                    },
                }
            }
            CompositorRequest::Submit {
                window_id,
                commands,
            } => match self.submit_render(window_id, commands) {
                Ok(()) => CompositorResponse::Ok,
                Err(e) => CompositorResponse::Error {
                    message: e.to_string(),
                },
            },
            CompositorRequest::Move { window_id, x, y } => {
                match self.move_window(window_id, x, y) {
                    Ok(()) => CompositorResponse::Ok,
                    Err(e) => CompositorResponse::Error {
                        message: e.to_string(),
                    },
                }
            }
            CompositorRequest::Resize {
                window_id,
                width,
                height,
            } => match self.resize_window(window_id, width, height) {
                Ok(()) => CompositorResponse::Ok,
                Err(e) => CompositorResponse::Error {
                    message: e.to_string(),
                },
            },
            CompositorRequest::Minimize { window_id } => {
                match self.minimize_window(window_id) {
                    Ok(()) => CompositorResponse::Ok,
                    Err(e) => CompositorResponse::Error {
                        message: e.to_string(),
                    },
                }
            }
            CompositorRequest::Maximize { window_id } => {
                match self.maximize_window(window_id) {
                    Ok(()) => CompositorResponse::Ok,
                    Err(e) => CompositorResponse::Error {
                        message: e.to_string(),
                    },
                }
            }
            CompositorRequest::Restore { window_id } => {
                match self.restore_window(window_id) {
                    Ok(()) => CompositorResponse::Ok,
                    Err(e) => CompositorResponse::Error {
                        message: e.to_string(),
                    },
                }
            }
            CompositorRequest::SetCursor { cursor, .. } => {
                self.cursor_shape = cursor;
                CompositorResponse::Ok
            }
            CompositorRequest::SetOpacity { window_id, opacity } => {
                match self.set_opacity(window_id, opacity) {
                    Ok(()) => CompositorResponse::Ok,
                    Err(e) => CompositorResponse::Error {
                        message: e.to_string(),
                    },
                }
            }
            CompositorRequest::GetDisplayInfo => {
                if let Some(display) = self.display_manager.primary() {
                    CompositorResponse::DisplayInfo {
                        width: display.width,
                        height: display.height,
                        refresh_rate: display.refresh_rate,
                        scale_factor: display.scale_factor,
                    }
                } else {
                    CompositorResponse::Error {
                        message: "no primary display".to_string(),
                    }
                }
            }
        }
    }

    /// Drain pending event notifications (would be sent to clients via IPC).
    pub fn drain_notifications(&mut self) -> Vec<EventNotification> {
        self.pending_notifications.drain(..).collect()
    }

    // -----------------------------------------------------------------------
    // Display management
    // -----------------------------------------------------------------------

    /// Handle a display resolution change.
    pub fn resize_display(&mut self, width: u32, height: u32) -> CompositorResult<()> {
        self.framebuffer.resize(width, height)?;

        // Update the primary display.
        if let Some(display) = self.display_manager.displays.first_mut() {
            display.width = width;
            display.height = height;
        }

        self.full_recomposite = true;
        self.damage.mark_full(width, height);
        Ok(())
    }

    /// Get the display manager.
    pub fn display_manager(&self) -> &DisplayManager {
        &self.display_manager
    }

    /// Get frame statistics.
    pub fn frame_stats(&self) -> &FrameStats {
        &self.frame_stats
    }

    /// Get the current cursor shape.
    pub fn cursor_shape(&self) -> CursorShape {
        self.cursor_shape
    }

    /// Get a reference to the front buffer for display output.
    pub fn front_buffer(&self) -> &[u32] {
        self.framebuffer.front_buffer()
    }

    /// Get the number of managed windows.
    pub fn window_count(&self) -> usize {
        self.windows.len()
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Find the index of a window by ID.
    fn window_index(&self, id: WindowId) -> Option<usize> {
        self.windows.iter().position(|w| w.id == id)
    }

    /// Get a reference to a window by ID.
    fn window_ref(&self, id: WindowId) -> Option<&Window> {
        self.windows.iter().find(|w| w.id == id)
    }

    /// Get a mutable reference to a window by ID.
    fn window_mut(&mut self, id: WindowId) -> Option<&mut Window> {
        self.windows.iter_mut().find(|w| w.id == id)
    }

    /// Update z_order fields on all windows based on their position in z_stack.
    fn update_z_orders(&mut self) {
        let z_stack_copy = self.z_stack.clone();
        for (i, &id) in z_stack_copy.iter().enumerate() {
            if let Some(win) = self.windows.iter_mut().find(|w| w.id == id) {
                win.z_order = i as u32;
            }
        }
    }

    /// Focus the topmost visible window.
    fn focus_topmost_visible(&mut self) {
        let topmost = self
            .z_stack
            .iter()
            .rev()
            .copied()
            .find(|&id| {
                self.window_ref(id)
                    .is_some_and(|w| w.visible && !w.minimized)
            });

        if let Some(id) = topmost {
            self.focus_window(id);
        }
    }

    /// Mark the area occupied by a window (including decorations) as damaged.
    fn damage_window(&mut self, window_id: WindowId) {
        if let Some(win) = self.window_ref(window_id) {
            let outer = win.outer_rect();
            self.damage.add(outer);
        }
    }
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

fn main() {
    // Initialize the compositor with a default 1920x1080 display at 60 Hz.
    let mut compositor = match Compositor::new(1920, 1080, 60) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("compositor: failed to initialize: {}", e);
            std::process::exit(1);
        }
    };

    eprintln!("compositor: initialized (1920x1080 @ 60Hz)");
    eprintln!("compositor: waiting for client connections...");

    // Main compositor event loop.
    // In production, this would:
    // 1. Poll for IPC messages (client requests) via SlateOS channels
    // 2. Poll for input events from the input driver
    // 3. Compose frames at the display refresh rate
    // 4. Send event notifications back to clients
    //
    // For now, we demonstrate the API with a simple test scenario.
    let window_id = compositor.create_window(
        "Welcome to Slate OS".to_string(),
        640,
        480,
        1,
    );

    // Submit some test render commands.
    let mut tree = RenderTree::new();
    tree.fill_rect(10.0, 10.0, 200.0, 40.0, Color::BLUE);
    tree.text(20.0, 20.0, "Hello from Slate OS Compositor!", Color::WHITE, 14.0);
    tree.fill_rect(10.0, 60.0, 620.0, 1.0, Color::LIGHT_GRAY);

    if let Err(e) = compositor.submit_render(window_id, tree.commands) {
        eprintln!("compositor: failed to submit render: {}", e);
    }

    // Compose and display the initial frame.
    compositor.compose_frame();

    eprintln!(
        "compositor: frame composited ({}us, {} windows)",
        compositor.frame_stats().last_frame_time_us,
        compositor.window_count(),
    );

    // In production, we would enter the real event loop here. For now, the
    // compositor has demonstrated its full API is operational.
    // The event loop stub:
    loop {
        // 1. Receive input events (stub: none available yet).
        // 2. Process any IPC requests from clients.
        // 3. Compose frame if needed.
        let composed = compositor.compose_frame();
        if composed {
            // Present the frame to display hardware.
            let _buffer = compositor.front_buffer();
            // In production: write buffer to framebuffer device or DRM plane.
        }

        // 4. Drain and send notifications to clients.
        let notifications = compositor.drain_notifications();
        for _notification in &notifications {
            // In production: send via IPC channel to the owning client.
        }

        // 5. Sleep until next frame or input event.
        // In production: use an event-driven approach (poll/epoll equivalent).
        std::thread::sleep(Duration::from_millis(16));
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_id_uniqueness() {
        let id1 = WindowId::allocate();
        let id2 = WindowId::allocate();
        let id3 = WindowId::allocate();
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_rect_contains() {
        let rect = Rect::new(10, 20, 100, 50);
        assert!(rect.contains(10, 20));
        assert!(rect.contains(50, 40));
        assert!(rect.contains(109, 69));
        assert!(!rect.contains(110, 70));
        assert!(!rect.contains(9, 20));
        assert!(!rect.contains(10, 19));
    }

    #[test]
    fn test_rect_intersect() {
        let a = Rect::new(0, 0, 100, 100);
        let b = Rect::new(50, 50, 100, 100);
        let intersection = a.intersect(&b);
        assert!(intersection.is_some());
        let i = intersection.unwrap();
        assert_eq!(i.x, 50);
        assert_eq!(i.y, 50);
        assert_eq!(i.width, 50);
        assert_eq!(i.height, 50);

        // Non-overlapping
        let c = Rect::new(200, 200, 50, 50);
        assert!(a.intersect(&c).is_none());
    }

    #[test]
    fn test_rect_union() {
        let a = Rect::new(10, 10, 50, 50);
        let b = Rect::new(40, 40, 80, 80);
        let u = a.union(&b);
        assert_eq!(u.x, 10);
        assert_eq!(u.y, 10);
        assert_eq!(u.width, 110);
        assert_eq!(u.height, 110);
    }

    #[test]
    fn test_framebuffer_creation() {
        let fb = Framebuffer::new(800, 600);
        assert!(fb.is_ok());
        let fb = fb.unwrap();
        assert_eq!(fb.width, 800);
        assert_eq!(fb.height, 600);
    }

    #[test]
    fn test_framebuffer_invalid_dimensions() {
        assert!(Framebuffer::new(0, 100).is_err());
        assert!(Framebuffer::new(100, 0).is_err());
        assert!(Framebuffer::new(MAX_FB_WIDTH + 1, 100).is_err());
    }

    #[test]
    fn test_framebuffer_pixel_ops() {
        let mut fb = Framebuffer::new(100, 100).unwrap();
        fb.set_pixel(50, 50, 0xFF_FF_00_00);
        assert_eq!(fb.get_pixel(50, 50), Some(0xFF_FF_00_00));
        assert_eq!(fb.get_pixel(200, 200), None); // Out of bounds
    }

    #[test]
    fn test_framebuffer_swap() {
        let mut fb = Framebuffer::new(10, 10).unwrap();
        fb.set_pixel(0, 0, 0xFF_11_22_33);
        fb.swap();
        // After swap, front buffer should have the pixel.
        assert_eq!(fb.front_buffer()[0], 0xFF_11_22_33);
        // Back buffer should be the old front (initial black).
        assert_eq!(fb.get_pixel(0, 0), Some(0xFF_00_00_00));
    }

    #[test]
    fn test_compositor_creation() {
        let comp = Compositor::new(1920, 1080, 60);
        assert!(comp.is_ok());
    }

    #[test]
    fn test_create_and_destroy_window() {
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        let id = comp.create_window("Test".to_string(), 400, 300, 42);
        assert_eq!(comp.window_count(), 1);
        assert_eq!(comp.focused_window, Some(id));

        assert!(comp.destroy_window(id).is_ok());
        assert_eq!(comp.window_count(), 0);
        assert_eq!(comp.focused_window, None);
    }

    #[test]
    fn test_window_focus() {
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        let id1 = comp.create_window("Win1".to_string(), 300, 200, 1);
        let id2 = comp.create_window("Win2".to_string(), 300, 200, 2);

        // Second window should be focused.
        assert_eq!(comp.focused_window, Some(id2));

        // Focus the first window.
        comp.focus_window(id1);
        assert_eq!(comp.focused_window, Some(id1));

        // First window should be on top of z-stack.
        assert_eq!(comp.z_stack.last(), Some(&id1));
    }

    #[test]
    fn test_window_minimize_restore() {
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        let id = comp.create_window("Test".to_string(), 400, 300, 1);

        assert!(comp.minimize_window(id).is_ok());
        let win = comp.window_ref(id).unwrap();
        assert!(win.minimized);
        assert!(!win.visible);

        assert!(comp.restore_window(id).is_ok());
        let win = comp.window_ref(id).unwrap();
        assert!(!win.minimized);
        assert!(win.visible);
    }

    #[test]
    fn test_window_maximize_restore() {
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        let id = comp.create_window("Test".to_string(), 400, 300, 1);

        let original_width = comp.window_ref(id).unwrap().width;
        assert!(comp.maximize_window(id).is_ok());

        let win = comp.window_ref(id).unwrap();
        assert!(win.maximized);
        assert!(win.width > original_width);

        assert!(comp.restore_window(id).is_ok());
        let win = comp.window_ref(id).unwrap();
        assert!(!win.maximized);
        assert_eq!(win.width, original_width);
    }

    #[test]
    fn test_damage_region() {
        let mut damage = DamageRegion::new();
        assert!(!damage.has_damage());

        damage.add(Rect::new(10, 10, 50, 50));
        assert!(damage.has_damage());
        assert_eq!(damage.rects().len(), 1);

        // Add non-overlapping rect.
        damage.add(Rect::new(200, 200, 30, 30));
        assert_eq!(damage.rects().len(), 2);

        damage.clear();
        assert!(!damage.has_damage());
    }

    #[test]
    fn test_damage_region_merge() {
        let mut damage = DamageRegion::new();
        damage.add(Rect::new(10, 10, 50, 50));
        // Overlapping rect should merge.
        damage.add(Rect::new(30, 30, 50, 50));
        assert_eq!(damage.rects().len(), 1);
        // Merged rect should be the union.
        let merged = &damage.rects()[0];
        assert_eq!(merged.x, 10);
        assert_eq!(merged.y, 10);
        assert_eq!(merged.width, 70);
        assert_eq!(merged.height, 70);
    }

    #[test]
    fn test_compositor_request_response() {
        let mut comp = Compositor::new(800, 600, 60).unwrap();

        let resp = comp.handle_request(CompositorRequest::CreateWindow {
            title: "Protocol Test".to_string(),
            width: 320,
            height: 240,
            client_pid: 99,
        });
        let window_id = match resp {
            CompositorResponse::WindowCreated { window_id } => window_id,
            _ => panic!("expected WindowCreated response"),
        };

        // Set title.
        let resp = comp.handle_request(CompositorRequest::SetTitle {
            window_id,
            title: "Renamed".to_string(),
        });
        assert!(matches!(resp, CompositorResponse::Ok));
        assert_eq!(comp.window_ref(window_id).unwrap().title, "Renamed");

        // Get display info.
        let resp = comp.handle_request(CompositorRequest::GetDisplayInfo);
        match resp {
            CompositorResponse::DisplayInfo {
                width,
                height,
                refresh_rate,
                ..
            } => {
                assert_eq!(width, 800);
                assert_eq!(height, 600);
                assert_eq!(refresh_rate, 60);
            }
            _ => panic!("expected DisplayInfo response"),
        }
    }

    #[test]
    fn test_hit_testing() {
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        let id1 = comp.create_window("Win1".to_string(), 200, 150, 1);
        let _id2 = comp.create_window("Win2".to_string(), 200, 150, 2);

        // Windows are placed at offset positions. The first window is at (100, 80).
        let win1 = comp.window_ref(id1).unwrap();
        let center_x = win1.x + win1.width as i32 / 2;
        let center_y = win1.y + win1.height as i32 / 2;

        // Win1 is below Win2 in z-order, but if we click in its non-overlapping area,
        // we should hit it.
        let hit = comp.window_at(center_x, center_y);
        // This could be id1 or id2 depending on overlap. Just verify we get something.
        assert!(hit.is_some());
    }

    #[test]
    fn test_compose_frame() {
        let mut comp = Compositor::new(320, 240, 60).unwrap();
        comp.create_window("Test".to_string(), 200, 150, 1);

        // First compose should work (full_recomposite is set).
        assert!(comp.compose_frame());

        // Second compose without damage should not compose.
        // (Need to wait for frame interval, but in tests frame_stats starts fresh.)
        // Force frame timing to allow immediate recompose.
        comp.frame_stats.last_frame_start = None;
        assert!(!comp.compose_frame()); // No damage.
    }

    #[test]
    fn test_input_routing_keyboard() {
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        let id = comp.create_window("Focused".to_string(), 400, 300, 1);

        comp.handle_input(InputEvent::KeyDown {
            scancode: 30,
            character: Some('a'),
        });

        let notifications = comp.drain_notifications();
        // Should have focus notification from create + key event.
        let key_event = notifications.iter().find(|n| {
            matches!(
                n,
                EventNotification::KeyEvent {
                    window_id,
                    scancode: 30,
                    pressed: true,
                    ..
                } if *window_id == id
            )
        });
        assert!(key_event.is_some());
    }

    #[test]
    fn test_display_resize() {
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        assert!(comp.resize_display(1920, 1080).is_ok());
        assert_eq!(comp.framebuffer.width, 1920);
        assert_eq!(comp.framebuffer.height, 1080);
        assert!(comp.full_recomposite);
    }

    #[test]
    fn test_color_to_argb() {
        let color = Color::rgba(255, 128, 64, 200);
        let argb = color_to_argb(&color);
        assert_eq!((argb >> 24) & 0xFF, 200);
        assert_eq!((argb >> 16) & 0xFF, 255);
        assert_eq!((argb >> 8) & 0xFF, 128);
        assert_eq!(argb & 0xFF, 64);
    }

    #[test]
    fn test_window_rects() {
        let win = Window::new("Test".to_string(), 100, 100, 400, 300, 1);
        let client = win.client_rect();
        assert_eq!(client, Rect::new(100, 100, 400, 300));

        let title_bar = win.title_bar_rect();
        assert_eq!(title_bar.y, 100 - TITLE_BAR_HEIGHT as i32);
        assert_eq!(title_bar.height, TITLE_BAR_HEIGHT);

        let close = win.close_button_rect();
        // Close button should be within the title bar.
        assert!(title_bar.contains(close.x, close.y));
    }

    #[test]
    fn test_frame_stats() {
        let mut stats = FrameStats::new(Duration::from_millis(16));
        assert!(stats.should_compose());

        stats.begin_frame();
        let within_budget = stats.end_frame();
        assert!(within_budget);
        assert_eq!(stats.frames_composited, 1);
        assert_eq!(stats.dropped_frames, 0);
    }

    #[test]
    fn test_display_manager() {
        let dm = DisplayManager::new(1920, 1080, 144);
        assert_eq!(dm.displays().len(), 1);
        assert_eq!(dm.primary_refresh_rate(), 144);

        let bounds = dm.virtual_bounds();
        assert_eq!(bounds.width, 1920);
        assert_eq!(bounds.height, 1080);
    }

    #[test]
    fn test_multi_monitor() {
        let mut dm = DisplayManager::new(1920, 1080, 60);
        dm.add_display(Display::new(1, 2560, 1440, 144, 1.5, false));

        assert_eq!(dm.displays().len(), 2);
        let bounds = dm.virtual_bounds();
        // Second display should be placed to the right.
        assert_eq!(bounds.width, 1920 + 2560);
        assert_eq!(bounds.height, 1440); // Max height
    }

    #[test]
    fn test_opacity() {
        let mut comp = Compositor::new(400, 300, 60).unwrap();
        let id = comp.create_window("Ghost".to_string(), 200, 150, 1);

        assert!(comp.set_opacity(id, 0.5).is_ok());
        assert_eq!(comp.window_ref(id).unwrap().opacity, 0.5);

        // Clamp to valid range.
        assert!(comp.set_opacity(id, 2.0).is_ok());
        assert_eq!(comp.window_ref(id).unwrap().opacity, 1.0);

        assert!(comp.set_opacity(id, -1.0).is_ok());
        assert_eq!(comp.window_ref(id).unwrap().opacity, 0.0);
    }

    #[test]
    fn test_window_not_found_error() {
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        let fake_id = WindowId(99999);

        assert!(comp.destroy_window(fake_id).is_err());
        assert!(comp.move_window(fake_id, 0, 0).is_err());
        assert!(comp.resize_window(fake_id, 100, 100).is_err());
    }

    #[test]
    fn test_render_commands_execution() {
        let mut comp = Compositor::new(400, 300, 60).unwrap();
        let id = comp.create_window("Render".to_string(), 200, 150, 1);

        let commands = vec![
            RenderCommand::FillRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 50.0,
                color: Color::RED,
                corner_radii: CornerRadii::ZERO,
            },
            RenderCommand::Text {
                x: 10.0,
                y: 10.0,
                text: "Test".to_string(),
                color: Color::WHITE,
                font_size: 14.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            },
            RenderCommand::Line {
                x1: 0.0,
                y1: 50.0,
                x2: 200.0,
                y2: 50.0,
                color: Color::BLACK,
                width: 1.0,
            },
        ];

        assert!(comp.submit_render(id, commands).is_ok());
        // Compose should succeed with the submitted content.
        assert!(comp.compose_frame());
    }

    #[test]
    fn test_clip_stack() {
        let mut stack = ClipStack::default();
        stack.push(Rect::new(0, 0, 100, 100));
        assert_eq!(stack.current(), Some(&Rect::new(0, 0, 100, 100)));

        // Pushing a smaller clip should intersect.
        stack.push(Rect::new(50, 50, 100, 100));
        assert_eq!(stack.current(), Some(&Rect::new(50, 50, 50, 50)));

        stack.pop();
        assert_eq!(stack.current(), Some(&Rect::new(0, 0, 100, 100)));

        stack.pop();
        assert_eq!(stack.current(), None);
    }

    #[test]
    fn test_translate_stack() {
        let mut stack = TranslateStack::default();
        stack.push(10.0, 20.0);
        assert_eq!(stack.offset(), (10.0, 20.0));

        stack.push(5.0, 3.0);
        assert_eq!(stack.offset(), (15.0, 23.0));

        stack.pop();
        assert_eq!(stack.offset(), (10.0, 20.0));

        stack.pop();
        assert_eq!(stack.offset(), (0.0, 0.0));
    }
}
