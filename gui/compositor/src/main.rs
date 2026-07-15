//! Slate OS Compositor — Display Server
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
//! - Stub IPC layer ready for real Slate OS channel-based IPC when available.

// Drawing primitives (fill_rect, stroke_rect, draw_text, draw_line) and the
// renderer execute() pump take 8-9 args (framebuffer + geometry + color +
// optional clip / font / weight / stroke-width). Grouping into a struct
// would help marginally but obscures the per-call clarity at the call site
// — every primitive needs every arg.
#![allow(clippy::too_many_arguments)]

use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

mod buffer;
pub use buffer::{BufferFormat, SharedBuffer};
// Remote draw-command streaming uses the shared `guiremote` crate's scene
// protocol (multi-window deltas built on its single-window RenderCommand wire
// codec), rather than a compositor-local duplicate.
use guiremote::scene::{SceneFrame, SceneSession, WindowSnapshot};

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
    /// A client supplied an invalid shared buffer (bad geometry/stride/size).
    InvalidBuffer(String),
    /// A client shared buffer exceeds the supported pixel cap.
    BufferTooLarge { width: u32, height: u32 },
    /// The referenced remote stream session id is not active.
    StreamNotFound(u64),
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
            Self::InvalidBuffer(msg) => write!(f, "invalid shared buffer: {}", msg),
            Self::BufferTooLarge { width, height } => write!(
                f,
                "shared buffer too large: {}x{} (max {}x{})",
                width, height, MAX_FB_WIDTH, MAX_FB_HEIGHT
            ),
            Self::StreamNotFound(id) => write!(f, "stream session not found: {}", id),
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
    /// An attached shared pixel buffer (DMA-BUF path). When `Some`, the
    /// compositor blits these pixels into the client area instead of replaying
    /// `render_tree`; the client renders directly into shared memory.
    pub buffer: Option<SharedBuffer>,
    /// Whether the window is in true fullscreen mode: it owns the entire
    /// display with no decorations. Distinct from `maximized` (which keeps the
    /// title bar/borders and respects panel reservations). Fullscreen is the
    /// state that enables direct-scanout bypass for games/video.
    pub fullscreen: bool,
    /// Geometry saved before entering fullscreen (for restore). Kept separate
    /// from `restore_rect` so fullscreen and maximize don't clobber each other.
    pub fs_restore_rect: Option<Rect>,
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
            buffer: None,
            fullscreen: false,
            fs_restore_rect: None,
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
    ///
    /// OPT (BENCH-COMPOSITOR-SLOW): a full 4K clear writes ~33 MB, enough that a
    /// single core does not saturate DRAM write bandwidth. Above
    /// [`PARALLEL_FILL_THRESHOLD_PX`] the fill is split into disjoint row-bands
    /// filled on scoped worker threads (`split_at_mut` guarantees each thread
    /// owns a non-overlapping `&mut [u32]`, so this is safe with no `unsafe`).
    /// Below the threshold, or when the platform reports no usable parallelism
    /// (e.g. a target whose std has not implemented `available_parallelism`), it
    /// falls back to a single-threaded `[u32]::fill` — so this never pessimizes
    /// small buffers or single-core targets.
    pub fn clear(&mut self, color: u32) {
        let workers = Self::fill_worker_count(self.back.len());
        if workers <= 1 {
            self.back.fill(color);
            return;
        }
        let band_stride = self.back.len().div_ceil(workers);
        std::thread::scope(|s| {
            for chunk in self.back.chunks_mut(band_stride) {
                s.spawn(move || chunk.fill(color));
            }
        });
    }

    /// Number of worker threads to use for a solid fill covering `px` pixels.
    ///
    /// Returns 1 (single-threaded) for small fills or when the platform cannot
    /// report parallelism, so callers can use the result to decide whether to
    /// spawn threads at all. Capped at 8 to bound per-frame thread-spawn cost.
    fn fill_worker_count(px: usize) -> usize {
        // ~1M px (e.g. > 1024×1024). Below this the thread-spawn overhead is not
        // worth it; the per-frame spawn cost would dominate the fill savings.
        const PARALLEL_FILL_THRESHOLD_PX: usize = 1 << 20;
        if px < PARALLEL_FILL_THRESHOLD_PX {
            return 1;
        }
        std::thread::available_parallelism()
            .map(std::num::NonZeroUsize::get)
            .unwrap_or(1)
            .min(8)
    }

    /// Fill `buf` — which holds `band_rows` contiguous scanlines of `width`
    /// pixels each, the first of which is at absolute framebuffer row `y0` — with
    /// `color`, skipping the horizontal spans covered by any `covered` rect.
    ///
    /// Shared by the single-threaded and parallel [`clear_except`] paths so the
    /// per-scanline span-merging logic lives in exactly one place. `covered`
    /// rects are given in absolute framebuffer coordinates; the vertical overlap
    /// test uses the absolute row `y0 + r`, and writes target the band-local row
    /// offset `r * width`.
    fn fill_uncovered_band(
        buf: &mut [u32],
        y0: u32,
        band_rows: u32,
        width: u32,
        color: u32,
        covered: &[Rect],
        fb_height: u32,
    ) {
        let width_usize = width as usize;
        // Reused across scanlines so this allocates once, not per row.
        let mut spans: Vec<(u32, u32)> = Vec::with_capacity(covered.len());
        for r in 0..band_rows {
            let abs_y = y0.saturating_add(r);
            spans.clear();
            for rect in covered {
                let ry0 = rect.y.max(0) as u32;
                let ry1 = (rect.y.saturating_add(rect.height as i32).max(0) as u32).min(fb_height);
                if abs_y >= ry0 && abs_y < ry1 {
                    let x0 = rect.x.max(0) as u32;
                    let x1 = (rect.x.saturating_add(rect.width as i32).max(0) as u32).min(width);
                    if x1 > x0 {
                        spans.push((x0, x1));
                    }
                }
            }
            let row_base = r as usize * width_usize;
            if spans.is_empty() {
                if let Some(s) = buf.get_mut(row_base..row_base + width_usize) {
                    s.fill(color);
                }
                continue;
            }
            // Sort covered spans by start, then fill the complementary gaps.
            spans.sort_unstable_by_key(|&(a, _)| a);
            let mut cursor = 0u32;
            for &(a, b) in &spans {
                if a > cursor {
                    let lo = row_base + cursor as usize;
                    let hi = row_base + a as usize;
                    if let Some(s) = buf.get_mut(lo..hi) {
                        s.fill(color);
                    }
                }
                cursor = cursor.max(b);
            }
            if cursor < width {
                let lo = row_base + cursor as usize;
                let hi = row_base + width_usize;
                if let Some(s) = buf.get_mut(lo..hi) {
                    s.fill(color);
                }
            }
        }
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

    /// Clear the back buffer to `color`, but skip pixels covered by any rect in
    /// `covered`.
    ///
    /// The caller guarantees every `covered` rect will be fully overwritten by
    /// opaque content later in the frame, so clearing those pixels first is pure
    /// overdraw. Skipping it is bit-identical to a full [`clear`](Self::clear)
    /// followed by the opaque paints, but writes fewer bytes — the dominant cost
    /// on a full 4K recomposite is memory bandwidth (BENCH-COMPOSITOR-SLOW).
    ///
    /// Correctness: because the covered regions are opaque, their final pixels
    /// never depend on what the background clear wrote (opaque paint replaces,
    /// and any translucent window above blends against the opaque pixel, not the
    /// cleared background). Rects are clipped to the framebuffer; overlapping and
    /// unsorted rects are handled by per-scanline span merging.
    ///
    /// OPT (BENCH-COMPOSITOR-SLOW): culls the desktop-background clear under
    /// fully-opaque covering windows. Per-scanline interval math is O(rows ×
    /// covered) which is negligible next to the pixel stores it elides.
    pub fn clear_except(&mut self, color: u32, covered: &[Rect]) {
        if covered.is_empty() {
            // Delegates to the (possibly parallel) full-buffer clear.
            self.clear(color);
            return;
        }
        let width = self.width;
        let height = self.height;
        let workers = Self::fill_worker_count(self.back.len());
        if workers <= 1 {
            Self::fill_uncovered_band(&mut self.back, 0, height, width, color, covered, height);
            return;
        }
        // Partition the scanlines into `workers` disjoint row-bands. Each band is
        // a non-overlapping `&mut [u32]` (via chunks_mut), so the scoped threads
        // never alias — safe parallel fill with no `unsafe`.
        let rows_per_band = height.div_ceil(workers as u32);
        let band_stride = rows_per_band as usize * width as usize;
        std::thread::scope(|s| {
            for (band_idx, chunk) in self.back.chunks_mut(band_stride).enumerate() {
                let y0 = band_idx as u32 * rows_per_band;
                let band_rows = (chunk.len() / width as usize) as u32;
                s.spawn(move || {
                    Self::fill_uncovered_band(chunk, y0, band_rows, width, color, covered, height);
                });
            }
        });
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

    /// Copy an opaque run of source pixels into back-buffer row `y` starting at
    /// column `x`, clipped to the framebuffer.
    ///
    /// This is the fast path for blitting opaque content: it does no per-pixel
    /// blending or bounds checking — the visible sub-run is resolved once and
    /// written with a single `copy_from_slice`. Negative `x` (partially
    /// offscreen on the left) and right-edge overflow are clipped by slicing.
    pub fn copy_row(&mut self, x: i32, y: u32, src: &[u32]) {
        if y >= self.height || src.is_empty() {
            return;
        }
        // Resolve the source offset (when clipped on the left) and destination
        // column, both within bounds.
        let (src_off, dst_x) = if x < 0 {
            let skip = x.unsigned_abs() as usize;
            if skip >= src.len() {
                return;
            }
            (skip, 0usize)
        } else {
            (0usize, x as usize)
        };
        if dst_x >= self.width as usize {
            return;
        }
        let count = (src.len() - src_off).min(self.width as usize - dst_x);
        if count == 0 {
            return;
        }
        let row_off = y as usize * self.width as usize + dst_x;
        if let (Some(dst), Some(s)) = (
            self.back.get_mut(row_off..row_off + count),
            src.get(src_off..src_off + count),
        ) {
            dst.copy_from_slice(s);
        }
    }

    /// Blit the opaque fast path of a client buffer onto the back buffer,
    /// parallelizing across destination row-bands on multicore hosts.
    ///
    /// Copies `rows` source scanlines of `cols` opaque pixels each from `buf`,
    /// placing the top-left source pixel at framebuffer coordinate
    /// (`win_x`, `win_y`). Left-clipping (negative `win_x`), right-edge clipping,
    /// and vertical clipping are byte-identical to running
    /// [`copy_row`](Self::copy_row) per source row (the previous serial path).
    ///
    /// OPT (BENCH-COMPOSITOR-SLOW): a maximized buffer-backed window is the
    /// dominant per-frame blit cost, and the opaque path is pure per-row
    /// `copy_from_slice`s that are independent across destination rows. Above
    /// [`fill_worker_count`](Self::fill_worker_count)'s threshold the framebuffer
    /// is partitioned into disjoint row-bands (`chunks_mut` — non-overlapping
    /// `&mut [u32]`, safe with no `unsafe`) and each band is filled on a scoped
    /// worker. `buf` is shared read-only (`&SharedBuffer` is `Sync`); each worker
    /// only writes rows that fall inside its own band. Below the threshold, or on
    /// single-core targets, it runs single-threaded — no pessimization.
    fn blit_opaque(&mut self, buf: &SharedBuffer, win_x: i32, win_y: i32, cols: u32, rows: u32) {
        let width = self.width;
        let height = self.height;
        // Work proportional to the visible pixel count; reuse the fill heuristic.
        let workers = Self::fill_worker_count(rows as usize * cols as usize);
        if workers <= 1 {
            Self::blit_opaque_band(&mut self.back, 0, height, width, buf, win_x, win_y, cols, rows);
            return;
        }
        let rows_per_band = height.div_ceil(workers as u32);
        let band_stride = rows_per_band as usize * width as usize;
        std::thread::scope(|s| {
            for (band_idx, chunk) in self.back.chunks_mut(band_stride).enumerate() {
                let by0 = band_idx as u32 * rows_per_band;
                let band_rows = (chunk.len() / width as usize) as u32;
                s.spawn(move || {
                    Self::blit_opaque_band(
                        chunk, by0, band_rows, width, buf, win_x, win_y, cols, rows,
                    );
                });
            }
        });
    }

    /// Copy the opaque source rows that land inside one framebuffer row-band.
    ///
    /// `band` holds `band_rows` contiguous scanlines of `fb_width` pixels each,
    /// the first at absolute framebuffer row `by0`. For each source row
    /// `r in 0..rows`, the destination row is `win_y + r`; rows outside
    /// `[by0, by0 + band_rows)` belong to another band and are skipped, so calling
    /// this over a full row-band partition of the framebuffer reproduces the
    /// serial per-row [`copy_row`](Self::copy_row) blit exactly. Horizontal
    /// clipping (left `src_off` when `win_x < 0`, right-edge `min`) mirrors
    /// `copy_row` byte-for-byte.
    #[allow(clippy::too_many_arguments)]
    fn blit_opaque_band(
        band: &mut [u32],
        by0: u32,
        band_rows: u32,
        fb_width: u32,
        buf: &SharedBuffer,
        win_x: i32,
        win_y: i32,
        cols: u32,
        rows: u32,
    ) {
        let width_usize = fb_width as usize;
        let band_end = by0.saturating_add(band_rows);
        for r in 0..rows {
            let sy = win_y.saturating_add(r as i32);
            if sy < 0 {
                continue;
            }
            let sy = sy as u32;
            // Rows past the framebuffer bottom (band_end == fb_height for the last
            // band) and rows owned by other bands are both handled by this range.
            if sy < by0 || sy >= band_end {
                continue;
            }
            let Some(full_src) = buf.row(r) else {
                continue;
            };
            // Matches blit_buffer: clamp the source run to `cols`, but never grow
            // it past the actual row length.
            let src = full_src.get(..cols as usize).unwrap_or(full_src);
            if src.is_empty() {
                continue;
            }
            // Replicate copy_row's clipping exactly for byte-identical output.
            let (src_off, dst_x) = if win_x < 0 {
                let skip = win_x.unsigned_abs() as usize;
                if skip >= src.len() {
                    continue;
                }
                (skip, 0usize)
            } else {
                (0usize, win_x as usize)
            };
            if dst_x >= width_usize {
                continue;
            }
            let count = (src.len() - src_off).min(width_usize - dst_x);
            if count == 0 {
                continue;
            }
            let row_off = (sy - by0) as usize * width_usize + dst_x;
            if let (Some(dst), Some(s)) = (
                band.get_mut(row_off..row_off + count),
                src.get(src_off..src_off + count),
            ) {
                dst.copy_from_slice(s);
            }
        }
    }

    /// Fill a horizontal span of back-buffer row `y`, columns `[x_start, x_end)`,
    /// with a fully-opaque `color` using a single slice `fill()`.
    ///
    /// This is the fast path for solid (alpha 255) rectangle fills: it skips the
    /// per-pixel alpha math and bounds check that `blend_pixel` performs. The
    /// caller must have already clipped `x_start`/`x_end`/`y` to the framebuffer.
    ///
    /// OPT: replaces a per-pixel `blend_pixel` loop with one `[u32]::fill`, which
    /// the compiler lowers to a `memset`-style store. This is the dominant win in
    /// the 4K compositor benchmark (BENCH-COMPOSITOR-SLOW): opaque `FillRect`s
    /// (window backgrounds, decorations) no longer pay per-pixel float alpha cost.
    #[inline]
    fn fill_row_solid(&mut self, y: u32, x_start: u32, x_end: u32, color: u32) {
        if y >= self.height || x_end <= x_start {
            return;
        }
        let x_hi = x_end.min(self.width);
        if x_hi <= x_start {
            return;
        }
        let row_base = y as usize * self.width as usize;
        let lo = row_base + x_start as usize;
        let hi = row_base + x_hi as usize;
        if let Some(span) = self.back.get_mut(lo..hi) {
            span.fill(color | 0xFF_00_00_00);
        }
    }

    /// Blend a horizontal span of back-buffer row `y`, columns `[x_start, x_end)`,
    /// with `src_color` at pre-computed integer alpha `src_a` (0..=255).
    ///
    /// OPT: hoists the alpha computation and per-pixel branch/float conversion out
    /// of the inner loop (versus calling `blend_pixel` per pixel). Only the integer
    /// channel blend runs per pixel. Caller guarantees `0 < src_a < 255`.
    #[inline]
    fn blend_row(&mut self, y: u32, x_start: u32, x_end: u32, src_color: u32, src_a: u32) {
        if y >= self.height || x_end <= x_start {
            return;
        }
        let x_hi = x_end.min(self.width);
        if x_hi <= x_start {
            return;
        }
        let inv_a = 255 - src_a;
        let src_r = (src_color >> 16) & 0xFF;
        let src_g = (src_color >> 8) & 0xFF;
        let src_b = src_color & 0xFF;
        let sr = src_r * src_a;
        let sg = src_g * src_a;
        let sb = src_b * src_a;
        let row_base = y as usize * self.width as usize;
        let lo = row_base + x_start as usize;
        let hi = row_base + x_hi as usize;
        if let Some(span) = self.back.get_mut(lo..hi) {
            for pixel in span {
                let dst = *pixel;
                let dst_r = (dst >> 16) & 0xFF;
                let dst_g = (dst >> 8) & 0xFF;
                let dst_b = dst & 0xFF;
                let out_r = (sr + dst_r * inv_a) / 255;
                let out_g = (sg + dst_g * inv_a) / 255;
                let out_b = (sb + dst_b * inv_a) / 255;
                *pixel = 0xFF_00_00_00 | (out_r << 16) | (out_g << 8) | out_b;
            }
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
    /// Frames presented via fullscreen direct-scanout bypass (no compositing).
    pub bypass_frames: u64,
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
            bypass_frames: 0,
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
    /// Enter or leave fullscreen (enables direct-scanout bypass for games).
    SetFullscreen { window_id: WindowId, enable: bool },
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
    /// Begin a remote draw-command stream session (returns a stream id).
    StreamStart,
    /// Capture the current scene for a stream session as an encoded wire frame.
    StreamCapture { stream_id: u64 },
    /// End a remote draw-command stream session.
    StreamStop { stream_id: u64 },
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
    /// A remote stream session was started.
    StreamStarted { stream_id: u64 },
    /// An encoded draw-command stream frame (see [`stream`] wire format).
    StreamFrame { data: Vec<u8> },
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
        if x_end <= x_start || y_end <= y_start {
            return;
        }

        // Resolve the effective alpha once (color alpha scaled by window opacity)
        // and pick a per-row fast path instead of blending pixel-by-pixel.
        // OPT (BENCH-COMPOSITOR-SLOW): opaque fills become a single slice memset
        // per row; translucent fills hoist the alpha math out of the inner loop.
        let src_a_raw = ((color >> 24) & 0xFF) as f32;
        let src_a = ((src_a_raw * opacity) as u32).min(255);
        if src_a == 0 {
            return;
        }
        if src_a == 255 {
            for row in y_start..y_end {
                fb.fill_row_solid(row, x_start, x_end, color);
            }
        } else {
            for row in y_start..y_end {
                fb.blend_row(row, x_start, x_end, color, src_a);
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

/// How the most recently presented frame was produced.
///
/// In the [`Direct`](Scanout::Direct) case the displayed pixels come straight
/// from a fullscreen client's shared buffer — the compositor never touched the
/// framebuffer for that frame (true zero-copy direct scanout). Otherwise the
/// frame was composited normally into the framebuffer's front buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scanout {
    /// Frame composited into the framebuffer.
    Composited,
    /// Frame scanned out directly from the named window's shared buffer.
    Direct(WindowId),
}

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
    /// How the last presented frame was produced (composited vs direct scanout).
    scanout: Scanout,
    /// Active remote draw-command stream sessions, keyed by stream id. Each
    /// tracks its own per-window delta state so multiple remote viewers can be
    /// served independently.
    stream_sessions: BTreeMap<u64, SceneSession>,
    /// Monotonic allocator for stream session ids.
    next_stream_id: u64,
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
            scanout: Scanout::Composited,
            stream_sessions: BTreeMap::new(),
            next_stream_id: 1,
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

    // -----------------------------------------------------------------------
    // Fullscreen / direct-scanout bypass
    // -----------------------------------------------------------------------

    /// Enter or leave true fullscreen for a window.
    ///
    /// Entering saves the window's geometry, removes decorations, and resizes
    /// the client area to cover the entire display. Leaving restores the saved
    /// geometry. A fullscreen window with an opaque, display-sized shared
    /// buffer is eligible for direct-scanout bypass (see [`compose_frame`]).
    ///
    /// [`compose_frame`]: Compositor::compose_frame
    ///
    /// # Errors
    ///
    /// [`CompositorError::WindowNotFound`] if the window does not exist.
    pub fn set_fullscreen(
        &mut self,
        window_id: WindowId,
        enable: bool,
    ) -> CompositorResult<()> {
        self.damage_window(window_id);

        let fb_w = self.framebuffer.width;
        let fb_h = self.framebuffer.height;

        let resized = {
            let window = self
                .window_mut(window_id)
                .ok_or(CompositorError::WindowNotFound(window_id))?;

            if enable {
                if !window.fullscreen {
                    window.fs_restore_rect =
                        Some(Rect::new(window.x, window.y, window.width, window.height));
                }
                window.fullscreen = true;
                window.x = 0;
                window.y = 0;
                window.width = fb_w;
                window.height = fb_h;
                window.dirty = true;
                Some((fb_w, fb_h))
            } else if window.fullscreen {
                window.fullscreen = false;
                let restored = window.fs_restore_rect.take();
                if let Some(r) = restored {
                    window.x = r.x;
                    window.y = r.y;
                    window.width = r.width;
                    window.height = r.height;
                }
                window.dirty = true;
                restored.map(|r| (r.width, r.height))
            } else {
                None
            }
        };

        self.damage_window(window_id);
        self.full_recomposite = true;

        if let Some((w, h)) = resized {
            self.pending_notifications
                .push_back(EventNotification::WindowResized {
                    window_id,
                    width: w,
                    height: h,
                });
        }

        Ok(())
    }

    /// Whether a window is currently in fullscreen mode.
    #[must_use]
    pub fn is_fullscreen(&self, window_id: WindowId) -> bool {
        self.window_ref(window_id).is_some_and(|w| w.fullscreen)
    }

    /// Determine whether the current frame can bypass compositing via direct
    /// scanout, returning the window whose shared buffer should be scanned out.
    ///
    /// All conditions must hold: the topmost visible window is fullscreen and
    /// fully opaque, covers the entire framebuffer, has an attached shared
    /// buffer whose dimensions exactly match the display, and nothing visible
    /// sits above it in the z-order. When eligible, the compositor presents the
    /// client's buffer pixels directly — no per-frame blit, no occluded windows
    /// drawn. A buffer smaller/larger than the display is rejected (a partial
    /// buffer would leave the rest of the screen stale), preserving correctness.
    fn direct_scanout_window(&self) -> Option<WindowId> {
        // Topmost visible window in z-order (z_stack top == last).
        let &top = self
            .z_stack
            .iter()
            .rev()
            .find(|&&id| self.window_ref(id).is_some_and(|w| w.visible && !w.minimized))?;

        let win = self.window_ref(top)?;
        if !win.fullscreen || win.opacity < 1.0 {
            return None;
        }
        // Must cover the whole framebuffer.
        if win.x > 0 || win.y > 0 {
            return None;
        }
        let covers_w = win
            .x
            .saturating_add(win.width as i32) as i64
            >= self.framebuffer.width as i64;
        let covers_h = win
            .y
            .saturating_add(win.height as i32) as i64
            >= self.framebuffer.height as i64;
        if !covers_w || !covers_h {
            return None;
        }
        // The attached buffer must match the display exactly for a valid,
        // fully-covering scanout.
        let buf = win.buffer.as_ref()?;
        if buf.width() == self.framebuffer.width && buf.height() == self.framebuffer.height {
            Some(top)
        } else {
            None
        }
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
    // Shared-buffer (DMA-BUF) surface path
    // -----------------------------------------------------------------------

    /// Import a client-shared pixel buffer and attach it to a window.
    ///
    /// While a buffer is attached, the compositor blits it directly into the
    /// window's client area each frame instead of replaying the render tree.
    /// The buffer is validated against hostile geometry by
    /// [`SharedBuffer::import`].
    ///
    /// # Errors
    ///
    /// [`CompositorError::WindowNotFound`] if the window is gone, or any error
    /// from [`SharedBuffer::import`] if the client's geometry is invalid.
    pub fn attach_buffer(
        &mut self,
        window_id: WindowId,
        handle: u64,
        width: u32,
        height: u32,
        stride: u32,
        format: BufferFormat,
        bytes: &[u8],
    ) -> CompositorResult<()> {
        // Validate before touching window state so a bad buffer is a no-op.
        let buffer = SharedBuffer::import(handle, width, height, stride, format, bytes)?;
        let window = self
            .window_mut(window_id)
            .ok_or(CompositorError::WindowNotFound(window_id))?;
        window.buffer = Some(buffer);
        window.dirty = true;
        self.damage_window(window_id);
        Ok(())
    }

    /// Detach any shared buffer from a window, reverting to the render-tree
    /// path. Returns the detached buffer's handle if one was attached.
    pub fn detach_buffer(&mut self, window_id: WindowId) -> Option<u64> {
        let handle = {
            let window = self.window_mut(window_id)?;
            let h = window.buffer.take().map(|b| b.handle());
            if h.is_some() {
                window.dirty = true;
            }
            h
        };
        if handle.is_some() {
            self.damage_window(window_id);
        }
        handle
    }

    /// Drain the handles of all buffers the compositor has finished reading
    /// since the last call, clearing their release flags. The IPC layer sends a
    /// `wl_buffer.release`-style notification per handle so clients may reuse
    /// the shared memory.
    pub fn take_released_buffer_handles(&mut self) -> Vec<u64> {
        let mut handles = Vec::new();
        for window in &mut self.windows {
            if let Some(buf) = window.buffer.as_mut()
                && let Some(h) = buf.take_release()
            {
                handles.push(h);
            }
        }
        handles
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

        // Fullscreen direct-scanout bypass: if the topmost window owns the whole
        // display with an opaque, display-sized shared buffer, present its
        // pixels straight from shared memory and skip compositing entirely —
        // no framebuffer clear, no per-pixel blit, no occluded windows drawn,
        // no buffer swap. The presented pixels come directly from the client
        // buffer (see `present_buffer`). This is the path games/video use.
        if let Some(wid) = self.direct_scanout_window() {
            self.scanout = Scanout::Direct(wid);
            // The compositor "consumed" the buffer for this frame; flag it for a
            // wl_buffer.release-style notification so the client may reuse it.
            if let Some(win) = self.windows.iter_mut().find(|w| w.id == wid)
                && let Some(buf) = win.buffer.as_mut()
            {
                buf.mark_released();
            }
            self.full_recomposite = false;
            self.damage.clear();
            self.frame_stats.bypass_frames = self.frame_stats.bypass_frames.saturating_add(1);
            self.frame_stats.end_frame();
            return true;
        }
        self.scanout = Scanout::Composited;

        if self.full_recomposite {
            // Full recomposite: clear and redraw everything.
            self.full_recomposite_into_back();
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

    /// Full recomposite into the back buffer: clear to the desktop
    /// background and redraw every window bottom-to-top, then clear the
    /// pending-recomposite/damage state.
    ///
    /// Shared by [`compose_frame`](Compositor::compose_frame)'s
    /// full-recomposite branch and the benchmark hook
    /// [`bench_full_composite`](Compositor::bench_full_composite) so the two
    /// measure exactly the same work and can never drift. Does NOT swap
    /// buffers — the caller owns presentation.
    fn full_recomposite_into_back(&mut self) {
        // OPT (BENCH-COMPOSITOR-SLOW): don't clear the desktop background under
        // windows that will fully overwrite it with opaque content — that clear
        // is pure overdraw. `clear_except` fills only the uncovered region.
        let covered = self.opaque_cover_rects();
        self.framebuffer
            .clear_except(self.theme.desktop_background, &covered);
        self.render_all_windows();
        self.full_recomposite = false;
        self.damage.clear();
    }

    /// Collect the screen-space rectangles that are guaranteed to be fully
    /// overwritten with opaque content during this recomposite.
    ///
    /// Used by [`full_recomposite_into_back`](Self::full_recomposite_into_back)
    /// to cull the desktop-background clear under opaque windows. Only windows
    /// whose *client area* is provably opaque and fully covered are included:
    ///
    /// - buffer-less windows whose first render command opaquely covers the
    ///   whole client area (same predicate the per-window bg-fill cull uses),
    ///   at full window opacity; and
    /// - buffer-backed windows carrying an opaque buffer at full opacity, over
    ///   the sub-rectangle actually covered by the buffer.
    ///
    /// Decorations (title bar, border, shadow) are deliberately excluded: they
    /// lie outside the client rect and the shadow is translucent, so the
    /// background under them must still be cleared. Being conservative here only
    /// costs a little extra (correct) overdraw, never correctness.
    fn opaque_cover_rects(&self) -> Vec<Rect> {
        let mut rects = Vec::new();
        for win in &self.windows {
            if !win.visible || win.minimized || win.opacity < 1.0 {
                continue;
            }
            if let Some(buf) = win.buffer.as_ref() {
                // Opaque shared buffer: covers min(buffer, client) from the
                // client origin.
                if buf.is_opaque() {
                    let cols = buf.width().min(win.width);
                    let rows = buf.height().min(win.height);
                    if cols > 0 && rows > 0 {
                        rects.push(Rect::new(win.x, win.y, cols, rows));
                    }
                }
            } else if Self::first_command_covers_client(
                &win.render_tree.commands,
                win.width,
                win.height,
                win.opacity,
            ) {
                rects.push(Rect::new(win.x, win.y, win.width, win.height));
            }
        }
        rects
    }

    /// Benchmark/test hook: perform one full recomposite and buffer swap
    /// immediately, bypassing the vsync frame-rate gate that
    /// [`compose_frame`](Compositor::compose_frame) enforces.
    ///
    /// This exists so benchmarks can measure the raw composite cost
    /// deterministically (a tight loop over `compose_frame` would be
    /// throttled by `should_compose` and skip most iterations). It runs the
    /// same `full_recomposite_into_back` + `framebuffer.swap` sequence as the
    /// real full-recomposite path. Production code must call `compose_frame`,
    /// which honors vsync timing and the direct-scanout / partial-damage fast
    /// paths.
    #[doc(hidden)]
    pub fn bench_full_composite(&mut self) {
        self.full_recomposite = true;
        self.full_recomposite_into_back();
        self.framebuffer.swap();
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
    /// True when the client's first render command is an opaque, square-cornered
    /// `FillRect` that fully covers the client area and the window is fully
    /// opaque — meaning the compositor's default white background fill would be
    /// entirely painted over and can be skipped.
    ///
    /// Coordinates in render commands are client-local (origin at the client
    /// top-left), so a covering rect starts at or above/left of (0,0) and extends
    /// at least to `(win_width, win_height)`. Rounded corners are rejected because
    /// they would leave the corner pixels showing the background, and any window
    /// opacity < 1.0 is rejected because the top rect would then blend rather than
    /// fully replace the pixels beneath it.
    fn first_command_covers_client(
        commands: &[RenderCommand],
        win_width: u32,
        win_height: u32,
        opacity: f32,
    ) -> bool {
        if opacity < 1.0 {
            return false;
        }
        match commands.first() {
            Some(RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                color,
                corner_radii,
            }) => {
                color.a == 255
                    && *corner_radii == CornerRadii::ZERO
                    && *x <= 0.0
                    && *y <= 0.0
                    && *x + *width >= win_width as f32
                    && *y + *height >= win_height as f32
            }
            _ => false,
        }
    }

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
                    win.buffer.is_some(),
                    win.fullscreen,
                )
            }
            _ => return,
        };

        let (
            win_x,
            win_y,
            win_width,
            win_height,
            opacity,
            focused,
            title,
            commands,
            has_buffer,
            fullscreen,
        ) = win_data;

        // Fullscreen windows have no decorations — they own the whole display.
        if !fullscreen {
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
            self.render_title_bar(win_x, win_y, win_width, focused, &title, opacity);
        }

        if has_buffer {
            // Shared-buffer (DMA-BUF) path: blit the client's pixels directly.
            // Disjoint field borrows: `windows` for the buffer, `framebuffer`
            // for the destination — distinct fields, so this is sound.
            if let Some(win) = self.windows.iter_mut().find(|w| w.id == window_id)
                && let Some(buf) = win.buffer.as_mut()
            {
                Self::blit_buffer(
                    &mut self.framebuffer,
                    buf,
                    win_x,
                    win_y,
                    win_width,
                    win_height,
                    opacity,
                );
                // The compositor is done reading this buffer for the frame;
                // flag it for a wl_buffer.release-style notification.
                buf.mark_released();
            }
        } else {
            // 4. Fill client area background (white) — UNLESS the client's first
            //    command already paints the whole client area opaquely, in which
            //    case the white fill is 100% overdraw. OPT (BENCH-COMPOSITOR-SLOW):
            //    skipping it removes a full-window fill per such window per frame
            //    (~29% of the 4K-benchmark's opaque stores). Only safe when the
            //    window itself is fully opaque (opacity >= 1.0) — otherwise the
            //    top rect blends and the background would show through.
            if !Self::first_command_covers_client(&commands, win_width, win_height, opacity) {
                self.render_engine.fill_rect(
                    &mut self.framebuffer,
                    win_x,
                    win_y,
                    win_width,
                    win_height,
                    0xFF_FF_FF_FF,
                    opacity,
                );
            }

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
        }

        // Mark window as no longer dirty.
        if let Some(win) = self.window_mut(window_id) {
            win.dirty = false;
        }
    }

    /// Blit an attached shared buffer into a window's client area.
    ///
    /// The buffer is top-left aligned and clipped to the overlap of the buffer
    /// dimensions and the client rectangle; pixels are alpha-blended through
    /// `Framebuffer::blend_pixel` honoring the window opacity, so per-pixel
    /// alpha (ARGB) and translucent windows both compose correctly. All writes
    /// are bounds-checked and offscreen (negative) coordinates are skipped.
    ///
    /// OPT: when the buffer is opaque (Xrgb) and the window is fully opaque, the
    /// per-row content is copied straight into the framebuffer
    /// (`Framebuffer::copy_row`) instead of running a per-pixel float-alpha
    /// blend — O(h) row memcpys vs O(w*h) blends. This is the common
    /// game/video case and the path runs every frame for non-fullscreen
    /// buffer-backed windows (fullscreen ones bypass blitting via direct
    /// scanout). The opaque copy is bit-identical to the blend result because
    /// blend_pixel writes `src | 0xFF000000` for opaque pixels and imported
    /// Xrgb pixels already carry 0xFF alpha.
    fn blit_buffer(
        fb: &mut Framebuffer,
        buf: &SharedBuffer,
        win_x: i32,
        win_y: i32,
        win_width: u32,
        win_height: u32,
        opacity: f32,
    ) {
        let cols = buf.width().min(win_width);
        let rows = buf.height().min(win_height);
        let fast = opacity >= 1.0 && buf.is_opaque();
        if fast {
            // Per-row-independent opaque copies — parallelized across row bands.
            fb.blit_opaque(buf, win_x, win_y, cols, rows);
            return;
        }
        for row in 0..rows {
            let sy = win_y.saturating_add(row as i32);
            if sy < 0 {
                continue;
            }
            for col in 0..cols {
                let sx = win_x.saturating_add(col as i32);
                if sx < 0 {
                    continue;
                }
                if let Some(px) = buf.pixel(col, row) {
                    fb.blend_pixel(sx as u32, sy as u32, px, opacity);
                }
            }
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
            CompositorRequest::SetFullscreen { window_id, enable } => {
                match self.set_fullscreen(window_id, enable) {
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
            CompositorRequest::StreamStart => {
                let stream_id = self.start_stream();
                CompositorResponse::StreamStarted { stream_id }
            }
            CompositorRequest::StreamCapture { stream_id } => {
                match self.capture_stream(stream_id) {
                    Ok(data) => CompositorResponse::StreamFrame { data },
                    Err(e) => CompositorResponse::Error {
                        message: e.to_string(),
                    },
                }
            }
            CompositorRequest::StreamStop { stream_id } => {
                if self.stop_stream(stream_id) {
                    CompositorResponse::Ok
                } else {
                    CompositorResponse::Error {
                        message: CompositorError::StreamNotFound(stream_id).to_string(),
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

    /// Get a reference to the framebuffer's front buffer (the composited
    /// surface). Note: when the last frame was a direct-scanout bypass this is
    /// *stale* — use [`present_pixels`](Compositor::present_pixels) for the
    /// pixels actually being displayed.
    pub fn front_buffer(&self) -> &[u32] {
        self.framebuffer.front_buffer()
    }

    /// Get the pixels actually being presented to the display this frame.
    ///
    /// For a composited frame this is the framebuffer front buffer; for a
    /// fullscreen direct-scanout bypass it is the client's shared-buffer pixels
    /// referenced directly (zero copy). Falls back to the front buffer if the
    /// scanned-out window/buffer vanished between compose and present.
    #[must_use]
    pub fn present_pixels(&self) -> &[u32] {
        if let Scanout::Direct(wid) = self.scanout
            && let Some(win) = self.window_ref(wid)
            && let Some(buf) = win.buffer.as_ref()
        {
            return buf.pixels();
        }
        self.framebuffer.front_buffer()
    }

    /// How the last presented frame was produced.
    #[must_use]
    pub fn scanout(&self) -> Scanout {
        self.scanout
    }

    /// Whether the last presented frame used direct-scanout bypass.
    #[must_use]
    pub fn is_scanout_bypassed(&self) -> bool {
        matches!(self.scanout, Scanout::Direct(_))
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

    /// Capture the current scene as a draw-command stream frame for a remote
    /// viewer (native compositor-level streaming).
    ///
    /// Walks the z-stack bottom-to-top, includes every visible, non-minimized
    /// window, and hands the per-window render-command lists to `session`,
    /// which forwards only the commands that changed since the last frame
    /// (geometry-only deltas otherwise). The buffer (DMA-BUF) path has no
    /// vector commands to forward, so such windows stream as empty command
    /// lists — pixel forwarding for those is the video-encoded fallback's job,
    /// not this path's.
    pub fn capture_stream_frame(&self, session: &mut SceneSession) -> SceneFrame {
        let mut snaps: Vec<WindowSnapshot<'_>> = Vec::with_capacity(self.z_stack.len());
        for &id in &self.z_stack {
            if let Some(win) = self.window_ref(id) {
                if !win.visible || win.minimized {
                    continue;
                }
                snaps.push(WindowSnapshot {
                    id: win.id.raw(),
                    x: win.x,
                    y: win.y,
                    width: win.width,
                    height: win.height,
                    opacity: win.opacity,
                    commands: &win.render_tree,
                });
            }
        }
        session.build_frame(self.framebuffer.width, self.framebuffer.height, &snaps)
    }

    /// Begin a remote draw-command stream session and return its id. A remote
    /// desktop service calls this once per connected viewer, then polls
    /// [`capture_stream`](Self::capture_stream) each frame.
    pub fn start_stream(&mut self) -> u64 {
        let id = self.next_stream_id;
        self.next_stream_id = self.next_stream_id.wrapping_add(1);
        self.stream_sessions.insert(id, SceneSession::new());
        id
    }

    /// Capture the current scene for stream `stream_id` and return the encoded
    /// wire frame (geometry-only deltas for unchanged windows). Errors if the
    /// id is unknown (e.g. the session was already stopped).
    pub fn capture_stream(&mut self, stream_id: u64) -> CompositorResult<Vec<u8>> {
        // Take ownership of the session so capture_stream_frame can borrow
        // &self immutably while mutating the (now-local) session; reinsert after.
        let mut session = self
            .stream_sessions
            .remove(&stream_id)
            .ok_or(CompositorError::StreamNotFound(stream_id))?;
        let frame = self.capture_stream_frame(&mut session);
        let bytes = guiremote::scene::encode_scene_frame(&frame);
        self.stream_sessions.insert(stream_id, session);
        Ok(bytes)
    }

    /// Stop a stream session, freeing its delta-tracking state. Returns whether
    /// a session with that id existed.
    pub fn stop_stream(&mut self, stream_id: u64) -> bool {
        self.stream_sessions.remove(&stream_id).is_some()
    }

    /// Number of active stream sessions (for diagnostics/tests).
    #[must_use]
    pub fn stream_session_count(&self) -> usize {
        self.stream_sessions.len()
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
    // 1. Poll for IPC messages (client requests) via Slate OS channels
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
    fn test_first_command_covers_client() {
        // Opaque, square-cornered, full-cover FillRect at a fully-opaque window
        // => the white background fill can be skipped.
        let full = vec![RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 150.0,
            color: Color::rgba(10, 20, 30, 255),
            corner_radii: CornerRadii::ZERO,
        }];
        assert!(Compositor::first_command_covers_client(&full, 200, 150, 1.0));
        // Overshooting origin/size still counts as full cover.
        let overshoot = vec![RenderCommand::FillRect {
            x: -5.0,
            y: -5.0,
            width: 300.0,
            height: 300.0,
            color: Color::rgba(0, 0, 0, 255),
            corner_radii: CornerRadii::ZERO,
        }];
        assert!(Compositor::first_command_covers_client(
            &overshoot,
            200,
            150,
            1.0
        ));

        // Translucent window => must NOT skip (top rect would blend).
        assert!(!Compositor::first_command_covers_client(&full, 200, 150, 0.5));
        // Non-opaque color => must NOT skip.
        let translucent = vec![RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 150.0,
            color: Color::rgba(10, 20, 30, 128),
            corner_radii: CornerRadii::ZERO,
        }];
        assert!(!Compositor::first_command_covers_client(
            &translucent,
            200,
            150,
            1.0
        ));
        // Rounded corners => must NOT skip (corner pixels show background).
        let rounded = vec![RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 150.0,
            color: Color::rgba(10, 20, 30, 255),
            corner_radii: CornerRadii::all(8.0),
        }];
        assert!(!Compositor::first_command_covers_client(
            &rounded, 200, 150, 1.0
        ));
        // Partial-cover rect => must NOT skip.
        let partial = vec![RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 150.0,
            color: Color::rgba(10, 20, 30, 255),
            corner_radii: CornerRadii::ZERO,
        }];
        assert!(!Compositor::first_command_covers_client(
            &partial, 200, 150, 1.0
        ));
        // First command is not a FillRect => must NOT skip.
        let text_first = vec![RenderCommand::Text {
            x: 0.0,
            y: 0.0,
            text: "hi".to_string(),
            color: Color::WHITE,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        }];
        assert!(!Compositor::first_command_covers_client(
            &text_first,
            200,
            150,
            1.0
        ));
        // Empty command list => must NOT skip.
        assert!(!Compositor::first_command_covers_client(&[], 200, 150, 1.0));
    }

    /// Benchmark: full-desktop recomposite cost at 4K (3840x2160).
    ///
    /// CLAUDE.md's performance-critical-subsystems table requires the
    /// compositor to "composite a full desktop in < 2ms at 4K to not miss
    /// 144Hz vsync". This measures the raw full-recomposite cost — clear the
    /// 4K back buffer, redraw every decorated window with toolkit-style client
    /// content, and swap — via `bench_full_composite`, which bypasses the
    /// vsync frame-rate gate (a tight loop over `compose_frame` would be
    /// throttled by `should_compose` and skip most iterations).
    ///
    /// Run it explicitly (it is `#[ignore]`d so it never slows the normal
    /// correctness run) on a RELEASE build — the debug build's unoptimised
    /// per-pixel loops are not representative:
    ///
    /// ```text
    /// cargo test -p compositor --target x86_64-pc-windows-gnu --release \
    ///   -- --ignored --nocapture bench_compose_frame_4k
    /// ```
    ///
    /// The `< 2ms/4K` target is judged on a release build (ideally on real
    /// hardware); the recorded dev-host baseline lives in
    /// `bench/baselines.toml` under `[compositor_frame_4k]`. As of 2026-07-02
    /// the compositor is still over target (~15.8ms/frame release on the dev host
    /// after the row-wise `fill_rect` rewrite + redundant-bg-fill occlusion cull,
    /// down from ~48.6ms — see known-issues BENCH-COMPOSITOR-SLOW; the remaining
    /// gap is memory-bandwidth bound on a full recomposite). This test therefore
    /// does NOT assert the 2ms target (it would always fail on a full-recomposite
    /// stress).
    /// It prints a PASS/OVER verdict for tracking and hard-fails only on a
    /// catastrophic regression (mean > 150 ms/frame, ~3x the current baseline)
    /// so an accidental super-linear blow-up is still caught without flaking.
    #[test]
    #[ignore = "measurement benchmark; run explicitly with --release --ignored --nocapture"]
    fn bench_compose_frame_4k() {
        const W: u32 = 3840;
        const H: u32 = 2160;
        const NUM_WINDOWS: usize = 16;
        const WARMUP: usize = 5;
        const ITERS: usize = 60;
        const TARGET_MS: f64 = 2.0;

        let mut comp = Compositor::new(W, H, 144).expect("4K compositor");

        // A representative desktop: overlapping decorated windows, each
        // carrying toolkit-style client content (a titlebar band, a large
        // content panel, and a text label), cascaded across the screen.
        for i in 0..NUM_WINDOWS {
            let ww = 1100u32;
            let wh = 720u32;
            let id = comp.create_window(format!("Window {i}"), ww, wh, i as u64 + 1);
            let step = i as i32;
            comp.move_window(id, 60 + step * 170, 40 + step * 110)
                .expect("move_window");
            let commands = vec![
                RenderCommand::FillRect {
                    x: 0.0,
                    y: 0.0,
                    width: ww as f32,
                    height: wh as f32,
                    color: Color::rgba(30, 34, 40, 255),
                    corner_radii: CornerRadii::ZERO,
                },
                RenderCommand::FillRect {
                    x: 20.0,
                    y: 20.0,
                    width: (ww - 40) as f32,
                    height: 80.0,
                    color: Color::rgba(60, 120, 200, 255),
                    corner_radii: CornerRadii::ZERO,
                },
                RenderCommand::FillRect {
                    x: 20.0,
                    y: 120.0,
                    width: (ww - 40) as f32,
                    height: (wh - 160) as f32,
                    color: Color::rgba(45, 48, 54, 255),
                    corner_radii: CornerRadii::ZERO,
                },
                RenderCommand::Text {
                    x: 30.0,
                    y: 40.0,
                    text: format!("Panel {i}"),
                    color: Color::WHITE,
                    font_size: 18.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                },
            ];
            comp.submit_render(id, commands).expect("submit_render");
        }

        // Warm up: page in both framebuffers, prime caches/predictors.
        for _ in 0..WARMUP {
            comp.bench_full_composite();
        }

        let mut min_ns = u64::MAX;
        let mut total_ns = 0u64;
        for _ in 0..ITERS {
            let start = std::time::Instant::now();
            comp.bench_full_composite();
            let ns = start.elapsed().as_nanos() as u64;
            min_ns = min_ns.min(ns);
            total_ns = total_ns.saturating_add(ns);
        }
        let mean_ns = total_ns / ITERS as u64;
        let min_ms = min_ns as f64 / 1_000_000.0;
        let mean_ms = mean_ns as f64 / 1_000_000.0;
        let verdict = if min_ms <= TARGET_MS { "PASS" } else { "OVER" };
        let profile = if cfg!(debug_assertions) { "debug" } else { "release" };

        println!(
            "[compositor-bench] compose_frame 4K ({W}x{H}, {NUM_WINDOWS} windows, \
             {profile} build): min={min_ms:.3}ms mean={mean_ms:.3}ms  \
             target<{TARGET_MS}ms => {verdict}  (target judged on release+hardware)"
        );

        // Catastrophic-regression guard only (see doc): the current baseline
        // is ~16ms (still over the 2ms target, tracked separately); a mean past
        // 80ms (~5x the baseline, and worse than the pre-optimization ~50ms)
        // means a super-linear blow-up crept into the path.
        assert!(
            mean_ms < 80.0,
            "compositor 4K recomposite mean {mean_ms:.3}ms is a catastrophic regression (>80ms)"
        );
    }

    /// Build `w*h` tightly-packed (stride = w*4) ARGB bytes all of `color`.
    fn solid_buffer_bytes(w: u32, h: u32, color: u32) -> Vec<u8> {
        let mut bytes = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..(w * h) {
            bytes.extend_from_slice(&color.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn test_attach_buffer_blits_pixels() {
        let mut comp = Compositor::new(400, 300, 60).unwrap();
        // First window: client area lands at (100, 80).
        let id = comp.create_window("Buf".to_string(), 8, 8, 1);

        let color = 0xFF11_2233u32;
        let bytes = solid_buffer_bytes(4, 4, color);
        assert!(comp
            .attach_buffer(id, 55, 4, 4, 16, BufferFormat::Argb8888, &bytes)
            .is_ok());
        assert!(comp.compose_frame());

        // The 4x4 buffer should have been blitted at the client origin (100,80).
        let front = comp.framebuffer.front_buffer();
        let stride = 400usize;
        assert_eq!(front[80 * stride + 100], color, "buffer top-left pixel");
        assert_eq!(front[83 * stride + 103], color, "buffer bottom-right pixel");
        // Beyond the 4x4 buffer (within the 8x8 client area) is NOT buffer
        // content — the client owns its surface, uncovered area stays cleared.
        assert_ne!(front[80 * stride + 106], color, "outside buffer extent");
    }

    #[test]
    fn test_attach_buffer_rejects_bad_geometry() {
        let mut comp = Compositor::new(400, 300, 60).unwrap();
        let id = comp.create_window("Buf".to_string(), 8, 8, 1);

        // Stride too small for width 4 (needs 16 bytes/row).
        let bytes = vec![0u8; 64];
        assert!(matches!(
            comp.attach_buffer(id, 1, 4, 4, 8, BufferFormat::Argb8888, &bytes),
            Err(CompositorError::InvalidBuffer(_))
        ));
        // The failed attach must leave the window on the render-tree path.
        assert!(comp.window_ref(id).unwrap().buffer.is_none());
    }

    #[test]
    fn test_attach_buffer_window_not_found() {
        let mut comp = Compositor::new(400, 300, 60).unwrap();
        let bytes = solid_buffer_bytes(2, 2, 0xFFFFFFFF);
        assert!(matches!(
            comp.attach_buffer(WindowId(424242), 1, 2, 2, 8, BufferFormat::Argb8888, &bytes),
            Err(CompositorError::WindowNotFound(_))
        ));
    }

    #[test]
    fn test_detach_buffer_reverts_to_commands() {
        let mut comp = Compositor::new(400, 300, 60).unwrap();
        let id = comp.create_window("Buf".to_string(), 8, 8, 1);
        let bytes = solid_buffer_bytes(4, 4, 0xFF00FF00);

        assert!(comp
            .attach_buffer(id, 77, 4, 4, 16, BufferFormat::Argb8888, &bytes)
            .is_ok());
        assert!(comp.window_ref(id).unwrap().buffer.is_some());

        assert_eq!(comp.detach_buffer(id), Some(77));
        assert!(comp.window_ref(id).unwrap().buffer.is_none());
        // Detaching again returns None (nothing attached).
        assert_eq!(comp.detach_buffer(id), None);
    }

    #[test]
    fn test_buffer_release_notification() {
        let mut comp = Compositor::new(400, 300, 60).unwrap();
        let id = comp.create_window("Buf".to_string(), 8, 8, 1);
        let bytes = solid_buffer_bytes(4, 4, 0xFF334455);

        assert!(comp
            .attach_buffer(id, 0xABCD, 4, 4, 16, BufferFormat::Argb8888, &bytes)
            .is_ok());
        // Before compositing, nothing has been read yet.
        assert!(comp.take_released_buffer_handles().is_empty());

        assert!(comp.compose_frame());
        // After compositing, the buffer is released exactly once.
        assert_eq!(comp.take_released_buffer_handles(), vec![0xABCD]);
        assert!(comp.take_released_buffer_handles().is_empty());
    }

    #[test]
    fn test_copy_row_clips_left_right_and_vertical() {
        let mut fb = Framebuffer::new(4, 2).unwrap();
        fb.clear(0xFF000000);
        let src = [0xFFAAAAAA, 0xFFBBBBBB, 0xFFCCCCCC, 0xFFDDDDDD, 0xFFEEEEEE];

        // Partly offscreen on the left: x=-2 skips the first two src pixels,
        // writes the remaining 3 at columns 0..3.
        fb.copy_row(-2, 0, &src);
        assert_eq!(fb.get_pixel(0, 0), Some(0xFFCCCCCC));
        assert_eq!(fb.get_pixel(1, 0), Some(0xFFDDDDDD));
        assert_eq!(fb.get_pixel(2, 0), Some(0xFFEEEEEE));
        assert_eq!(fb.get_pixel(3, 0), Some(0xFF000000)); // untouched

        // Partly offscreen on the right: x=2 writes only 2 of 5 (cols 2,3).
        fb.clear(0xFF000000);
        fb.copy_row(2, 1, &src);
        assert_eq!(fb.get_pixel(2, 1), Some(0xFFAAAAAA));
        assert_eq!(fb.get_pixel(3, 1), Some(0xFFBBBBBB));
        assert_eq!(fb.get_pixel(0, 1), Some(0xFF000000));

        // Out-of-range y or fully-offscreen x is a no-op.
        fb.clear(0xFF121212);
        fb.copy_row(0, 5, &src);
        fb.copy_row(-10, 0, &src);
        fb.copy_row(4, 0, &src);
        assert_eq!(fb.get_pixel(0, 0), Some(0xFF121212));
    }

    #[test]
    fn test_opaque_buffer_fast_path_matches_blend() {
        // An Xrgb (opaque) buffer at full opacity must blit bit-identically to
        // the per-pixel blend path: blend writes `src | 0xFF000000` and Xrgb
        // import already forced 0xFF alpha, so the fast copy is exact.
        let mut comp = Compositor::new(400, 300, 60).unwrap();
        let id = comp.create_window("Buf".to_string(), 8, 8, 1);
        let bytes = solid_buffer_bytes(4, 4, 0x0011_2233); // alpha 0x00 in source
        assert!(comp
            .attach_buffer(id, 1, 4, 4, 16, BufferFormat::Xrgb8888, &bytes)
            .is_ok());
        assert!(comp.compose_frame());

        let front = comp.framebuffer.front_buffer();
        let stride = 400usize;
        // First window's client area lands at (100, 80) by default placement;
        // read via the window's actual client position to stay robust.
        let (wx, wy) = {
            let w = comp.window_ref(id).unwrap();
            (w.x as usize, w.y as usize)
        };
        assert_eq!(front[wy * stride + wx], 0xFF11_2233, "opaque fast-path pixel");
        assert_eq!(front[(wy + 3) * stride + wx + 3], 0xFF11_2233);
    }

    #[test]
    fn test_fullscreen_sets_geometry_and_clears() {
        let mut comp = Compositor::new(400, 300, 60).unwrap();
        let id = comp.create_window("Game".to_string(), 200, 150, 1);
        assert!(!comp.is_fullscreen(id));

        assert!(comp.set_fullscreen(id, true).is_ok());
        assert!(comp.is_fullscreen(id));
        {
            let win = comp.window_ref(id).unwrap();
            assert_eq!((win.x, win.y), (0, 0));
            assert_eq!((win.width, win.height), (400, 300));
            assert!(win.fullscreen);
        }

        // Leaving fullscreen restores the original client geometry.
        assert!(comp.set_fullscreen(id, false).is_ok());
        assert!(!comp.is_fullscreen(id));
        let win = comp.window_ref(id).unwrap();
        assert_eq!((win.width, win.height), (200, 150));
    }

    #[test]
    fn test_direct_scanout_bypass_presents_buffer() {
        let mut comp = Compositor::new(64, 48, 60).unwrap();
        let id = comp.create_window("Game".to_string(), 64, 48, 1);
        assert!(comp.set_fullscreen(id, true).is_ok());

        // A display-sized opaque buffer makes the window scanout-eligible.
        let color = 0xFF_AB_CD_EFu32;
        let bytes = solid_buffer_bytes(64, 48, color);
        assert!(comp
            .attach_buffer(id, 9, 64, 48, 64 * 4, BufferFormat::Argb8888, &bytes)
            .is_ok());

        assert!(comp.compose_frame());
        // Frame should have bypassed compositing entirely.
        assert!(comp.is_scanout_bypassed());
        assert_eq!(comp.scanout(), Scanout::Direct(id));
        assert_eq!(comp.frame_stats.bypass_frames, 1);

        // Presented pixels come straight from the client buffer (zero copy).
        let present = comp.present_pixels();
        assert_eq!(present.len(), 64 * 48);
        assert_eq!(present[0], color);
        assert_eq!(present[64 * 48 - 1], color);

        // The buffer is released exactly once for reuse.
        assert_eq!(comp.take_released_buffer_handles(), vec![9]);
    }

    #[test]
    fn test_no_bypass_when_buffer_smaller_than_display() {
        let mut comp = Compositor::new(64, 48, 60).unwrap();
        let id = comp.create_window("Game".to_string(), 64, 48, 1);
        assert!(comp.set_fullscreen(id, true).is_ok());

        // Buffer smaller than the display must NOT bypass (would leave the rest
        // of the screen stale); the compositor falls back to compositing.
        let bytes = solid_buffer_bytes(32, 24, 0xFF112233);
        assert!(comp
            .attach_buffer(id, 1, 32, 24, 32 * 4, BufferFormat::Argb8888, &bytes)
            .is_ok());

        assert!(comp.compose_frame());
        assert!(!comp.is_scanout_bypassed());
        assert_eq!(comp.scanout(), Scanout::Composited);
        // Composited path presents the framebuffer front buffer.
        assert_eq!(comp.present_pixels().len(), 64 * 48);
    }

    #[test]
    fn test_no_bypass_when_not_fullscreen() {
        let mut comp = Compositor::new(64, 48, 60).unwrap();
        let id = comp.create_window("Win".to_string(), 64, 48, 1);
        // Display-sized buffer but the window is NOT fullscreen → no bypass.
        let bytes = solid_buffer_bytes(64, 48, 0xFF445566);
        assert!(comp
            .attach_buffer(id, 1, 64, 48, 64 * 4, BufferFormat::Argb8888, &bytes)
            .is_ok());
        assert!(comp.compose_frame());
        assert!(!comp.is_scanout_bypassed());
    }

    #[test]
    fn test_no_bypass_when_translucent() {
        let mut comp = Compositor::new(64, 48, 60).unwrap();
        let id = comp.create_window("Game".to_string(), 64, 48, 1);
        assert!(comp.set_fullscreen(id, true).is_ok());
        comp.set_opacity(id, 0.5).ok();
        let bytes = solid_buffer_bytes(64, 48, 0xFF778899);
        assert!(comp
            .attach_buffer(id, 1, 64, 48, 64 * 4, BufferFormat::Argb8888, &bytes)
            .is_ok());
        assert!(comp.compose_frame());
        // A translucent fullscreen window must blend with what's beneath, so it
        // cannot be scanned out directly.
        assert!(!comp.is_scanout_bypassed());
    }

    #[test]
    fn test_stream_ipc_lifecycle() {
        let mut comp = Compositor::new(400, 300, 60).unwrap();

        // Start a stream via IPC.
        let stream_id = match comp.handle_request(CompositorRequest::StreamStart) {
            CompositorResponse::StreamStarted { stream_id } => stream_id,
            other => panic!("expected StreamStarted, got {other:?}"),
        };
        assert_eq!(comp.stream_session_count(), 1);

        // Capture produces a decodable wire frame.
        let data = match comp.handle_request(CompositorRequest::StreamCapture { stream_id }) {
            CompositorResponse::StreamFrame { data } => data,
            other => panic!("expected StreamFrame, got {other:?}"),
        };
        let frame =
            guiremote::scene::decode_scene_frame(&data).expect("decode captured frame");
        assert_eq!(frame.sequence, 0);
        assert_eq!(frame.display_width, 400);
        assert_eq!(frame.display_height, 300);

        // Stop frees the session; a second stop reports an error.
        assert!(matches!(
            comp.handle_request(CompositorRequest::StreamStop { stream_id }),
            CompositorResponse::Ok
        ));
        assert_eq!(comp.stream_session_count(), 0);
        assert!(matches!(
            comp.handle_request(CompositorRequest::StreamStop { stream_id }),
            CompositorResponse::Error { .. }
        ));
        // Capturing a stopped session is an error, not a panic.
        assert!(matches!(
            comp.handle_request(CompositorRequest::StreamCapture { stream_id }),
            CompositorResponse::Error { .. }
        ));
    }

    #[test]
    fn test_stream_capture_forwards_window_commands() {
        let mut comp = Compositor::new(200, 150, 60).unwrap();
        let id = comp.create_window("Streamed".to_string(), 100, 80, 1);
        let commands = vec![RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 30.0,
            color: Color::RED,
            corner_radii: CornerRadii::ZERO,
        }];
        assert!(comp.submit_render(id, commands).is_ok());

        let stream_id = comp.start_stream();

        // First capture: the window is new to the session → commands present.
        let f0 =
            guiremote::scene::decode_scene_frame(&comp.capture_stream(stream_id).unwrap()).unwrap();
        assert_eq!(f0.windows.len(), 1);
        assert_eq!(f0.windows[0].id, id.raw());
        let cmds = f0.windows[0]
            .commands
            .as_ref()
            .expect("new window forwards commands");
        assert_eq!(cmds.commands.len(), 1);

        // Second capture with unchanged content: geometry-only delta.
        let f1 =
            guiremote::scene::decode_scene_frame(&comp.capture_stream(stream_id).unwrap()).unwrap();
        assert_eq!(f1.sequence, 1);
        assert!(f1.windows[0].commands.is_none());

        // Destroying the window makes the next frame report it as removed.
        assert!(comp.destroy_window(id).is_ok());
        let f2 =
            guiremote::scene::decode_scene_frame(&comp.capture_stream(stream_id).unwrap()).unwrap();
        assert!(f2.windows.is_empty());
        assert_eq!(f2.removed, vec![id.raw()]);
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

    // ---- clear_except (occlusion-culled desktop clear) ---------------------

    #[test]
    fn test_clear_except_empty_covered_fills_all() {
        // No covered rects => behaves exactly like `clear`.
        let mut fb = Framebuffer::new(6, 4).unwrap();
        fb.set_pixel(3, 2, 0xFF_12_34_56); // dirty the back buffer first
        fb.clear_except(0xFF_AA_BB_CC, &[]);
        for y in 0..4 {
            for x in 0..6 {
                assert_eq!(fb.get_pixel(x, y), Some(0xFF_AA_BB_CC));
            }
        }
    }

    #[test]
    fn test_clear_except_single_rect_preserves_covered() {
        // A single covered rect: pixels inside keep their prior value, pixels
        // outside get the clear color.
        let mut fb = Framebuffer::new(8, 6).unwrap();
        fb.clear(0xFF_00_00_00); // known prior state everywhere
        let covered = [Rect::new(2, 1, 3, 2)]; // x:2..5, y:1..3
        fb.clear_except(0xFF_FF_FF_FF, &covered);
        for y in 0..6 {
            for x in 0..8 {
                let inside = (2..5).contains(&x) && (1..3).contains(&y);
                let expect = if inside { 0xFF_00_00_00 } else { 0xFF_FF_FF_FF };
                assert_eq!(
                    fb.get_pixel(x, y),
                    Some(expect),
                    "pixel ({x},{y}) inside={inside}"
                );
            }
        }
    }

    #[test]
    fn test_clear_except_overlapping_rects_merge_spans() {
        // Two overlapping rects on the same rows must merge into one covered
        // span (no clear color bleeds into the overlap or the seam between
        // them). Covered union on rows 0..3: x in 1..7.
        let mut fb = Framebuffer::new(10, 4).unwrap();
        fb.clear(0xFF_00_00_00);
        let covered = [Rect::new(1, 0, 4, 3), Rect::new(4, 0, 3, 3)]; // 1..5 and 4..7
        fb.clear_except(0xFF_FF_FF_FF, &covered);
        for y in 0..4 {
            for x in 0..10 {
                let inside = (1..7).contains(&x) && (0..3).contains(&y);
                let expect = if inside { 0xFF_00_00_00 } else { 0xFF_FF_FF_FF };
                assert_eq!(fb.get_pixel(x, y), Some(expect), "pixel ({x},{y})");
            }
        }
    }

    #[test]
    fn test_clear_except_clips_offscreen_and_oversized_rects() {
        // Rects extending past the framebuffer edges (and with negative
        // origins) must be clipped, never panic or write OOB.
        let mut fb = Framebuffer::new(6, 5).unwrap();
        fb.clear(0xFF_00_00_00);
        // Straddles the top-left corner and overshoots bottom-right.
        let covered = [Rect::new(-3, -2, 100, 100)];
        fb.clear_except(0xFF_FF_FF_FF, &covered);
        // The whole framebuffer is covered by the clipped rect => nothing gets
        // the clear color.
        for y in 0..5 {
            for x in 0..6 {
                assert_eq!(fb.get_pixel(x, y), Some(0xFF_00_00_00), "pixel ({x},{y})");
            }
        }

        // A fully-offscreen rect covers nothing => full clear.
        let mut fb2 = Framebuffer::new(6, 5).unwrap();
        fb2.clear(0xFF_00_00_00);
        fb2.clear_except(0xFF_FF_FF_FF, &[Rect::new(50, 50, 4, 4)]);
        for y in 0..5 {
            for x in 0..6 {
                assert_eq!(fb2.get_pixel(x, y), Some(0xFF_FF_FF_FF), "pixel ({x},{y})");
            }
        }
    }

    #[test]
    fn test_clear_except_parallel_band_boundaries() {
        // A framebuffer above the parallel-fill threshold (>1M px) so the
        // multi-threaded row-band path runs on a multicore host. A covered rect
        // whose vertical extent straddles band boundaries must be skipped
        // correctly in every band (the key parallel-correctness risk), and the
        // result must be bit-identical to a single-threaded clear_except.
        const W: u32 = 2048;
        const H: u32 = 1024; // 2M px > 1<<20 threshold
        let covered = [
            Rect::new(100, 50, 400, 900),  // tall: crosses many band boundaries
            Rect::new(1500, 300, 400, 200), // offset block
        ];

        let mut par = Framebuffer::new(W, H).unwrap();
        par.clear(0xFF_00_00_00);
        par.clear_except(0xFF_AB_CD_EF, &covered);

        // Ground truth: fill the same buffer single-threaded via the shared helper.
        let mut reference = vec![0xFF_00_00_00u32; (W * H) as usize];
        Framebuffer::fill_uncovered_band(&mut reference, 0, H, W, 0xFF_AB_CD_EF, &covered, H);

        assert_eq!(par.back.len(), reference.len());
        assert!(
            par.back == reference,
            "parallel clear_except must match single-threaded reference"
        );

        // Spot-check a covered pixel (kept prior) and an uncovered one (cleared),
        // both far from row 0 so at least one band boundary was crossed.
        assert_eq!(par.get_pixel(200, 600), Some(0xFF_00_00_00));
        assert_eq!(par.get_pixel(900, 600), Some(0xFF_AB_CD_EF));
    }

    // ---- blit_opaque parallel row-band blit ---------------------------------

    /// Build an opaque (Xrgb) `SharedBuffer` whose pixel `(x, y)` carries a
    /// deterministic non-black value, so a copy can be verified per-pixel.
    #[cfg(test)]
    fn make_opaque_test_buffer(w: u32, h: u32) -> SharedBuffer {
        let mut bytes = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let off = ((y * w + x) * 4) as usize;
                // Xrgb: bytes are little-endian [B, G, R, X]; distinct per pixel.
                bytes[off] = (x & 0xFF) as u8;
                bytes[off + 1] = (y & 0xFF) as u8;
                bytes[off + 2] = ((x ^ y) & 0xFF) as u8;
                bytes[off + 3] = 0;
            }
        }
        SharedBuffer::import(9, w, h, w * 4, BufferFormat::Xrgb8888, &bytes).expect("import")
    }

    /// Serial reference: exactly the old blit_buffer opaque fast path.
    #[cfg(test)]
    fn blit_opaque_reference(fb: &mut Framebuffer, buf: &SharedBuffer, win_x: i32, win_y: i32) {
        let cols = buf.width();
        let rows = buf.height();
        for row in 0..rows {
            let sy = win_y.saturating_add(row as i32);
            if sy < 0 {
                continue;
            }
            if let Some(src) = buf.row(row) {
                let run = src.get(..cols as usize).unwrap_or(src);
                fb.copy_row(win_x, sy as u32, run);
            }
        }
    }

    #[test]
    fn test_blit_opaque_matches_serial_reference_large() {
        // Above the parallel threshold so the multi-band path runs on multicore.
        // The buffer is larger than 1<<20 px to force blit_opaque to parallelize.
        const W: u32 = 2048;
        const H: u32 = 1024; // 2M px > threshold
        let buf = make_opaque_test_buffer(1200, 900);

        for &(wx, wy) in &[(0i32, 0i32), (100, 200), (-50, -30), (1900, 800), (2000, 0)] {
            let mut par = Framebuffer::new(W, H).unwrap();
            par.clear(0xFF_00_00_00);
            par.blit_opaque(&buf, wx, wy, buf.width(), buf.height());

            let mut reference = Framebuffer::new(W, H).unwrap();
            reference.clear(0xFF_00_00_00);
            blit_opaque_reference(&mut reference, &buf, wx, wy);

            assert!(
                par.back == reference.back,
                "blit_opaque({wx},{wy}) must match serial reference"
            );
        }
    }

    #[test]
    fn test_blit_opaque_clips_edges_small() {
        // Small buffers exercise the single-threaded path plus all clip corners.
        let buf = make_opaque_test_buffer(16, 12);
        for &(wx, wy) in &[
            (0i32, 0i32),
            (-5, -3), // top-left straddle
            (28, 20), // fully offscreen bottom-right
            (-20, 5), // fully offscreen left
            (25, -2), // right-straddle + top-straddle
        ] {
            let mut got = Framebuffer::new(32, 24).unwrap();
            got.clear(0xFF_11_22_33);
            got.blit_opaque(&buf, wx, wy, buf.width(), buf.height());

            let mut want = Framebuffer::new(32, 24).unwrap();
            want.clear(0xFF_11_22_33);
            blit_opaque_reference(&mut want, &buf, wx, wy);

            assert!(got.back == want.back, "blit_opaque clip mismatch at ({wx},{wy})");
        }
    }

    // ---- opaque_cover_rects (which windows cull the desktop clear) ---------

    #[test]
    fn test_opaque_cover_rects_reports_opaque_command_window() {
        // A full-opacity window whose first command opaquely covers the client
        // area is reported over its whole client rect.
        let mut comp = Compositor::new(800, 600, 60).unwrap();
        let id = comp.create_window("Solid".to_string(), 200, 150, 1);
        comp.move_window(id, 120, 90).unwrap();
        comp.submit_render(
            id,
            vec![RenderCommand::FillRect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 150.0,
                color: Color::rgba(30, 34, 40, 255),
                corner_radii: CornerRadii::ZERO,
            }],
        )
        .unwrap();

        let rects = comp.opaque_cover_rects();
        assert_eq!(rects, vec![Rect::new(120, 90, 200, 150)]);
    }

    #[test]
    fn test_opaque_cover_rects_excludes_translucent_and_hidden() {
        let mut comp = Compositor::new(800, 600, 60).unwrap();

        // Translucent window: opaque command but window opacity < 1.0 => excluded.
        let ghost = comp.create_window("Ghost".to_string(), 100, 100, 1);
        comp.submit_render(
            ghost,
            vec![RenderCommand::FillRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
                color: Color::rgba(10, 20, 30, 255),
                corner_radii: CornerRadii::ZERO,
            }],
        )
        .unwrap();
        comp.set_opacity(ghost, 0.5).unwrap();

        // Minimized window: opaque + full opacity but not visible => excluded.
        let hidden = comp.create_window("Hidden".to_string(), 100, 100, 2);
        comp.submit_render(
            hidden,
            vec![RenderCommand::FillRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
                color: Color::rgba(10, 20, 30, 255),
                corner_radii: CornerRadii::ZERO,
            }],
        )
        .unwrap();
        comp.minimize_window(hidden).unwrap();

        // Rounded-corner window: corners show background => not a full cover.
        let rounded = comp.create_window("Rounded".to_string(), 100, 100, 3);
        comp.submit_render(
            rounded,
            vec![RenderCommand::FillRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
                color: Color::rgba(10, 20, 30, 255),
                corner_radii: CornerRadii::all(8.0),
            }],
        )
        .unwrap();

        assert!(
            comp.opaque_cover_rects().is_empty(),
            "no window should cull the desktop clear"
        );
    }

    #[test]
    fn test_opaque_cover_rects_buffer_window_uses_covered_subrect() {
        // An Xrgb (opaque) buffer smaller than the client area only covers the
        // sub-rectangle it actually spans, from the client origin.
        let mut comp = Compositor::new(400, 300, 60).unwrap();
        let id = comp.create_window("Buf".to_string(), 20, 20, 1);
        let (wx, wy) = {
            let w = comp.window_ref(id).unwrap();
            (w.x, w.y)
        };
        let bytes = solid_buffer_bytes(8, 6, 0x0011_2233);
        comp.attach_buffer(id, 1, 8, 6, 8 * 4, BufferFormat::Xrgb8888, &bytes)
            .unwrap();

        let rects = comp.opaque_cover_rects();
        assert_eq!(rects, vec![Rect::new(wx, wy, 8, 6)]);

        // An Argb buffer (not is_opaque) must NOT be reported.
        let id2 = comp.create_window("Argb".to_string(), 20, 20, 2);
        let bytes2 = solid_buffer_bytes(8, 6, 0xFF00_FF00);
        comp.attach_buffer(id2, 2, 8, 6, 8 * 4, BufferFormat::Argb8888, &bytes2)
            .unwrap();
        let id2_pos = {
            let w = comp.window_ref(id2).unwrap();
            Rect::new(w.x, w.y, 8, 6)
        };
        assert!(
            !comp.opaque_cover_rects().contains(&id2_pos),
            "Argb buffer window must not be treated as opaque"
        );
    }

    #[test]
    fn test_full_recomposite_cull_matches_uncovered_background() {
        // Visual-equivalence: after a full recomposite, the desktop background
        // shows through where nothing covers it, and covered pixels carry the
        // window's opaque content — identical to a plain clear+draw.
        let mut comp = Compositor::new(300, 200, 60).unwrap();
        let id = comp.create_window("Solid".to_string(), 120, 80, 1);
        comp.move_window(id, 50, 40).unwrap();
        comp.submit_render(
            id,
            vec![RenderCommand::FillRect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 80.0,
                color: Color::rgba(30, 34, 40, 255),
                corner_radii: CornerRadii::ZERO,
            }],
        )
        .unwrap();

        comp.bench_full_composite();

        // `bench_full_composite` swaps, so the composited result is in front.
        let bg = comp.theme.desktop_background;
        let front = comp.framebuffer.front_buffer();
        let stride = 300usize;
        let at = |x: usize, y: usize| front[y * stride + x];
        // A pixel well inside the client area carries the window content.
        assert_eq!(at(60, 50), 0xFF_1E_22_28);
        assert_ne!(at(60, 50), bg);
        // A pixel far from any window keeps the desktop background.
        assert_eq!(at(250, 150), bg);
        // A pixel just left of the client rect (still background region).
        assert_eq!(at(10, 50), bg);
    }
}
