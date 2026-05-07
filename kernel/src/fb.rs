//! Framebuffer 2D graphics primitives.
//!
//! Provides basic pixel-level drawing operations on top of the Limine-provided
//! linear framebuffer.  This module is the foundation for the GUI compositor:
//! it handles filled/outlined rectangles, lines (Bresenham), circles (midpoint
//! algorithm), bitmap blitting, and a hardware-cursor-style mouse pointer.
//!
//! ## Color format
//!
//! All colors are 32-bit BGRA (`0xAARRGGBB` in little-endian u32):
//! - Bits 0-7:   Blue
//! - Bits 8-15:  Green
//! - Bits 16-23: Red
//! - Bits 24-31: Alpha (0xFF = opaque, 0x00 = transparent)
//!
//! ## Thread safety
//!
//! The framebuffer is a shared resource.  This module does not provide
//! synchronization — callers must coordinate access (typically the compositor
//! holds an exclusive lock during rendering).
//!
//! ## Mouse cursor
//!
//! A 12x19 pixel arrow cursor is rendered via XOR blitting so it can be
//! drawn and erased without a backing store.  For the final compositor, a
//! proper alpha-blended cursor with save/restore will be used.

use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicI16, AtomicU32, AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Framebuffer state
// ---------------------------------------------------------------------------

/// Virtual address of the framebuffer.
static FB_ADDR: AtomicU64 = AtomicU64::new(0);
/// Width in pixels.
static FB_WIDTH: AtomicU32 = AtomicU32::new(0);
/// Height in pixels.
static FB_HEIGHT: AtomicU32 = AtomicU32::new(0);
/// Bytes per row (pitch).
static FB_PITCH: AtomicU32 = AtomicU32::new(0);
/// Whether the module has been initialized.
static INITIALIZED: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Mouse cursor state
// ---------------------------------------------------------------------------

/// Current cursor X position on screen.
static CURSOR_X: AtomicI16 = AtomicI16::new(0);
/// Current cursor Y position on screen.
static CURSOR_Y: AtomicI16 = AtomicI16::new(0);
/// Whether the cursor is currently visible (drawn on screen).
static CURSOR_VISIBLE: AtomicBool = AtomicBool::new(false);

/// Mouse cursor bitmap (12x19 pixels).
/// 0 = transparent, 1 = black (outline), 2 = white (fill).
const CURSOR_WIDTH: u32 = 12;
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
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the framebuffer graphics module.
///
/// Reads the framebuffer parameters from the console module and stores them
/// for use by drawing functions.
///
/// Must be called after `console::init()`.
pub fn init() {
    if let Some((addr, width, height, pitch)) = crate::console::framebuffer_info() {
        FB_ADDR.store(addr, Ordering::Release);
        FB_WIDTH.store(width, Ordering::Release);
        FB_HEIGHT.store(height, Ordering::Release);
        FB_PITCH.store(pitch, Ordering::Release);
        INITIALIZED.store(true, Ordering::Release);

        // Place cursor in the center of the screen.
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        {
            CURSOR_X.store((width / 2) as i16, Ordering::Release);
            CURSOR_Y.store((height / 2) as i16, Ordering::Release);
        }

        crate::serial_println!("[fb] Graphics primitives initialized ({}x{} px)", width, height);
    } else {
        crate::serial_println!("[fb] WARNING: framebuffer not available, graphics disabled");
    }
}

/// Return true if the framebuffer graphics module is ready.
#[inline]
pub fn is_initialized() -> bool {
    INITIALIZED.load(Ordering::Acquire)
}

/// Return framebuffer dimensions (width, height) or (0, 0) if not initialized.
pub fn dimensions() -> (u32, u32) {
    (FB_WIDTH.load(Ordering::Acquire), FB_HEIGHT.load(Ordering::Acquire))
}

// ---------------------------------------------------------------------------
// Pixel operations
// ---------------------------------------------------------------------------

