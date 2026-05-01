//! Framebuffer text console for kernel output.
//!
//! Renders text to a linear framebuffer provided by the Limine bootloader
//! using an 8x16 bitmap font.  The console maintains cursor position,
//! handles newlines/tabs/carriage returns, scrolls when the cursor
//! reaches the bottom, and mirrors all output to the serial port for
//! debugging.
//!
//! ## Pixel format
//!
//! The framebuffer uses 32-bit BGRA pixels (Blue in the low byte,
//! then Green, Red, Alpha).  Each pixel is written as a `u32`.
//!
//! ## Thread safety
//!
//! All mutable state is behind a `spin::Mutex`.  The public API acquires
//! the lock internally, so callers do not need to worry about
//! synchronization.

use core::fmt;
use core::ptr;
use spin::Mutex;

use crate::font;

// ---------------------------------------------------------------------------
// Colors (BGRA format: 0xAARRGGBB stored as u32 in little-endian memory)
// ---------------------------------------------------------------------------

/// Foreground: light gray (0xCCCCCC), fully opaque.
const FG_COLOR: u32 = 0x00CC_CCCC;

/// Background: black, fully opaque.
const BG_COLOR: u32 = 0x0000_0000;

/// Glyph dimensions in pixels.
const GLYPH_WIDTH: u32 = 8;
const GLYPH_HEIGHT: u32 = 16;

/// Tab stop interval in columns.
const TAB_STOP: u32 = 8;

// ---------------------------------------------------------------------------
// Console state
// ---------------------------------------------------------------------------

/// Internal console state, protected by a mutex.
struct ConsoleInner {
    /// Virtual address of the framebuffer start.
    fb_addr: u64,
    /// Framebuffer width in pixels.
    fb_width: u32,
    /// Framebuffer height in pixels.
    fb_height: u32,
    /// Bytes per row in the framebuffer (may include padding beyond width).
    fb_pitch: u32,
    /// Number of text columns (fb_width / GLYPH_WIDTH).
    cols: u32,
    /// Number of text rows (fb_height / GLYPH_HEIGHT).
    rows: u32,
    /// Current cursor column (0-based).
    cursor_col: u32,
    /// Current cursor row (0-based).
    cursor_row: u32,
    /// Whether init() has been called.
    initialized: bool,
}

impl ConsoleInner {
    /// Create an uninitialized console.
    const fn new() -> Self {
        Self {
            fb_addr: 0,
            fb_width: 0,
            fb_height: 0,
            fb_pitch: 0,
            cols: 0,
            rows: 0,
            cursor_col: 0,
            cursor_row: 0,
            initialized: false,
        }
    }
}

/// Global console state.
static CONSOLE: Mutex<ConsoleInner> = Mutex::new(ConsoleInner::new());

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the framebuffer console.
///
/// After this call, [`putchar`], [`write_str`], [`clear`], and the
/// `console_println!` macro can render text to the screen.
///
/// # Safety
///
/// - `addr` must be a valid virtual address pointing to a mapped
///   framebuffer of at least `height * pitch` bytes.
/// - `bpp` must be 32 (only 32-bit BGRA is supported).
/// - Must be called exactly once, before any other console functions.
///
/// # Panics
///
/// Does not panic.  If `bpp` is not 32 the console remains
/// uninitialized and all output is silently dropped (serial still
/// works).
// Pixel math uses checked / saturating arithmetic; the truncations from
// u64 to u32 are intentional for dimension fields that are always small.
#[allow(clippy::cast_possible_truncation)]
pub unsafe fn init(addr: u64, width: u32, height: u32, pitch: u32, bpp: u16) {
    if bpp != 32 {
        crate::serial_println!(
            "[console] WARNING: unsupported bpp {} (expected 32), console disabled",
            bpp
        );
        return;
    }

    let cols = width / GLYPH_WIDTH;
    let rows = height / GLYPH_HEIGHT;

    let mut con = CONSOLE.lock();
    con.fb_addr = addr;
    con.fb_width = width;
    con.fb_height = height;
    con.fb_pitch = pitch;
    con.cols = cols;
    con.rows = rows;
    con.cursor_col = 0;
    con.cursor_row = 0;
    con.initialized = true;

    // Clear the screen to the background color so we start fresh.
    drop(con);
    clear();

    crate::serial_println!(
        "[console] Framebuffer console: {}x{} chars ({}x{} px)",
        cols,
        rows,
        width,
        height
    );
}

