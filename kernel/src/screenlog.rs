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
//! It keeps each row's text as well as drawing it, so that it can be drawn
//! again. That is what lets something borrow the screen for a while without
//! taking it -- a display driver's self-test, which shows its own frames
//! there (`console::lend_display`): the pane is [`pause`]d, goes on keeping
//! the lines logged meanwhile without drawing them, and is drawn again, all
//! of it, at the matching [`resume`]. A CPU that stops for good
//! (`cpu::halt_loop`, the end of every panic and fatal fault) draws it again
//! first, so what the kernel said on the way down is on the screen even when
//! it died during a loan. It stops for good when something takes the display
//! (`console::hide_hint`, called on every mode set and page flip a program
//! makes), so it never draws over a desktop.
//!
//! Escape sequences are dropped, and a multi-byte UTF-8 character is drawn
//! as `~`: the font has ASCII only.

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
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

/// One row's text, as drawn there.
type RowText = [u8; MAX_COLS];

/// The pane: where it is on the framebuffer, where the next line goes, and
/// the line being assembled. Its rows' text is kept beside it ([`Log`]).
struct Pane {
    /// The framebuffer's virtual address, and its bytes per pixel row.
    fb: u64,
    pitch: u32,
    /// The pane's first pixel row, and its size in glyph cells.
    top: u32,
    cols: u32,
    rows: u32,
    /// The framebuffer's width and height in pixels: what [`Pane::blank`]
    /// clears, rows `top..height`.
    width: u32,
    height: u32,
    /// The row the next line is drawn at.
    row: u32,
    /// How many columns of each row hold text: what a later line there
    /// must cover, and how much of the row's kept text is its own.
    row_len: [u16; MAX_ROWS],
    /// The line being assembled, `len` bytes of it.
    line: [u8; MAX_COLS],
    len: usize,
    /// Inside an escape sequence (after ESC, until its final letter).
    escape: bool,
    /// The framebuffer's base and pitch are 8-byte aligned, so two pixels
    /// go in one store; otherwise one pixel a store.
    wide: bool,
}

/// The pane and its rows' text, under one lock. All zeros until [`start`]
/// places the pane -- a pane of no rows -- so the 128 KiB of text costs no
/// stack and no space in the kernel image.
struct Log {
    pane: Pane,
    text: [RowText; MAX_ROWS],
}

/// The pane draws: set once it is placed, cleared for good by [`stop`].
static ACTIVE: AtomicBool = AtomicBool::new(false);
static LOG: Mutex<Log> = Mutex::new(Log {
    pane: Pane::UNPLACED,
    text: [[0; MAX_COLS]; MAX_ROWS],
});
/// How many [`pause`]s are not yet [`resume`]d: the pane draws nothing while
/// any is, and keeps what it would have drawn.
static PAUSED: AtomicU32 = AtomicU32::new(0);

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
/// nothing else may draw at rows `top..height` while the pane is active,
/// unless it is [`pause`]d.
pub unsafe fn start(fb: u64, pitch: u32, width: u32, top: u32, height: u32) {
    let cols = (width / GLYPH_WIDTH).min(MAX_COLS as u32);
    let rows = (height.saturating_sub(top) / GLYPH_HEIGHT).min(MAX_ROWS as u32);
    if cols == 0 || rows < 2 {
        return;
    }
    let mut log = LOG.lock();
    log.pane = Pane::new(fb, pitch, top, cols, rows, width, height);
    log.pane.blank();
    log.pane.mark(0, true);
    drop(log);
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

/// Draw nothing until the matching [`resume`], keeping the lines logged
/// meanwhile: something is about to draw on the screen for a while without
/// taking it (`console::lend_display`). Returns once no line is being drawn,
/// on any CPU.
pub fn pause() {
    PAUSED.fetch_add(1, Ordering::AcqRel);
    // A line another CPU was drawing when the count rose is finished before
    // this returns: `feed` reads the count under the lock.
    drop(LOG.lock());
}

/// Undo a [`pause`]. The last one outstanding draws the pane again, all of
/// it: its rows as they are now, the lines kept during the pause included,
/// over whatever was drawn there meanwhile. Nothing is drawn once the pane
/// has [`stop`]ped.
///
/// Returns false, changing nothing, when there is no pause to undo: a
/// caller's pause and resume do not pair, which is a bug in the caller.
#[must_use]
pub fn resume() -> bool {
    let mut log = LOG.lock();
    match PAUSED.fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_sub(1)) {
        Ok(1) => {
            if active() {
                let Log { pane, text } = &mut *log;
                pane.repaint(text);
            }
            true
        }
        Ok(_) => true,
        Err(_) => false,
    }
}