/// Set a single pixel at (x, y) to the given color.
///
/// Performs bounds checking; out-of-bounds writes are silently ignored.
#[inline]
pub fn set_pixel(x: u32, y: u32, color: u32) {
    let width = FB_WIDTH.load(Ordering::Relaxed);
    let height = FB_HEIGHT.load(Ordering::Relaxed);
    if x >= width || y >= height {
        return;
    }
    let fb = FB_ADDR.load(Ordering::Relaxed);
    let pitch = FB_PITCH.load(Ordering::Relaxed);
    let offset = u64::from(y) * u64::from(pitch) + u64::from(x) * 4;
    // SAFETY: Bounds checked above, framebuffer covers width*height pixels.
    unsafe {
        ptr::write_volatile((fb + offset) as *mut u32, color);
    }
}

/// Read a single pixel at (x, y).
///
/// Returns 0 (black) for out-of-bounds coordinates.
#[inline]
pub fn get_pixel(x: u32, y: u32) -> u32 {
    let width = FB_WIDTH.load(Ordering::Relaxed);
    let height = FB_HEIGHT.load(Ordering::Relaxed);
    if x >= width || y >= height {
        return 0;
    }
    let fb = FB_ADDR.load(Ordering::Relaxed);
    let pitch = FB_PITCH.load(Ordering::Relaxed);
    let offset = u64::from(y) * u64::from(pitch) + u64::from(x) * 4;
    // SAFETY: Bounds checked above.
    unsafe { ptr::read_volatile((fb + offset) as *const u32) }
}

// ---------------------------------------------------------------------------
// Color helpers
// ---------------------------------------------------------------------------

