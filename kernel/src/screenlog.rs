//! The kernel's log on the screen, for a machine whose serial port nobody
//! can read.
//!
//! With `log.screen=1` on the command line, every line the kernel writes to
//! its serial log is also drawn on the boot framebuffer: the console keeps
//! the top quarter of the screen (`console::init`), and this pane the rest.
//! A boot on real hardware that stops -- a fault, a hang, a driver waiting
//! on a device that never answers -- then shows where, through anything
//! that can see the screen (a KVM box).
//!
//! The pane wraps rather than scrolls: each line is drawn at the next row,
//! from the top again after the last, and the row after it is cleared and
//! labelled as the marker -- the newest line is the one above it. Scrolling
//! would copy the framebuffer, and on real hardware that is device memory,
//! slow to read back: a boot's ~17,000 lines would take hours. The pane only
//! writes, and only as far along a row as its new text or its old reaches,
//! two pixels a store.
//!
//! It stops for good when something takes the display (`console::hide_hint`,
//! called on every mode set and page flip), so it never draws over a
//! desktop. Escape sequences are dropped, and a multi-byte UTF-8 character
//! is drawn as `~`: the font has ASCII only.

use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

/// Glyph cell size in pixels, as the console's font has them.
const GLYPH_WIDTH: u32 = 8;
const GLYPH_HEIGHT: u32 = 16;
/// The widest row the pane keeps; a wider framebuffer's spare columns stay
/// blank.
const MAX_COLS: usize = 512;
/// The most rows the pane keeps; a taller framebuffer's spare rows stay
/// blank.
const MAX_ROWS: usize = 256;
/// What the marker row says.
const MARKER: &[u8] = b"-- newest line above --";
/// Colours, as the framebuffer takes them (32-bit, blue in the low byte).
const FG: u32 = 0x00C8_C8C8;
const BG: u32 = 0x0000_0000;
const MARK: u32 = 0x00FF_A000;

/// The pane: where it is on the framebuffer, where the next line goes, and
/// the line being assembled.
struct Pane {
    /// The framebuffer's virtual address, and its bytes per pixel row.
    fb: u64,
    pitch: u32,
    /// The pane's first pixel row, and its size in glyph cells.
    top: u32,
    cols: u32,
    rows: u32,
    /// The row the next line is drawn at.
    row: u32,
    /// How many columns of each row hold text: what a later line there
    /// must cover.
    row_len: [u16; MAX_ROWS],
    /// The line being assembled, `len` bytes of it.
    line: [u8; MAX_COLS],
    len: usize,
    /// Inside an escape sequence (after ESC, until its final letter).
    escape: bool,
}

/// The pane draws: set once it is placed, cleared for good by [`stop`].
static ACTIVE: AtomicBool = AtomicBool::new(false);
static PANE: Mutex<Option<Pane>> = Mutex::new(None);

/// Whether this boot's command line asks for the pane: `log.screen=1`.
#[must_use]
pub fn requested() -> bool {
    crate::boot::kernel_cmdline()
        .is_some_and(|line| line.split_ascii_whitespace().any(|w| w == "log.screen=1"))
}

/// Start drawing the log on the framebuffer at `fb` (`pitch` bytes a pixel
/// row, `width` pixels wide), from pixel row `top` down to row `height`.
/// Nothing, for a pane smaller than two rows.
///
/// # Safety
///
/// `fb..fb + pitch * height` must be the framebuffer, mapped writable, and
/// nothing else may draw at rows `top..height` while the pane is active.
pub unsafe fn start(fb: u64, pitch: u32, width: u32, top: u32, height: u32) {
    let cols = (width / GLYPH_WIDTH).min(MAX_COLS as u32);
    let rows = (height.saturating_sub(top) / GLYPH_HEIGHT).min(MAX_ROWS as u32);
    if cols == 0 || rows < 2 {
        return;
    }
    let mut pane = Pane {
        fb,
        pitch,
        top,
        cols,
        rows,
        row: 0,
        row_len: [0; MAX_ROWS],
        line: [0; MAX_COLS],
        len: 0,
        escape: false,
    };
    pane.mark(0);
    *PANE.lock() = Some(pane);
    ACTIVE.store(true, Ordering::Release);
}

