//! Software compositor and window manager with DRM/KMS display backend.
//!
//! Renders windows to a GEM-backed scanout buffer, then uses DRM page flip
//! to display the result.  Falls back to direct framebuffer writes (`fb.rs`)
//! when DRM buffers are not available.
//!
//! ## Rendering Pipeline (DRM path)
//!
//! 1. `start()` allocates two GEM-backed scanout buffers (double-buffering).
//! 2. `compose()` renders desktop + windows + cursor to the back buffer.
//! 3. DRM `page_flip` copies the back buffer to the display hardware.
//! 4. Front/back buffer indices are swapped.
//!
//! ## Rendering Pipeline (fb fallback)
//!
//! When DRM buffers are unavailable (allocation failure, no DRM device),
//! the compositor renders directly to the Limine framebuffer via `fb.rs`.
//! The XOR cursor from `fb.rs` is used in this mode.
//!
//! ## Lock Ordering
//!
//! `COMPOSITOR` → `SCANOUT` → DRM device registry.  Never reversed.
//!
//! ## Architecture
//!
//! Windows are stored in a z-ordered list (back to front).  Each window has
//! an off-screen pixel buffer.  The compositor blits windows to the
//! scanout buffer on demand, handling overlapping correctly.  A simple
//! title bar with close/minimize buttons allows basic interaction.

// Window manager API surface (set_pixel, move/raise/list windows, etc.) is
// exposed for userspace and kshell commands; not every helper is called
// from kernel-internal paths.
#![allow(dead_code)]

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use crate::drm::{self, DrmObjectId};
use crate::drm::mode::PixelFormat;
use crate::fb;
use crate::mm::frame::FRAME_SIZE;
use crate::sync::Mutex;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Title bar height in pixels.
const TITLE_BAR_HEIGHT: u32 = 24;
/// Window border width in pixels.
const BORDER_WIDTH: u32 = 1;
/// Maximum number of windows.
const MAX_WINDOWS: usize = 32;

// Colors (BGRA format: 0xAARRGGBB in little-endian u32).
const COLOR_TITLE_BAR_ACTIVE: u32 = 0xFF_33_66_99; // Steel blue
const COLOR_TITLE_BAR_INACTIVE: u32 = 0xFF_55_55_55; // Gray
const COLOR_TITLE_TEXT: u32 = 0xFF_FF_FF_FF; // White
const COLOR_BORDER: u32 = 0xFF_44_44_44; // Dark gray
const COLOR_CLOSE_BTN: u32 = 0xFF_CC_33_33; // Red
const COLOR_DESKTOP_BG: u32 = 0xFF_1A_1A_2E; // Dark navy
const COLOR_CURSOR_OUTLINE: u32 = 0xFF_00_00_00; // Black
const COLOR_CURSOR_FILL: u32 = 0xFF_FF_FF_FF; // White

// ---------------------------------------------------------------------------
// Mouse cursor bitmap (12x19, same as fb.rs)
// ---------------------------------------------------------------------------

/// 0 = transparent, 1 = outline (black), 2 = fill (white).
#[allow(dead_code)]
const CURSOR_WIDTH: u32 = 12;
#[allow(dead_code)]
const CURSOR_HEIGHT: u32 = 19;
#[rustfmt::skip]
static CURSOR_BITMAP: [[u8; 12]; 19] = [
    [1,0,0,0,0,0,0,0,0,0,0,0],
    [1,1,0,0,0,0,0,0,0,0,0,0],
    [1,2,1,0,0,0,0,0,0,0,0,0],
    [1,2,2,1,0,0,0,0,0,0,0,0],
    [1,2,2,2,1,0,0,0,0,0,0,0],
    [1,2,2,2,2,1,0,0,0,0,0,0],
    [1,2,2,2,2,2,1,0,0,0,0,0],
    [1,2,2,2,2,2,2,1,0,0,0,0],
    [1,2,2,2,2,2,2,2,1,0,0,0],
    [1,2,2,2,2,2,2,2,2,1,0,0],
    [1,2,2,2,2,2,2,2,2,2,1,0],
    [1,2,2,2,2,2,2,2,2,2,2,1],
    [1,2,2,2,2,2,1,1,1,1,1,1],
    [1,2,2,2,2,2,1,0,0,0,0,0],
    [1,2,2,1,2,2,1,0,0,0,0,0],
    [1,2,1,0,1,2,2,1,0,0,0,0],
    [1,1,0,0,1,2,2,1,0,0,0,0],
    [1,0,0,0,0,1,2,1,0,0,0,0],
    [0,0,0,0,0,1,1,1,0,0,0,0],
];

// ---------------------------------------------------------------------------
// Scanout buffer — GPU-backed pixel buffer for compositing
// ---------------------------------------------------------------------------