/// Undo every [`pause`] and draw the pane again now: this CPU is stopping
/// for good (`cpu::halt_loop`), and what the kernel said on the way down is
/// what the pane is for. A pane busy on this CPU -- the end came while it
/// drew -- is left as it is.
pub fn unpause_all() {
    if PAUSED.swap(0, Ordering::AcqRel) == 0 || !active() {
        return;
    }
    if let Some(mut log) = LOG.try_lock() {
        let Log { pane, text } = &mut *log;
        pane.repaint(text);
    }
}

/// Bytes just written to the serial log: each line they complete is kept,
/// and drawn unless the pane is paused.
///
/// Called only from the serial log's own print path, under its protocol.
/// A print nested inside another (an exception that interrupted this CPU
/// while it drew) finds the pane busy and is left to the serial log.
pub fn feed(bytes: &[u8]) {
    if !active() {
        return;
    }
    let Some(mut log) = LOG.try_lock() else {
        return;
    };
    let show = PAUSED.load(Ordering::Acquire) == 0;
    let Log { pane, text } = &mut *log;
    pane.feed(bytes, text, show);
}

impl Pane {
    /// No pane: what [`LOG`] holds until [`start`].
    const UNPLACED: Pane = Pane {
        fb: 0,
        pitch: 0,
        top: 0,
        cols: 0,
        rows: 0,
        width: 0,
        height: 0,
        row: 0,
        row_len: [0; MAX_ROWS],
        line: [0; MAX_COLS],
        len: 0,
        escape: false,
        wide: false,
    };

    /// A pane of `cols` by `rows` glyph cells, from pixel row `top` of the
    /// framebuffer at `fb`, `pitch` bytes a pixel row, `width` by `height`
    /// pixels; nothing drawn yet.
    fn new(fb: u64, pitch: u32, top: u32, cols: u32, rows: u32, width: u32, height: u32) -> Self {
        Self {
            fb,
            pitch,
            top,
            cols,
            rows,
            width,
            height,
            wide: fb.is_multiple_of(8) && pitch.is_multiple_of(8),
            ..Self::UNPLACED
        }
    }

    /// Paint the pane's pixel rows, `top` down to the framebuffer's height,
    /// all its width, the background: a line is drawn only as far as its text
    /// reaches, so whatever the screen held before -- a loader's image, the
    /// previous kernel's desktop after a kexec -- would show beside it.
    fn blank(&self) {
        // The background is black, all zero bytes: a row is one fill, as wide
        // as the CPU stores, rather than a store a pixel -- a 4K screen's pane
        // is millions of pixels.
        const _: () = assert!(BG == 0);
        let row_bytes = usize::try_from(self.width).unwrap_or(0).saturating_mul(4);
        for y in self.top..self.height {
            let row = self
                .fb
                .wrapping_add(u64::from(y).wrapping_mul(u64::from(self.pitch)));
            // SAFETY: inside the pane's rows of the framebuffer, which
            // `start`'s caller vouched for (`width` pixels a row, rows
            // `top..height`).
            unsafe { core::ptr::write_bytes(row as *mut u8, 0, row_bytes) };
        }
    }

    /// Draw the pane again, all of it, from `text`: its rows blank, each
    /// row's kept text, and the marker where the next line goes.
    fn repaint(&self, text: &[RowText]) {
        self.blank();
        for row in 0..self.rows {
            if row == self.row {
                self.draw_marker(row, 0);
            } else {
                let len = self.row_len(row);
                self.draw_text(row, text, len, len);
            }
        }
    }

