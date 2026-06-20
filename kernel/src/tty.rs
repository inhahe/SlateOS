//! Terminal (TTY) line-discipline and `termios` state for the system console.
//!
//! This module implements the kernel side of the Linux terminal ABI for the
//! single system console: the `termios` structure that `TCGETS`/`TCSETS`
//! exchange with userspace, the `winsize` structure that `TIOCGWINSZ` reports,
//! and the canonical/raw line-discipline policy that the console `read(2)` path
//! consults.
//!
//! ## Why a kernel TTY at all
//!
//! Before this module, a `read(2)` on the console returned exactly one
//! keystroke and `ioctl(fd, TCGETS, …)` returned `ENOTTY` — so `isatty(3)`
//! answered "no" and interactive programs (a shell, anything using readline or
//! `tcgetattr`/`tcsetattr`) could neither detect the terminal nor configure it.
//! A real interactive console *is* a terminal, so the console now answers the
//! terminal-control ioctls and exposes a line discipline.
//!
//! ## Single shared console termios
//!
//! Linux keeps one `termios` per tty device, shared by every file descriptor
//! open on that tty (so a `tcsetattr` by the shell is observed by its
//! children).  We have exactly one console device, so the termios state is a
//! single global guarded by a [`Mutex`].  All `Console`-kind Linux fds resolve
//! to it.
//!
//! ## What lives here vs. the syscall layer
//!
//! This module owns the *data* (the termios/winsize structs, their byte
//! serialisation, the default "sane terminal" settings, and the global state).
//! The Linux syscall translator (`kernel/src/syscall/linux.rs`) owns the
//! *plumbing*: routing `TCGETS`/`TCSETS`/`TIOCGWINSZ` for `Console` fds here and
//! consulting [`is_canonical`]/[`echo_enabled`] from the console read path.

// The canonical line-discipline read path and several c_cc control characters
// are wired incrementally; not every accessor has an in-tree caller yet.
#![allow(dead_code)]

use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

/// Number of control characters in the Linux *kernel* `struct termios`
/// (`NCCS`).  Note: the glibc *user* `struct termios` has a larger array plus
/// `c_ispeed`/`c_ospeed`; glibc's `tcgetattr` issues `TCGETS` with this 36-byte
/// kernel layout and translates into its own struct, so this is the correct
/// wire format for `TCGETS`/`TCSETS`.
pub const NCCS: usize = 19;

/// Serialised size of the kernel `struct termios`: four `u32` flag words, a
/// one-byte `c_line`, and `NCCS` control bytes (4*4 + 1 + 19 = 36).
pub const TERMIOS_BYTES: usize = 4 * 4 + 1 + NCCS;

/// Serialised size of `struct winsize`: four `u16` fields.
pub const WINSIZE_BYTES: usize = 4 * 2;

// --- c_iflag bits (input modes) ---
pub mod iflag {
    pub const IGNBRK: u32 = 0x0001;
    pub const BRKINT: u32 = 0x0002;
    pub const ICRNL: u32 = 0x0100;
    pub const IXON: u32 = 0x0400;
    pub const IMAXBEL: u32 = 0x2000;
    pub const IUTF8: u32 = 0x4000;
}

// --- c_oflag bits (output modes) ---
pub mod oflag {
    pub const OPOST: u32 = 0x0001;
    pub const ONLCR: u32 = 0x0004;
}

// --- c_cflag bits (control modes) ---
pub mod cflag {
    pub const B38400: u32 = 0x000f;
    pub const CS8: u32 = 0x0030;
    pub const CREAD: u32 = 0x0080;
    pub const HUPCL: u32 = 0x4000;
}

// --- c_lflag bits (local modes) ---
pub mod lflag {
    /// Generate signals (INTR/QUIT/SUSP) from the corresponding control chars.
    pub const ISIG: u32 = 0x0001;
    /// Canonical (line-buffered) input mode.
    pub const ICANON: u32 = 0x0002;
    /// Echo input characters.
    pub const ECHO: u32 = 0x0008;
    /// Echo erase as backspace-space-backspace (with `ICANON`).
    pub const ECHOE: u32 = 0x0010;
    /// Echo the `KILL` character by erasing the line (with `ICANON`).
    pub const ECHOK: u32 = 0x0020;
    /// Echo a newline even when `ECHO` is off (with `ICANON`).
    pub const ECHONL: u32 = 0x0040;
    /// Disable flushing the input queue when `INTR`/`QUIT`/`SUSP` generate a
    /// signal. Without this, a signal character discards the in-progress
    /// (canonical) line; with it set, the buffered input is preserved.
    pub const NOFLSH: u32 = 0x0080;
    /// Echo control chars as `^X`.
    pub const ECHOCTL: u32 = 0x0200;
    /// Visual erase for the line kill.
    pub const ECHOKE: u32 = 0x0800;
    /// Enable extended (implementation-defined) input processing.
    pub const IEXTEN: u32 = 0x8000;
}

// --- c_cc indices (Linux kernel order) ---
pub mod cc {
    pub const VINTR: usize = 0;
    pub const VQUIT: usize = 1;
    pub const VERASE: usize = 2;
    pub const VKILL: usize = 3;
    pub const VEOF: usize = 4;
    pub const VTIME: usize = 5;
    pub const VMIN: usize = 6;
    pub const VSWTC: usize = 7;
    pub const VSTART: usize = 8;
    pub const VSTOP: usize = 9;
    pub const VSUSP: usize = 10;
    pub const VEOL: usize = 11;
    pub const VREPRINT: usize = 12;
    pub const VDISCARD: usize = 13;
    pub const VWERASE: usize = 14;
    pub const VLNEXT: usize = 15;
    pub const VEOL2: usize = 16;
}