// ---------------------------------------------------------------------------
// Screen operations
// ---------------------------------------------------------------------------

/// Fill the entire screen with the background color.
pub fn clear() {
    let con = CONSOLE.lock();
    if !con.initialized {
        return;
    }

    let fb = con.fb_addr;
    let width = con.fb_width;
    let height = con.fb_height;
    let pitch = con.fb_pitch;
    drop(con);

    // Write each pixel row-by-row.  We respect `pitch` which may
    // include padding bytes beyond the visible width.
    for y in 0..height {
        for x in 0..width {
            put_pixel(fb, pitch, x, y, BG_COLOR);
        }
    }

    // Reset cursor to top-left.
    let mut con = CONSOLE.lock();
    con.cursor_col = 0;
    con.cursor_row = 0;
}

/// Render a single character at the current cursor position and advance
/// the cursor.
///
/// Handles `\n` (newline), `\r` (carriage return), and `\t` (tab).
/// Non-printable characters outside those three are ignored.
pub fn putchar(c: u8) {
    let mut con = CONSOLE.lock();
    if !con.initialized {
        return;
    }

    match c {
        b'\n' => {
            con.cursor_col = 0;
            con.cursor_row = con.cursor_row.saturating_add(1);
            if con.cursor_row >= con.rows {
                scroll_up_locked(&mut con);
            }
        }
        b'\r' => {
            con.cursor_col = 0;
        }
        b'\x08' => {
            // Backspace: move cursor back one column (if not at start
            // of line).  Does NOT erase the character — the caller is
            // responsible for overwriting with a space if desired.
            if con.cursor_col > 0 {
                con.cursor_col = con.cursor_col.saturating_sub(1);
            }
        }
        b'\t' => {
            // Advance to the next tab stop (multiple of TAB_STOP).
            // If already at or past the last tab stop on the line,
            // wrap to the next line.
            let next = (con.cursor_col / TAB_STOP).saturating_add(1).saturating_mul(TAB_STOP);
            if next >= con.cols {
                con.cursor_col = 0;
                con.cursor_row = con.cursor_row.saturating_add(1);
                if con.cursor_row >= con.rows {
                    scroll_up_locked(&mut con);
                }
            } else {
                con.cursor_col = next;
            }
        }
        _ => {
            // Render the glyph at the current cursor position.
            let col = con.cursor_col;
            let row = con.cursor_row;
            let fb = con.fb_addr;
            let pitch = con.fb_pitch;

            draw_glyph(fb, pitch, col, row, c);

            // Advance cursor.
            con.cursor_col = col.saturating_add(1);
            if con.cursor_col >= con.cols {
                con.cursor_col = 0;
                con.cursor_row = con.cursor_row.saturating_add(1);
                if con.cursor_row >= con.rows {
                    scroll_up_locked(&mut con);
                }
            }
        }
    }
}

/// Render a string to the console.
///
/// Each byte is passed through [`putchar`].  Also mirrors the string
/// to the serial port for debugging.
pub fn write_str(s: &str) {
    // Mirror to serial first so it appears even if the framebuffer is
    // not yet initialized.
    crate::serial_print!("{}", s);

    for byte in s.bytes() {
        putchar(byte);
    }
}

// ---------------------------------------------------------------------------
// fmt::Write implementation — enables write!() / writeln!()
// ---------------------------------------------------------------------------

/// A handle to the global console for use with `core::fmt::Write`.
///
/// This is a zero-sized type; all state lives in the `CONSOLE` static.
pub struct ConsoleWriter;

impl fmt::Write for ConsoleWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        // Mirror to serial.
        crate::serial_print!("{}", s);

        for byte in s.bytes() {
            putchar(byte);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Macros
// ---------------------------------------------------------------------------

/// Print formatted text to the framebuffer console (and serial).
#[macro_export]
macro_rules! console_print {
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        let mut writer = $crate::console::ConsoleWriter;
        let _ = write!(writer, $($arg)*);
    }};
}

/// Print formatted text to the framebuffer console (and serial) with
/// a trailing newline.
#[macro_export]
macro_rules! console_println {
    ()            => { $crate::console_print!("\n") };
    ($($arg:tt)*) => { $crate::console_print!("{}\n", format_args!($($arg)*)) };
}