/// A GEM-backed pixel buffer that can be page-flipped to the display.
///
/// Holds pre-computed HHDM virtual addresses for each backing frame so
/// that pixel writes do not require taking the DRM lock.  Only
/// `page_flip()` needs the DRM lock (briefly, for the copy to HW).
///
/// ## Frame Boundary Handling
///
/// GEM buffers are backed by non-contiguous 16 KiB physical frames.
/// A scanline may span two frames if `row_offset + pitch > FRAME_SIZE`.
/// All write methods handle this transparently.
///
/// ## Safety Invariant
///
/// `frame_addrs[i]` is a valid HHDM-mapped virtual address for the
/// duration of the buffer's lifetime.  Addresses become invalid after
/// `destroy()` frees the underlying GEM object.
struct ScanoutBuffer {
    /// Pre-computed virtual addresses of each backing frame.
    frame_addrs: Vec<u64>,
    /// Width in pixels.
    width: u32,
    /// Height in pixels.
    height: u32,
    /// Bytes per row (64-byte aligned).
    pitch: u32,
    /// GEM buffer handle (for cleanup).
    gem_handle: u32,
    /// DRM framebuffer ID (for page_flip).
    fb_id: DrmObjectId,
    /// CRTC ID for page_flip.
    crtc_id: DrmObjectId,
}

impl ScanoutBuffer {
    /// Allocate a DRM-backed scanout buffer.
    ///
    /// Creates a GEM object and DRM framebuffer through the primary DRM
    /// device.  Extracts and caches frame virtual addresses for fast
    /// pixel access without holding the DRM lock.
    fn new(width: u32, height: u32) -> Result<Self, crate::error::KernelError> {
        let (gem_handle, fb_id, crtc_id, frame_addrs, pitch) = drm::with_primary_mut(|dev| {
            let handle = dev.gem_create(width, height, PixelFormat::Xrgb8888)?;
            let p = dev.gem_pitch(handle)?;
            let fid = dev.fb_create(handle, width, height, p, PixelFormat::Xrgb8888)?;
            let addrs = dev.gem_frame_addrs(handle)?;
            let cid = dev.first_crtc_id()
                .ok_or(crate::error::KernelError::NotFound)?;
            Ok((handle, fid, cid, addrs, p))
        })?;

        Ok(Self {
            frame_addrs,
            width,
            height,
            pitch,
            gem_handle,
            fb_id,
            crtc_id,
        })
    }

    /// Write a single pixel at (x, y).
    ///
    /// Bounds-checked; out-of-bounds writes are silently ignored.
    ///
    /// ## Safety Argument
    ///
    /// Because pitch is 64-byte aligned and pixels are 4 bytes, a single
    /// u32 write never crosses a frame boundary.  The byte offset is
    /// always 4-aligned, so `frame_off + 4 <= FRAME_SIZE` (16380+4=16384).
    #[allow(clippy::arithmetic_side_effects)]
    fn write_pixel(&self, x: u32, y: u32, color: u32) {
        if x >= self.width || y >= self.height {
            return;
        }
        let byte_off = (y as usize) * (self.pitch as usize) + (x as usize) * 4;
        let frame_idx = byte_off / FRAME_SIZE;
        let frame_off = byte_off % FRAME_SIZE;
        if let Some(&addr) = self.frame_addrs.get(frame_idx) {
            // SAFETY: addr is a valid HHDM-mapped address for a GEM-owned
            // frame.  frame_off is 4-aligned and frame_off+4 <= FRAME_SIZE
            // (see safety argument in doc comment).  The buffer is alive
            // (destroy() hasn't been called).
            unsafe {
                core::ptr::write_volatile((addr + frame_off as u64) as *mut u32, color);
            }
        }
    }

    /// Fill a rectangle with a solid color.
    ///
    /// Coordinates are clipped to the buffer bounds.  Uses per-row
    /// frame tracking for efficiency: only recomputes frame index on
    /// frame boundary crossings (rare: ~1 per 4096 pixels at 32bpp).
    #[allow(clippy::arithmetic_side_effects, clippy::cast_sign_loss)]
    fn fill_rect(&self, x: i32, y: i32, w: u32, h: u32, color: u32) {
        let x0 = x.max(0) as u32;
        let y0 = y.max(0) as u32;
        let x1 = ((x as i64 + w as i64).min(self.width as i64).max(0)) as u32;
        let y1 = ((y as i64 + h as i64).min(self.height as i64).max(0)) as u32;
        if x0 >= x1 || y0 >= y1 {
            return;
        }

        for row in y0..y1 {
            let row_byte_start = (row as usize) * (self.pitch as usize) + (x0 as usize) * 4;
            let mut frame_idx = row_byte_start / FRAME_SIZE;
            let mut frame_off = row_byte_start % FRAME_SIZE;
            let mut frame_addr = match self.frame_addrs.get(frame_idx) {
                Some(&a) => a,
                None => continue,
            };

            for _ in x0..x1 {
                // SAFETY: frame_addr + frame_off is within a valid GEM frame.
                // frame_off is 4-aligned, so frame_off+4 <= FRAME_SIZE.
                unsafe {
                    core::ptr::write_volatile(
                        (frame_addr + frame_off as u64) as *mut u32,
                        color,
                    );
                }
                frame_off += 4;
                if frame_off >= FRAME_SIZE {
                    frame_off = 0;
                    frame_idx += 1;
                    frame_addr = match self.frame_addrs.get(frame_idx) {
                        Some(&a) => a,
                        None => break,
                    };
                }
            }
        }
    }