/// The kernel `struct termios` (the `TCGETS`/`TCSETS` wire format).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Termios {
    pub c_iflag: u32,
    pub c_oflag: u32,
    pub c_cflag: u32,
    pub c_lflag: u32,
    pub c_line: u8,
    pub c_cc: [u8; NCCS],
}

impl Termios {
    /// The default "sane terminal" settings, mirroring Linux's
    /// `tty_std_termios` (canonical mode, echo on, the conventional control
    /// characters).  A freshly-opened console starts here.
    #[must_use]
    pub const fn sane_default() -> Self {
        // INIT_C_CC from Linux (include/linux/tty.h), in kernel c_cc order.
        let mut c_cc = [0u8; NCCS];
        c_cc[cc::VINTR] = 3; // ^C
        c_cc[cc::VQUIT] = 28; // ^\
        c_cc[cc::VERASE] = 127; // DEL
        c_cc[cc::VKILL] = 21; // ^U
        c_cc[cc::VEOF] = 4; // ^D
        c_cc[cc::VTIME] = 0;
        c_cc[cc::VMIN] = 1;
        c_cc[cc::VSWTC] = 0;
        c_cc[cc::VSTART] = 17; // ^Q
        c_cc[cc::VSTOP] = 19; // ^S
        c_cc[cc::VSUSP] = 26; // ^Z
        c_cc[cc::VEOL] = 0;
        c_cc[cc::VREPRINT] = 18; // ^R
        c_cc[cc::VDISCARD] = 15; // ^O
        c_cc[cc::VWERASE] = 23; // ^W
        c_cc[cc::VLNEXT] = 22; // ^V
        c_cc[cc::VEOL2] = 0;
        Self {
            c_iflag: iflag::ICRNL | iflag::IXON | iflag::IMAXBEL | iflag::IUTF8,
            c_oflag: oflag::OPOST | oflag::ONLCR,
            c_cflag: cflag::B38400 | cflag::CS8 | cflag::CREAD,
            c_lflag: lflag::ISIG
                | lflag::ICANON
                | lflag::ECHO
                | lflag::ECHOE
                | lflag::ECHOK
                | lflag::ECHOCTL
                | lflag::ECHOKE
                | lflag::IEXTEN,
            c_line: 0,
            c_cc,
        }
    }

    /// Serialise into the 36-byte kernel `struct termios` wire format
    /// (little-endian, matching x86_64).
    #[must_use]
    pub fn to_bytes(self) -> [u8; TERMIOS_BYTES] {
        let mut buf = [0u8; TERMIOS_BYTES];
        // Write a u32 little-endian at `off`; `off+4 <= 16 < 36` always holds
        // for the four flag words, so the slice is in-bounds — but we still go
        // through `get_mut` to keep the indexing-slicing lint satisfied.
        let mut put_u32 = |off: usize, val: u32| {
            if let Some(dst) = buf.get_mut(off..off.saturating_add(4)) {
                dst.copy_from_slice(&val.to_le_bytes());
            }
        };
        put_u32(0, self.c_iflag);
        put_u32(4, self.c_oflag);
        put_u32(8, self.c_cflag);
        put_u32(12, self.c_lflag);
        if let Some(b) = buf.get_mut(16) {
            *b = self.c_line;
        }
        if let Some(dst) = buf.get_mut(17..17usize.saturating_add(NCCS)) {
            dst.copy_from_slice(&self.c_cc);
        }
        buf
    }

    /// Parse from the 36-byte kernel `struct termios` wire format.
    #[must_use]
    pub fn from_bytes(buf: &[u8; TERMIOS_BYTES]) -> Self {
        let get_u32 = |off: usize| -> u32 {
            match buf.get(off..off.saturating_add(4)) {
                Some(s) => {
                    let mut b = [0u8; 4];
                    b.copy_from_slice(s);
                    u32::from_le_bytes(b)
                }
                None => 0,
            }
        };
        let c_line = buf.get(16).copied().unwrap_or(0);
        let mut c_cc = [0u8; NCCS];
        if let Some(src) = buf.get(17..17usize.saturating_add(NCCS)) {
            c_cc.copy_from_slice(src);
        }
        Self {
            c_iflag: get_u32(0),
            c_oflag: get_u32(4),
            c_cflag: get_u32(8),
            c_lflag: get_u32(12),
            c_line,
            c_cc,
        }
    }

    /// `true` when canonical (line-buffered) input mode is active.
    #[must_use]
    pub const fn is_canonical(&self) -> bool {
        self.c_lflag & lflag::ICANON != 0
    }

    /// `true` when input characters should be echoed.
    #[must_use]
    pub const fn echo_enabled(&self) -> bool {
        self.c_lflag & lflag::ECHO != 0
    }

    /// The `VMIN` control value (minimum bytes for a non-canonical read).
    #[must_use]
    pub fn vmin(&self) -> u8 {
        self.c_cc.get(cc::VMIN).copied().unwrap_or(1)
    }