    /// Assemble `bytes` into lines, keeping each one completed in `text`,
    /// and drawing it when `show`.
    fn feed(&mut self, bytes: &[u8], text: &mut [RowText], show: bool) {
        for &b in bytes {
            if self.escape {
                // CSI parameters and the like run until a letter ends them.
                self.escape = !b.is_ascii_alphabetic();
                continue;
            }
            let shown = match b {
                b'\n' => {
                    self.flush(text, show);
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
                self.flush(text, show);
            }
            if let Some(slot) = self.line.get_mut(self.len) {
                *slot = shown;
                self.len = self.len.saturating_add(1);
            }
        }
    }

    /// Keep the line assembled as the current row's text and, when `show`,
    /// draw it -- as far as it, or the line it replaces, reaches -- and the
    /// marker at the row after it.
    fn flush(&mut self, text: &mut [RowText], show: bool) {
        let row = self.row;
        let len = self.len;
        if let (Some(kept), Some(line)) = (
            text.get_mut(row as usize).and_then(|t| t.get_mut(..len)),
            self.line.get(..len),
        ) {
            kept.copy_from_slice(line);
        }
        if show {
            let reach = len.max(self.row_len(row));
            self.draw_text(row, text, len, reach);
        }
        self.set_row_len(row, len);
        self.len = 0;
        self.row = row.saturating_add(1).checked_rem(self.rows).unwrap_or(0);
        self.mark(self.row, show);
    }

    /// The marker row: where the next line goes, below the newest. Its old
    /// text is cleared when `show`, and it is labelled.
    fn mark(&mut self, row: u32, show: bool) {
        let label = MARKER.len().min(self.cols as usize);
        if show {
            self.draw_marker(row, self.row_len(row));
        }
        self.set_row_len(row, label);
    }

    /// Draw `row`'s first `len` characters of `text`, and spaces from there
    /// to `reach`, covering the row's old text.
    fn draw_text(&self, row: u32, text: &[RowText], len: usize, reach: usize) {
        let kept = text.get(row as usize);
        for col in 0..u32::try_from(reach).unwrap_or(0).min(self.cols) {
            let ch = match kept.and_then(|t| t.get(col as usize)) {
                Some(&c) if (col as usize) < len => c,
                _ => b' ',
            };
            self.glyph(col, row, ch, FG);
        }
    }

    /// Draw the marker at `row`, and spaces to `reach`, covering old text.
    fn draw_marker(&self, row: u32, reach: usize) {
        let label = MARKER.len().min(self.cols as usize);
        for col in 0..u32::try_from(reach.max(label)).unwrap_or(0).min(self.cols) {
            match MARKER.get(col as usize) {
                Some(&c) => self.glyph(col, row, c, MARK),
                None => self.glyph(col, row, b' ', FG),
            }
        }
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

    /// Draw `ch` at glyph cell (`col`, `row`) of the pane: two pixels a store
    /// on an 8-byte-aligned framebuffer, one otherwise.
    fn glyph(&self, col: u32, row: u32, ch: u8, fg: u32) {
        let bits = crate::font::glyph(ch);
        let x0 = u64::from(col.saturating_mul(GLYPH_WIDTH)).saturating_mul(4);
        let y0 = self.top.saturating_add(row.saturating_mul(GLYPH_HEIGHT));
        for (gy, &line) in (0u32..).zip(bits.iter()) {
            let at = self
                .fb
                .wrapping_add(u64::from(y0.saturating_add(gy)).wrapping_mul(u64::from(self.pitch)))
                .wrapping_add(x0);
            // The font's most significant bit is the leftmost pixel.
            let pixel = |x: u32| if line & (0x80 >> x) != 0 { fg } else { BG };
            if self.wide {
                for pair in 0..GLYPH_WIDTH / 2 {
                    let left = pair.saturating_mul(2);
                    // The leftmost of a pair is the lower address: the low half.
                    let both =
                        u64::from(pixel(left)) | (u64::from(pixel(left.saturating_add(1))) << 32);
                    // SAFETY: inside the pane's rows of the framebuffer, which
                    // `start`'s caller vouched for: col < cols and row < rows
                    // keep x and y inside it. A cell starts 32 bytes into its
                    // row, so with base and pitch 8-byte aligned (`wide`), so
                    // is the store.
                    unsafe {
                        core::ptr::write_volatile(
                            at.wrapping_add(u64::from(pair).wrapping_mul(8)) as *mut u64,
                            both,
                        );
                    }
                }
            } else {
                for x in 0..GLYPH_WIDTH {
                    // SAFETY: as above; a framebuffer's pixels are 4-byte aligned.
                    unsafe {
                        core::ptr::write_volatile(
                            at.wrapping_add(u64::from(x).wrapping_mul(4)) as *mut u32,
                            pixel(x),
                        );
                    }
                }
            }
        }
    }
}

/// The pane's line assembly and drawing, on a framebuffer in memory: lines
/// drawn at their rows, the marker after the newest, escape sequences
/// dropped, a long line split, and the rows wrapping to the top -- on an
/// 8-byte-aligned framebuffer, two pixels a store, and on one that is not,
/// one a store; lines kept undrawn while paused and the whole pane drawn
/// again over a frame drawn across it; and the pause count, which nests,
/// refuses an unmatched resume, and is cleared for a CPU that stops.
pub fn self_test() -> crate::error::KernelResult<()> {
    crate::serial_println!("[screenlog] Running self-test...");
    for wide in [true, false] {
        if let Err(what) = self_test_pane(wide) {
            crate::serial_println!(
                "[screenlog]   FAIL ({} stores): {}",
                if wide { "wide" } else { "narrow" },
                what
            );
            return Err(crate::error::KernelError::InternalError);
        }
    }
    if let Err(what) = self_test_pause_count() {
        crate::serial_println!("[screenlog]   FAIL: {}", what);
        return Err(crate::error::KernelError::InternalError);
    }
    crate::serial_println!(
        "[screenlog]   lines, the marker, escapes dropped, splitting, wrapping, both store widths; kept while paused and drawn again whole; pauses nest, an unmatched resume refused, all undone for a CPU that stops: OK"
    );
    Ok(())
}

/// The pause count, through [`pause`], [`resume`] and [`unpause_all`]
/// themselves. On a boot with the pane on screen this pauses and redraws the
/// real pane, which leaves it as it was.
fn self_test_pause_count() -> Result<(), &'static str> {
    let count = || PAUSED.load(Ordering::Acquire);
    let before = count();
    if before != 0 {
        return Err("the screen is lent while the self-tests run");
    }
    pause();
    pause();
    if count() != 2 {
        return Err("two pauses are not counted as two");
    }
    if !resume() || count() != 1 {
        return Err("a resume did not undo one of two pauses");
    }
    if !resume() || count() != 0 {
        return Err("the last resume did not undo the last pause");
    }
    if resume() || count() != 0 {
        return Err("a resume with no pause was taken, not refused");
    }
    pause();
    pause();
    unpause_all();
    if count() != 0 {
        return Err("a CPU stopping did not undo every pause");
    }
    Ok(())
}

/// [`self_test`]'s pane on a framebuffer whose base is 8-byte aligned
/// (`wide`) or 4 bytes past that.
fn self_test_pane(wide: bool) -> Result<(), &'static str> {
    // A framebuffer of 4 columns by 3 rows of glyphs, holding an old image,
    // placed in a buffer one pixel longer so that it can start either on an
    // 8-byte boundary or 4 bytes past one. Every access to it, the pane's and
    // the test's, goes through `base`, taken once.
    const COLS: u32 = 4;
    const ROWS: u32 = 3;
    const WIDTH: u32 = COLS * GLYPH_WIDTH;
    const PIXELS: usize = (WIDTH * ROWS * GLYPH_HEIGHT) as usize;
    const STALE: u32 = 0x0012_3456;
    let width = WIDTH;
    let mut buf = alloc::vec![STALE; PIXELS + 1];
    let aligned = (buf.as_ptr() as usize).is_multiple_of(8);
    let base = buf.as_mut_ptr().wrapping_add(usize::from(aligned != wide));
    let fb = || {
        // SAFETY: `base` points into `buf`, which outlives the test, with
        // PIXELS pixels after it; nothing writes while the slice is held.
        unsafe { core::slice::from_raw_parts(base, PIXELS) }
    };
    let paint = |value: u32| {
        // SAFETY: as for `fb`; no slice from it is held across this.
        (0..PIXELS).for_each(|i| unsafe { base.add(i).write(value) });
    };
    let mut text = alloc::vec![[0u8; MAX_COLS]; ROWS as usize];
    let mut pane = Pane::new(
        base as u64,
        WIDTH * 4,
        0,
        COLS,
        ROWS,
        WIDTH,
        ROWS * GLYPH_HEIGHT,
    );
    if pane.wide != wide {
        return Err("the store width does not follow the framebuffer's alignment");
    }
    pane.blank();
    if fb().iter().any(|&p| p != BG) {
        return Err("the old image is left in the pane");
    }
    pane.mark(0, true);
    // The glyph drawn at a cell: true where a pixel has `fg`, false where it
    // has the background; None if any pixel is neither.
    let cell = |col: u32, row: u32, fg: u32| -> Option<[u8; 16]> {
        let mut bits = [0u8; 16];
        for (gy, out) in (0u32..).zip(bits.iter_mut()) {
            for gx in 0..GLYPH_WIDTH {
                let at = row
                    .saturating_mul(GLYPH_HEIGHT)
                    .saturating_add(gy)
                    .saturating_mul(width)
                    .saturating_add(col.saturating_mul(GLYPH_WIDTH))
                    .saturating_add(gx);
                match fb().get(at as usize).copied() {
                    Some(p) if p == fg => *out |= 0x80 >> gx,
                    Some(p) if p == BG => {}
                    _ => return None,
                }
            }
        }
        Some(bits)
    };
    let shows = |row: u32, text: &[u8; 4], fg: u32| {
        (0..COLS).all(|col| {
            let ch = text.get(col as usize).copied().unwrap_or(b' ');
            cell(col, row, fg) == Some(*crate::font::glyph(ch))
        })
    };