/// Stop drawing, for good: something else has taken the display.
pub fn stop() {
    ACTIVE.store(false, Ordering::Release);
}

/// Whether the pane is drawing.
#[must_use]
pub fn active() -> bool {
    ACTIVE.load(Ordering::Acquire)
}

/// Bytes just written to the serial log: each line they complete is drawn.
///
/// Called only from the serial log's own print path, under its protocol.
/// A print nested inside another (an exception that interrupted this CPU
/// while it drew) finds the pane busy and is left to the serial log.
pub fn feed(bytes: &[u8]) {
    if !active() {
        return;
    }
    let Some(mut guard) = PANE.try_lock() else {
        return;
    };
    if let Some(pane) = guard.as_mut() {
        pane.feed(bytes);
    }
}

impl Pane {
    /// Assemble `bytes` into lines, drawing each one completed.
    fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            if self.escape {
                // CSI parameters and the like run until a letter ends them.
                self.escape = !b.is_ascii_alphabetic();
                continue;
            }
            let shown = match b {
                b'\n' => {
                    self.flush();
                    continue;
                }
                0x1B => {
                    self.escape = true;
                    continue;
                }
                // A UTF-8 continuation byte: its character is drawn at its lead.
                0x80..=0xBF | b'\r' => continue,
                0xC0..=0xFF => b'~',
                b'\t' => b' ',
                0x20..=0x7E => b,
                _ => b'?',
            };
            if self.len >= self.cols as usize {
                self.flush();
            }
            if let Some(slot) = self.line.get_mut(self.len) {
                *slot = shown;
                self.len = self.len.saturating_add(1);
            }
        }
    }

    /// Draw the line assembled at the current row -- as far as it, or the
    /// line it replaces, reaches -- and the marker at the row after it.
    fn flush(&mut self) {
        let row = self.row;
        let reach = self.len.max(self.row_len(row));
        for col in 0..u32::try_from(reach).unwrap_or(0).min(self.cols) {
            let ch = match self.line.get(col as usize) {
                Some(&c) if (col as usize) < self.len => c,
                _ => b' ',
            };
            self.glyph(col, row, ch, FG);
        }
        self.set_row_len(row, self.len);
        self.len = 0;
        self.row = row.saturating_add(1).checked_rem(self.rows).unwrap_or(0);
        self.mark(self.row);
    }

    /// The marker row: where the next line goes, below the newest. Its old
    /// text is cleared, and it is labelled.
    fn mark(&mut self, row: u32) {
        let label = MARKER.len().min(self.cols as usize);
        let reach = self.row_len(row).max(label);
        for col in 0..u32::try_from(reach).unwrap_or(0).min(self.cols) {
            match MARKER.get(col as usize) {
                Some(&c) => self.glyph(col, row, c, MARK),
                None => self.glyph(col, row, b' ', FG),
            }
        }
        self.set_row_len(row, label);
    }

    /// How many columns of `row` hold text.
    fn row_len(&self, row: u32) -> usize {
        self.row_len
            .get(row as usize)
            .map_or(0, |&n| usize::from(n))
    }

    fn set_row_len(&mut self, row: u32, len: usize) {
        if let Some(n) = self.row_len.get_mut(row as usize) {
            *n = u16::try_from(len).unwrap_or(u16::MAX);
        }
    }

    /// Draw `ch` at glyph cell (`col`, `row`) of the pane, two pixels a store.
    fn glyph(&self, col: u32, row: u32, ch: u8, fg: u32) {
        let bits = crate::font::glyph(ch);
        let x0 = u64::from(col.saturating_mul(GLYPH_WIDTH)).saturating_mul(4);
        let y0 = self.top.saturating_add(row.saturating_mul(GLYPH_HEIGHT));
        for (gy, &line) in (0u32..).zip(bits.iter()) {
            let at = self
                .fb
                .wrapping_add(u64::from(y0.saturating_add(gy)).wrapping_mul(u64::from(self.pitch)))
                .wrapping_add(x0);
            for pair in 0..GLYPH_WIDTH / 2 {
                // The font's most significant bit is the leftmost pixel, and
                // the leftmost of a pair is the lower address: the low half.
                let shift = pair.saturating_mul(2);
                let left = if line & (0x80 >> shift) != 0 { fg } else { BG };
                let right = if line & (0x40 >> shift) != 0 { fg } else { BG };
                // SAFETY: inside the pane's rows of the framebuffer, which
                // `start`'s caller vouched for: col < cols and row < rows keep
                // x and y inside it. A cell starts 32 bytes into its row, so
                // the store is 8-byte aligned on any framebuffer whose base
                // and pitch are.
                unsafe {
                    core::ptr::write_volatile(
                        at.wrapping_add(u64::from(pair).wrapping_mul(8)) as *mut u64,
                        u64::from(left) | (u64::from(right) << 32),
                    );
                }
            }
        }
    }
}

