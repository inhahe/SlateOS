//! Terminal (TTY) line discipline and `termios` state, for N terminal devices.
//!
//! This module implements the kernel side of the Linux terminal ABI: the
//! `termios` structure that `TCGETS`/`TCSETS` exchange with userspace, the
//! `winsize` structure that `TIOCGWINSZ` reports, and the canonical/raw
//! line-discipline policy that a terminal `read(2)` consults.
//!
//! ## Why a kernel TTY at all
//!
//! Before this module, a `read(2)` on the console returned exactly one
//! keystroke and `ioctl(fd, TCGETS, …)` returned `ENOTTY` — so `isatty(3)`
//! answered "no" and interactive programs (a shell, anything using readline or
//! `tcgetattr`/`tcsetattr`) could neither detect the terminal nor configure it.
//! A real interactive console *is* a terminal, so the console answers the
//! terminal-control ioctls and exposes a line discipline.
//!
//! ## Devices
//!
//! A **terminal device** is a [`TtyId`] plus the state in `TtyDevice`:
//! `termios`, `winsize`, and the leftover bytes of a canonical line that
//! overflowed a reader's buffer.  [`CONSOLE`] (id 0) is the physical keyboard
//! and screen; every other id is the slave end of a pseudo-terminal created by
//! [`pty::create`].
//!
//! What makes a device a device is only where its bytes come from and where
//! its echo goes — [`Backend`].  Everything else in this file is shared by
//! every terminal, and was already device-independent before ptys existed:
//! [`feed`] is the pure line editor, `canonical_read`/`raw_read` are the
//! `VMIN`/`VTIME` and `ICANON` policy, and `Termios`/`WinSize` are wire
//! formats.  Generalising the console to N devices was therefore a matter of
//! moving three globals into a table and routing four keyboard calls through
//! [`Backend`] — not of writing a second line discipline, which is exactly the
//! outcome to want: a pty whose `^C` handling differs from the console's is a
//! pty that will surprise somebody.
//!
//! ## One `termios` per device, shared by both ends
//!
//! Linux keeps one `termios` per tty device, shared by every file descriptor
//! open on that tty, so a `tcsetattr` by the shell is observed by its
//! children.  For a pty that sharing crosses an address space: the shell holds
//! the slave and clears `ECHO` for a password prompt, and the terminal
//! emulator holding the master must stop echoing *immediately*.  That shared
//! word is the reason a pty has to be a kernel object at all — a libc-only pty
//! built from two socketpair ends has nowhere to put it.
//!
//! ## Locking order
//!
//! `DEVICES` (this module) is taken **before** `pty::PTYS`, and neither is ever
//! held across a `sched` call.  Both are dropped before a park or a wake, in
//! the `waiters` module's documented idiom, so a terminal read that blocks
//! cannot hold the table another task needs in order to unblock it.
//!
//! ## What lives here vs. the syscall layer
//!
//! This module owns the *data* (the termios/winsize structs, their byte
//! serialisation, the default "sane terminal" settings, the device table) and
//! the *policy* (the line discipline).  The Linux syscall translator
//! (`kernel/src/syscall/linux.rs`) owns the *plumbing*: routing
//! `TCGETS`/`TCSETS`/`TIOCGWINSZ` to the right device and delivering the
//! signals this module decides are due.

// The canonical line-discipline read path and several c_cc control characters
// are wired incrementally; not every accessor has an in-tree caller yet.
#![allow(dead_code)]

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

pub mod pty;

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
    /// Send `SIGTTOU` to a **background** process that writes to the terminal.
    /// Off in the default termios (as on Linux), so background output is
    /// normally interleaved rather than stopped; a shell that wants the
    /// classic "background job blocks on output" behaviour sets it. Only the
    /// *write* gate is conditional like this — the read gate (`SIGTTIN`) and
    /// the terminal-control gate (`tcsetattr`/`tcsetpgrp`) always apply.
    pub const TOSTOP: u32 = 0x0100;
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

    /// `true` when a `\n` sent to this terminal must go out as CRLF.
    ///
    /// That is `OPOST` (do output processing at all) *and* `ONLCR` (the
    /// specific rule), which is the default pair — a terminal emulator's cursor
    /// stays in the right-hand column without it. Both the output path and the
    /// echo path ask this, and asking it in one place is what keeps them
    /// agreeing about what a line break looks like.
    #[must_use]
    pub const fn opost_nl_is_crlf(&self) -> bool {
        self.c_oflag & oflag::OPOST != 0 && self.c_oflag & oflag::ONLCR != 0
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

// ---------------------------------------------------------------------------
// The device table
// ---------------------------------------------------------------------------

/// Identifies a terminal device.  `0` is the console; higher ids are
/// pseudo-terminal slaves, allocated by [`pty::create`].
pub type TtyId = u32;

/// The physical keyboard-and-screen terminal.
pub const CONSOLE: TtyId = 0;

/// Where a terminal device's line discipline gets raw input bytes, and where
/// its echo goes.
///
/// This enum *is* the difference between one terminal and another. Everything
/// else — the editor, the `VMIN`/`VTIME` matrix, `ISIG` classification, the
/// termios wire format — is shared, which is deliberate: a pty whose `^C`
/// behaved differently from the console's would be a pty that surprises
/// people, and two copies of a line discipline drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// Device 0: bytes come from the keyboard ring buffer (filled by the PS/2
    /// IRQ and the USB HID poll) and echo is performed by the keyboard driver,
    /// which is kept in sync with the `ECHO` bit.
    Console,
    /// A pseudo-terminal: bytes come from what the master end wrote (that end
    /// is "the keyboard"), and echo is written back to the master (that end is
    /// also "the screen").
    Pty,
}