// ---------------------------------------------------------------------------
// Boot progress display
// ---------------------------------------------------------------------------

/// Boot step status for the framebuffer display.
#[derive(Debug, Clone, Copy)]
pub enum BootStatus {
    /// Step is in progress (yellow dot).
    Running,
    /// Step completed successfully (green checkmark).
    Ok,
    /// Step failed but is non-fatal (red X, boot continues).
    #[allow(dead_code)] // Used by boot_step_fail in main.rs.
    Warn,
}

/// Accent color: green for success indicators (BGRA: 0x00_66CC66).
const COLOR_GREEN: u32 = 0x0066_CC66;

/// Accent color: yellow for in-progress indicators (BGRA: 0x00_CCCC33).
const COLOR_YELLOW: u32 = 0x00CC_CC33;

/// Accent color: red-ish for warnings (BGRA: 0x00_CC6666).
const COLOR_RED: u32 = 0x00CC_6666;

/// Dim color for boot step descriptions.
const COLOR_DIM: u32 = 0x0099_9999;

/// Show a boot progress step on the framebuffer console.
///
/// Prints a colored status indicator followed by the step description.
/// Call with `BootStatus::Running` when starting a step, then call
/// `boot_step_update` with `BootStatus::Ok` when it completes.
///
/// Format:  `  [*] Description...`  (Running)
///          `  [✓] Description...`  (Ok)
///          `  [!] Description...`  (Warn)
pub fn boot_step(status: BootStatus, description: &str) {
    let mut con = CONSOLE.lock();
    if !con.initialized {
        return;
    }

    let fb = con.fb_addr;
    let pitch = con.fb_pitch;
    let row = con.cursor_row;

    // Clear the current line (overwrite any previous content for updates).
    let cols = con.cols;
    for c in 0..cols {
        draw_glyph(fb, pitch, c, row, b' ');
    }

    // Draw status indicator with color.
    let (indicator, color) = match status {
        BootStatus::Running => (b'*', COLOR_YELLOW),
        BootStatus::Ok      => (b'+', COLOR_GREEN),
        BootStatus::Warn    => (b'!', COLOR_RED),
    };

    // "  [" prefix
    draw_glyph(fb, pitch, 0, row, b' ');
    draw_glyph(fb, pitch, 1, row, b' ');
    draw_glyph(fb, pitch, 2, row, b'[');

    // Colored indicator character
    draw_glyph_colored(fb, pitch, 3, row, indicator, color);

    // "] " suffix
    draw_glyph(fb, pitch, 4, row, b']');
    draw_glyph(fb, pitch, 5, row, b' ');

    // Description in dim text
    let max_desc = (cols as usize).saturating_sub(6);
    for (i, &byte) in description.as_bytes().iter().take(max_desc).enumerate() {
        #[allow(clippy::cast_possible_truncation)]
        let col = 6u32.wrapping_add(i as u32);
        draw_glyph_colored(fb, pitch, col, row, byte, COLOR_DIM);
    }

    // Only advance to next line on Running (Ok/Warn overwrites current line).
    if matches!(status, BootStatus::Running) {
        con.cursor_col = 0;
        con.cursor_row = row.wrapping_add(1);
        if con.cursor_row >= con.rows {
            scroll_up_locked(&mut con);
        }
    }
}