    /// The `VTIME` control value (read timeout in deciseconds, non-canonical).
    #[must_use]
    pub fn vtime(&self) -> u8 {
        self.c_cc.get(cc::VTIME).copied().unwrap_or(0)
    }
}

impl Default for Termios {
    fn default() -> Self {
        Self::sane_default()
    }
}

/// `struct winsize` — terminal dimensions in character cells (and pixels, which
/// we leave zero).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WinSize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

impl WinSize {
    /// Serialise into the 8-byte `struct winsize` wire format (little-endian).
    #[must_use]
    pub fn to_bytes(self) -> [u8; WINSIZE_BYTES] {
        let mut buf = [0u8; WINSIZE_BYTES];
        let fields = [self.ws_row, self.ws_col, self.ws_xpixel, self.ws_ypixel];
        for (i, field) in fields.iter().enumerate() {
            let off = i.saturating_mul(2);
            if let Some(dst) = buf.get_mut(off..off.saturating_add(2)) {
                dst.copy_from_slice(&field.to_le_bytes());
            }
        }
        buf
    }

    /// Parse from the 8-byte `struct winsize` wire format.
    #[must_use]
    pub fn from_bytes(buf: &[u8; WINSIZE_BYTES]) -> Self {
        let read_u16 = |off: usize| -> u16 {
            match buf.get(off..off.saturating_add(2)) {
                Some(s) => {
                    let mut b = [0u8; 2];
                    b.copy_from_slice(s);
                    u16::from_le_bytes(b)
                }
                None => 0,
            }
        };
        Self {
            ws_row: read_u16(0),
            ws_col: read_u16(2),
            ws_xpixel: read_u16(4),
            ws_ypixel: read_u16(6),
        }
    }
}

/// The single shared console terminal settings (Linux keeps one `termios` per
/// tty device, shared by all fds open on it).
static CONSOLE_TERMIOS: Mutex<Termios> = Mutex::new(Termios::sane_default());

/// The console's stored window size.  `TIOCSWINSZ` updates this; `TIOCGWINSZ`
/// reports the live console dimensions folded with any explicit override.
static CONSOLE_WINSIZE: Mutex<WinSize> = Mutex::new(WinSize {
    ws_row: 0,
    ws_col: 0,
    ws_xpixel: 0,
    ws_ypixel: 0,
});

/// Get a copy of the console termios (for `TCGETS`).
#[must_use]
pub fn get_termios() -> Termios {
    *CONSOLE_TERMIOS.lock()
}

/// Replace the console termios (for `TCSETS`/`TCSETSW`/`TCSETSF`).
///
/// Keeps the keyboard driver's echo in sync with the new `ECHO` bit so that a
/// program clearing `ECHO` (e.g. a password prompt) stops the driver echoing
/// immediately, and one setting it restores echo.
pub fn set_termios(new: Termios) {
    *CONSOLE_TERMIOS.lock() = new;
    crate::keyboard::set_echo(new.echo_enabled());
}

/// `true` when the console is in canonical (line-buffered) input mode.
#[must_use]
pub fn is_canonical() -> bool {
    CONSOLE_TERMIOS.lock().is_canonical()
}

/// `true` when the console echoes input characters.
#[must_use]
pub fn echo_enabled() -> bool {
    CONSOLE_TERMIOS.lock().echo_enabled()
}

/// Current console window size for `TIOCGWINSZ`.
///
/// If userspace set an explicit size via `TIOCSWINSZ`, that is returned;
/// otherwise the live console character dimensions are reported.
#[must_use]
pub fn get_winsize() -> WinSize {
    let stored = *CONSOLE_WINSIZE.lock();
    if stored.ws_row != 0 || stored.ws_col != 0 {
        return stored;
    }
    let (cols, rows) = crate::console::dimensions();
    WinSize {
        ws_row: u16::try_from(rows).unwrap_or(u16::MAX),
        ws_col: u16::try_from(cols).unwrap_or(u16::MAX),
        ws_xpixel: 0,
        ws_ypixel: 0,
    }
}

/// Store an explicit console window size (for `TIOCSWINSZ`).
pub fn set_winsize(ws: WinSize) {
    *CONSOLE_WINSIZE.lock() = ws;
}

// ---------------------------------------------------------------------------
// Line discipline (canonical / raw console reads)
// ---------------------------------------------------------------------------

/// Maximum bytes buffered in one canonical line (Linux `MAX_CANON`).  Input
/// past this in a single line is dropped until a line terminator arrives.
pub const MAX_CANON: usize = 4096;

/// Outcome of feeding one input byte to the canonical line editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineStep {
    /// Byte consumed; keep editing the current line.
    Pending,
    /// A line terminator (`\n`) completed the line; deliver it.
    Line,
    /// `VEOF` (`^D`): caller delivers the buffer as-is (empty ⇒ read returns 0).
    Eof,
    /// `VINTR`/`VQUIT`/`VSUSP` under `ISIG`: the line was discarded.  The
    /// carried value is the signal number (`SIGINT`=2 / `SIGQUIT`=3 /
    /// `SIGTSTP`=20) the foreground process group must receive; the syscall
    /// layer (`deliver_console_signal`) routes it to the console's foreground
    /// pgrp and returns the restart/`EINTR` sentinel to the blocked reader.
    Signal(u8),
}

/// A fixed-capacity in-progress line buffer for the canonical editor.
struct LineBuf {
    buf: [u8; MAX_CANON],
    len: usize,
}