    /// Draw a 1-pixel rectangle outline.
    #[allow(clippy::cast_possible_wrap)]
    fn draw_rect(&self, x: i32, y: i32, w: u32, h: u32, color: u32) {
        if w == 0 || h == 0 {
            return;
        }
        // Top edge.
        self.fill_rect(x, y, w, 1, color);
        if h > 1 {
            // Bottom edge.
            self.fill_rect(x, y + (h as i32 - 1), w, 1, color);
        }
        if h > 2 {
            // Left and right edges (excluding corners).
            self.fill_rect(x, y + 1, 1, h - 2, color);
            self.fill_rect(x + (w as i32 - 1), y + 1, 1, h - 2, color);
        }
    }

    /// Draw a line using Bresenham's algorithm.
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    fn draw_line(&self, x0: i32, y0: i32, x1: i32, y1: i32, color: u32) {
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx: i32 = if x0 < x1 { 1 } else { -1 };
        let sy: i32 = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let mut cx = x0;
        let mut cy = y0;

        loop {
            if cx >= 0 && cy >= 0 {
                self.write_pixel(cx as u32, cy as u32, color);
            }
            if cx == x1 && cy == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                if cx == x1 {
                    break;
                }
                err += dy;
                cx += sx;
            }
            if e2 <= dx {
                if cy == y1 {
                    break;
                }
                err += dx;
                cy += sy;
            }
        }
    }

    /// Blit a rectangular region from a pixel buffer to this scanout buffer.
    ///
    /// `src` is a row-major array of BGRA u32 pixels with stride `src_w`.
    /// Pixels with alpha < 128 are treated as transparent.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_sign_loss)]
    fn blit(&self, src: &[u32], src_w: u32, dst_x: i32, dst_y: i32, w: u32, h: u32) {
        // Compute clipped source/destination regions.
        let src_x0 = if dst_x < 0 { (-dst_x) as u32 } else { 0 };
        let src_y0 = if dst_y < 0 { (-dst_y) as u32 } else { 0 };
        let dst_x0 = dst_x.max(0) as u32;
        let dst_y0 = dst_y.max(0) as u32;
        let clip_w = w.saturating_sub(src_x0)
            .min(self.width.saturating_sub(dst_x0));
        let clip_h = h.saturating_sub(src_y0)
            .min(self.height.saturating_sub(dst_y0));
        if clip_w == 0 || clip_h == 0 {
            return;
        }

        for row in 0..clip_h {
            let src_row = src_y0 + row;
            let dst_row = dst_y0 + row;

            let row_byte_start =
                (dst_row as usize) * (self.pitch as usize) + (dst_x0 as usize) * 4;
            let mut frame_idx = row_byte_start / FRAME_SIZE;
            let mut frame_off = row_byte_start % FRAME_SIZE;
            let mut frame_addr = match self.frame_addrs.get(frame_idx) {
                Some(&a) => a,
                None => continue,
            };

            for col in 0..clip_w {
                let src_col = src_x0 + col;
                let src_idx = (src_row * src_w + src_col) as usize;
                let pixel = match src.get(src_idx) {
                    Some(&p) => p,
                    None => {
                        // Advance frame offset even if source is OOB.
                        frame_off += 4;
                        if frame_off >= FRAME_SIZE {
                            frame_off = 0;
                            frame_idx += 1;
                            if let Some(&a) = self.frame_addrs.get(frame_idx) {
                                frame_addr = a;
                            }
                        }
                        continue;
                    }
                };

                let alpha = pixel >> 24;
                if alpha >= 128 {
                    // SAFETY: frame_addr + frame_off is within a valid GEM frame.
                    unsafe {
                        core::ptr::write_volatile(
                            (frame_addr + frame_off as u64) as *mut u32,
                            pixel | 0xFF00_0000,
                        );
                    }
                }

                frame_off += 4;
                if frame_off >= FRAME_SIZE {
                    frame_off = 0;
                    frame_idx += 1;
                    frame_addr = match self.frame_addrs.get(frame_idx) {
                        Some(&a) => a,
                        None => break,
                    };
                }
            }
        }
    }

    /// Page-flip this buffer to the display via DRM.
    fn flip(&self) -> Result<(), crate::error::KernelError> {
        drm::with_primary_mut(|dev| {
            dev.page_flip(self.crtc_id, self.fb_id)
        })
    }

    /// Free the GEM object and DRM framebuffer.
    fn destroy(&self) {
        let _ = drm::with_primary_mut(|dev| {
            // Destroy framebuffer first (references the GEM handle).
            let _ = dev.fb_destroy(self.fb_id);
            dev.gem_destroy(self.gem_handle)
        });
    }
}

// ---------------------------------------------------------------------------
// Window
// ---------------------------------------------------------------------------