/// One terminal device's state.
///
/// Boxed in the table because `LineBuf` and `PendingLine` each embed a
/// `MAX_CANON` (4 KiB) array, and a `BTreeMap` moves its values when it
/// rebalances.
struct TtyDevice {
    backend: Backend,
    /// Shared by every fd open on this terminal, and — for a pty — by both
    /// ends. See the module docs on why that sharing is the point.
    termios: Termios,
    winsize: WinSize,
    /// The canonical line currently being edited.
    ///
    /// This belongs to the *device*, not to the reader's stack frame, for two
    /// reasons. A read cut short by a signal must be restartable without
    /// throwing away what the user already typed — Linux keeps the editing
    /// buffer in the tty for exactly this. And two processes reading the same
    /// terminal are editing one line between them, not one line each.
    line: LineBuf,
    /// Bytes from a completed canonical line that did not fit in the reader's
    /// buffer, held for the next `read(2)`.
    pending: PendingLine,
}

impl TtyDevice {
    fn new(backend: Backend) -> Self {
        Self {
            backend,
            termios: Termios::sane_default(),
            winsize: WinSize {
                ws_row: 0,
                ws_col: 0,
                ws_xpixel: 0,
                ws_ypixel: 0,
            },
            line: LineBuf::new(),
            pending: PendingLine::new(),
        }
    }
}

/// Every terminal device in the system.
///
/// Device [`CONSOLE`] is materialised on first access rather than at boot, so
/// that this table needs no initialisation call and cannot be consulted before
/// it exists. Every other entry is created by [`pty::create`] and removed when
/// both of that pty's ends are closed.
///
/// Locking order: taken before `pty::PTYS`, and never held across a park or a
/// wake (see the module docs).
static DEVICES: Mutex<BTreeMap<TtyId, Box<TtyDevice>>> = Mutex::new(BTreeMap::new());

/// Run `f` against device `id`, materialising the console if it is missing.
///
/// Returns `None` for a *pty* id with no device — a pty that was destroyed, or
/// never existed. The console is never absent, so `None` unambiguously means
/// "that pty is gone", which is what a caller must distinguish in order to
/// answer `EIO` rather than silently operating on a fresh default terminal.
fn with_device<R>(id: TtyId, f: impl FnOnce(&mut TtyDevice) -> R) -> Option<R> {
    let mut table = DEVICES.lock();
    if id == CONSOLE {
        return Some(f(table
            .entry(CONSOLE)
            .or_insert_with(|| Box::new(TtyDevice::new(Backend::Console)))));
    }
    table.get_mut(&id).map(|d| f(d))
}

/// Create the device record for a new pty slave. Called only by [`pty::create`].
pub(crate) fn create_device(id: TtyId) {
    DEVICES
        .lock()
        .insert(id, Box::new(TtyDevice::new(Backend::Pty)));
}

/// Drop a pty's device record. Called only when both pty ends are closed.
///
/// Refuses to remove the console: id 0 has no owner that could close it, and a
/// removed console would be silently recreated with default settings by the
/// next [`with_device`] call, discarding a `tcsetattr` nobody asked to undo.
pub(crate) fn destroy_device(id: TtyId) {
    if id != CONSOLE {
        DEVICES.lock().remove(&id);
    }
}

/// Whether `id` names a live terminal device.
#[must_use]
pub fn exists(id: TtyId) -> bool {
    id == CONSOLE || DEVICES.lock().contains_key(&id)
}

/// Get a copy of a terminal's termios (for `TCGETS`).
///
/// Returns the sane default for a device that does not exist, because every
/// caller is a `TCGETS` that has already validated its handle; a vanished pty
/// races with `close`, and reporting a plausible terminal is better than
/// panicking in a getter.
#[must_use]
pub fn get_termios(id: TtyId) -> Termios {
    with_device(id, |d| d.termios).unwrap_or_else(Termios::sane_default)
}

/// Replace a terminal's termios (for `TCSETS`/`TCSETSW`/`TCSETSF`).
///
/// For the console this keeps the keyboard driver's echo in sync with the new
/// `ECHO` bit, so a program clearing `ECHO` (e.g. a password prompt) stops the
/// driver echoing immediately and one setting it restores echo. A pty's echo
/// is performed by the discipline itself, which reads the bit directly.
pub fn set_termios(id: TtyId, new: Termios) {
    let backend = with_device(id, |d| {
        d.termios = new;
        d.backend
    });
    if backend == Some(Backend::Console) {
        crate::keyboard::set_echo(new.echo_enabled());
    }
}

/// `true` when a terminal is in canonical (line-buffered) input mode.
#[must_use]
pub fn is_canonical(id: TtyId) -> bool {
    get_termios(id).is_canonical()
}

/// `true` when a terminal echoes input characters.
#[must_use]
pub fn echo_enabled(id: TtyId) -> bool {
    get_termios(id).echo_enabled()
}