impl LineBuf {
    const fn new() -> Self {
        Self { buf: [0u8; MAX_CANON], len: 0 }
    }

    /// Append a byte; `false` if the line is already at `MAX_CANON`.
    fn push(&mut self, c: u8) -> bool {
        if let Some(slot) = self.buf.get_mut(self.len) {
            *slot = c;
            self.len = self.len.saturating_add(1);
            true
        } else {
            false
        }
    }

    /// Remove the last byte (erase); `false` if the line is empty.
    fn pop(&mut self) -> bool {
        if self.len > 0 {
            self.len = self.len.saturating_sub(1);
            true
        } else {
            false
        }
    }

    fn clear(&mut self) {
        self.len = 0;
    }

    fn as_slice(&self) -> &[u8] {
        self.buf.get(..self.len).unwrap_or(&[])
    }
}

/// Feed one raw input byte to the canonical line editor.
///
/// This is the *pure* core of the line discipline — no I/O, no echo — so it is
/// exercised directly by the boot self-test.  Echo is handled by the keyboard
/// driver (synced to the `ECHO` termios bit); this function only maintains the
/// line buffer and decides when a read should complete.
fn feed(line: &mut LineBuf, raw: u8, t: &Termios) -> LineStep {
    // Input translation: ICRNL maps a received CR to NL (the common case so
    // that the Enter key — which the keyboard delivers as '\n' already, but a
    // serial line would deliver as '\r' — terminates a canonical line).
    let mut ch = raw;
    if ch == b'\r' && (t.c_iflag & iflag::ICRNL != 0) {
        ch = b'\n';
    }

    let g = |idx: usize, dflt: u8| t.c_cc.get(idx).copied().unwrap_or(dflt);
    let verase = g(cc::VERASE, 127);
    let vkill = g(cc::VKILL, 21);
    let veof = g(cc::VEOF, 4);
    let vintr = g(cc::VINTR, 3);
    let vquit = g(cc::VQUIT, 28);
    let vsusp = g(cc::VSUSP, 26);

    if t.c_lflag & lflag::ISIG != 0 {
        // POSIX: a signal character flushes the input queue (here, the
        // in-progress canonical line) UNLESS NOFLSH is set, in which case the
        // buffered input is preserved and only the signal is generated.
        let flush = t.c_lflag & lflag::NOFLSH == 0;
        if ch == vintr {
            if flush {
                line.clear();
            }
            return LineStep::Signal(2); // SIGINT
        }
        if ch == vquit {
            if flush {
                line.clear();
            }
            return LineStep::Signal(3); // SIGQUIT
        }
        if ch == vsusp {
            // ^Z: stop the foreground job. SIGTSTP's default action stops the
            // process; SIGCONT (e.g. shell `fg`/`bg`) resumes it. The
            // in-progress line is flushed unless NOFLSH is set.
            if flush {
                line.clear();
            }
            return LineStep::Signal(20); // SIGTSTP
        }
    }

    if ch == veof {
        // ^D: submit the line so far (without the EOF byte).  An empty buffer
        // becomes a zero-length read (end of file).
        return LineStep::Eof;
    }
    if ch == verase {
        line.pop();
        return LineStep::Pending;
    }
    if ch == vkill {
        line.clear();
        return LineStep::Pending;
    }
    if ch == b'\n' {
        // The newline is part of the canonical line returned to the reader.
        let _ = line.push(b'\n');
        return LineStep::Line;
    }

    // Ordinary byte: append (silently dropped if the line is full).
    let _ = line.push(ch);
    LineStep::Pending
}

/// Bytes from a completed canonical line that did not fit in the reader's
/// buffer, held for the next `read(2)`.
struct PendingLine {
    buf: [u8; MAX_CANON],
    pos: usize,
    len: usize,
}

impl PendingLine {
    const fn new() -> Self {
        Self { buf: [0u8; MAX_CANON], pos: 0, len: 0 }
    }

    fn has_data(&self) -> bool {
        self.pos < self.len
    }

    /// Replace the held bytes with `src` (truncated to `MAX_CANON`).
    fn fill(&mut self, src: &[u8]) {
        let n = src.len().min(MAX_CANON);
        if let (Some(dst), Some(s)) = (self.buf.get_mut(..n), src.get(..n)) {
            dst.copy_from_slice(s);
        }
        self.pos = 0;
        self.len = n;
    }

    /// Copy as many held bytes as fit into `out`, advancing the read cursor.
    fn drain_into(&mut self, out: &mut [u8]) -> usize {
        let avail = self.len.saturating_sub(self.pos);
        let n = avail.min(out.len());
        if let (Some(dst), Some(src)) =
            (out.get_mut(..n), self.buf.get(self.pos..self.pos.saturating_add(n)))
        {
            dst.copy_from_slice(src);
        }
        self.pos = self.pos.saturating_add(n);
        n
    }
}

/// Leftover bytes of a canonical line that overflowed a small reader buffer.
static PENDING: Mutex<PendingLine> = Mutex::new(PendingLine::new());

/// The process group ID currently in the *foreground* of the console — the
/// group that owns the terminal for the purpose of job control.  A `^C`/`^\`
/// under `ISIG` delivers `SIGINT`/`SIGQUIT` to this group (see
/// [`ConsoleRead::Signal`]); `TIOCGPGRP`/`TIOCSPGRP` read and set it.
///
/// `0` means "no foreground group set" (the kernel-startup / no-shell state),
/// in which case a generated terminal signal has no group to target and is
/// dropped.  This mirrors Linux's `tty->pgrp`, which an interactive shell
/// installs via `tcsetpgrp(3)` for each job it foregrounds.
static FOREGROUND_PGID: AtomicU64 = AtomicU64::new(0);