/// Window identifier (stable across z-order changes).
pub type WindowId = u32;

/// A managed window.
pub struct Window {
    /// Unique identifier.
    pub id: WindowId,
    /// Window title.
    pub title: String,
    /// X position of top-left corner (includes border).
    pub x: i32,
    /// Y position of top-left corner (includes border).
    pub y: i32,
    /// Client area width (excludes border/title bar).
    pub width: u32,
    /// Client area height (excludes border/title bar).
    pub height: u32,
    /// Off-screen pixel buffer for the client area (row-major BGRA).
    pub pixels: Vec<u32>,
    /// Whether the window is visible.
    pub visible: bool,
    /// Whether the window is minimized.
    pub minimized: bool,
}

impl Window {
    /// Create a new window with a blank (black) client area.
    pub fn new(id: WindowId, title: &str, x: i32, y: i32, width: u32, height: u32) -> Self {
        let pixel_count = (width * height) as usize;
        Self {
            id,
            title: String::from(title),
            x,
            y,
            width,
            height,
            pixels: vec![0xFF_00_00_00; pixel_count], // Black background
            visible: true,
            minimized: false,
        }
    }

    /// Total width including border.
    pub fn total_width(&self) -> u32 {
        self.width + 2 * BORDER_WIDTH
    }

    /// Total height including title bar and border.
    pub fn total_height(&self) -> u32 {
        self.height + TITLE_BAR_HEIGHT + 2 * BORDER_WIDTH
    }

    /// Client area X offset (from window origin to content).
    pub fn client_x(&self) -> i32 {
        self.x + BORDER_WIDTH as i32
    }

    /// Client area Y offset (from window origin to content start).
    pub fn client_y(&self) -> i32 {
        self.y + TITLE_BAR_HEIGHT as i32 + BORDER_WIDTH as i32
    }

    /// Set a pixel in the client area buffer.
    pub fn set_pixel(&mut self, x: u32, y: u32, color: u32) {
        if x < self.width && y < self.height {
            let idx = (y * self.width + x) as usize;
            if let Some(p) = self.pixels.get_mut(idx) {
                *p = color;
            }
        }
    }

    /// Fill the client area with a solid color.
    pub fn fill(&mut self, color: u32) {
        for p in self.pixels.iter_mut() {
            *p = color;
        }
    }

