//! VT100/xterm-compatible framebuffer text console.
//!
//! Renders text to a linear framebuffer provided by the Limine bootloader
//! using an 8x16 bitmap font.  The console maintains cursor position,
//! handles newlines/tabs/carriage returns, scrolls when the cursor
//! reaches the bottom, and mirrors all output to the serial port for
//! debugging.
//!
//! ## ANSI/VT100 escape sequence support
//!
//! The console implements a comprehensive subset of VT100/xterm escape
//! sequences, sufficient for curses-based programs (nano, less, vi):
//!
//! - **Cursor movement**: CUU(A), CUD(B), CUF(C), CUB(D), CUP(H/f),
//!   CNL(E), CPL(F), CHA(G), VPA(d)
//! - **Cursor save/restore**: ESC 7/8 (DECSC/DECRC with attributes),
//!   ESC[s/u (SCP/RCP position only)
//! - **Erase**: ED(J) 0/1/2/3, EL(K) 0/1/2, ECH(X)
//! - **Insert/delete**: ICH(@), DCH(P), IL(L), DL(M)
//! - **Scroll**: SU(S), SD(T), DECSTBM(r) scroll regions,
//!   ESC D (IND), ESC M (RI), ESC E (NEL)
//! - **SGR attributes**: bold(1), dim(2), underline(4), reverse(7),
//!   invisible(8), strikethrough(9), and their off codes
//! - **Colors**: 16 standard ANSI, 256-color (38;5;n), truecolor (38;2;r;g;b)
//! - **DEC private modes**: ?25 cursor visibility, ?1049 alt screen,
//!   ?7 auto-wrap
//! - **Device status**: DSR(6n) cursor position report
//! - **Reset**: ESC c (RIS)
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

// ---------------------------------------------------------------------------
// ANSI color table (standard 8 + bright 8 = 16 colors)
// ---------------------------------------------------------------------------

/// Standard ANSI 16-color palette (BGRA format).
const ANSI_COLORS: [u32; 16] = [
    0x0000_0000, // 0  Black
    0x00AA_0000, // 1  Red
    0x0000_AA00, // 2  Green
    0x00AA_5500, // 3  Brown/Yellow
    0x0000_00AA, // 4  Blue
    0x00AA_00AA, // 5  Magenta
    0x0000_AAAA, // 6  Cyan
    0x00AA_AAAA, // 7  White (light gray)
    0x0055_5555, // 8  Bright black (dark gray)
    0x00FF_5555, // 9  Bright red
    0x0055_FF55, // 10 Bright green
    0x00FF_FF55, // 11 Bright yellow
    0x0055_55FF, // 12 Bright blue
    0x00FF_55FF, // 13 Bright magenta
    0x0055_FFFF, // 14 Bright cyan
    0x00FF_FFFF, // 15 Bright white
];

/// ANSI escape sequence parser state.
#[derive(Clone, Copy, PartialEq)]
enum AnsiState {
    /// Normal character output.
    Normal,
    /// Saw ESC (0x1B), waiting for '[' or other control character.
    Escape,
    /// In a CSI sequence (ESC [), accumulating parameter bytes.
    Csi,
}