/// Read the console's foreground process-group ID (0 if none is set).
#[must_use]
pub fn foreground_pgid() -> u64 {
    FOREGROUND_PGID.load(Ordering::Relaxed)
}

/// Set the console's foreground process-group ID (`tcsetpgrp(3)` /
/// `TIOCSPGRP`).  A value of `0` clears the foreground group.
pub fn set_foreground_pgid(pgid: u64) {
    FOREGROUND_PGID.store(pgid, Ordering::Relaxed);
}

/// Outcome of a console [`console_read`].
///
/// A normal read yields [`ConsoleRead::Data`] with the number of bytes written
/// to the caller's buffer (`0` means end-of-file on a `^D` at an empty line, or
/// nothing immediately available in a polling raw read).  A `^C`/`^\` typed
/// under `ISIG` interrupts the read and yields [`ConsoleRead::Signal`] carrying
/// the signal number (`SIGINT`=2 / `SIGQUIT`=3) the foreground process group
/// must receive; the syscall layer performs the actual group delivery and
/// returns the restart/`EINTR` sentinel to the blocked reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleRead {
    /// `n` bytes were written to the caller's buffer (`0` ⇒ EOF / no data).
    Data(usize),
    /// A terminal signal (`SIGINT`/`SIGQUIT`) was generated; deliver it to the
    /// foreground process group.  No bytes were written to the caller's buffer.
    Signal(u8),
}

/// Read from the console into `out` per the current line discipline.
///
/// In canonical mode this blocks until a full line (terminated by `\n` or
/// `VEOF`) is available, then returns up to `out.len()` bytes of it (stashing
/// any remainder for the next call).  A `^D` on an empty line returns `0`
/// (end of file).  In non-canonical (raw) mode it honours both `VMIN` and
/// `VTIME` per POSIX (a pure poll, a read timeout, a byte count, or an
/// inter-byte timer depending on the `(VMIN, VTIME)` pair) and still applies
/// `ISIG` signal characters — see [`raw_read`].  A `^C`/`^\`/`^Z` in either
/// mode yields [`ConsoleRead::Signal`].
///
/// Echo is performed by the keyboard driver, which this function first syncs
/// to the termios `ECHO` bit so that raw/no-echo programs (password prompts,
/// full-screen editors) suppress echo correctly.
///
/// Returns a [`ConsoleRead`]: either the number of bytes written to `out`, or a
/// [`ConsoleRead::Signal`] when a `^C`/`^\` interrupted a canonical read.
pub fn console_read(out: &mut [u8]) -> ConsoleRead {
    if out.is_empty() {
        return ConsoleRead::Data(0);
    }
    let t = get_termios();

    // The Linux read path is authoritative for console echo: keep the keyboard
    // driver's echo in sync with this terminal's ECHO bit.
    crate::keyboard::set_echo(t.echo_enabled());

    // Serve any leftover bytes from a previously-overflowed line first.
    {
        let mut p = PENDING.lock();
        if p.has_data() {
            return ConsoleRead::Data(p.drain_into(out));
        }
    }

    if t.is_canonical() {
        canonical_read(&t, out)
    } else {
        raw_read(&t, out)
    }
}

/// Canonical-mode read: edit a line until a terminator, then deliver it.
///
/// A `^C`/`^\` typed under `ISIG` interrupts the read immediately and returns
/// [`ConsoleRead::Signal`]; the line in progress has already been discarded by
/// [`feed`], so no partial data is delivered (matching Linux: an interrupted
/// canonical read returns `-EINTR`, not the editing buffer).
fn canonical_read(t: &Termios, out: &mut [u8]) -> ConsoleRead {
    let mut line = LineBuf::new();
    loop {
        let raw = crate::keyboard::read_char();
        match feed(&mut line, raw, t) {
            LineStep::Pending => {}
            LineStep::Line => break,
            LineStep::Eof => {
                if line.len == 0 {
                    return ConsoleRead::Data(0); // EOF
                }
                break;
            }
            // A signal char (^C/^\) flushed the in-progress line: abandon the
            // read and let the syscall layer deliver the signal to the
            // foreground process group, returning EINTR/ERESTARTSYS to us.
            LineStep::Signal(sig) => return ConsoleRead::Signal(sig),
        }
    }
    let mut p = PENDING.lock();
    p.fill(line.as_slice());
    ConsoleRead::Data(p.drain_into(out))
}