    pane.feed(b"AB\nx", &mut text, true);
    if !shows(0, b"AB  ", FG) || !shows(1, b"-- n", MARK) || pane.len != 1 {
        return Err("a line is not drawn at its row with the marker after it");
    }
    pane.feed(b"\x1b[1;32mOK\x1b[0m\n", &mut text, true);
    // The line covers the marker it replaces, the label's last letter too.
    if !shows(1, b"xOK ", FG) || !shows(2, b"-- n", MARK) {
        return Err("an escape sequence was drawn, a line split, or old text left");
    }
    // Six bytes into four columns: split, and the rows wrap to the top.
    pane.feed(b"abcdef\n", &mut text, true);
    if !shows(2, b"abcd", FG) || !shows(0, b"ef  ", FG) || !shows(1, b"-- n", MARK) {
        return Err("a long line is not split, or the rows do not wrap");
    }
    // "é" (two bytes) and a tab.
    pane.feed(b"\xc3\xa9\tz\n", &mut text, true);
    if !shows(1, b"~ z ", FG) {
        return Err("a UTF-8 character or a tab is not drawn as it should be");
    }

    // Paused: lines kept, nothing drawn -- an escape sequence left open at
    // the end of the pause runs on into the first line after it.
    let before: alloc::vec::Vec<u32> = fb().to_vec();
    pane.feed(b"p1\nq2\n\x1b[1", &mut text, false);
    if fb() != before.as_slice() {
        return Err("a paused pane drew");
    }
    // A frame drawn over all of it, then the pane drawn again: every row as
    // it would have been drawn, the paused lines and the marker included.
    paint(STALE);
    pane.repaint(&text);
    if !shows(2, b"p1  ", FG) || !shows(0, b"q2  ", FG) || !shows(1, b"-- n", MARK) {
        return Err("the pane was not drawn again whole, the paused lines included");
    }
    if fb().iter().any(|&p| p != BG && p != FG && p != MARK) {
        return Err("drawing the pane again left the frame drawn over it");
    }
    pane.feed(b"mok\n", &mut text, true);
    if !shows(1, b"ok  ", FG) || !shows(2, b"-- n", MARK) {
        return Err("an escape sequence open across a pause was not dropped whole");
    }
    drop(buf);
    Ok(())
}