/// The pane's line assembly and drawing, on a framebuffer in memory: lines
/// drawn at their rows, the marker after the newest, escape sequences
/// dropped, a long line split, and the rows wrapping to the top.
pub fn self_test() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;
    fn fail(what: &str) -> crate::error::KernelResult<()> {
        crate::serial_println!("[screenlog]   FAIL: {}", what);
        Err(KernelError::InternalError)
    }
    crate::serial_println!("[screenlog] Running self-test...");
    // A framebuffer of 4 columns by 3 rows of glyphs.
    const COLS: u32 = 4;
    const ROWS: u32 = 3;
    const WIDTH: u32 = COLS * GLYPH_WIDTH;
    const PIXELS: usize = (WIDTH * ROWS * GLYPH_HEIGHT) as usize;
    let width = WIDTH;
    // Black, as the console leaves the screen.
    let mut fb = alloc::vec![BG; PIXELS];
    let mut pane = Pane {
        fb: fb.as_mut_ptr() as u64,
        pitch: WIDTH * 4,
        top: 0,
        cols: COLS,
        rows: ROWS,
        row: 0,
        row_len: [0; MAX_ROWS],
        line: [0; MAX_COLS],
        len: 0,
        escape: false,
    };
    // The glyph drawn at a cell: true where a pixel has `fg`, false where it
    // has the background; None if any pixel is neither.
    let cell = |fb: &[u32], col: u32, row: u32, fg: u32| -> Option<[u8; 16]> {
        let mut bits = [0u8; 16];
        for (gy, out) in (0u32..).zip(bits.iter_mut()) {
            for gx in 0..GLYPH_WIDTH {
                let at = row
                    .saturating_mul(GLYPH_HEIGHT)
                    .saturating_add(gy)
                    .saturating_mul(width)
                    .saturating_add(col.saturating_mul(GLYPH_WIDTH))
                    .saturating_add(gx);
                match fb.get(at as usize).copied() {
                    Some(p) if p == fg => *out |= 0x80 >> gx,
                    Some(p) if p == BG => {}
                    _ => return None,
                }
            }
        }
        Some(bits)
    };
    let shows = |fb: &[u32], row: u32, text: &[u8; 4], fg: u32| {
        (0..COLS).all(|col| {
            let ch = text.get(col as usize).copied().unwrap_or(b' ');
            cell(fb, col, row, fg) == Some(*crate::font::glyph(ch))
        })
    };

    pane.feed(b"AB\nx");
    if !shows(&fb, 0, b"AB  ", FG) || !shows(&fb, 1, b"-- n", MARK) || pane.len != 1 {
        return fail("a line is not drawn at its row with the marker after it");
    }
    pane.feed(b"\x1b[1;32mOK\x1b[0m\n");
    // The line covers the marker it replaces, the label's last letter too.
    if !shows(&fb, 1, b"xOK ", FG) || !shows(&fb, 2, b"-- n", MARK) {
        return fail("an escape sequence was drawn, a line split, or old text left");
    }
    // Six bytes into four columns: split, and the rows wrap to the top.
    pane.feed(b"abcdef\n");
    if !shows(&fb, 2, b"abcd", FG) || !shows(&fb, 0, b"ef  ", FG) || !shows(&fb, 1, b"-- n", MARK) {
        return fail("a long line is not split, or the rows do not wrap");
    }
    // "é" (two bytes) and a tab.
    pane.feed(b"\xc3\xa9\tz\n");
    if !shows(&fb, 1, b"~ z ", FG) {
        return fail("a UTF-8 character or a tab is not drawn as it should be");
    }
    crate::serial_println!(
        "[screenlog]   lines, the marker, escapes dropped, splitting, wrapping: OK"
    );
    Ok(())
}