/// Non-canonical (raw) read honouring both `VMIN` and `VTIME` (see
/// [`console_read`]).
///
/// The four `(VMIN, VTIME)` combinations follow POSIX (`termios(3)` "Canonical
/// and noncanonical mode"):
///
/// * **`MIN==0, TIME==0`** — pure poll: return whatever is immediately
///   available (possibly `0`), never blocking.
/// * **`MIN==0, TIME>0`** — read timeout: block up to `TIME` deciseconds for
///   the first byte; if any arrives, drain what is ready and return; on
///   timeout return `0`.
/// * **`MIN>0, TIME==0`** — count: block until `MIN` bytes (or the buffer
///   fills), then drain any extra bytes already ready.
/// * **`MIN>0, TIME>0`** — inter-byte timer: block indefinitely for the first
///   byte, then restart a `TIME`-decisecond timer after each byte; return when
///   `MIN` bytes are collected, the buffer fills, or the timer expires (which
///   can only happen once at least one byte has been read).
///
/// `ISIG` still applies in non-canonical mode: a `VINTR`/`VQUIT`/`VSUSP`
/// character generates the corresponding signal and aborts the read (returning
/// [`ConsoleRead::Signal`]), discarding any bytes collected so far in this call
/// — matching Linux's input flush on a signal char. `NOFLSH` (which preserves
/// buffered input) is honoured only in canonical mode (see [`feed`]): raw reads
/// keep no kernel-side input queue across calls — each call reads straight from
/// the keyboard — so there is no buffered input for `NOFLSH` to preserve here.
/// Programs that want the signal characters delivered as literal data (most
/// full-screen apps) clear `ISIG`, in which case no signal is generated.
fn raw_read(t: &Termios, out: &mut [u8]) -> ConsoleRead {
    let cap = out.len();
    if cap == 0 {
        return ConsoleRead::Data(0);
    }
    let vmin = t.vmin() as usize;
    // VTIME is in deciseconds (tenths of a second).
    const DECISECOND_NS: u64 = 100_000_000;
    let vtime_ns = u64::from(t.vtime()).saturating_mul(DECISECOND_NS);
    let mut n = 0usize;

    // Signal-character classification (only when ISIG is set).  Returns the
    // signal number for VINTR/VQUIT/VSUSP, else None.
    let isig = t.c_lflag & lflag::ISIG != 0;
    let g = |idx: usize, dflt: u8| t.c_cc.get(idx).copied().unwrap_or(dflt);
    let vintr = g(cc::VINTR, 3);
    let vquit = g(cc::VQUIT, 28);
    let vsusp = g(cc::VSUSP, 26);
    let sig_for = |ch: u8| -> Option<u8> {
        if !isig {
            return None;
        }
        match ch {
            c if c == vintr => Some(2),  // SIGINT
            c if c == vquit => Some(3),  // SIGQUIT
            c if c == vsusp => Some(20), // SIGTSTP
            _ => None,
        }
    };

    // Store one byte at the current cursor, advancing it.
    let mut store = |slot_n: usize, c: u8| {
        if let Some(slot) = out.get_mut(slot_n) {
            *slot = c;
        }
    };

    match (vmin == 0, vtime_ns == 0) {
        // MIN=0, TIME=0: pure poll.
        (true, true) => {
            while n < cap {
                match crate::keyboard::try_read_char() {
                    Some(c) => {
                        if let Some(s) = sig_for(c) {
                            return ConsoleRead::Signal(s);
                        }
                        store(n, c);
                        n = n.saturating_add(1);
                    }
                    None => break,
                }
            }
        }
        // MIN=0, TIME>0: bounded read timeout on the first byte.
        (true, false) => {
            let deadline = crate::hrtimer::now_ns().saturating_add(vtime_ns);
            if let Some(c) = crate::keyboard::read_char_timeout(deadline) {
                if let Some(s) = sig_for(c) {
                    return ConsoleRead::Signal(s);
                }
                store(n, c);
                n = n.saturating_add(1);
                // Drain any bytes already buffered alongside the first.
                while n < cap {
                    match crate::keyboard::try_read_char() {
                        Some(c) => {
                            if let Some(s) = sig_for(c) {
                                return ConsoleRead::Signal(s);
                            }
                            store(n, c);
                            n = n.saturating_add(1);
                        }
                        None => break,
                    }
                }
            }
        }
        // MIN>0, TIME=0: block for VMIN bytes, then drain ready extras.
        (false, true) => {
            while n < cap {
                let next = if n >= vmin {
                    match crate::keyboard::try_read_char() {
                        Some(c) => c,
                        None => break,
                    }
                } else {
                    crate::keyboard::read_char()
                };
                if let Some(s) = sig_for(next) {
                    return ConsoleRead::Signal(s);
                }
                store(n, next);
                n = n.saturating_add(1);
            }
        }
        // MIN>0, TIME>0: block for the first byte, then inter-byte timer.
        (false, false) => {
            let first = crate::keyboard::read_char();
            if let Some(s) = sig_for(first) {
                return ConsoleRead::Signal(s);
            }
            store(n, first);
            n = n.saturating_add(1);
            while n < cap && n < vmin {
                let deadline = crate::hrtimer::now_ns().saturating_add(vtime_ns);
                match crate::keyboard::read_char_timeout(deadline) {
                    Some(c) => {
                        if let Some(s) = sig_for(c) {
                            return ConsoleRead::Signal(s);
                        }
                        store(n, c);
                        n = n.saturating_add(1);
                    }
                    None => break, // inter-byte timer expired
                }
            }
        }
    }
    ConsoleRead::Data(n)
}

// ---------------------------------------------------------------------------
// Boot self-test
// ---------------------------------------------------------------------------