    /// Fill a rectangle within the client area.
    pub fn fill_rect(&mut self, rx: u32, ry: u32, rw: u32, rh: u32, color: u32) {
        for row in ry..ry.saturating_add(rh).min(self.height) {
            for col in rx..rx.saturating_add(rw).min(self.width) {
                let idx = (row * self.width + col) as usize;
                if let Some(p) = self.pixels.get_mut(idx) {
                    *p = color;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Compositor state
// ---------------------------------------------------------------------------

/// Window management state (protected by COMPOSITOR lock).
struct CompositorState {
    /// Windows in z-order (index 0 = back, last = front/focused).
    windows: Vec<Window>,
    /// Next window ID to assign.
    next_id: WindowId,
    /// Whether the desktop needs a full redraw.
    dirty: bool,
    /// Whether the compositor is running.
    running: bool,
    /// Currently dragging: (window_id, offset_x, offset_y).
    dragging: Option<(WindowId, i32, i32)>,
}

impl CompositorState {
    const fn new() -> Self {
        Self {
            windows: Vec::new(),
            next_id: 1,
            dirty: true,
            running: false,
            dragging: None,
        }
    }
}

/// Double-buffer state for DRM-backed rendering (separate lock from windows).
struct ScanoutState {
    /// Two scanout buffers for double-buffering (Some when DRM is active).
    buffers: Option<(ScanoutBuffer, ScanoutBuffer)>,
    /// Which buffer is the back buffer (0 or 1).
    back_idx: usize,
    /// Cached display width.
    display_w: u32,
    /// Cached display height.
    display_h: u32,
}

impl ScanoutState {
    const fn new() -> Self {
        Self {
            buffers: None,
            back_idx: 0,
            display_w: 0,
            display_h: 0,
        }
    }
}

static COMPOSITOR: Mutex<CompositorState> = Mutex::new(CompositorState::new());
static SCANOUT: Mutex<ScanoutState> = Mutex::new(ScanoutState::new());
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Whether the DRM rendering path is active (vs fb fallback).
static DRM_ACTIVE: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Cursor position tracking
// ---------------------------------------------------------------------------

/// Cursor X position (compositor-managed, independent of fb.rs).
static CURSOR_X: AtomicI32 = AtomicI32::new(0);
/// Cursor Y position (compositor-managed, independent of fb.rs).
static CURSOR_Y: AtomicI32 = AtomicI32::new(0);

/// Get the current compositor cursor position.
fn cursor_pos() -> (i32, i32) {
    (CURSOR_X.load(Ordering::Relaxed), CURSOR_Y.load(Ordering::Relaxed))
}

/// Update cursor position from mouse delta.
///
/// PS/2 convention: positive dy = up, but screen Y increases downward,
/// so we subtract dy.
fn update_cursor(dx: i16, dy: i16) {
    let (max_x, max_y) = display_dimensions();
    let old_x = CURSOR_X.load(Ordering::Relaxed);
    let old_y = CURSOR_Y.load(Ordering::Relaxed);
    let new_x = (old_x + dx as i32).clamp(0, max_x.saturating_sub(1) as i32);
    let new_y = (old_y - dy as i32).clamp(0, max_y.saturating_sub(1) as i32);
    CURSOR_X.store(new_x, Ordering::Release);
    CURSOR_Y.store(new_y, Ordering::Release);
}

/// Get display dimensions from DRM or fb.
fn display_dimensions() -> (u32, u32) {
    if DRM_ACTIVE.load(Ordering::Acquire) {
        let scanout = SCANOUT.lock();
        (scanout.display_w, scanout.display_h)
    } else {
        fb::dimensions()
    }
}

// ---------------------------------------------------------------------------
// DRM buffer management
// ---------------------------------------------------------------------------

/// Allocate double-buffered scanout buffers via DRM.
///
/// Returns true on success.  On failure, the compositor will use the
/// fb.rs fallback path.
fn init_drm_buffers() -> bool {
    // Query display size from DRM.
    let (w, h) = match drm::with_primary(|dev| Ok(dev.display_size())) {
        Ok((w, h)) if w > 0 && h > 0 => (w, h),
        _ => return false,
    };

    // Allocate two scanout buffers.
    let buf0 = match ScanoutBuffer::new(w, h) {
        Ok(b) => b,
        Err(e) => {
            crate::serial_println!(
                "[compositor] DRM buffer 0 alloc failed: {:?}, using fb fallback",
                e
            );
            return false;
        }
    };
    let buf1 = match ScanoutBuffer::new(w, h) {
        Ok(b) => b,
        Err(e) => {
            crate::serial_println!(
                "[compositor] DRM buffer 1 alloc failed: {:?}, using fb fallback",
                e
            );
            buf0.destroy();
            return false;
        }
    };

    let mut scanout = SCANOUT.lock();
    scanout.buffers = Some((buf0, buf1));
    scanout.back_idx = 0;
    scanout.display_w = w;
    scanout.display_h = h;

    crate::serial_println!("[compositor] DRM double-buffer allocated ({}x{})", w, h);
    true
}

/// Free DRM scanout buffers.
fn free_drm_buffers() {
    let mut scanout = SCANOUT.lock();
    if let Some((b0, b1)) = scanout.buffers.take() {
        b0.destroy();
        b1.destroy();
    }
    scanout.display_w = 0;
    scanout.display_h = 0;
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Start the compositor.
///
/// Tries DRM-backed rendering first; falls back to fb.rs if DRM buffers
/// cannot be allocated.
pub fn start() {
    // Try DRM path first.
    let drm_ok = if drm::device_count() > 0 {
        init_drm_buffers()
    } else {
        false
    };

    if !drm_ok && !fb::is_initialized() {
        crate::serial_println!("[compositor] Cannot start: no display available");
        return;
    }

    let mut state = COMPOSITOR.lock();
    state.running = true;
    state.dirty = true;
    drop(state);

    ACTIVE.store(true, Ordering::Release);
    DRM_ACTIVE.store(drm_ok, Ordering::Release);

    // Initialize cursor position to center of screen.
    let (w, h) = if drm_ok {
        let s = SCANOUT.lock();
        (s.display_w, s.display_h)
    } else {
        fb::dimensions()
    };
    #[allow(clippy::cast_possible_wrap)]
    {
        CURSOR_X.store((w / 2) as i32, Ordering::Release);
        CURSOR_Y.store((h / 2) as i32, Ordering::Release);
    }

    // Draw initial desktop.
    compose();

    // In fb fallback mode, show the XOR cursor.
    if !drm_ok {
        fb::show_cursor();
    }

    crate::serial_println!(
        "[compositor] Started ({})",
        if drm_ok { "DRM double-buffered" } else { "fb fallback" }
    );
}

/// Stop the compositor and return to text console.
pub fn stop() {
    ACTIVE.store(false, Ordering::Release);

    let was_drm = DRM_ACTIVE.load(Ordering::Acquire);
    DRM_ACTIVE.store(false, Ordering::Release);

    if !was_drm {
        fb::hide_cursor();
    }

    let mut state = COMPOSITOR.lock();
    state.running = false;
    drop(state);

    // Free DRM buffers.
    if was_drm {
        free_drm_buffers();
    }

    // Restore text console.
    crate::console::clear();
    crate::serial_println!("[compositor] Stopped");
}

/// Return true if the compositor is active.
pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Acquire)
}

/// Create a new window and return its ID.
pub fn create_window(title: &str, x: i32, y: i32, width: u32, height: u32) -> WindowId {
    let mut state = COMPOSITOR.lock();
    if state.windows.len() >= MAX_WINDOWS {
        return 0; // No more windows.
    }
    let id = state.next_id;
    state.next_id += 1;
    let window = Window::new(id, title, x, y, width, height);
    state.windows.push(window);
    state.dirty = true;
    id
}

/// Close (destroy) a window by ID.
pub fn close_window(id: WindowId) {
    let mut state = COMPOSITOR.lock();
    state.windows.retain(|w| w.id != id);
    state.dirty = true;
}

/// Move a window to a new position.
pub fn move_window(id: WindowId, x: i32, y: i32) {
    let mut state = COMPOSITOR.lock();
    if let Some(w) = state.windows.iter_mut().find(|w| w.id == id) {
        w.x = x;
        w.y = y;
        state.dirty = true;
    }
}

/// Bring a window to the front (make it focused).
pub fn raise_window(id: WindowId) {
    let mut state = COMPOSITOR.lock();
    if let Some(pos) = state.windows.iter().position(|w| w.id == id) {
        let window = state.windows.remove(pos);
        state.windows.push(window);
        state.dirty = true;
    }
}

/// Access a window's pixel buffer via a closure.
pub fn with_window<F, R>(id: WindowId, f: F) -> Option<R>
where
    F: FnOnce(&mut Window) -> R,
{
    let mut state = COMPOSITOR.lock();
    if let Some(w) = state.windows.iter_mut().find(|w| w.id == id) {
        let result = f(w);
        state.dirty = true;
        Some(result)
    } else {
        None
    }
}

/// Get the number of open windows.
pub fn window_count() -> usize {
    COMPOSITOR.lock().windows.len()
}

// ---------------------------------------------------------------------------
// Rendering — DRM path
// ---------------------------------------------------------------------------

/// Compose all windows to the back scanout buffer and page-flip.
fn compose_drm(state: &CompositorState) {
    let mut scanout = SCANOUT.lock();
    let (display_w, display_h) = (scanout.display_w, scanout.display_h);
    let back_idx = scanout.back_idx;

    let back = match scanout.buffers {
        Some(ref bufs) => {
            if back_idx == 0 { &bufs.0 } else { &bufs.1 }
        }
        None => return,
    };

    // Desktop background.
    back.fill_rect(0, 0, display_w, display_h, COLOR_DESKTOP_BG);

    // Draw each visible window from back to front.
    let window_count = state.windows.len();
    for (idx, window) in state.windows.iter().enumerate() {
        if !window.visible || window.minimized {
            continue;
        }
        let is_focused = idx == window_count.saturating_sub(1);
        draw_window_drm(back, window, is_focused);
    }

    // Draw mouse cursor on top of everything.
    let (cx, cy) = cursor_pos();
    draw_cursor_drm(back, cx, cy);

    // Page-flip the back buffer to the display.
    if let Err(e) = back.flip() {
        crate::serial_println!("[compositor] page_flip failed: {:?}", e);
    }

    // Swap front/back.
    scanout.back_idx = 1 - back_idx;
}

/// Draw a single window to the DRM scanout buffer.
fn draw_window_drm(buf: &ScanoutBuffer, window: &Window, focused: bool) {
    let x = window.x;
    let y = window.y;
    let tw = window.total_width();
    let th = window.total_height();

    // Border.
    buf.draw_rect(x, y, tw, th, COLOR_BORDER);

    // Title bar background.
    let title_color = if focused {
        COLOR_TITLE_BAR_ACTIVE
    } else {
        COLOR_TITLE_BAR_INACTIVE
    };
    buf.fill_rect(
        x + BORDER_WIDTH as i32,
        y + BORDER_WIDTH as i32,
        window.width,
        TITLE_BAR_HEIGHT - BORDER_WIDTH,
        title_color,
    );

    // Title text.
    draw_title_text_drm(
        buf,
        x + BORDER_WIDTH as i32 + 4,
        y + BORDER_WIDTH as i32 + 4,
        &window.title,
        COLOR_TITLE_TEXT,
    );

    // Close button (small red square in top-right).
    let close_x = x + tw as i32 - BORDER_WIDTH as i32 - 18;
    let close_y = y + BORDER_WIDTH as i32 + 4;
    buf.fill_rect(close_x, close_y, 14, 14, COLOR_CLOSE_BTN);
    // Draw X in the close button.
    buf.draw_line(close_x + 3, close_y + 3, close_x + 11, close_y + 11, COLOR_TITLE_TEXT);
    buf.draw_line(close_x + 11, close_y + 3, close_x + 3, close_y + 11, COLOR_TITLE_TEXT);

    // Client area: blit the window's pixel buffer.
    let client_x = window.client_x();
    let client_y = window.client_y();
    buf.blit(
        &window.pixels,
        window.width,
        client_x,
        client_y,
        window.width,
        window.height,
    );
}

/// Draw title text to a DRM scanout buffer using the kernel's bitmap font.
fn draw_title_text_drm(buf: &ScanoutBuffer, x: i32, y: i32, text: &str, color: u32) {
    let mut cx = x;
    for ch in text.bytes().take(32) {
        if ch < 0x20 || ch > 0x7E {
            continue;
        }
        let glyph = crate::font::glyph(ch);
        for (row_idx, &glyph_row) in glyph.iter().enumerate().step_by(2) {
            for col in 0..8u32 {
                let bit = (glyph_row >> (7 - col)) & 1;
                if bit != 0 {
                    #[allow(clippy::cast_possible_truncation)]
                    buf.write_pixel(
                        (cx + col as i32) as u32,
                        (y + row_idx as i32 / 2) as u32,
                        color,
                    );
                }
            }
        }
        cx += 8;
    }
}

/// Draw the mouse cursor sprite to a DRM scanout buffer.
fn draw_cursor_drm(buf: &ScanoutBuffer, cx: i32, cy: i32) {
    for (row_idx, row) in CURSOR_BITMAP.iter().enumerate() {
        for (col_idx, &pixel) in row.iter().enumerate() {
            if pixel == 0 {
                continue; // Transparent.
            }
            let px = cx + col_idx as i32;
            let py = cy + row_idx as i32;
            if px < 0 || py < 0 {
                continue;
            }
            let color = if pixel == 1 {
                COLOR_CURSOR_OUTLINE
            } else {
                COLOR_CURSOR_FILL
            };
            buf.write_pixel(px as u32, py as u32, color);
        }
    }
}

// ---------------------------------------------------------------------------
// Rendering — fb fallback path
// ---------------------------------------------------------------------------

/// Compose all windows directly to the framebuffer (fb.rs).
fn compose_fb(state: &CompositorState) {
    let (fb_w, fb_h) = fb::dimensions();
    fb::fill_rect(0, 0, fb_w, fb_h, COLOR_DESKTOP_BG);

    let window_count = state.windows.len();
    for (idx, window) in state.windows.iter().enumerate() {
        if !window.visible || window.minimized {
            continue;
        }
        let is_focused = idx == window_count.saturating_sub(1);
        draw_window_fb(window, is_focused);
    }
}

/// Draw a single window to the framebuffer (fb.rs path).
fn draw_window_fb(window: &Window, focused: bool) {
    let x = window.x;
    let y = window.y;
    let tw = window.total_width();
    let th = window.total_height();

    fb::draw_rect(x, y, tw, th, COLOR_BORDER);

    let title_color = if focused {
        COLOR_TITLE_BAR_ACTIVE
    } else {
        COLOR_TITLE_BAR_INACTIVE
    };
    fb::fill_rect(
        x + BORDER_WIDTH as i32,
        y + BORDER_WIDTH as i32,
        window.width,
        TITLE_BAR_HEIGHT - BORDER_WIDTH,
        title_color,
    );

    draw_title_text_fb(
        x + BORDER_WIDTH as i32 + 4,
        y + BORDER_WIDTH as i32 + 4,
        &window.title,
        COLOR_TITLE_TEXT,
    );

    let close_x = x + tw as i32 - BORDER_WIDTH as i32 - 18;
    let close_y = y + BORDER_WIDTH as i32 + 4;
    fb::fill_rect(close_x, close_y, 14, 14, COLOR_CLOSE_BTN);
    fb::draw_line(close_x + 3, close_y + 3, close_x + 11, close_y + 11, COLOR_TITLE_TEXT);
    fb::draw_line(close_x + 11, close_y + 3, close_x + 3, close_y + 11, COLOR_TITLE_TEXT);

    let client_x = window.client_x();
    let client_y = window.client_y();
    fb::blit(
        &window.pixels,
        window.width,
        client_x,
        client_y,
        window.width,
        window.height,
    );
}

/// Draw title text to the framebuffer (fb.rs path).
fn draw_title_text_fb(x: i32, y: i32, text: &str, color: u32) {
    let mut cx = x;
    for ch in text.bytes().take(32) {
        if ch < 0x20 || ch > 0x7E {
            continue;
        }
        let glyph = crate::font::glyph(ch);
        for (row_idx, &glyph_row) in glyph.iter().enumerate().step_by(2) {
            for col in 0..8u32 {
                let bit = (glyph_row >> (7 - col)) & 1;
                if bit != 0 {
                    #[allow(clippy::cast_possible_truncation)]
                    fb::set_pixel(
                        (cx + col as i32) as u32,
                        (y + row_idx as i32 / 2) as u32,
                        color,
                    );
                }
            }
        }
        cx += 8;
    }
}

// ---------------------------------------------------------------------------
// Rendering — dispatch
// ---------------------------------------------------------------------------

/// Compose all windows to the display.
///
/// Dispatches to the DRM or fb path based on which is active.
pub fn compose() {
    let state = COMPOSITOR.lock();
    if !state.running {
        return;
    }

    if DRM_ACTIVE.load(Ordering::Acquire) {
        compose_drm(&state);
    } else {
        compose_fb(&state);
    }
}

// ---------------------------------------------------------------------------
// Input handling
// ---------------------------------------------------------------------------

/// Process all pending mouse events and update the compositor.
///
/// Call this in a loop (e.g., from a kshell "desktop" command or a
/// dedicated compositor task).
pub fn process_input() {
    let drm = DRM_ACTIVE.load(Ordering::Acquire);
    let mut any_moved = false;

    while let Some(ev) = crate::mouse::try_read_event() {
        // Update compositor cursor position.
        update_cursor(ev.dx, ev.dy);
        any_moved = true;

        // In fb fallback mode, also update the XOR cursor.
        if !drm {
            fb::move_cursor(ev.dx, ev.dy);
        }

        let (cx, cy) = cursor_pos();

        if ev.buttons & 1 != 0 {
            // Left button pressed.
            handle_left_click(cx, cy);
        } else {
            // Button released — stop dragging.
            let mut state = COMPOSITOR.lock();
            if state.dragging.is_some() {
                state.dragging = None;
                state.dirty = true;
            }
        }

        // Handle dragging.
        let mut state = COMPOSITOR.lock();
        if let Some((wid, off_x, off_y)) = state.dragging {
            if let Some(w) = state.windows.iter_mut().find(|w| w.id == wid) {
                w.x = cx - off_x;
                w.y = cy - off_y;
                state.dirty = true;
            }
        }
    }

    // In DRM mode, any mouse movement triggers a recompose (cursor is
    // rendered in the composition buffer, not via XOR overlay).
    if drm && any_moved {
        let mut state = COMPOSITOR.lock();
        state.dirty = true;
    }

    // Recompose if dirty.
    let state = COMPOSITOR.lock();
    if state.dirty {
        drop(state);
        compose();
        let mut state = COMPOSITOR.lock();
        state.dirty = false;
    }
}

/// Handle a left-click at screen coordinates (sx, sy).
fn handle_left_click(sx: i32, sy: i32) {
    let mut state = COMPOSITOR.lock();

    // Find the topmost window under the click (iterate back to front).
    let mut clicked_id: Option<WindowId> = None;

    for window in state.windows.iter().rev() {
        if !window.visible || window.minimized {
            continue;
        }
        let wx = window.x;
        let wy = window.y;
        let ww = window.total_width() as i32;
        let wh = window.total_height() as i32;

        if sx >= wx && sx < wx + ww && sy >= wy && sy < wy + wh {
            clicked_id = Some(window.id);

            // Check if click is on the close button.
            let close_x = wx + ww - BORDER_WIDTH as i32 - 18;
            let close_y = wy + BORDER_WIDTH as i32 + 4;
            if sx >= close_x && sx < close_x + 14 && sy >= close_y && sy < close_y + 14 {
                let id = window.id;
                drop(state);
                close_window(id);
                return;
            }

            // Check if click is on the title bar (dragging).
            let title_top = wy + BORDER_WIDTH as i32;
            let title_bottom = title_top + TITLE_BAR_HEIGHT as i32;
            if sy >= title_top && sy < title_bottom {
                let off_x = sx - wx;
                let off_y = sy - wy;
                state.dragging = Some((window.id, off_x, off_y));
            }

            break;
        }
    }

    // Raise the clicked window to front.
    if let Some(id) = clicked_id {
        if let Some(pos) = state.windows.iter().position(|w| w.id == id) {
            let window = state.windows.remove(pos);
            state.windows.push(window);
            state.dirty = true;
        }
    }
}

// ---------------------------------------------------------------------------
// Self-test / demo
// ---------------------------------------------------------------------------

/// Run a compositor demo: create some windows and process input briefly.
pub fn demo() {
    if !fb::is_initialized() && drm::device_count() == 0 {
        crate::console_println!("Compositor demo requires a display (fb or DRM)");
        return;
    }

    start();

    // Create a few demo windows.
    let w1 = create_window("Hello World", 50, 50, 200, 150);
    let w2 = create_window("Terminal", 280, 100, 250, 180);
    let w3 = create_window("About", 150, 250, 180, 120);

    // Fill windows with different colors.
    with_window(w1, |w| w.fill(fb::rgb(30, 30, 60)));
    with_window(w2, |w| {
        w.fill(fb::rgb(10, 10, 10));
        // Draw some "text lines" as colored rectangles.
        w.fill_rect(4, 4, 200, 2, fb::rgb(0, 200, 0));
        w.fill_rect(4, 10, 150, 2, fb::rgb(0, 200, 0));
        w.fill_rect(4, 16, 180, 2, fb::rgb(0, 200, 0));
    });
    with_window(w3, |w| {
        w.fill(fb::rgb(40, 40, 50));
        // A simple icon-like square.
        w.fill_rect(70, 30, 40, 40, fb::rgb(100, 150, 255));
    });

    // Compose once to show everything.
    compose();

    crate::console_println!("Compositor demo running. Move the mouse!");
    crate::console_println!("Click title bars to drag, X to close, any key to exit.");

    // Process input until a key is pressed.
    loop {
        process_input();
        // Check for keyboard input to exit.
        if crate::keyboard::try_read_char().is_some() {
            break;
        }
        crate::sched::yield_now();
    }

    stop();

    // Clean up windows.
    close_window(w1);
    close_window(w2);
    close_window(w3);
}