/// Construct a BGRA color from RGB components (fully opaque).
#[inline]
pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    (0xFF << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

/// Construct a BGRA color with alpha.
#[inline]
pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

/// Alpha-blend a foreground color over a background color.
///
/// Uses integer approximation: `out = fg * alpha + bg * (255 - alpha)`.
#[inline]
pub fn blend(fg: u32, bg: u32) -> u32 {
    let alpha = (fg >> 24) & 0xFF;
    if alpha == 0xFF {
        return fg;
    }
    if alpha == 0 {
        return bg;
    }
    let inv_alpha = 255 - alpha;

    let r = ((((fg >> 16) & 0xFF) * alpha + ((bg >> 16) & 0xFF) * inv_alpha) / 255) & 0xFF;
    let g = ((((fg >> 8) & 0xFF) * alpha + ((bg >> 8) & 0xFF) * inv_alpha) / 255) & 0xFF;
    let b = (((fg & 0xFF) * alpha + (bg & 0xFF) * inv_alpha) / 255) & 0xFF;

    0xFF00_0000 | (r << 16) | (g << 8) | b
}

// ---------------------------------------------------------------------------
// Filled rectangle
// ---------------------------------------------------------------------------

/// Draw a filled rectangle.
///
/// `x`, `y` is the top-left corner.  Coordinates are clipped to the screen.
pub fn fill_rect(x: i32, y: i32, w: u32, h: u32, color: u32) {
    if !INITIALIZED.load(Ordering::Acquire) {
        return;
    }
    let fb_w = FB_WIDTH.load(Ordering::Relaxed);
    let fb_h = FB_HEIGHT.load(Ordering::Relaxed);
    let fb = FB_ADDR.load(Ordering::Relaxed);
    let pitch = FB_PITCH.load(Ordering::Relaxed);

    // Clip to screen bounds.
    let x0 = x.max(0) as u32;
    let y0 = y.max(0) as u32;
    #[allow(clippy::cast_sign_loss)]
    let x1 = ((x as i64 + w as i64).min(fb_w as i64).max(0)) as u32;
    #[allow(clippy::cast_sign_loss)]
    let y1 = ((y as i64 + h as i64).min(fb_h as i64).max(0)) as u32;

    if x0 >= x1 || y0 >= y1 {
        return;
    }

    for row in y0..y1 {
        let row_base = fb + u64::from(row) * u64::from(pitch) + u64::from(x0) * 4;
        for col in 0..(x1 - x0) {
            let addr = row_base + u64::from(col) * 4;
            // SAFETY: Within framebuffer bounds (clipped above).
            unsafe {
                ptr::write_volatile(addr as *mut u32, color);
            }
        }
    }
}

/// Draw a rectangle outline (1-pixel border).
pub fn draw_rect(x: i32, y: i32, w: u32, h: u32, color: u32) {
    if w == 0 || h == 0 {
        return;
    }
    // Top and bottom edges.
    fill_rect(x, y, w, 1, color);
    if h > 1 {
        #[allow(clippy::cast_possible_wrap)]
        fill_rect(x, y + (h as i32 - 1), w, 1, color);
    }
    // Left and right edges (excluding corners already drawn).
    if h > 2 {
        fill_rect(x, y + 1, 1, h - 2, color);
        #[allow(clippy::cast_possible_wrap)]
        fill_rect(x + (w as i32 - 1), y + 1, 1, h - 2, color);
    }
}

// ---------------------------------------------------------------------------
// Line drawing (Bresenham)
// ---------------------------------------------------------------------------

/// Draw a line from (x0, y0) to (x1, y1) using Bresenham's algorithm.
#[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
pub fn draw_line(x0: i32, y0: i32, x1: i32, y1: i32, color: u32) {
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx: i32 = if x0 < x1 { 1 } else { -1 };
    let sy: i32 = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let mut cx = x0;
    let mut cy = y0;

    loop {
        if cx >= 0 && cy >= 0 {
            set_pixel(cx as u32, cy as u32, color);
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

// ---------------------------------------------------------------------------
// Circle (midpoint algorithm)
// ---------------------------------------------------------------------------

/// Draw a circle outline centered at (cx, cy) with the given radius.
#[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
pub fn draw_circle(cx: i32, cy: i32, radius: u32, color: u32) {
    if radius == 0 {
        set_pixel(cx.max(0) as u32, cy.max(0) as u32, color);
        return;
    }

    let mut x: i32 = radius as i32;
    let mut y: i32 = 0;
    let mut d: i32 = 1 - x;

    while x >= y {
        // Draw the 8 symmetric points.
        circle_point(cx, cy, x, y, color);
        y += 1;
        if d <= 0 {
            d += 2 * y + 1;
        } else {
            x -= 1;
            d += 2 * (y - x) + 1;
        }
    }
}

/// Draw a filled circle centered at (cx, cy) with the given radius.
#[allow(clippy::cast_sign_loss)]
pub fn fill_circle(cx: i32, cy: i32, radius: u32, color: u32) {
    if radius == 0 {
        set_pixel(cx.max(0) as u32, cy.max(0) as u32, color);
        return;
    }

    let mut x: i32 = radius as i32;
    let mut y: i32 = 0;
    let mut d: i32 = 1 - x;

    while x >= y {
        // Fill horizontal spans for the 8 octants (combined into 4 lines).
        fill_rect(cx - x, cy + y, (2 * x + 1) as u32, 1, color);
        fill_rect(cx - x, cy - y, (2 * x + 1) as u32, 1, color);
        fill_rect(cx - y, cy + x, (2 * y + 1) as u32, 1, color);
        fill_rect(cx - y, cy - x, (2 * y + 1) as u32, 1, color);
        y += 1;
        if d <= 0 {
            d += 2 * y + 1;
        } else {
            x -= 1;
            d += 2 * (y - x) + 1;
        }
    }
}

/// Helper: plot a point in all 8 octants.
#[allow(clippy::cast_sign_loss)]
fn circle_point(cx: i32, cy: i32, x: i32, y: i32, color: u32) {
    let points = [
        (cx + x, cy + y),
        (cx - x, cy + y),
        (cx + x, cy - y),
        (cx - x, cy - y),
        (cx + y, cy + x),
        (cx - y, cy + x),
        (cx + y, cy - x),
        (cx - y, cy - x),
    ];
    for (px, py) in points {
        if px >= 0 && py >= 0 {
            set_pixel(px as u32, py as u32, color);
        }
    }
}

// ---------------------------------------------------------------------------
// Mouse cursor
// ---------------------------------------------------------------------------

/// Move the mouse cursor by (dx, dy) pixels and redraw it.
///
/// Call this from the mouse event processing loop.
pub fn move_cursor(dx: i16, dy: i16) {
    if !INITIALIZED.load(Ordering::Acquire) {
        return;
    }

    // Erase old cursor.
    if CURSOR_VISIBLE.load(Ordering::Acquire) {
        draw_cursor_xor();
    }

    // Update position (clamped to screen bounds).
    let width = FB_WIDTH.load(Ordering::Relaxed) as i16;
    let height = FB_HEIGHT.load(Ordering::Relaxed) as i16;

    let old_x = CURSOR_X.load(Ordering::Relaxed);
    let old_y = CURSOR_Y.load(Ordering::Relaxed);

    // PS/2 mouse: positive dy = up, but screen Y increases downward.
    let new_x = (old_x + dx).clamp(0, width.saturating_sub(1));
    let new_y = (old_y - dy).clamp(0, height.saturating_sub(1));

    CURSOR_X.store(new_x, Ordering::Release);
    CURSOR_Y.store(new_y, Ordering::Release);

    // Draw new cursor.
    draw_cursor_xor();
    CURSOR_VISIBLE.store(true, Ordering::Release);
}

/// Show the mouse cursor at its current position.
pub fn show_cursor() {
    if !INITIALIZED.load(Ordering::Acquire) {
        return;
    }
    if !CURSOR_VISIBLE.load(Ordering::Acquire) {
        draw_cursor_xor();
        CURSOR_VISIBLE.store(true, Ordering::Release);
    }
}

/// Hide the mouse cursor.
pub fn hide_cursor() {
    if !INITIALIZED.load(Ordering::Acquire) {
        return;
    }
    if CURSOR_VISIBLE.load(Ordering::Acquire) {
        draw_cursor_xor();
        CURSOR_VISIBLE.store(false, Ordering::Release);
    }
}

/// Get the current cursor position.
pub fn cursor_pos() -> (i16, i16) {
    (CURSOR_X.load(Ordering::Relaxed), CURSOR_Y.load(Ordering::Relaxed))
}

/// Draw (or erase) the cursor using XOR blitting.
///
/// XOR means calling this twice restores the original framebuffer content.
/// This avoids needing a backing store buffer.
fn draw_cursor_xor() {
    let cx = CURSOR_X.load(Ordering::Relaxed);
    let cy = CURSOR_Y.load(Ordering::Relaxed);
    let fb = FB_ADDR.load(Ordering::Relaxed);
    let pitch = FB_PITCH.load(Ordering::Relaxed);
    let width = FB_WIDTH.load(Ordering::Relaxed);
    let height = FB_HEIGHT.load(Ordering::Relaxed);

    for (row_idx, row) in CURSOR_BITMAP.iter().enumerate() {
        #[allow(clippy::cast_possible_truncation)]
        let py = cy as i32 + row_idx as i32;
        if py < 0 || py >= height as i32 {
            continue;
        }
        for (col_idx, &pixel) in row.iter().enumerate() {
            if pixel == 0 {
                continue; // Transparent.
            }
            #[allow(clippy::cast_possible_truncation)]
            let px = cx as i32 + col_idx as i32;
            if px < 0 || px >= width as i32 {
                continue;
            }

            let offset = u64::from(py as u32) * u64::from(pitch)
                + u64::from(px as u32) * 4;
            let addr = (fb + offset) as *mut u32;

            // SAFETY: Bounds checked above.
            unsafe {
                let existing = ptr::read_volatile(addr);
                let cursor_color = match pixel {
                    1 => 0xFF00_0000, // Black outline
                    _ => 0xFFFF_FFFF, // White fill
                };
                // XOR the cursor color with existing framebuffer content.
                ptr::write_volatile(addr, existing ^ cursor_color);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Bitmap blitting
// ---------------------------------------------------------------------------

/// Blit a rectangular region from a source buffer to the framebuffer.
///
/// `src` is a row-major array of BGRA u32 pixels.
/// `src_w` is the width of the source buffer in pixels (stride).
/// `dst_x`, `dst_y` is where to place the top-left corner on screen.
/// `w`, `h` is the size of the region to copy.
///
/// Pixels with alpha < 128 are treated as transparent (not drawn).
#[allow(clippy::cast_sign_loss)]
pub fn blit(src: &[u32], src_w: u32, dst_x: i32, dst_y: i32, w: u32, h: u32) {
    if !INITIALIZED.load(Ordering::Acquire) {
        return;
    }
    let fb = FB_ADDR.load(Ordering::Relaxed);
    let pitch = FB_PITCH.load(Ordering::Relaxed);
    let fb_w = FB_WIDTH.load(Ordering::Relaxed);
    let fb_h = FB_HEIGHT.load(Ordering::Relaxed);

    for row in 0..h {
        let screen_y = dst_y + row as i32;
        if screen_y < 0 || screen_y >= fb_h as i32 {
            continue;
        }
        for col in 0..w {
            let screen_x = dst_x + col as i32;
            if screen_x < 0 || screen_x >= fb_w as i32 {
                continue;
            }
            let src_idx = (row * src_w + col) as usize;
            if src_idx >= src.len() {
                continue;
            }
            let pixel = src[src_idx];
            let alpha = pixel >> 24;
            if alpha < 128 {
                continue; // Transparent.
            }

            let offset = u64::from(screen_y as u32) * u64::from(pitch)
                + u64::from(screen_x as u32) * 4;
            // SAFETY: Bounds checked above.
            unsafe {
                ptr::write_volatile((fb + offset) as *mut u32, pixel | 0xFF00_0000);
            }
        }
    }
}

/// Fill the entire screen with a solid color.
pub fn clear_screen(color: u32) {
    let fb_w = FB_WIDTH.load(Ordering::Relaxed);
    let fb_h = FB_HEIGHT.load(Ordering::Relaxed);
    fill_rect(0, 0, fb_w, fb_h, color);
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Basic self-test: verify initialization and draw a test pattern.
pub fn self_test() -> Result<(), &'static str> {
    if !INITIALIZED.load(Ordering::Acquire) {
        return Err("framebuffer graphics not initialized");
    }

    let (w, h) = dimensions();
    if w == 0 || h == 0 {
        return Err("zero-dimension framebuffer");
    }

    // Test pixel read/write.
    let test_x = w / 2;
    let test_y = h / 2;
    let original = get_pixel(test_x, test_y);
    set_pixel(test_x, test_y, 0xFF_FF_00_FF); // Magenta
    let readback = get_pixel(test_x, test_y);
    set_pixel(test_x, test_y, original); // Restore
    if readback != 0xFF_FF_00_FF {
        return Err("pixel read/write mismatch");
    }

    // Test color helpers.
    let c = rgb(0x12, 0x34, 0x56);
    if c != 0xFF_12_34_56 {
        return Err("rgb() produces wrong value");
    }
    let c2 = rgba(0xAB, 0xCD, 0xEF, 0x80);
    if c2 != 0x80_AB_CD_EF {
        return Err("rgba() produces wrong value");
    }

    // Test blend.
    let blended = blend(rgba(0xFF, 0xFF, 0xFF, 0x80), rgb(0, 0, 0));
    // Expected: approximately rgb(128, 128, 128)
    let r = (blended >> 16) & 0xFF;
    let g = (blended >> 8) & 0xFF;
    let b = blended & 0xFF;
    if r < 126 || r > 130 || g < 126 || g > 130 || b < 126 || b > 130 {
        return Err("blend() produces wrong value");
    }

    crate::serial_println!("[fb] Self-test passed ({}x{} framebuffer)", w, h);
    Ok(())
}