/// Boot-time self-test for the TTY/termios layer.
///
/// The `#[cfg(test)]` unit tests below do not run on the bare-metal custom
/// target, so this mirrors their assertions and is invoked from `main` during
/// kernel bring-up.  It verifies the wire-format size, the canonical/echo
/// defaults, the Linux `INIT_C_CC` control characters, byte round-tripping
/// (including raw-mode flag clearing), and that `TIOCGWINSZ` reports a live,
/// non-zero console size.
pub fn self_test() {
    crate::serial_println!("[tty] Running self-test...");

    // Wire-format sizes must match the Linux kernel structs exactly.
    assert_eq!(TERMIOS_BYTES, 36, "termios wire size");
    assert_eq!(WINSIZE_BYTES, 8, "winsize wire size");

    // Defaults: canonical line mode with echo, VMIN=1/VTIME=0.
    let t = Termios::sane_default();
    assert!(t.is_canonical(), "default should be canonical");
    assert!(t.echo_enabled(), "default should echo");
    assert_eq!(t.vmin(), 1, "default VMIN");
    assert_eq!(t.vtime(), 0, "default VTIME");

    // Control characters mirror Linux INIT_C_CC.
    assert_eq!(t.c_cc.get(cc::VINTR).copied(), Some(3), "VINTR=^C");
    assert_eq!(t.c_cc.get(cc::VEOF).copied(), Some(4), "VEOF=^D");
    assert_eq!(t.c_cc.get(cc::VERASE).copied(), Some(127), "VERASE=DEL");
    assert_eq!(t.c_cc.get(cc::VKILL).copied(), Some(21), "VKILL=^U");

    // termios round-trips losslessly through the 36-byte wire format.
    let back = Termios::from_bytes(&t.to_bytes());
    assert_eq!(t, back, "termios round-trip");
    crate::serial_println!("[tty]   termios round-trip + defaults: OK");

    // Raw mode: clearing ICANON|ECHO survives serialisation.
    let mut raw = Termios::sane_default();
    raw.c_lflag &= !(lflag::ICANON | lflag::ECHO);
    let raw_back = Termios::from_bytes(&raw.to_bytes());
    assert!(!raw_back.is_canonical(), "raw clears ICANON");
    assert!(!raw_back.echo_enabled(), "raw clears ECHO");
    crate::serial_println!("[tty]   raw-mode flag clearing: OK");

    // winsize round-trips, and TIOCGWINSZ reports a live non-zero size.
    let w = WinSize { ws_row: 24, ws_col: 80, ws_xpixel: 0, ws_ypixel: 0 };
    assert_eq!(WinSize::from_bytes(&w.to_bytes()), w, "winsize round-trip");
    let live = get_winsize();
    assert!(
        live.ws_row != 0 && live.ws_col != 0,
        "TIOCGWINSZ should report a live console size"
    );
    crate::serial_println!(
        "[tty]   winsize: {}x{} (cols x rows) OK",
        live.ws_col,
        live.ws_row
    );

    // Line discipline: drive the pure `feed` core with scripted input.
    {
        let t = Termios::sane_default();

        // "hi\n" → a complete line of exactly "hi\n".
        let mut line = LineBuf::new();
        assert_eq!(feed(&mut line, b'h', &t), LineStep::Pending);
        assert_eq!(feed(&mut line, b'i', &t), LineStep::Pending);
        assert_eq!(feed(&mut line, b'\n', &t), LineStep::Line);
        assert_eq!(line.as_slice(), b"hi\n", "canonical line content");

        // VERASE (DEL) erases the last byte: "ax\x7fb\n" → "ab\n".
        let mut e = LineBuf::new();
        let _ = feed(&mut e, b'a', &t);
        let _ = feed(&mut e, b'x', &t);
        assert_eq!(feed(&mut e, 127, &t), LineStep::Pending); // erase 'x'
        let _ = feed(&mut e, b'b', &t);
        assert_eq!(feed(&mut e, b'\n', &t), LineStep::Line);
        assert_eq!(e.as_slice(), b"ab\n", "VERASE erases prior byte");

        // VKILL (^U) clears the whole line.
        let mut k = LineBuf::new();
        let _ = feed(&mut k, b'j', &t);
        let _ = feed(&mut k, b'u', &t);
        assert_eq!(feed(&mut k, 21, &t), LineStep::Pending); // ^U
        assert_eq!(k.as_slice(), b"", "VKILL clears the line");

        // VEOF (^D) on an empty line signals end-of-file.
        let mut eof = LineBuf::new();
        assert_eq!(feed(&mut eof, 4, &t), LineStep::Eof);
        assert_eq!(eof.len, 0, "VEOF on empty line ⇒ EOF");

        // VINTR (^C) under ISIG flushes the line and reports SIGINT.
        let mut sig = LineBuf::new();
        let _ = feed(&mut sig, b'z', &t);
        assert_eq!(feed(&mut sig, 3, &t), LineStep::Signal(2));
        assert_eq!(sig.as_slice(), b"", "VINTR flushes the line");

        // VQUIT (^\) under ISIG flushes the line and reports SIGQUIT.
        let mut q = LineBuf::new();
        let _ = feed(&mut q, b'q', &t);
        assert_eq!(feed(&mut q, 28, &t), LineStep::Signal(3));
        assert_eq!(q.as_slice(), b"", "VQUIT flushes the line");

        // VSUSP (^Z) under ISIG flushes the line and reports SIGTSTP.
        let mut z = LineBuf::new();
        let _ = feed(&mut z, b's', &t);
        assert_eq!(feed(&mut z, 26, &t), LineStep::Signal(20));
        assert_eq!(z.as_slice(), b"", "VSUSP flushes the line");

        // With NOFLSH set, a signal char generates the signal but preserves
        // the in-progress line (no input flush).
        let mut noflsh = Termios::sane_default();
        noflsh.c_lflag |= lflag::NOFLSH;
        let mut nf = LineBuf::new();
        let _ = feed(&mut nf, b'a', &noflsh);
        let _ = feed(&mut nf, b'b', &noflsh);
        assert_eq!(feed(&mut nf, 3, &noflsh), LineStep::Signal(2)); // ^C
        assert_eq!(nf.as_slice(), b"ab", "NOFLSH preserves the line on ^C");
        // ...and the preserved line still completes normally afterwards.
        assert_eq!(feed(&mut nf, b'\n', &noflsh), LineStep::Line);
        assert_eq!(nf.as_slice(), b"ab\n", "NOFLSH line completes after signal");

        // With ISIG cleared, a ^C is just an ordinary byte in the line.
        let mut noisig = Termios::sane_default();
        noisig.c_lflag &= !lflag::ISIG;
        let mut n = LineBuf::new();
        assert_eq!(feed(&mut n, 3, &noisig), LineStep::Pending);
        assert_eq!(feed(&mut n, b'\n', &noisig), LineStep::Line);
        assert_eq!(n.as_slice(), &[3u8, b'\n'], "ISIG off ⇒ ^C is literal");

        crate::serial_println!("[tty]   line discipline (canon/erase/kill/eof/intr/quit/susp/noflsh): OK");
    }

    // PendingLine: a line longer than the reader buffer is delivered in pieces.
    {
        let mut p = PendingLine::new();
        p.fill(b"abcdef\n");
        let mut small = [0u8; 3];
        assert_eq!(p.drain_into(&mut small), 3);
        assert_eq!(&small, b"abc");
        let mut rest = [0u8; 16];
        assert_eq!(p.drain_into(&mut rest), 4);
        assert_eq!(rest.get(..4), Some(&b"def\n"[..]));
        assert!(!p.has_data(), "pending fully drained");
        crate::serial_println!("[tty]   pending-line chunked delivery: OK");
    }

    // Foreground process group (job control): set/get round-trips and 0 clears.
    {
        let saved = foreground_pgid();
        set_foreground_pgid(0);
        assert_eq!(foreground_pgid(), 0, "0 clears the foreground group");
        set_foreground_pgid(4242);
        assert_eq!(foreground_pgid(), 4242, "foreground pgid round-trips");
        set_foreground_pgid(0);
        assert_eq!(foreground_pgid(), 0, "foreground pgid re-cleared");
        // Restore whatever the running system had installed (none, at boot).
        set_foreground_pgid(saved);
        crate::serial_println!("[tty]   foreground pgrp set/get: OK");
    }

    crate::serial_println!("[tty] Self-test passed.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn termios_roundtrip() {
        let t = Termios::sane_default();
        let bytes = t.to_bytes();
        assert_eq!(bytes.len(), TERMIOS_BYTES);
        let back = Termios::from_bytes(&bytes);
        assert_eq!(t, back);
    }

    #[test]
    fn default_is_canonical_with_echo() {
        let t = Termios::sane_default();
        assert!(t.is_canonical());
        assert!(t.echo_enabled());
        assert_eq!(t.vmin(), 1);
        assert_eq!(t.vtime(), 0);
    }

    #[test]
    fn control_chars_match_linux_init() {
        let t = Termios::sane_default();
        assert_eq!(t.c_cc[cc::VINTR], 3);
        assert_eq!(t.c_cc[cc::VEOF], 4);
        assert_eq!(t.c_cc[cc::VERASE], 127);
        assert_eq!(t.c_cc[cc::VKILL], 21);
    }

    #[test]
    fn raw_mode_clears_canon_and_echo() {
        let mut t = Termios::sane_default();
        t.c_lflag &= !(lflag::ICANON | lflag::ECHO);
        let back = Termios::from_bytes(&t.to_bytes());
        assert!(!back.is_canonical());
        assert!(!back.echo_enabled());
    }

    #[test]
    fn winsize_roundtrip() {
        let w = WinSize { ws_row: 24, ws_col: 80, ws_xpixel: 0, ws_ypixel: 0 };
        let back = WinSize::from_bytes(&w.to_bytes());
        assert_eq!(w, back);
    }

    #[test]
    fn isig_flushes_line_unless_noflsh() {
        let t = Termios::sane_default();

        // Default (NOFLSH clear): ^C generates SIGINT and flushes the line.
        let mut a = LineBuf::new();
        let _ = feed(&mut a, b'x', &t);
        assert_eq!(feed(&mut a, 3, &t), LineStep::Signal(2));
        assert_eq!(a.as_slice(), b"");

        // NOFLSH set: ^C generates SIGINT but preserves the line, which then
        // completes normally on the next newline.
        let mut nf = Termios::sane_default();
        nf.c_lflag |= lflag::NOFLSH;
        let mut b = LineBuf::new();
        let _ = feed(&mut b, b'a', &nf);
        let _ = feed(&mut b, b'b', &nf);
        assert_eq!(feed(&mut b, 3, &nf), LineStep::Signal(2));
        assert_eq!(b.as_slice(), b"ab");
        assert_eq!(feed(&mut b, b'\n', &nf), LineStep::Line);
        assert_eq!(b.as_slice(), b"ab\n");
    }
}