/// Current window size for `TIOCGWINSZ`.
///
/// If userspace set an explicit size via `TIOCSWINSZ`, that is returned. The
/// console otherwise reports its live character dimensions; a pty otherwise
/// reports zeroes, which is what Linux does for a pty nobody has sized and is
/// how a program detects "size unknown".
#[must_use]
pub fn get_winsize(id: TtyId) -> WinSize {
    let (stored, backend) = with_device(id, |d| (d.winsize, d.backend))
        .unwrap_or((WinSize::default(), Backend::Pty));
    if stored.ws_row != 0 || stored.ws_col != 0 || backend != Backend::Console {
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

/// Store an explicit window size (for `TIOCSWINSZ`).
///
/// Returns `true` if the stored size actually changed. A resize is what
/// `SIGWINCH` reports, and a `TIOCSWINSZ` that sets the same size again is not
/// a resize — signalling it would wake every full-screen program on the
/// terminal to redraw an unchanged screen.
pub fn set_winsize(id: TtyId, ws: WinSize) -> bool {
    with_device(id, |d| {
        let changed = d.winsize != ws;
        d.winsize = ws;
        changed
    })
    .unwrap_or(false)
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
        Self {
            buf: [0u8; MAX_CANON],
            len: 0,
        }
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

    /// The last byte in the line, if any.
    ///
    /// `VERASE` needs this *before* popping, because how many columns the
    /// erased character occupied on screen depends on what it was (a control
    /// byte echoed as `^X` takes two).
    fn last(&self) -> Option<u8> {
        self.buf.get(self.len.checked_sub(1)?).copied()
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

/// What the line discipline should echo in response to one input byte.
///
/// Echo is *decided* here and *performed* by the backend, which keeps [`feed`]
/// pure and testable while still putting the policy in one place. It has to be
/// the discipline's business rather than the driver's, because a pty has no
/// driver: echo on a pty is the kernel writing the byte back to the master,
/// which is the entirety of what a terminal emulator displays when you type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Echo {
    /// Emit nothing.
    None,
    /// Emit this byte verbatim.
    Byte(u8),
    /// Emit `^X` for a control byte (`ECHOCTL`), where `X` is the byte + 0x40.
    Ctrl(u8),
    /// Rub out `n` characters, each as backspace-space-backspace (`ECHOE`,
    /// and `ECHOKE` for a whole-line kill). `n == 0` emits nothing.
    Erase(usize),
    /// Emit a newline (the `\n` of a completed line, `ECHONL`, or `ECHOK`'s
    /// "start a fresh line" rendering of a line kill).
    Newline,
}

/// Whether `ch` is rendered as a two-column `^X` when `ECHOCTL` is set.
///
/// This is Linux's rule (`n_tty.c`'s `echo_char`): the C0 controls *and* `DEL`
/// qualify, but `\t` is exempt (it is echoed literally so it still reaches the
/// next tab stop) and so is `\n` (echoed raw as a line break, never as `^J`).
const fn is_ctrl_echo(ch: u8) -> bool {
    (ch < 0x20 || ch == 0x7f) && ch != b'\t' && ch != b'\n'
}

/// How `ch` should be echoed given the current `ECHO`/`ECHOCTL` settings.
///
/// Shared by the canonical editor ([`feed`]) and the raw path ([`raw_read`]) so
/// that the two cannot disagree about how a byte appears on screen.
fn render_echo(ch: u8, t: &Termios) -> Echo {
    if t.c_lflag & lflag::ECHO == 0 {
        Echo::None
    } else if is_ctrl_echo(ch) && (t.c_lflag & lflag::ECHOCTL != 0) {
        Echo::Ctrl(ch)
    } else {
        Echo::Byte(ch)
    }
}

/// How wide `ch` is on screen once echoed, so an erase can rub out the right
/// number of columns.
///
/// A control byte echoed as `^X` under `ECHOCTL` occupies two columns; `\t`
/// would occupy up to eight, which this deliberately does not model — see the
/// note in [`feed`].
///
/// Deliberately independent of the `ECHO` bit: this answers "how wide is it",
/// not "is it shown", and the callers already gate on `ECHO` themselves.
fn echo_width(ch: u8, t: &Termios) -> usize {
    if is_ctrl_echo(ch) && (t.c_lflag & lflag::ECHOCTL != 0) {
        2
    } else {
        1
    }
}

/// [`feed`], discarding the echo half of the answer.
///
/// Assertion helper only. Most line-editing checks care what a byte did to
/// the *buffer*, and threading a `_` through every one of them buries the
/// fact being asserted; echo rendering has its own dedicated assertions
/// instead, so a change to it fails a test about echo rather than thirty
/// tests about line editing.
fn step(line: &mut LineBuf, raw: u8, t: &Termios) -> LineStep {
    feed(line, raw, t).0
}

/// Feed one raw input byte to the canonical line editor.
///
/// This is the *pure* core of the line discipline — no I/O — so it is
/// exercised directly by the boot self-test.  It maintains the line buffer,
/// decides when a read should complete, and returns what should be echoed;
/// performing that echo is the caller's job (the keyboard driver for the
/// console, a write to the master end for a pty).
///
/// **Not modelled:** the column width of a literal tab. `ECHOE` rubs out one
/// column per erased character (two for a `^X`-echoed control byte), which is
/// wrong for a `\t` that expanded to a tab stop. Linux tracks the real column
/// to get this right. Doing so needs the discipline to know the cursor
/// position, which for a pty it cannot know at all — the emulator on the far
/// end owns the screen. Recorded in `todo.txt`.
fn feed(line: &mut LineBuf, raw: u8, t: &Termios) -> (LineStep, Echo) {
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

    let echo_on = t.c_lflag & lflag::ECHO != 0;

    // How an accepted byte is rendered: `^X` for a control byte under ECHOCTL,
    // otherwise verbatim. Used for ordinary bytes and for the signal
    // characters, which Linux echoes too (that is why `^C` appears on screen).
    let render = |c: u8| -> Echo { render_echo(c, t) };

    if t.c_lflag & lflag::ISIG != 0 {
        // POSIX: a signal character flushes the input queue (here, the
        // in-progress canonical line) UNLESS NOFLSH is set, in which case the
        // buffered input is preserved and only the signal is generated.
        let flush = t.c_lflag & lflag::NOFLSH == 0;
        let mut signal = |sig: u8| -> (LineStep, Echo) {
            if flush {
                line.clear();
            }
            (LineStep::Signal(sig), render(ch))
        };
        if ch == vintr {
            return signal(2); // SIGINT
        }
        if ch == vquit {
            return signal(3); // SIGQUIT
        }
        if ch == vsusp {
            // ^Z: stop the foreground job. SIGTSTP's default action stops the
            // process; SIGCONT (e.g. shell `fg`/`bg`) resumes it. The
            // in-progress line is flushed unless NOFLSH is set.
            return signal(20); // SIGTSTP
        }
    }

    if ch == veof {
        // ^D: submit the line so far (without the EOF byte).  An empty buffer
        // becomes a zero-length read (end of file).  Not echoed: the point of
        // ^D is that it is invisible punctuation, and Linux suppresses it.
        return (LineStep::Eof, Echo::None);
    }
    if ch == verase {
        // Erase echoes only if something was actually erased — rubbing out a
        // character that is not there would eat the prompt.
        let last = line.last();
        let erased = line.pop();
        let echo = match (erased, last) {
            (true, Some(c)) if echo_on && (t.c_lflag & lflag::ECHOE != 0) => {
                Echo::Erase(echo_width(c, t))
            }
            _ => Echo::None,
        };
        return (LineStep::Pending, echo);
    }
    if ch == vkill {
        // ECHOKE rubs the whole line out in place; ECHOK (the older, weaker
        // behaviour) just starts a fresh line. ECHOKE wins when both are set,
        // matching Linux.
        let width: usize = line
            .as_slice()
            .iter()
            .map(|c| echo_width(*c, t))
            .fold(0usize, |a, b| a.saturating_add(b));
        line.clear();
        let echo = if !echo_on {
            Echo::None
        } else if t.c_lflag & lflag::ECHOKE != 0 {
            Echo::Erase(width)
        } else if t.c_lflag & lflag::ECHOK != 0 {
            Echo::Newline
        } else {
            Echo::None
        };
        return (LineStep::Pending, echo);
    }
    if ch == b'\n' {
        // The newline is part of the canonical line returned to the reader.
        // ECHONL echoes it even with ECHO off — that is the bit's whole
        // purpose, so a password prompt still moves to the next line.
        let _ = line.push(b'\n');
        let echo = if echo_on || (t.c_lflag & lflag::ECHONL != 0) {
            Echo::Newline
        } else {
            Echo::None
        };
        return (LineStep::Line, echo);
    }

    // Ordinary byte: append (silently dropped if the line is full).
    let pushed = line.push(ch);
    let echo = if pushed { render(ch) } else { Echo::None };
    (LineStep::Pending, echo)
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
        Self {
            buf: [0u8; MAX_CANON],
            pos: 0,
            len: 0,
        }
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
        if let (Some(dst), Some(src)) = (
            out.get_mut(..n),
            self.buf.get(self.pos..self.pos.saturating_add(n)),
        ) {
            dst.copy_from_slice(src);
        }
        self.pos = self.pos.saturating_add(n);
        n
    }
}

/// Read a terminal's foreground process-group ID — the group that owns that
/// terminal for the purpose of job control.  A `^C`/`^\`/`^Z` under `ISIG`
/// delivers `SIGINT`/`SIGQUIT`/`SIGTSTP` to this group (see
/// [`ConsoleRead::Signal`]).
///
/// `0` means "no foreground group" — either no session holds this terminal
/// (the kernel-startup / no-shell state, or a pty nobody has `TIOCSCTTY`'d)
/// or the holder has released it — in which case a generated terminal signal
/// has no group to target and is dropped.  This mirrors Linux's `tty->pgrp`,
/// which an interactive shell installs via `tcsetpgrp(3)` for each job it
/// foregrounds.
///
/// This module deliberately keeps **no storage** of its own for it.  It used
/// to own a `FOREGROUND_PGID` atomic, which made the foreground group two
/// unrelated values: the Linux shim's `TIOCSPGRP` wrote here, libc's
/// `tcsetpgrp` wrote to a userspace static, and neither could see the other
/// — so the group that received `^C` and the group userspace believed was in
/// the foreground could disagree indefinitely.  The single copy lives with
/// the session that holds the terminal, in `proc::pcb`, and this is a derived
/// read of it.  With ptys that argument gets stronger rather than weaker: the
/// master end and the slave's shell are different processes in different
/// sessions, so a device-local copy would be wrong in one of them by
/// construction.
#[must_use]
pub fn foreground_pgid(id: TtyId) -> u64 {
    crate::proc::pcb::ctty_fg_pgrp(id).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Backend I/O — the only device-specific part of the discipline
// ---------------------------------------------------------------------------

/// One byte from a terminal's input side, or why there was not one.
///
/// A plain `Option<u8>` cannot carry the distinction that matters most here:
/// "nothing yet" (retry), "nothing ever" (hangup — deliver a short count and
/// then EOF) and "stop and handle a signal" (restart the syscall) demand three
/// different answers from the reader, and conflating any two of them produces a
/// hang or a spurious EOF.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Input {
    /// An input byte.
    Byte(u8),
    /// Nothing is available right now (a poll came up empty, or a `VTIME`
    /// deadline expired). More may arrive later.
    Empty,
    /// End of input: a pty whose last master handle has been closed. Nothing
    /// more will ever arrive.
    Hangup,
    /// The wait was cut short by a deliverable signal; the syscall must be
    /// restarted (or aborted with `EINTR`) rather than resumed here.
    Interrupted,
}

/// Block until an input byte is available for `id`.
///
/// The console never reports [`Input::Hangup`] — a program cannot unplug the
/// keyboard — and never reports [`Input::Interrupted`] either, because
/// `keyboard::read_char` is an uninterruptible HLT-spin with no waiter set to
/// register against. That is a pre-existing console limitation (a task blocked
/// reading the console cannot be killed until a key is pressed), tracked in
/// `known-issues.md`; it is not one this refactor introduces, and the pty path
/// deliberately does better.
fn backend_read_char(id: TtyId, backend: Backend) -> Input {
    match backend {
        Backend::Console => Input::Byte(crate::keyboard::read_char()),
        Backend::Pty => pty::slave_read_input_blocking(id),
    }
}

/// Take an input byte for `id` if one is ready, without blocking.
fn backend_try_read_char(id: TtyId, backend: Backend) -> Input {
    match backend {
        Backend::Console => crate::keyboard::try_read_char().map_or(Input::Empty, Input::Byte),
        Backend::Pty => pty::slave_try_read_input(id),
    }
}

/// Block until an input byte is available for `id` or the monotonic clock
/// reaches `deadline_ns`.
fn backend_read_char_timeout(id: TtyId, backend: Backend, deadline_ns: u64) -> Input {
    match backend {
        Backend::Console => crate::keyboard::read_char_timeout(deadline_ns)
            .map_or(Input::Empty, Input::Byte),
        Backend::Pty => pty::slave_read_input_timeout(id, deadline_ns),
    }
}

/// Echo `bytes` back to whoever is "typing" on `id`.
///
/// The console's echo is done by the keyboard driver as a side effect of
/// reading (it owns the cursor), so this is a no-op there and the `ECHO` bit
/// is instead pushed into the driver by [`set_termios`] and [`read`]. A pty
/// has no driver to delegate to: echo is the discipline writing the byte to
/// the master, which is the whole of what a terminal emulator sees when you
/// type.
fn backend_echo(id: TtyId, backend: Backend, bytes: &[u8]) {
    if backend == Backend::Pty {
        pty::master_push_output(id, bytes);
    }
}

/// The letter shown after `^` when a control byte is echoed under `ECHOCTL`.
///
/// Linux (`n_tty.c`) uses `c ^ 0x40`, not `c + 0x40`. The XOR is what makes
/// the mapping run in both directions: it turns 0x03 into `'C'` *and* `DEL`
/// (0x7f) into `'?'`, whereas addition would carry 0x7f past the ASCII range
/// and print garbage where every terminal shows `^?`.
const fn caret_letter(ch: u8) -> u8 {
    ch ^ 0x40
}

/// Perform the echo the line discipline decided on.
///
/// Splitting "decide" ([`feed`] / [`render_echo`]) from "perform" (here) is what
/// lets the discipline stay pure and unit-testable while still driving a device
/// that has no driver to delegate echo to.
///
/// Two Linux details are reproduced deliberately (`n_tty.c`,
/// `__process_echoes`):
///
/// * An ordinary echoed byte goes through output post-processing when `OPOST`
///   is set, which is why a newline echoes as CRLF under `ONLCR` — without it
///   the emulator's cursor would stay in the right-hand column.
/// * The `^X` rendering of a control byte and the backspace-space-backspace of
///   an erase do **not**; they are written raw, because they are the
///   discipline's own screen drawing rather than the user's data.
fn echo_step(id: TtyId, backend: Backend, t: &Termios, echo: Echo) {
    // The console echoes inside the keyboard driver as a side effect of
    // reading — it owns the cursor — so echoing again here would double every
    // keystroke. `read`/`set_termios` push the ECHO bit down to the driver
    // instead.
    if backend == Backend::Console {
        return;
    }
    let newline: &[u8] = if t.opost_nl_is_crlf() { b"\r\n" } else { b"\n" };
    match echo {
        Echo::None => {}
        // A literal `\n` byte takes the newline path so ONLCR applies to it
        // whether it arrived as a completed line or as raw-mode input.
        Echo::Newline | Echo::Byte(b'\n') => backend_echo(id, backend, newline),
        Echo::Byte(c) => backend_echo(id, backend, &[c]),
        Echo::Ctrl(c) => backend_echo(id, backend, &[b'^', caret_letter(c)]),
        Echo::Erase(n) => {
            for _ in 0..n {
                backend_echo(id, backend, b"\x08 \x08");
            }
        }
    }
}

/// Outcome of a terminal [`read`].
///
/// A normal read yields [`ConsoleRead::Data`] with the number of bytes written
/// to the caller's buffer (`0` means end-of-file on a `^D` at an empty line, or
/// nothing immediately available in a polling raw read).  A `^C`/`^\`/`^Z`
/// typed under `ISIG` interrupts the read and yields [`ConsoleRead::Signal`]
/// carrying the signal number (`SIGINT`=2 / `SIGQUIT`=3 / `SIGTSTP`=20) the
/// foreground process group must receive; the syscall layer performs the actual
/// group delivery and returns the restart/`EINTR` sentinel to the blocked
/// reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleRead {
    /// `n` bytes were written to the caller's buffer (`0` ⇒ EOF / no data).
    Data(usize),
    /// A terminal signal (`SIGINT`/`SIGQUIT`/`SIGTSTP`) was generated; deliver
    /// it to the foreground process group.  No bytes were written to the
    /// caller's buffer.
    Signal(u8),
    /// A signal already pending for the *reader* cut the wait short.  No bytes
    /// were written and nothing needs delivering — the syscall layer just
    /// returns the restart sentinel so the signal checkpoint runs.  Any line
    /// typed so far is still in the device's editor and will be there when the
    /// read restarts.
    Interrupted,
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
/// [`ConsoleRead::Signal`] when a `^C`/`^\`/`^Z` interrupted a canonical read.
pub fn read(id: TtyId, out: &mut [u8]) -> ConsoleRead {
    if out.is_empty() {
        return ConsoleRead::Data(0);
    }
    // Take the termios, the backend, and any leftover bytes in one pass, then
    // drop the device table: everything below can block, and holding the table
    // across a park would deadlock against the writer that unblocks us.
    let Some((t, backend, leftover)) = with_device(id, |d| {
        let leftover = if d.pending.has_data() {
            Some(d.pending.drain_into(out))
        } else {
            None
        };
        (d.termios, d.backend, leftover)
    }) else {
        // The pty was destroyed. A read on a terminal that no longer exists is
        // end of file, not an error: the reader's own handle is still valid,
        // and EOF is what every caller of a vanished terminal must do anyway.
        return ConsoleRead::Data(0);
    };
    if let Some(n) = leftover {
        return ConsoleRead::Data(n);
    }

    // The Linux read path is authoritative for console echo: keep the keyboard
    // driver's echo in sync with this terminal's ECHO bit.
    if backend == Backend::Console {
        crate::keyboard::set_echo(t.echo_enabled());
    }

    if t.is_canonical() {
        canonical_read(id, backend, &t, out)
    } else {
        raw_read(id, backend, &t, out)
    }
}

/// Canonical-mode read: edit a line until a terminator, then deliver it.
///
/// A `^C`/`^\`/`^Z` typed under `ISIG` interrupts the read immediately and
/// returns [`ConsoleRead::Signal`]; the line in progress has already been
/// discarded by [`feed`] (unless `NOFLSH` is set), and this function delivers
/// no partial data either way (matching Linux: an interrupted canonical read
/// returns `-EINTR`, not the editing buffer).
fn canonical_read(id: TtyId, backend: Backend, t: &Termios, out: &mut [u8]) -> ConsoleRead {
    loop {
        // Blocking input is taken with no lock held: the writer that unblocks
        // us has to take the device table to do it.
        let raw = match backend_read_char(id, backend) {
            Input::Byte(b) => b,
            // End of input — a pty whose master end has been closed. Deliver
            // whatever has been typed so far (an unterminated final line,
            // exactly as Linux delivers on hangup); the buffer is now empty, so
            // the next call naturally returns 0.
            Input::Hangup | Input::Empty => return deliver_line(id, out),
            // Leave the edited line in the device — that is the whole reason it
            // lives there — so a restarted read resumes mid-line.
            Input::Interrupted => return ConsoleRead::Interrupted,
        };
        let Some((step, echo)) = with_device(id, |d| feed(&mut d.line, raw, t)) else {
            // The pty was destroyed while we were blocked. EOF, not an error.
            return ConsoleRead::Data(0);
        };
        echo_step(id, backend, t, echo);
        match step {
            LineStep::Pending => {}
            // Both terminators submit the buffer; the difference is only that
            // `feed` left the `\n` in it for `Line` and not for `Eof`, so an
            // empty `Eof` line delivers zero bytes — which *is* end of file.
            LineStep::Line | LineStep::Eof => return deliver_line(id, out),
            // A signal char (^C/^\) flushed the in-progress line: abandon the
            // read and let the syscall layer deliver the signal to the
            // foreground process group, returning EINTR/ERESTARTSYS to us.
            LineStep::Signal(sig) => return ConsoleRead::Signal(sig),
        }
    }
}

/// Move the finished editor line into the device's pending buffer and drain as
/// much of it as fits in `out`.
///
/// The two-step exists so that a reader whose buffer is smaller than the line
/// gets the remainder on its next call rather than losing it.
fn deliver_line(id: TtyId, out: &mut [u8]) -> ConsoleRead {
    with_device(id, |d| {
        d.pending.fill(d.line.as_slice());
        d.line.clear();
        d.pending.drain_into(out)
    })
    .map_or(ConsoleRead::Data(0), ConsoleRead::Data)
}

/// Non-canonical (raw) read honouring both `VMIN` and `VTIME` (see
/// [`read`]).
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
fn raw_read(id: TtyId, backend: Backend, t: &Termios, out: &mut [u8]) -> ConsoleRead {
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

    // Accept one byte: a signal character aborts the read (echoed first, as
    // Linux's `n_tty_receive_signal_char` does — that is why `^C` still appears
    // even in raw mode); anything else is stored and echoed.
    //
    // A macro rather than a closure because it both borrows `out` mutably and
    // returns from `raw_read`, which no closure can do.
    macro_rules! accept {
        ($c:expr) => {{
            let c: u8 = $c;
            echo_step(id, backend, t, render_echo(c, t));
            if let Some(s) = sig_for(c) {
                return ConsoleRead::Signal(s);
            }
            if let Some(slot) = out.get_mut(n) {
                *slot = c;
            }
            n = n.saturating_add(1);
        }};
    }

    match (vmin == 0, vtime_ns == 0) {
        // MIN=0, TIME=0: pure poll.
        (true, true) => {
            while n < cap {
                match backend_try_read_char(id, backend) {
                    Input::Byte(c) => accept!(c),
                    // A poll never blocks, so it cannot be interrupted; every
                    // other answer means "return what we have".
                    Input::Empty | Input::Hangup | Input::Interrupted => break,
                }
            }
        }
        // MIN=0, TIME>0: bounded read timeout on the first byte.
        (true, false) => {
            let deadline = crate::hrtimer::now_ns().saturating_add(vtime_ns);
            match backend_read_char_timeout(id, backend, deadline) {
                Input::Byte(c) => {
                    accept!(c);
                    // Drain any bytes already buffered alongside the first.
                    while n < cap {
                        match backend_try_read_char(id, backend) {
                            Input::Byte(c) => accept!(c),
                            Input::Empty | Input::Hangup | Input::Interrupted => break,
                        }
                    }
                }
                // Nothing has been consumed yet, so a signal can be reported
                // without losing input.
                Input::Interrupted => return ConsoleRead::Interrupted,
                // Timeout or hangup: MIN=0 means a zero-byte return is legal.
                Input::Empty | Input::Hangup => {}
            }
        }
        // MIN>0, TIME=0: block for VMIN bytes, then drain ready extras.
        (false, true) => {
            while n < cap {
                let got = if n >= vmin {
                    backend_try_read_char(id, backend)
                } else {
                    backend_read_char(id, backend)
                };
                match got {
                    Input::Byte(c) => accept!(c),
                    Input::Interrupted if n == 0 => return ConsoleRead::Interrupted,
                    // Hangup is not a timeout, and a signal after some bytes
                    // have already been consumed must not discard them: both
                    // deliver the short count and let the next call decide.
                    Input::Empty | Input::Hangup | Input::Interrupted => break,
                }
            }
        }
        // MIN>0, TIME>0: block for the first byte, then inter-byte timer.
        (false, false) => {
            match backend_read_char(id, backend) {
                Input::Byte(c) => accept!(c),
                Input::Interrupted => return ConsoleRead::Interrupted,
                Input::Empty | Input::Hangup => return ConsoleRead::Data(0),
            }
            while n < cap && n < vmin {
                let deadline = crate::hrtimer::now_ns().saturating_add(vtime_ns);
                match backend_read_char_timeout(id, backend, deadline) {
                    Input::Byte(c) => accept!(c),
                    // Inter-byte timer expired, hangup, or a signal: all end
                    // the read with what has been collected.
                    Input::Empty | Input::Hangup | Input::Interrupted => break,
                }
            }
        }
    }
    ConsoleRead::Data(n)
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

/// Write program output to terminal `id`.
///
/// The output counterpart of [`read`], and it exists for the same reason: a
/// program does not know, and must not have to know, which backend is behind
/// its terminal. Before this existed the read path was already device-aware
/// (`tty_read_into_user` resolves the caller's controlling terminal) while the
/// write path went straight to the physical console — so a shell started on a
/// pty would have taken its input from the pty and printed its output on the
/// screen behind the terminal emulator. Every write to a terminal goes through
/// here.
///
/// # Why OPOST lives behind the backend split rather than here
///
/// `ONLCR` exists to turn the line discipline's `\n` into whatever the thing on
/// the other end considers a line break. For a pty that is CRLF, because the
/// other end is a terminal emulator; [`pty::slave_write`] applies it. For our
/// framebuffer console the other end is a `putchar` that already treats `\n` as
/// a line break, so applying ONLCR would emit a stray CR. The transformation is
/// therefore a property of the backend, not of the caller, which is exactly why
/// it belongs on this side of the dispatch — a caller that had to know would be
/// back to knowing which backend it is on.
///
/// # Errors
///
/// * `IoError` — the terminal no longer exists, or a pty whose master has
///   closed (the terminal was unplugged).
/// * `Interrupted` — a deliverable signal arrived before any byte was written.
pub fn write(id: TtyId, data: &[u8]) -> KernelResult<usize> {
    if data.is_empty() {
        return Ok(0);
    }
    // The backend, then drop the table: the pty path blocks on the output ring,
    // and holding the device table across a park would deadlock against the
    // master read that frees the space.
    let Some(backend) = with_device(id, |d| d.backend) else {
        return Err(KernelError::IoError);
    };
    match backend {
        Backend::Console => {
            console_write_bytes(data);
            Ok(data.len())
        }
        Backend::Pty => pty::slave_write(pty::PtyHandle::new_slave(id), data),
    }
}

/// Push bytes at the framebuffer/serial console.
///
/// `write_str` when the whole buffer is valid UTF-8, because that path reaches
/// both the framebuffer and the serial log; otherwise byte-at-a-time, since
/// `putchar` takes a byte and the serial mirror can only note the size.
fn console_write_bytes(bytes: &[u8]) {
    if let Ok(s) = core::str::from_utf8(bytes) {
        crate::console::write_str(s);
    } else {
        for &b in bytes {
            crate::console::putchar(b);
        }
        crate::serial_print!("<{} bytes>", bytes.len());
    }
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
    let w = WinSize {
        ws_row: 24,
        ws_col: 80,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    assert_eq!(WinSize::from_bytes(&w.to_bytes()), w, "winsize round-trip");
    let live = get_winsize(CONSOLE);
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
        assert_eq!(step(&mut line, b'h', &t), LineStep::Pending);
        assert_eq!(step(&mut line, b'i', &t), LineStep::Pending);
        assert_eq!(step(&mut line, b'\n', &t), LineStep::Line);
        assert_eq!(line.as_slice(), b"hi\n", "canonical line content");

        // VERASE (DEL) erases the last byte: "ax\x7fb\n" → "ab\n".
        let mut e = LineBuf::new();
        let _ = step(&mut e, b'a', &t);
        let _ = step(&mut e, b'x', &t);
        assert_eq!(step(&mut e, 127, &t), LineStep::Pending); // erase 'x'
        let _ = step(&mut e, b'b', &t);
        assert_eq!(step(&mut e, b'\n', &t), LineStep::Line);
        assert_eq!(e.as_slice(), b"ab\n", "VERASE erases prior byte");

        // VKILL (^U) clears the whole line.
        let mut k = LineBuf::new();
        let _ = step(&mut k, b'j', &t);
        let _ = step(&mut k, b'u', &t);
        assert_eq!(step(&mut k, 21, &t), LineStep::Pending); // ^U
        assert_eq!(k.as_slice(), b"", "VKILL clears the line");

        // VEOF (^D) on an empty line signals end-of-file.
        let mut eof = LineBuf::new();
        assert_eq!(step(&mut eof, 4, &t), LineStep::Eof);
        assert_eq!(eof.len, 0, "VEOF on empty line ⇒ EOF");

        // VINTR (^C) under ISIG flushes the line and reports SIGINT.
        let mut sig = LineBuf::new();
        let _ = step(&mut sig, b'z', &t);
        assert_eq!(step(&mut sig, 3, &t), LineStep::Signal(2));
        assert_eq!(sig.as_slice(), b"", "VINTR flushes the line");

        // VQUIT (^\) under ISIG flushes the line and reports SIGQUIT.
        let mut q = LineBuf::new();
        let _ = step(&mut q, b'q', &t);
        assert_eq!(step(&mut q, 28, &t), LineStep::Signal(3));
        assert_eq!(q.as_slice(), b"", "VQUIT flushes the line");

        // VSUSP (^Z) under ISIG flushes the line and reports SIGTSTP.
        let mut z = LineBuf::new();
        let _ = step(&mut z, b's', &t);
        assert_eq!(step(&mut z, 26, &t), LineStep::Signal(20));
        assert_eq!(z.as_slice(), b"", "VSUSP flushes the line");

        // With NOFLSH set, a signal char generates the signal but preserves
        // the in-progress line (no input flush).
        let mut noflsh = Termios::sane_default();
        noflsh.c_lflag |= lflag::NOFLSH;
        let mut nf = LineBuf::new();
        let _ = step(&mut nf, b'a', &noflsh);
        let _ = step(&mut nf, b'b', &noflsh);
        assert_eq!(step(&mut nf, 3, &noflsh), LineStep::Signal(2)); // ^C
        assert_eq!(nf.as_slice(), b"ab", "NOFLSH preserves the line on ^C");
        // ...and the preserved line still completes normally afterwards.
        assert_eq!(step(&mut nf, b'\n', &noflsh), LineStep::Line);
        assert_eq!(nf.as_slice(), b"ab\n", "NOFLSH line completes after signal");

        // With ISIG cleared, a ^C is just an ordinary byte in the line.
        let mut noisig = Termios::sane_default();
        noisig.c_lflag &= !lflag::ISIG;
        let mut n = LineBuf::new();
        assert_eq!(step(&mut n, 3, &noisig), LineStep::Pending);
        assert_eq!(step(&mut n, b'\n', &noisig), LineStep::Line);
        assert_eq!(n.as_slice(), &[3u8, b'\n'], "ISIG off ⇒ ^C is literal");

        crate::serial_println!(
            "[tty]   line discipline (canon/erase/kill/eof/intr/quit/susp/noflsh): OK"
        );
    }

    // Echo rendering.  `feed` only *decides* what appears on screen; the
    // backend performs it.  These assertions pin the decision, because a pty
    // has no keyboard driver to fall back on — whatever `feed` returns here is
    // literally what the terminal emulator on the master end will draw.
    {
        let t = Termios::sane_default();

        // A printable byte echoes as itself; a newline is its own case so
        // ONLCR can turn it into CRLF at the backend.
        let mut l = LineBuf::new();
        assert_eq!(feed(&mut l, b'a', &t).1, Echo::Byte(b'a'), "printable echo");
        assert_eq!(feed(&mut l, b'\n', &t).1, Echo::Newline, "newline echo");

        // ECHOCTL renders a control byte as `^X`, and the caret letter comes
        // from `caret_letter` — the XOR mapping, so DEL shows as `^?` rather
        // than as the out-of-range byte an addition would produce.
        let mut c = LineBuf::new();
        assert_eq!(feed(&mut c, 1, &t).1, Echo::Ctrl(1), "^A renders as Ctrl");
        assert_eq!(caret_letter(1), b'A', "caret letter for ^A");
        assert_eq!(caret_letter(3), b'C', "caret letter for ^C");
        assert_eq!(caret_letter(127), b'?', "caret letter for DEL is '?'");

        // A tab is exempt from ECHOCTL: it must be echoed literally or it
        // would never reach the next tab stop.
        let mut tab = LineBuf::new();
        assert_eq!(feed(&mut tab, b'\t', &t).1, Echo::Byte(b'\t'), "tab echo");

        // ECHOE rubs out the erased character, two columns for a `^X`.
        let mut e = LineBuf::new();
        let _ = step(&mut e, b'a', &t);
        assert_eq!(feed(&mut e, 127, &t).1, Echo::Erase(1), "erase a plain byte");
        // ^A is only *stored* (rather than generating a signal) with ISIG
        // cleared, which is the configuration that lets us erase it.
        let mut ctrl = Termios::sane_default();
        ctrl.c_lflag &= !lflag::ISIG;
        let mut e2 = LineBuf::new();
        let _ = step(&mut e2, 1, &ctrl);
        assert_eq!(
            feed(&mut e2, 127, &ctrl).1,
            Echo::Erase(2),
            "erasing a ^X-echoed byte rubs out two columns"
        );

        // Clearing ECHO silences everything the editor would have drawn.
        let mut off = Termios::sane_default();
        off.c_lflag &= !lflag::ECHO;
        let mut q = LineBuf::new();
        assert_eq!(feed(&mut q, b'a', &off).1, Echo::None, "ECHO off ⇒ silent");

        crate::serial_println!("[tty]   echo rendering (printable/^X/tab/erase/off): OK");
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

    // Foreground process group (job control).  There is nothing to set here
    // any more: the value is owned by whichever session holds the console
    // (`proc::pcb`'s controlling-terminal table), and this module only reads
    // it.  What is worth asserting is that the read agrees with that table
    // rather than caching — if this module ever reacquires storage of its
    // own, the two would drift and `^C` would go to the wrong job.
    {
        assert_eq!(
            foreground_pgid(CONSOLE),
            crate::proc::pcb::ctty_fg_pgrp(CONSOLE).unwrap_or(0),
            "console foreground pgrp must be a derived read of the ctty table"
        );
        crate::serial_println!("[tty]   foreground pgrp is session-owned: OK");
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
        let w = WinSize {
            ws_row: 24,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let back = WinSize::from_bytes(&w.to_bytes());
        assert_eq!(w, back);
    }

    #[test]
    fn isig_flushes_line_unless_noflsh() {
        let t = Termios::sane_default();

        // Default (NOFLSH clear): ^C generates SIGINT and flushes the line.
        let mut a = LineBuf::new();
        let _ = step(&mut a, b'x', &t);
        assert_eq!(step(&mut a, 3, &t), LineStep::Signal(2));
        assert_eq!(a.as_slice(), b"");

        // NOFLSH set: ^C generates SIGINT but preserves the line, which then
        // completes normally on the next newline.
        let mut nf = Termios::sane_default();
        nf.c_lflag |= lflag::NOFLSH;
        let mut b = LineBuf::new();
        let _ = step(&mut b, b'a', &nf);
        let _ = step(&mut b, b'b', &nf);
        assert_eq!(step(&mut b, 3, &nf), LineStep::Signal(2));
        assert_eq!(b.as_slice(), b"ab");
        assert_eq!(step(&mut b, b'\n', &nf), LineStep::Line);
        assert_eq!(b.as_slice(), b"ab\n");
    }
}