/// Internal console state, protected by a mutex.
// The console needs many boolean flags for VT100 attribute tracking
// (bold, dim, underline, reverse, invisible, strikethrough, etc.).
#[allow(clippy::struct_excessive_bools)]
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
    /// Current foreground color.
    fg_color: u32,
    /// Current background color.
    bg_color: u32,
    /// Whether bold/bright mode is active.
    bold: bool,
    /// Whether dim/faint mode is active (SGR 2).
    dim: bool,
    /// Whether underline mode is active (SGR 4).
    underline: bool,
    /// Whether reverse video mode is active (SGR 7).
    reverse: bool,
    /// Whether invisible/hidden mode is active (SGR 8).
    invisible: bool,
    /// Whether strikethrough mode is active (SGR 9).
    strikethrough: bool,
    /// Scroll region top row (inclusive, 0-based). Default = 0.
    scroll_top: u32,
    /// Scroll region bottom row (inclusive, 0-based). Default = rows - 1.
    scroll_bottom: u32,
    /// Saved cursor position (column) — for ESC 7 / ESC [ s.
    saved_cursor_col: u32,
    /// Saved cursor position (row) — for ESC 7 / ESC [ s.
    saved_cursor_row: u32,
    /// Saved foreground color (for DEC cursor save with attributes).
    saved_fg_color: u32,
    /// Saved background color (for DEC cursor save with attributes).
    saved_bg_color: u32,
    /// Saved bold state.
    saved_bold: bool,
    /// Whether the CSI sequence has a '?' prefix (DEC private mode).
    ansi_private: bool,
    /// ANSI escape sequence parser state.
    ansi_state: AnsiState,
    /// CSI parameter accumulator (up to 8 parameters).
    ansi_params: [u16; 8],
    /// Number of accumulated parameters.
    ansi_param_count: usize,
    /// Current parameter being accumulated.
    ansi_cur_param: u16,
    /// Whether we've seen a digit for the current parameter.
    ansi_has_digit: bool,
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
            fg_color: FG_COLOR,
            bg_color: BG_COLOR,
            bold: false,
            dim: false,
            underline: false,
            reverse: false,
            invisible: false,
            strikethrough: false,
            scroll_top: 0,
            scroll_bottom: 0, // Set properly in init()
            saved_cursor_col: 0,
            saved_cursor_row: 0,
            saved_fg_color: FG_COLOR,
            saved_bg_color: BG_COLOR,
            saved_bold: false,
            ansi_private: false,
            ansi_state: AnsiState::Normal,
            ansi_params: [0; 8],
            ansi_param_count: 0,
            ansi_cur_param: 0,
            ansi_has_digit: false,
        }
    }

    /// Reset ANSI parser state.
    fn ansi_reset(&mut self) {
        self.ansi_state = AnsiState::Normal;
        self.ansi_params = [0; 8];
        self.ansi_param_count = 0;
        self.ansi_cur_param = 0;
        self.ansi_has_digit = false;
        self.ansi_private = false;
    }

    /// Finalize the current CSI parameter and start a new one.
    fn ansi_next_param(&mut self) {
        if self.ansi_param_count < 8 {
            self.ansi_params[self.ansi_param_count] = self.ansi_cur_param;
            self.ansi_param_count += 1;
        }
        self.ansi_cur_param = 0;
        self.ansi_has_digit = false;
    }

    /// Get a CSI parameter by index, defaulting to `default` if not present.
    fn ansi_param(&self, idx: usize, default: u16) -> u16 {
        if idx < self.ansi_param_count {
            let v = self.ansi_params[idx];
            if v == 0 && !self.ansi_has_digit { default } else { v }
        } else {
            default
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
    con.scroll_top = 0;
    con.scroll_bottom = rows.saturating_sub(1);
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
// Framebuffer info accessor
// ---------------------------------------------------------------------------

/// Framebuffer parameters for use by other modules (e.g., the graphics layer).
///
/// Returns `None` if the console is not initialized.
pub fn framebuffer_info() -> Option<(u64, u32, u32, u32)> {
    let con = CONSOLE.lock();
    if !con.initialized {
        return None;
    }
    Some((con.fb_addr, con.fb_width, con.fb_height, con.fb_pitch))
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
/// Handles `\n` (newline), `\r` (carriage return), `\t` (tab), and
/// ANSI/VT100 escape sequences (colors, cursor movement, screen clearing).
pub fn putchar(c: u8) {
    let mut con = CONSOLE.lock();
    if !con.initialized {
        return;
    }

    match con.ansi_state {
        AnsiState::Normal => putchar_normal(&mut con, c),
        AnsiState::Escape => putchar_escape(&mut con, c),
        AnsiState::Csi => putchar_csi(&mut con, c),
    }
}

/// Handle a character in normal (non-escape) mode.
fn putchar_normal(con: &mut ConsoleInner, c: u8) {
    match c {
        0x1B => {
            // ESC — start of an escape sequence.
            con.ansi_state = AnsiState::Escape;
        }
        b'\n' => {
            con.cursor_col = 0;
            if con.cursor_row >= con.scroll_bottom {
                // At scroll region bottom — scroll the region up.
                scroll_up_locked(con);
            } else if con.cursor_row >= con.rows.saturating_sub(1) {
                // At absolute bottom — scroll full screen.
                scroll_up_locked(con);
            } else {
                con.cursor_row = con.cursor_row.saturating_add(1);
            }
        }
        b'\r' => {
            con.cursor_col = 0;
        }
        b'\x08' => {
            if con.cursor_col > 0 {
                con.cursor_col = con.cursor_col.saturating_sub(1);
            }
        }
        b'\t' => {
            let next = (con.cursor_col / TAB_STOP).saturating_add(1).saturating_mul(TAB_STOP);
            if next >= con.cols {
                con.cursor_col = 0;
                if con.cursor_row >= con.scroll_bottom {
                    scroll_up_locked(con);
                } else if con.cursor_row >= con.rows.saturating_sub(1) {
                    scroll_up_locked(con);
                } else {
                    con.cursor_row = con.cursor_row.saturating_add(1);
                }
            } else {
                con.cursor_col = next;
            }
        }
        _ => {
            // Render the glyph at the current cursor position with
            // attribute-aware colors (reverse, dim, invisible, underline,
            // strikethrough).
            let col = con.cursor_col;
            let row = con.cursor_row;
            let fb = con.fb_addr;
            let pitch = con.fb_pitch;
            let fg = effective_fg(con);
            let bg = effective_bg(con);

            draw_glyph_full(fb, pitch, col, row, c, fg, bg,
                            con.underline, con.strikethrough);

            // Advance cursor.
            con.cursor_col = col.saturating_add(1);
            if con.cursor_col >= con.cols {
                con.cursor_col = 0;
                if con.cursor_row >= con.scroll_bottom {
                    // At scroll region bottom — scroll region up.
                    scroll_up_locked(con);
                } else if con.cursor_row >= con.rows.saturating_sub(1) {
                    // At absolute bottom — scroll full screen.
                    scroll_up_locked(con);
                } else {
                    con.cursor_row = con.cursor_row.saturating_add(1);
                }
            }
        }
    }
}

/// Handle a character after ESC was received.
fn putchar_escape(con: &mut ConsoleInner, c: u8) {
    match c {
        b'[' => {
            // CSI (Control Sequence Introducer) — ESC [
            con.ansi_state = AnsiState::Csi;
            con.ansi_params = [0; 8];
            con.ansi_param_count = 0;
            con.ansi_cur_param = 0;
            con.ansi_has_digit = false;
            con.ansi_private = false;
        }
        b'c' => {
            // RIS (Reset to Initial State) — ESC c
            con.fg_color = FG_COLOR;
            con.bg_color = BG_COLOR;
            con.bold = false;
            con.dim = false;
            con.underline = false;
            con.reverse = false;
            con.invisible = false;
            con.strikethrough = false;
            con.scroll_top = 0;
            con.scroll_bottom = con.rows.saturating_sub(1);
            con.ansi_reset();
        }
        b'7' => {
            // DECSC (Save Cursor) — ESC 7
            con.saved_cursor_col = con.cursor_col;
            con.saved_cursor_row = con.cursor_row;
            con.saved_fg_color = con.fg_color;
            con.saved_bg_color = con.bg_color;
            con.saved_bold = con.bold;
            con.ansi_reset();
        }
        b'8' => {
            // DECRC (Restore Cursor) — ESC 8
            con.cursor_col = con.saved_cursor_col;
            con.cursor_row = con.saved_cursor_row;
            con.fg_color = con.saved_fg_color;
            con.bg_color = con.saved_bg_color;
            con.bold = con.saved_bold;
            con.ansi_reset();
        }
        b'D' => {
            // IND (Index — move cursor down, scroll if at bottom of region)
            if con.cursor_row >= con.scroll_bottom {
                scroll_region_up(con, 1);
            } else {
                con.cursor_row = con.cursor_row.saturating_add(1);
            }
            con.ansi_reset();
        }
        b'M' => {
            // RI (Reverse Index — move cursor up, scroll down if at top of region)
            if con.cursor_row <= con.scroll_top {
                scroll_region_down(con, 1);
            } else {
                con.cursor_row = con.cursor_row.saturating_sub(1);
            }
            con.ansi_reset();
        }
        b'E' => {
            // NEL (Next Line — cursor to beginning of next line)
            con.cursor_col = 0;
            if con.cursor_row >= con.scroll_bottom {
                scroll_region_up(con, 1);
            } else {
                con.cursor_row = con.cursor_row.saturating_add(1);
            }
            con.ansi_reset();
        }
        _ => {
            // Unknown escape — discard and return to normal.
            con.ansi_reset();
        }
    }
}

/// Handle a character within a CSI sequence (ESC [ ...).
///
/// Supports the standard VT100/xterm CSI commands needed for
/// curses-based terminal programs (nano, less, vi, etc.):
///
/// - Cursor movement: CUU(A), CUD(B), CUF(C), CUB(D), CUP(H/f), CNL(E), CPL(F), CHA(G)
/// - Erase: ED(J), EL(K), ECH(X)
/// - Insert/delete: ICH(@), DCH(P), IL(L), DL(M)
/// - Scroll: SU(S), SD(T), DECSTBM(r)
/// - Cursor save/restore: SCP(s), RCP(u)
/// - SGR attributes: bold, dim, underline, reverse, invisible, strikethrough, colors
/// - DEC private modes: cursor show/hide (?25h/l), alt screen (?1049h/l)
/// - Device status: DSR(n) — cursor position report
// The CSI handler is a large match statement covering ~30 VT100 commands.
// Splitting it further would hurt readability since each arm is 3-5 lines.
// Cursor/scroll arithmetic is small and checked/clamped.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::too_many_lines)]
fn putchar_csi(con: &mut ConsoleInner, c: u8) {
    match c {
        b'0'..=b'9' => {
            // Accumulate digit into current parameter.
            con.ansi_cur_param = con.ansi_cur_param.saturating_mul(10)
                .saturating_add((c - b'0') as u16);
            con.ansi_has_digit = true;
        }
        b';' => {
            // Parameter separator.
            con.ansi_next_param();
        }
        b'?' => {
            // DEC private mode prefix — sets flag for h/l commands.
            con.ansi_private = true;
        }
        // --- Final bytes (command characters) ---
        b'm' => {
            // SGR (Select Graphic Rendition) — colors and attributes.
            con.ansi_next_param();
            handle_sgr(con);
            con.ansi_reset();
        }
        b'H' | b'f' => {
            // CUP (Cursor Position) — ESC [ row ; col H
            con.ansi_next_param();
            let row = con.ansi_param(0, 1).saturating_sub(1) as u32;
            let col = con.ansi_param(1, 1).saturating_sub(1) as u32;
            con.cursor_row = row.min(con.rows.saturating_sub(1));
            con.cursor_col = col.min(con.cols.saturating_sub(1));
            con.ansi_reset();
        }
        b'A' => {
            // CUU (Cursor Up).
            con.ansi_next_param();
            let n = con.ansi_param(0, 1) as u32;
            con.cursor_row = con.cursor_row.saturating_sub(n);
            con.ansi_reset();
        }
        b'B' => {
            // CUD (Cursor Down).
            con.ansi_next_param();
            let n = con.ansi_param(0, 1) as u32;
            con.cursor_row = (con.cursor_row + n).min(con.rows.saturating_sub(1));
            con.ansi_reset();
        }
        b'C' => {
            // CUF (Cursor Forward/Right).
            con.ansi_next_param();
            let n = con.ansi_param(0, 1) as u32;
            con.cursor_col = (con.cursor_col + n).min(con.cols.saturating_sub(1));
            con.ansi_reset();
        }
        b'D' => {
            // CUB (Cursor Back/Left).
            con.ansi_next_param();
            let n = con.ansi_param(0, 1) as u32;
            con.cursor_col = con.cursor_col.saturating_sub(n);
            con.ansi_reset();
        }
        b'E' => {
            // CNL (Cursor Next Line) — move down n lines, to column 1.
            con.ansi_next_param();
            let n = con.ansi_param(0, 1) as u32;
            con.cursor_row = (con.cursor_row + n).min(con.rows.saturating_sub(1));
            con.cursor_col = 0;
            con.ansi_reset();
        }
        b'F' => {
            // CPL (Cursor Previous Line) — move up n lines, to column 1.
            con.ansi_next_param();
            let n = con.ansi_param(0, 1) as u32;
            con.cursor_row = con.cursor_row.saturating_sub(n);
            con.cursor_col = 0;
            con.ansi_reset();
        }
        b'G' => {
            // CHA (Cursor Horizontal Absolute) — move to column n.
            con.ansi_next_param();
            let col = con.ansi_param(0, 1).saturating_sub(1) as u32;
            con.cursor_col = col.min(con.cols.saturating_sub(1));
            con.ansi_reset();
        }
        b'J' => {
            // ED (Erase in Display).
            con.ansi_next_param();
            let mode = con.ansi_param(0, 0);
            handle_erase_display(con, mode);
            con.ansi_reset();
        }
        b'K' => {
            // EL (Erase in Line).
            con.ansi_next_param();
            let mode = con.ansi_param(0, 0);
            handle_erase_line(con, mode);
            con.ansi_reset();
        }
        b'L' => {
            // IL (Insert Lines) — insert n blank lines at cursor row,
            // scrolling existing lines down within the scroll region.
            con.ansi_next_param();
            let n = con.ansi_param(0, 1) as u32;
            handle_insert_lines(con, n);
            con.ansi_reset();
        }
        b'M' => {
            // DL (Delete Lines) — delete n lines at cursor row,
            // scrolling lines below up within the scroll region.
            con.ansi_next_param();
            let n = con.ansi_param(0, 1) as u32;
            handle_delete_lines(con, n);
            con.ansi_reset();
        }
        b'@' => {
            // ICH (Insert Characters) — insert n blank characters at cursor,
            // shifting existing characters right.
            con.ansi_next_param();
            let n = con.ansi_param(0, 1) as u32;
            handle_insert_chars(con, n);
            con.ansi_reset();
        }
        b'P' => {
            // DCH (Delete Characters) — delete n characters at cursor,
            // shifting remaining characters left.
            con.ansi_next_param();
            let n = con.ansi_param(0, 1) as u32;
            handle_delete_chars(con, n);
            con.ansi_reset();
        }
        b'X' => {
            // ECH (Erase Characters) — erase n characters starting at cursor
            // without moving the cursor.
            con.ansi_next_param();
            let n = con.ansi_param(0, 1) as u32;
            let fb = con.fb_addr;
            let pitch = con.fb_pitch;
            let bg = effective_bg(con);
            for i in 0..n {
                let col = con.cursor_col + i;
                if col >= con.cols { break; }
                erase_cell(fb, pitch, col, con.cursor_row, bg);
            }
            con.ansi_reset();
        }
        b'S' => {
            // SU (Scroll Up) — scroll the scroll region up by n lines.
            con.ansi_next_param();
            let n = con.ansi_param(0, 1) as u32;
            scroll_region_up(con, n);
            con.ansi_reset();
        }
        b'T' => {
            // SD (Scroll Down) — scroll the scroll region down by n lines.
            con.ansi_next_param();
            let n = con.ansi_param(0, 1) as u32;
            scroll_region_down(con, n);
            con.ansi_reset();
        }
        b'r' => {
            // DECSTBM (Set Top and Bottom Margins / Scroll Region).
            // ESC [ top ; bottom r
            // Default: top=1, bottom=rows (full screen).
            con.ansi_next_param();
            let top = con.ansi_param(0, 1).saturating_sub(1) as u32;
            let bottom = con.ansi_param(1, con.rows as u16).saturating_sub(1) as u32;
            let max_row = con.rows.saturating_sub(1);
            con.scroll_top = top.min(max_row);
            con.scroll_bottom = bottom.min(max_row);
            // Ensure top < bottom.
            if con.scroll_top >= con.scroll_bottom {
                con.scroll_top = 0;
                con.scroll_bottom = max_row;
            }
            // DECSTBM moves cursor to home position.
            con.cursor_col = 0;
            con.cursor_row = 0;
            con.ansi_reset();
        }
        b's' => {
            // SCP (Save Cursor Position) — ESC [ s
            con.saved_cursor_col = con.cursor_col;
            con.saved_cursor_row = con.cursor_row;
            con.ansi_reset();
        }
        b'u' => {
            // RCP (Restore Cursor Position) — ESC [ u
            con.cursor_col = con.saved_cursor_col.min(con.cols.saturating_sub(1));
            con.cursor_row = con.saved_cursor_row.min(con.rows.saturating_sub(1));
            con.ansi_reset();
        }
        b'd' => {
            // VPA (Vertical Position Absolute) — move to row n.
            con.ansi_next_param();
            let row = con.ansi_param(0, 1).saturating_sub(1) as u32;
            con.cursor_row = row.min(con.rows.saturating_sub(1));
            con.ansi_reset();
        }
        b'n' => {
            // DSR (Device Status Report).
            con.ansi_next_param();
            let mode = con.ansi_param(0, 0);
            if mode == 6 {
                // CPR (Cursor Position Report) — respond with ESC [ row ; col R.
                // In a real terminal this would be sent back via the input
                // stream.  For now, log it to serial for debugging.
                crate::serial_println!(
                    "\x1b[{};{}R",
                    con.cursor_row + 1,
                    con.cursor_col + 1
                );
            }
            con.ansi_reset();
        }
        b'h' | b'l' => {
            // Set (h) / Reset (l) mode.
            con.ansi_next_param();
            if con.ansi_private {
                let mode = con.ansi_param(0, 0);
                let set = c == b'h';
                handle_dec_private_mode(con, mode, set);
            }
            // Non-private modes are silently ignored.
            con.ansi_reset();
        }
        _ => {
            // Unknown command or intermediate byte — abort sequence.
            if c >= 0x40 && c <= 0x7E {
                // Final byte we don't handle.
                con.ansi_reset();
            }
            // If it's an intermediate byte (0x20-0x3F), keep accumulating.
            // But for safety, abort after too many characters.
            if con.ansi_param_count >= 8 {
                con.ansi_reset();
            }
        }
    }
}

/// Handle SGR (Select Graphic Rendition) parameters.
///
/// Supports: reset(0), bold(1), dim(2), underline(4), reverse(7),
/// invisible(8), strikethrough(9), normal intensity(22), no underline(24),
/// no reverse(27), visible(28), no strikethrough(29), foreground colors
/// (30-37, 39, 90-97), background colors (40-47, 49, 100-107),
/// and 256-color (38;5;n / 48;5;n).
fn handle_sgr(con: &mut ConsoleInner) {
    // If no parameters, treat as reset (SGR 0).
    if con.ansi_param_count == 0 {
        con.fg_color = FG_COLOR;
        con.bg_color = BG_COLOR;
        con.bold = false;
        con.dim = false;
        con.underline = false;
        con.reverse = false;
        con.invisible = false;
        con.strikethrough = false;
        return;
    }

    let mut i = 0;
    while i < con.ansi_param_count {
        let param = con.ansi_params[i];
        match param {
            0 => {
                // Reset all attributes.
                con.fg_color = FG_COLOR;
                con.bg_color = BG_COLOR;
                con.bold = false;
                con.dim = false;
                con.underline = false;
                con.reverse = false;
                con.invisible = false;
                con.strikethrough = false;
            }
            1 => {
                // Bold / bright.
                con.bold = true;
            }
            2 => {
                // Dim / faint.
                con.dim = true;
            }
            4 => {
                // Underline.
                con.underline = true;
            }
            7 => {
                // Reverse video.
                con.reverse = true;
            }
            8 => {
                // Invisible / hidden.
                con.invisible = true;
            }
            9 => {
                // Strikethrough (crossed-out).
                con.strikethrough = true;
            }
            22 => {
                // Normal intensity (not bold, not dim).
                con.bold = false;
                con.dim = false;
            }
            24 => {
                // Not underlined.
                con.underline = false;
            }
            27 => {
                // Not reversed.
                con.reverse = false;
            }
            28 => {
                // Visible (not hidden).
                con.invisible = false;
            }
            29 => {
                // Not strikethrough.
                con.strikethrough = false;
            }
            // Standard foreground colors (30-37).
            30..=37 => {
                let idx = (param - 30) as usize;
                let color_idx = if con.bold { idx + 8 } else { idx };
                con.fg_color = ANSI_COLORS[color_idx];
            }
            38 => {
                // Extended foreground color.
                // 38;5;n = 256-color, 38;2;r;g;b = truecolor.
                if i + 1 < con.ansi_param_count && con.ansi_params[i + 1] == 5 {
                    // 256-color mode.
                    if i + 2 < con.ansi_param_count {
                        let n = con.ansi_params[i + 2] as usize;
                        con.fg_color = color_256(n);
                        i += 2; // Skip the 5;n parameters.
                    }
                } else if i + 1 < con.ansi_param_count && con.ansi_params[i + 1] == 2 {
                    // Truecolor mode: 38;2;r;g;b
                    if i + 4 < con.ansi_param_count {
                        let r = (con.ansi_params[i + 2] & 0xFF) as u32;
                        let g = (con.ansi_params[i + 3] & 0xFF) as u32;
                        let b = (con.ansi_params[i + 4] & 0xFF) as u32;
                        con.fg_color = (r << 16) | (g << 8) | b;
                        i += 4;
                    }
                }
            }
            39 => {
                // Default foreground color.
                con.fg_color = FG_COLOR;
            }
            // Standard background colors (40-47).
            40..=47 => {
                let idx = (param - 40) as usize;
                con.bg_color = ANSI_COLORS[idx];
            }
            48 => {
                // Extended background color.
                if i + 1 < con.ansi_param_count && con.ansi_params[i + 1] == 5 {
                    // 256-color mode.
                    if i + 2 < con.ansi_param_count {
                        let n = con.ansi_params[i + 2] as usize;
                        con.bg_color = color_256(n);
                        i += 2;
                    }
                } else if i + 1 < con.ansi_param_count && con.ansi_params[i + 1] == 2 {
                    // Truecolor mode: 48;2;r;g;b
                    if i + 4 < con.ansi_param_count {
                        let r = (con.ansi_params[i + 2] & 0xFF) as u32;
                        let g = (con.ansi_params[i + 3] & 0xFF) as u32;
                        let b = (con.ansi_params[i + 4] & 0xFF) as u32;
                        con.bg_color = (r << 16) | (g << 8) | b;
                        i += 4;
                    }
                }
            }
            49 => {
                // Default background color.
                con.bg_color = BG_COLOR;
            }
            // Bright foreground colors (90-97).
            90..=97 => {
                let idx = (param - 90) as usize + 8;
                con.fg_color = ANSI_COLORS[idx];
            }
            // Bright background colors (100-107).
            100..=107 => {
                let idx = (param - 100) as usize + 8;
                con.bg_color = ANSI_COLORS[idx];
            }
            _ => {
                // Unsupported SGR parameter — ignore.
            }
        }
        i += 1;
    }
}

/// Convert a 256-color index to BGRA u32.
///
/// 0-7: standard colors, 8-15: bright colors, 16-231: 6×6×6 RGB cube,
/// 232-255: 24-step grayscale ramp.
// Color arithmetic is intentionally wrapping/truncating for palette math.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn color_256(n: usize) -> u32 {
    match n {
        0..=15 => ANSI_COLORS[n],
        16..=231 => {
            // 6×6×6 color cube: index = 16 + 36*r + 6*g + b
            let idx = n - 16;
            let b_val = (idx % 6) as u32;
            let g_val = ((idx / 6) % 6) as u32;
            let r_val = (idx / 36) as u32;
            // Scale 0-5 to 0-255: 0→0, 1→95, 2→135, 3→175, 4→215, 5→255
            let scale = |v: u32| -> u32 {
                if v == 0 { 0 } else { 55 + v * 40 }
            };
            (scale(r_val) << 16) | (scale(g_val) << 8) | scale(b_val)
        }
        232..=255 => {
            // Grayscale ramp: 24 shades from 8 to 238.
            let gray = ((n - 232) * 10 + 8) as u32;
            (gray << 16) | (gray << 8) | gray
        }
        _ => FG_COLOR, // Out of range — use default.
    }
}

/// Handle ED (Erase in Display) command.
fn handle_erase_display(con: &mut ConsoleInner, mode: u16) {
    let fb = con.fb_addr;
    let pitch = con.fb_pitch;
    let cols = con.cols;
    let rows = con.rows;
    let bg = con.bg_color;

    match mode {
        0 => {
            // Erase from cursor to end of display.
            // Clear rest of current line.
            for col in con.cursor_col..cols {
                erase_cell(fb, pitch, col, con.cursor_row, bg);
            }
            // Clear all lines below.
            for row in (con.cursor_row + 1)..rows {
                for col in 0..cols {
                    erase_cell(fb, pitch, col, row, bg);
                }
            }
        }
        1 => {
            // Erase from start to cursor.
            for row in 0..con.cursor_row {
                for col in 0..cols {
                    erase_cell(fb, pitch, col, row, bg);
                }
            }
            for col in 0..=con.cursor_col.min(cols.saturating_sub(1)) {
                erase_cell(fb, pitch, col, con.cursor_row, bg);
            }
        }
        2 | 3 => {
            // Erase entire display.
            for row in 0..rows {
                for col in 0..cols {
                    erase_cell(fb, pitch, col, row, bg);
                }
            }
            // Also reset cursor for mode 2.
            if mode == 2 {
                con.cursor_col = 0;
                con.cursor_row = 0;
            }
        }
        _ => {}
    }
}

/// Handle EL (Erase in Line) command.
fn handle_erase_line(con: &mut ConsoleInner, mode: u16) {
    let fb = con.fb_addr;
    let pitch = con.fb_pitch;
    let cols = con.cols;
    let row = con.cursor_row;
    let bg = con.bg_color;

    match mode {
        0 => {
            // Erase from cursor to end of line.
            for col in con.cursor_col..cols {
                erase_cell(fb, pitch, col, row, bg);
            }
        }
        1 => {
            // Erase from start of line to cursor.
            for col in 0..=con.cursor_col.min(cols.saturating_sub(1)) {
                erase_cell(fb, pitch, col, row, bg);
            }
        }
        2 => {
            // Erase entire line.
            for col in 0..cols {
                erase_cell(fb, pitch, col, row, bg);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Scroll region operations
// ---------------------------------------------------------------------------

/// Scroll the scroll region up by `n` lines.
///
/// Lines at the top of the region are lost; new blank lines appear at
/// the bottom.  Only the region between `scroll_top` and `scroll_bottom`
/// is affected.
// Row/pixel arithmetic is small and clamped.
#[allow(clippy::arithmetic_side_effects)]
fn scroll_region_up(con: &mut ConsoleInner, n: u32) {
    if !con.initialized { return; }
    let region_height = con.scroll_bottom - con.scroll_top + 1;
    let n = n.min(region_height);
    let fb = con.fb_addr;
    let pitch = con.fb_pitch;
    let cols = con.cols;
    let bg = effective_bg(con);

    // Copy rows: move row (top + n) to row (top), etc.
    for dst_row in con.scroll_top..=(con.scroll_bottom.saturating_sub(n)) {
        let src_row = dst_row + n;
        if src_row > con.scroll_bottom { break; }
        copy_row(fb, pitch, cols, src_row, dst_row);
    }

    // Clear the bottom n rows.
    for row in (con.scroll_bottom + 1 - n)..=con.scroll_bottom {
        for col in 0..cols {
            erase_cell(fb, pitch, col, row, bg);
        }
    }
}

/// Scroll the scroll region down by `n` lines.
///
/// Lines at the bottom of the region are lost; new blank lines appear
/// at the top.
// Row/pixel arithmetic is small and clamped.
#[allow(clippy::arithmetic_side_effects)]
fn scroll_region_down(con: &mut ConsoleInner, n: u32) {
    if !con.initialized { return; }
    let region_height = con.scroll_bottom - con.scroll_top + 1;
    let n = n.min(region_height);
    let fb = con.fb_addr;
    let pitch = con.fb_pitch;
    let cols = con.cols;
    let bg = effective_bg(con);

    // Copy rows bottom-up: move row (bottom - n) to row (bottom), etc.
    let mut dst_row = con.scroll_bottom;
    while dst_row >= con.scroll_top + n {
        let src_row = dst_row - n;
        copy_row(fb, pitch, cols, src_row, dst_row);
        if dst_row == 0 { break; }
        dst_row -= 1;
    }

    // Clear the top n rows.
    for row in con.scroll_top..(con.scroll_top + n).min(con.scroll_bottom + 1) {
        for col in 0..cols {
            erase_cell(fb, pitch, col, row, bg);
        }
    }
}

/// Copy one row of character cells to another row (pixel-level copy).
// Pixel arithmetic is small and bounded by framebuffer dimensions.
#[allow(clippy::arithmetic_side_effects)]
fn copy_row(fb: u64, pitch: u32, cols: u32, src_row: u32, dst_row: u32) {
    let src_y = src_row * GLYPH_HEIGHT;
    let dst_y = dst_row * GLYPH_HEIGHT;
    for gy in 0..GLYPH_HEIGHT {
        let src_offset = ((src_y + gy) as u64) * (pitch as u64);
        let dst_offset = ((dst_y + gy) as u64) * (pitch as u64);
        let row_bytes = (cols * GLYPH_WIDTH * 4) as usize; // 4 bytes per pixel
        // SAFETY: Both source and destination are within the framebuffer
        // (bounded by fb_height * pitch).  The copy is non-overlapping
        // because src_row != dst_row.
        unsafe {
            let src = (fb + src_offset) as *const u8;
            let dst = (fb + dst_offset) as *mut u8;
            ptr::copy_nonoverlapping(src, dst, row_bytes);
        }
    }
}

// ---------------------------------------------------------------------------
// Line insert/delete
// ---------------------------------------------------------------------------

/// Insert `n` blank lines at the cursor's current row.
///
/// Lines below the cursor (within the scroll region) shift down; lines
/// pushed past the scroll region bottom are lost.
// Row arithmetic is small and clamped.
#[allow(clippy::arithmetic_side_effects)]
fn handle_insert_lines(con: &mut ConsoleInner, n: u32) {
    if !con.initialized { return; }
    let cur = con.cursor_row;
    if cur < con.scroll_top || cur > con.scroll_bottom { return; }

    let n = n.min(con.scroll_bottom - cur + 1);
    let fb = con.fb_addr;
    let pitch = con.fb_pitch;
    let cols = con.cols;
    let bg = effective_bg(con);

    // Shift rows down (bottom-up to avoid overwriting).
    let mut dst = con.scroll_bottom;
    while dst >= cur + n {
        let src = dst - n;
        copy_row(fb, pitch, cols, src, dst);
        if dst == 0 { break; }
        dst -= 1;
    }

    // Clear the inserted lines.
    for row in cur..(cur + n).min(con.scroll_bottom + 1) {
        for col in 0..cols {
            erase_cell(fb, pitch, col, row, bg);
        }
    }
    con.cursor_col = 0;
}

/// Delete `n` lines at the cursor's current row.
///
/// Lines below shift up; blank lines appear at the scroll region bottom.
// Row arithmetic is small and clamped.
#[allow(clippy::arithmetic_side_effects)]
fn handle_delete_lines(con: &mut ConsoleInner, n: u32) {
    if !con.initialized { return; }
    let cur = con.cursor_row;
    if cur < con.scroll_top || cur > con.scroll_bottom { return; }

    let n = n.min(con.scroll_bottom - cur + 1);
    let fb = con.fb_addr;
    let pitch = con.fb_pitch;
    let cols = con.cols;
    let bg = effective_bg(con);

    // Shift rows up.
    for dst in cur..=(con.scroll_bottom.saturating_sub(n)) {
        let src = dst + n;
        if src > con.scroll_bottom { break; }
        copy_row(fb, pitch, cols, src, dst);
    }

    // Clear the bottom lines.
    for row in (con.scroll_bottom + 1 - n)..=con.scroll_bottom {
        for col in 0..cols {
            erase_cell(fb, pitch, col, row, bg);
        }
    }
    con.cursor_col = 0;
}

// ---------------------------------------------------------------------------
// Character insert/delete
// ---------------------------------------------------------------------------

/// Insert `n` blank characters at the cursor position.
///
/// Characters to the right shift right; characters pushed past the right
/// margin are lost.
// Column/pixel arithmetic is small and bounded.
#[allow(clippy::arithmetic_side_effects)]
fn handle_insert_chars(con: &mut ConsoleInner, n: u32) {
    if !con.initialized { return; }
    let fb = con.fb_addr;
    let pitch = con.fb_pitch;
    let row = con.cursor_row;
    let bg = effective_bg(con);
    let n = n.min(con.cols - con.cursor_col);

    // Shift characters right (from right to left to avoid overwriting).
    let mut dst_col = con.cols.saturating_sub(1);
    while dst_col >= con.cursor_col + n {
        let src_col = dst_col - n;
        copy_cell(fb, pitch, src_col, row, dst_col, row);
        if dst_col == 0 { break; }
        dst_col -= 1;
    }

    // Clear the inserted positions.
    for col in con.cursor_col..(con.cursor_col + n).min(con.cols) {
        erase_cell(fb, pitch, col, row, bg);
    }
}

/// Delete `n` characters at the cursor position.
///
/// Characters to the right shift left; blank characters appear at the
/// right margin.
// Column/pixel arithmetic is small and bounded.
#[allow(clippy::arithmetic_side_effects)]
fn handle_delete_chars(con: &mut ConsoleInner, n: u32) {
    if !con.initialized { return; }
    let fb = con.fb_addr;
    let pitch = con.fb_pitch;
    let row = con.cursor_row;
    let bg = effective_bg(con);
    let n = n.min(con.cols - con.cursor_col);

    // Shift characters left.
    for dst_col in con.cursor_col..(con.cols.saturating_sub(n)) {
        let src_col = dst_col + n;
        if src_col >= con.cols { break; }
        copy_cell(fb, pitch, src_col, row, dst_col, row);
    }

    // Clear the rightmost positions.
    for col in (con.cols - n)..con.cols {
        erase_cell(fb, pitch, col, row, bg);
    }
}

/// Copy a single character cell from (src_col, src_row) to (dst_col, dst_row).
// Pixel arithmetic is small and bounded by framebuffer dimensions.
#[allow(clippy::arithmetic_side_effects)]
fn copy_cell(fb: u64, pitch: u32, src_col: u32, src_row: u32, dst_col: u32, dst_row: u32) {
    let src_px_x = src_col * GLYPH_WIDTH;
    let src_px_y = src_row * GLYPH_HEIGHT;
    let dst_px_x = dst_col * GLYPH_WIDTH;
    let dst_px_y = dst_row * GLYPH_HEIGHT;
    for gy in 0..GLYPH_HEIGHT {
        for gx in 0..GLYPH_WIDTH {
            let src_x = src_px_x + gx;
            let src_y = src_px_y + gy;
            let dst_x = dst_px_x + gx;
            let dst_y = dst_px_y + gy;
            // SAFETY: Coordinates are within framebuffer bounds (bounded by
            // cols*GLYPH_WIDTH and rows*GLYPH_HEIGHT).
            let color = unsafe {
                let offset = (src_y as u64) * (pitch as u64) + (src_x as u64) * 4;
                ptr::read_volatile((fb + offset) as *const u32)
            };
            put_pixel(fb, pitch, dst_x, dst_y, color);
        }
    }
}

// ---------------------------------------------------------------------------
// DEC private mode handling
// ---------------------------------------------------------------------------

/// Handle DEC private mode set/reset (ESC [ ? N h/l).
///
/// Supports:
/// - ?25: cursor visibility (currently a no-op since we don't draw a
///   hardware cursor, but we track state for compatibility)
/// - ?1049: alternate screen buffer (clear screen on set, restore on reset)
/// - ?7: auto-wrap mode (silently accepted)
fn handle_dec_private_mode(con: &mut ConsoleInner, mode: u16, set: bool) {
    match mode {
        25 => {
            // DECTCEM — cursor visibility.  We don't draw a blinking cursor
            // yet, so this is a no-op.  Programs like vi send this.
        }
        1049 => {
            // Alternate screen buffer.
            if set {
                // Enter alt screen: save cursor and clear display.
                con.saved_cursor_col = con.cursor_col;
                con.saved_cursor_row = con.cursor_row;
                con.saved_fg_color = con.fg_color;
                con.saved_bg_color = con.bg_color;
                con.saved_bold = con.bold;
                con.cursor_col = 0;
                con.cursor_row = 0;
                // Clear the screen.
                let fb = con.fb_addr;
                let pitch = con.fb_pitch;
                let bg = effective_bg(con);
                for row in 0..con.rows {
                    for col in 0..con.cols {
                        erase_cell(fb, pitch, col, row, bg);
                    }
                }
            } else {
                // Leave alt screen: restore cursor.  A real terminal would
                // restore the main screen content, but we don't have a
                // backing store yet — just restore cursor and clear.
                con.cursor_col = con.saved_cursor_col;
                con.cursor_row = con.saved_cursor_row;
                con.fg_color = con.saved_fg_color;
                con.bg_color = con.saved_bg_color;
                con.bold = con.saved_bold;
                let fb = con.fb_addr;
                let pitch = con.fb_pitch;
                let bg = effective_bg(con);
                for row in 0..con.rows {
                    for col in 0..con.cols {
                        erase_cell(fb, pitch, col, row, bg);
                    }
                }
            }
        }
        7 => {
            // DECAWM — auto-wrap mode.  We always auto-wrap, so this is
            // accepted silently for compatibility.
        }
        _ => {
            // Unknown DEC private mode — ignore.
        }
    }
}

// ---------------------------------------------------------------------------
// Attribute helpers
// ---------------------------------------------------------------------------

/// Compute the effective foreground color, accounting for dim and reverse.
fn effective_fg(con: &ConsoleInner) -> u32 {
    if con.invisible {
        return effective_bg(con);
    }
    let fg = if con.reverse { con.bg_color } else { con.fg_color };
    if con.dim {
        // Dim: halve the RGB channels.
        let r = (fg >> 16) & 0xFF;
        let g = (fg >> 8) & 0xFF;
        let b = fg & 0xFF;
        ((r >> 1) << 16) | ((g >> 1) << 8) | (b >> 1)
    } else {
        fg
    }
}

/// Compute the effective background color, accounting for reverse.
fn effective_bg(con: &ConsoleInner) -> u32 {
    if con.reverse { con.fg_color } else { con.bg_color }
}

// ---------------------------------------------------------------------------
// Low-level cell operations
// ---------------------------------------------------------------------------

/// Erase a single character cell (fill with background color).
fn erase_cell(fb: u64, pitch: u32, col: u32, row: u32, bg: u32) {
    let px_x = col.wrapping_mul(GLYPH_WIDTH);
    let px_y = row.wrapping_mul(GLYPH_HEIGHT);
    for gy in 0..GLYPH_HEIGHT {
        for gx in 0..GLYPH_WIDTH {
            put_pixel(fb, pitch, px_x + gx, px_y + gy, bg);
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
    draw_glyph_full(fb, pitch, col, row, ch, fg, BG_COLOR, false, false);
}

/// Draw a single glyph with full attribute support.
///
/// Renders the glyph with custom foreground and background colors,
/// optional underline (horizontal line on row 14 of 16), and optional
/// strikethrough (horizontal line on row 8 of 16).
fn draw_glyph_full(
    fb: u64, pitch: u32, col: u32, row: u32, ch: u8,
    fg: u32, bg: u32, underline: bool, strikethrough: bool,
) {
    let glyph = font::glyph(ch);
    let px_x = col.wrapping_mul(GLYPH_WIDTH);
    let px_y = row.wrapping_mul(GLYPH_HEIGHT);

    for (gy, &glyph_row) in glyph.iter().enumerate() {
        // gy is in 0..16, fits in u32.
        #[allow(clippy::cast_possible_truncation)]
        let y = px_y.wrapping_add(gy as u32);

        // Underline on row 14 (second-to-last row of 16-pixel glyph).
        let is_underline_row = underline && gy == 14;
        // Strikethrough on row 8 (middle of glyph).
        let is_strike_row = strikethrough && gy == 8;

        for gx in 0..GLYPH_WIDTH {
            let x = px_x.wrapping_add(gx);
            // MSB (bit 7) is the leftmost pixel.  Check whether the
            // bit at position (7 - gx) is set.
            let shift = 7u32.wrapping_sub(gx);
            // shift is always 0..7, safe for u8.
            #[allow(clippy::cast_possible_truncation)]
            let bit = (glyph_row >> (shift as u8)) & 1;
            let color = if bit != 0 || is_underline_row || is_strike_row {
                fg
            } else {
                bg
            };
            put_pixel(fb, pitch, x, y, color);
        }
    }
}

/// Scroll the screen up by one text row within the scroll region.
///
/// The caller must hold the `CONSOLE` lock.
///
/// If the scroll region covers the full screen, uses a fast memmove.
/// Otherwise delegates to `scroll_region_up` for partial-screen scrolling.
fn scroll_up_locked(con: &mut ConsoleInner) {
    let fb = con.fb_addr;
    let pitch = con.fb_pitch;
    let rows = con.rows;

    // If cursor is within a scroll region, use region-aware scrolling.
    if con.scroll_top != 0 || con.scroll_bottom != rows.saturating_sub(1) {
        scroll_region_up(con, 1);
        con.cursor_row = con.scroll_bottom;
        con.cursor_col = 0;
        return;
    }

    // Fast path: full-screen scroll via memmove.
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

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test: exercise the ANSI escape sequence parser.
///
/// Tests state machine transitions, parameter parsing, cursor movement,
/// SGR attribute handling, scroll region, and color support without
/// relying on visual inspection — checks internal state via the CONSOLE
/// lock.
pub fn self_test() {
    crate::serial_println!("[console] Running self-test...");

    // Test 1: Basic initialization state.
    {
        let con = CONSOLE.lock();
        assert!(con.initialized, "console not initialized");
        assert!(con.cols > 0 && con.rows > 0, "invalid dimensions");
        crate::serial_println!("[console]   Dimensions: {}x{} OK", con.cols, con.rows);
    }

    // Test 2: Cursor positioning via CSI H.
    {
        // Move to row 5, col 10 (1-based).
        write_str_no_serial("\x1b[5;10H");
        let con = CONSOLE.lock();
        assert_eq!(con.cursor_row, 4, "CUP row");
        assert_eq!(con.cursor_col, 9, "CUP col");
        crate::serial_println!("[console]   CUP cursor positioning: OK");
    }

    // Test 3: Relative cursor movement.
    {
        write_str_no_serial("\x1b[1;1H"); // Home
        write_str_no_serial("\x1b[3B");    // Down 3
        write_str_no_serial("\x1b[5C");    // Right 5
        let con = CONSOLE.lock();
        assert_eq!(con.cursor_row, 3, "CUD");
        assert_eq!(con.cursor_col, 5, "CUF");
        drop(con);

        write_str_no_serial("\x1b[2A");    // Up 2
        write_str_no_serial("\x1b[1D");    // Left 1
        let con = CONSOLE.lock();
        assert_eq!(con.cursor_row, 1, "CUU");
        assert_eq!(con.cursor_col, 4, "CUB");
        crate::serial_println!("[console]   Relative cursor movement: OK");
    }

    // Test 4: CHA and VPA.
    {
        write_str_no_serial("\x1b[1;1H"); // Home
        write_str_no_serial("\x1b[15G");   // Column 15
        let con = CONSOLE.lock();
        assert_eq!(con.cursor_col, 14, "CHA");
        drop(con);

        write_str_no_serial("\x1b[8d");    // Row 8
        let con = CONSOLE.lock();
        assert_eq!(con.cursor_row, 7, "VPA");
        crate::serial_println!("[console]   CHA/VPA absolute positioning: OK");
    }

    // Test 5: CNL and CPL.
    {
        write_str_no_serial("\x1b[5;10H"); // Row 5, col 10
        write_str_no_serial("\x1b[2E");     // Next line ×2
        let con = CONSOLE.lock();
        assert_eq!(con.cursor_row, 6, "CNL row");
        assert_eq!(con.cursor_col, 0, "CNL col");
        drop(con);

        write_str_no_serial("\x1b[1F");     // Previous line ×1
        let con = CONSOLE.lock();
        assert_eq!(con.cursor_row, 5, "CPL row");
        assert_eq!(con.cursor_col, 0, "CPL col");
        crate::serial_println!("[console]   CNL/CPL next/prev line: OK");
    }

    // Test 6: SGR attribute tracking.
    {
        // Reset, then set bold + underline + reverse.
        write_str_no_serial("\x1b[0m");
        write_str_no_serial("\x1b[1;4;7m");
        let con = CONSOLE.lock();
        assert!(con.bold, "bold not set");
        assert!(con.underline, "underline not set");
        assert!(con.reverse, "reverse not set");
        assert!(!con.dim, "dim should not be set");
        drop(con);

        // Reset all.
        write_str_no_serial("\x1b[0m");
        let con = CONSOLE.lock();
        assert!(!con.bold, "bold not cleared");
        assert!(!con.underline, "underline not cleared");
        assert!(!con.reverse, "reverse not cleared");
        crate::serial_println!("[console]   SGR attributes: OK");
    }

    // Test 7: Foreground color selection.
    {
        write_str_no_serial("\x1b[0m");
        write_str_no_serial("\x1b[31m"); // Red
        let con = CONSOLE.lock();
        assert_eq!(con.fg_color, ANSI_COLORS[1], "fg should be red");
        drop(con);

        write_str_no_serial("\x1b[94m"); // Bright blue
        let con = CONSOLE.lock();
        assert_eq!(con.fg_color, ANSI_COLORS[12], "fg should be bright blue");
        drop(con);

        write_str_no_serial("\x1b[39m"); // Default fg
        let con = CONSOLE.lock();
        assert_eq!(con.fg_color, FG_COLOR, "fg should be default");
        crate::serial_println!("[console]   Foreground color: OK");
    }

    // Test 8: 256-color.
    {
        write_str_no_serial("\x1b[0m");
        // 256-color index 196 = bright red from the cube.
        // Index 196 = 16 + 36*5 + 6*0 + 0 → pure red
        let expected = color_256(196);
        write_str_no_serial("\x1b[38;5;196m");
        let con = CONSOLE.lock();
        assert_eq!(con.fg_color, expected, "256-color fg");
        drop(con);
        write_str_no_serial("\x1b[0m");
        crate::serial_println!("[console]   256-color support: OK");
    }

    // Test 9: Scroll region setup.
    {
        write_str_no_serial("\x1b[0m");
        let rows = {
            let con = CONSOLE.lock();
            con.rows
        };
        write_str_no_serial("\x1b[5;20r"); // Scroll region rows 5-20
        let con = CONSOLE.lock();
        assert_eq!(con.scroll_top, 4, "scroll_top");
        assert_eq!(con.scroll_bottom, 19, "scroll_bottom");
        // DECSTBM resets cursor to home.
        assert_eq!(con.cursor_row, 0, "DECSTBM cursor row");
        assert_eq!(con.cursor_col, 0, "DECSTBM cursor col");
        drop(con);

        // Reset scroll region.
        write_str_no_serial("\x1b[r");
        let con = CONSOLE.lock();
        assert_eq!(con.scroll_top, 0, "scroll_top reset");
        assert_eq!(con.scroll_bottom, rows.saturating_sub(1), "scroll_bottom reset");
        crate::serial_println!("[console]   Scroll region (DECSTBM): OK");
    }

    // Test 10: Cursor save/restore (ESC 7 / ESC 8).
    {
        write_str_no_serial("\x1b[0m\x1b[10;20H"); // Position at (10,20)
        write_str_no_serial("\x1b[32m");             // Green foreground
        write_str_no_serial("\x1b7");                 // Save cursor (DECSC)

        write_str_no_serial("\x1b[1;1H\x1b[31m");   // Move and change color

        write_str_no_serial("\x1b8");                 // Restore cursor (DECRC)
        let con = CONSOLE.lock();
        assert_eq!(con.cursor_row, 9, "DECRC row");
        assert_eq!(con.cursor_col, 19, "DECRC col");
        assert_eq!(con.fg_color, ANSI_COLORS[2], "DECRC fg color");
        crate::serial_println!("[console]   DECSC/DECRC cursor save/restore: OK");
    }

    // Test 11: SCP/RCP (ESC[s / ESC[u).
    {
        write_str_no_serial("\x1b[0m\x1b[3;7H");    // Position at (3,7)
        write_str_no_serial("\x1b[s");                // Save cursor (SCP)
        write_str_no_serial("\x1b[15;30H");           // Move elsewhere
        write_str_no_serial("\x1b[u");                // Restore (RCP)
        let con = CONSOLE.lock();
        assert_eq!(con.cursor_row, 2, "RCP row");
        assert_eq!(con.cursor_col, 6, "RCP col");
        crate::serial_println!("[console]   SCP/RCP cursor save/restore: OK");
    }

    // Test 12: DEC private mode ? prefix parsing.
    {
        // ESC[?25l should set ansi_private and handle mode 25.
        // We can't observe cursor visibility (it's a no-op) but we
        // can verify the parser doesn't break.
        write_str_no_serial("\x1b[?25l");
        write_str_no_serial("\x1b[?25h");
        // If we get here without panic, the parser handled ? prefix.
        crate::serial_println!("[console]   DEC private mode parsing: OK");
    }

    // Test 13: Full reset (ESC c).
    {
        write_str_no_serial("\x1b[1;4;7;31m"); // Bold+underline+reverse+red
        write_str_no_serial("\x1bc");            // RIS
        let con = CONSOLE.lock();
        assert!(!con.bold, "RIS bold");
        assert!(!con.underline, "RIS underline");
        assert!(!con.reverse, "RIS reverse");
        assert!(!con.dim, "RIS dim");
        assert_eq!(con.fg_color, FG_COLOR, "RIS fg");
        assert_eq!(con.bg_color, BG_COLOR, "RIS bg");
        assert_eq!(con.scroll_top, 0, "RIS scroll_top");
        crate::serial_println!("[console]   Full reset (RIS): OK");
    }

    // Clean up: reset state for normal operation.
    write_str_no_serial("\x1b[0m\x1b[r");
    crate::serial_println!("[console] Self-test PASSED");
}

/// Write a string to the console without mirroring to serial.
///
/// Used by self-test so escape sequences affect the framebuffer console
/// state without polluting the serial log with control characters.
fn write_str_no_serial(s: &str) {
    for byte in s.bytes() {
        putchar(byte);
    }
}