/// Update the most recent boot step's status (overwrites the previous line).
///
/// Moves the cursor back to the previous line, redraws with the new status,
/// and advances again.  Use after `boot_step(Running, ...)` to show success.
pub fn boot_step_update(status: BootStatus, description: &str) {
    let mut con = CONSOLE.lock();
    if !con.initialized {
        return;
    }
    // Move cursor back to the previous line.
    if con.cursor_row > 0 {
        con.cursor_row = con.cursor_row.wrapping_sub(1);
    }
    drop(con);

    boot_step(status, description);

    // Advance cursor past the updated line.
    let mut con = CONSOLE.lock();
    con.cursor_col = 0;
    con.cursor_row = con.cursor_row.wrapping_add(1);
    if con.cursor_row >= con.rows {
        scroll_up_locked(&mut con);
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Write a single 32-bit pixel to the framebuffer.
///
/// `fb` is the framebuffer base virtual address, `pitch` is bytes per
/// row, `x`/`y` are pixel coordinates.
#[inline]
fn put_pixel(fb: u64, pitch: u32, x: u32, y: u32, color: u32) {
    // Byte offset = y * pitch + x * 4 (32 bpp = 4 bytes per pixel).
    // Use u64 arithmetic to avoid overflow on large framebuffers.
    let offset = u64::from(y)
        .wrapping_mul(u64::from(pitch))
        .wrapping_add(u64::from(x).wrapping_mul(4));
    let addr = fb.wrapping_add(offset);

    // SAFETY: The caller of init() guarantees the framebuffer covers
    // at least height*pitch bytes starting at fb.  We only write
    // within the bounds established by fb_width and fb_height.
    // write_volatile ensures the store is not elided by the compiler
    // (the framebuffer is memory-mapped I/O).
    unsafe {
        ptr::write_volatile(addr as *mut u32, color);
    }
}

/// Draw a single glyph at text position (col, row) using default colors.
fn draw_glyph(fb: u64, pitch: u32, col: u32, row: u32, ch: u8) {
    draw_glyph_colored(fb, pitch, col, row, ch, FG_COLOR);
}

/// Draw a single glyph at text position (col, row) with a custom
/// foreground color.  Background is always [`BG_COLOR`].
fn draw_glyph_colored(fb: u64, pitch: u32, col: u32, row: u32, ch: u8, fg: u32) {
    let glyph = font::glyph(ch);
    let px_x = col.wrapping_mul(GLYPH_WIDTH);
    let px_y = row.wrapping_mul(GLYPH_HEIGHT);

    for (gy, &glyph_row) in glyph.iter().enumerate() {
        // gy is in 0..16, fits in u32.
        #[allow(clippy::cast_possible_truncation)]
        let y = px_y.wrapping_add(gy as u32);
        for gx in 0..GLYPH_WIDTH {
            let x = px_x.wrapping_add(gx);
            // MSB (bit 7) is the leftmost pixel.  Check whether the
            // bit at position (7 - gx) is set.
            let shift = 7u32.wrapping_sub(gx);
            // shift is always 0..7, safe for u8.
            #[allow(clippy::cast_possible_truncation)]
            let bit = (glyph_row >> (shift as u8)) & 1;
            let color = if bit != 0 { fg } else { BG_COLOR };
            put_pixel(fb, pitch, x, y, color);
        }
    }
}

/// Scroll the screen up by one text row (GLYPH_HEIGHT pixels).
///
/// The caller must hold the `CONSOLE` lock.
///
/// Copies all rows up by one glyph height using `core::ptr::copy`,
/// then clears the last row to the background color.
fn scroll_up_locked(con: &mut ConsoleInner) {
    let fb = con.fb_addr;
    let pitch = con.fb_pitch;
    let rows = con.rows;

    // Total pixel rows to copy = (rows - 1) * GLYPH_HEIGHT.
    let copy_pixel_rows = rows.saturating_sub(1).saturating_mul(GLYPH_HEIGHT);

    // Use ptr::copy (memmove-equivalent) to shift the framebuffer up.
    // Each pixel row is `pitch` bytes wide.
    let src_offset = u64::from(GLYPH_HEIGHT).wrapping_mul(u64::from(pitch));
    let src = fb.wrapping_add(src_offset) as *const u8;
    let dst = fb as *mut u8;

    // Total bytes to copy.
    if let Some(byte_count) = u64::from(copy_pixel_rows).checked_mul(u64::from(pitch)) {
        // SAFETY: Both src and dst are within the framebuffer (which
        // spans at least rows * GLYPH_HEIGHT * pitch bytes).  ptr::copy
        // handles overlapping regions correctly (like memmove).
        unsafe {
            ptr::copy(src, dst, byte_count as usize);
        }
    }

    // Clear the last row.
    let last_row_start_y = rows.saturating_sub(1).saturating_mul(GLYPH_HEIGHT);
    let fb_width = con.fb_width;
    for y in last_row_start_y..last_row_start_y.saturating_add(GLYPH_HEIGHT) {
        for x in 0..fb_width {
            put_pixel(fb, pitch, x, y, BG_COLOR);
        }
    }

    // Place cursor at the start of the (now cleared) last row.
    con.cursor_row = rows.saturating_sub(1);
    con.cursor_col = 0;
}
