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
//! `termios`, `winsize`, the canonical line being edited, and the *input
//! queue* of bytes that have been through the line discipline and are waiting
//! for a reader.  [`CONSOLE`] (id 0) is the physical keyboard and screen; every
//! other id is the slave end of a pseudo-terminal created by [`pty::create`].
//!
//! What makes a device a device is only where its bytes come from and where
//! its echo goes — [`Backend`].  Everything else in this file is shared by
//! every terminal: [`receive`] is the line discipline (signal characters,
//! input translation, and — through [`feed`] — canonical editing and echo),
//! `canonical_read`/`raw_read` are the `ICANON` and `VMIN`/`VTIME` policy for
//! taking bytes *out* of the input queue, and `Termios`/`WinSize` are wire
//! formats.  There is one line discipline, not one per backend: a pty whose
//! `^C` handling differs from the console's is a pty that will surprise
//! somebody.
//!
//! ## When input is processed: on arrival, not on read
//!
//! POSIX and every Unix put the line discipline on the *receive* side: a byte
//! is classified, edited and echoed when it arrives, and `read(2)` only takes
//! finished bytes out of a queue. The consequence that matters most is `^C`:
//! it must interrupt a program that is computing, sleeping or waiting on
//! something else — a program that is *not* reading the terminal — because
//! that is the only kind of program anybody ever wants to interrupt.
//!
//! A pty therefore runs [`receive`] inside the master's write
//! ([`pty::master_write`]): the signal is decided there and delivered by the
//! syscall layer before the write returns, the typed-ahead text is echoed
//! immediately, and complete lines wait in the input queue. Until 2026-09-24
//! the discipline ran inside the *reader* instead, so a `^C` written to a
//! master whose program was not reading sat in a ring forever and no signal
//! was ever generated — `ctest-pty`'s child spun for two million iterations
//! waiting for a `SIGINT` that nothing could raise (known-issues.md
//! `A-PTY-CTRL-C-IS-ONLY-SEEN-BY-A-READER`).
//!
//! **The console is the exception, and a known one.** Its raw input is the
//! keyboard ring, which the kernel shell and `SYS_CONSOLE_READ_CHAR` also read
//! directly and which must keep delivering byte 0x03 to them as data. So the
//! console receives a keystroke when a *terminal* reader pulls it off that
//! ring — every keystroke already typed is pulled and received before the
//! reader decides anything, so type-ahead is still processed in order, but a
//! console program that is not reading cannot be interrupted by `^C`. Fixing
//! that needs a decision about who owns the keyboard ring while a user session
//! holds the console: known-issues.md `A-CONSOLE-CTRL-C-IS-ONLY-SEEN-BY-A-READER`.
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
use alloc::vec::Vec;
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
    /// Translate a received NL into CR.
    pub const INLCR: u32 = 0x0040;
    /// Discard a received CR.
    pub const IGNCR: u32 = 0x0080;
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
/// Boxed in the table because `LineBuf` embeds a 4 KiB array, and a
/// `BTreeMap` moves its values when it rebalances.
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
    /// Bytes the line discipline has finished with, waiting for a reader:
    /// completed canonical lines, or raw bytes in non-canonical mode. See
    /// [`InputQueue`].
    input: InputQueue,
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
            input: InputQueue::new(),
        }
    }

    /// Discard all unread input: the line being edited and everything queued.
    ///
    /// What `^C` does under `ISIG` without `NOFLSH`, and what `TCSETSF` /
    /// `TCIFLUSH` ask for.
    fn flush_input(&mut self) {
        self.line.clear();
        self.input.clear();
    }

    /// Apply a new termios, carrying unread input across a change of mode.
    ///
    /// Input already received was processed under the *old* mode, so a change
    /// of `ICANON` must say what becomes of it rather than strand it — the
    /// same two rules as Linux's `n_tty_set_termios`:
    ///
    /// * **canonical → raw:** the half-typed line becomes ordinary raw input.
    ///   A shell that switches to raw mode to run its own line editor must
    ///   still see what the user typed ahead while the previous command ran.
    /// * **raw → canonical:** whatever raw input is unread becomes one line
    ///   that is already complete (Linux calls this a "push"). Otherwise those
    ///   bytes would wait for a newline the user already typed — or never will.
    fn set_termios(&mut self, new: Termios) {
        let was_canonical = self.termios.is_canonical();
        self.termios = new;
        match (was_canonical, new.is_canonical()) {
            (true, false) => {
                // End-of-file marks mean nothing to a raw reader, which gets
                // bytes rather than lines; removing them here keeps raw mode
                // free of them, so a full raw queue is always full of data.
                self.input.purge_eof_marks();
                // Anything that does not fit is dropped: the queue holds at
                // least `MAX_CANON` and a line is shorter than that, so this
                // only bites when complete lines are ALSO unread — in which
                // case the reader is so far behind that input is being lost
                // anyway.
                for &b in self.line.as_slice() {
                    if !self.input.push(b, 0) {
                        break;
                    }
                }
                self.line.clear();
            }
            (false, true) => self.input.mark_line_end(),
            _ => {}
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
///
/// A change of `ICANON` carries unread input across per the rules on
/// `TtyDevice::set_termios`.
///
/// A pty's parked readers are woken: a read waits for a condition of the mode
/// it started in (a complete line, `VMIN` bytes), and after a change of mode it
/// must re-dispatch rather than wait for something the new mode cannot
/// produce. They re-check under the lock, so a change that alters nothing
/// costs each of them one look.
pub fn set_termios(id: TtyId, new: Termios) {
    let backend = with_device(id, |d| {
        d.set_termios(new);
        d.backend
    });
    match backend {
        Some(Backend::Console) => crate::keyboard::set_echo(new.echo_enabled()),
        Some(Backend::Pty) => pty::wake_input_waiters(id),
        None => {}
    }
}

/// Discard a terminal's unread input (`TCSETSF`, `tcflush(TCIFLUSH)`).
///
/// For the console that includes keystrokes still in the keyboard ring: they
/// have not been through the line discipline yet (see the module docs on the
/// console's receive timing), but to the program asking they are exactly as
/// much "typed ahead" as the queued ones, and a password prompt that flushes
/// type-ahead must not then read it.
///
/// Wakes a pty master blocked on a full input queue: the flush is what makes
/// room for it.
pub fn flush_input(id: TtyId) {
    let backend = with_device(id, |d| {
        d.flush_input();
        d.backend
    });
    match backend {
        Some(Backend::Console) => while crate::keyboard::try_read_char().is_some() {},
        Some(Backend::Pty) => pty::wake_input_waiters(id),
        None => {}
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

/// Whether a read on terminal `id` would find input without waiting for more
/// to arrive — the `POLLIN` half of a slave's readiness.
///
/// **Exact in both modes**, which it could not be while the line discipline
/// ran inside the reader: then a canonical slave holding an unterminated line
/// could only be reported as an upper bound ("bytes are present"), and a poller
/// woken by it would issue a read that parked. Now the line is edited as it
/// arrives, so "a complete line is queued" is a fact that can be looked up.
///
/// * **Canonical:** a complete line is queued — including the empty line a
///   `^D` makes, which a read returns as end of file.
/// * **Raw:** at least `VMIN` bytes are queued when `VTIME` is 0 and `VMIN` is
///   not, otherwise at least one — Linux's `input_available_p` rule for a
///   poll, because a read with `VMIN = 4, VTIME = 0` really would park on
///   three bytes.
///
/// A hangup is not input and is not reported here; the pty layer adds it.
/// `false` for a terminal that does not exist.
#[must_use]
pub fn input_ready(id: TtyId) -> bool {
    with_device(id, |d| {
        if d.termios.is_canonical() {
            d.input.lines() > 0
        } else {
            let vmin = usize::from(d.termios.vmin());
            let want = if d.termios.vtime() == 0 && vmin > 0 {
                vmin
            } else {
                1
            };
            d.input.data_len() >= want
        }
    })
    .unwrap_or(false)
}

/// How many bytes a read on terminal `id` could return right now — `FIONREAD`
/// (`TIOCINQ`).
///
/// Exact, for the same reason [`input_ready`] is. In canonical mode it counts
/// the bytes of complete lines only (Linux's `inq_canon`): the line still being
/// edited is not readable until its terminator arrives, and an end-of-file mark
/// is not a byte. In raw mode it counts every queued byte.
///
/// Returns 0 for a terminal that does not exist, which is the same answer a
/// `read` on it gives.
#[must_use]
pub fn input_bytes(id: TtyId) -> usize {
    with_device(id, |d| {
        if d.termios.is_canonical() {
            d.input.committed_data_len()
        } else {
            d.input.data_len()
        }
    })
    .unwrap_or(0)
}

/// Whether terminal `id`'s input queue has room for at least one more byte —
/// the `POLLOUT` half of a pty *master's* readiness.
///
/// A hint rather than a promise, in the direction that is safe: a canonical
/// terminal can also accept ordinary bytes into its line editor while the queue
/// is full, so this under-reports rather than invites a write that would park.
#[must_use]
pub fn input_room(id: TtyId) -> bool {
    with_device(id, |d| d.input.free() > 0).unwrap_or(false)
}

/// Current window size for `TIOCGWINSZ`.
///
/// If userspace set an explicit size via `TIOCSWINSZ`, that is returned. The
/// console otherwise reports its live character dimensions; a pty otherwise
/// reports zeroes, which is what Linux does for a pty nobody has sized and is
/// how a program detects "size unknown".
#[must_use]
pub fn get_winsize(id: TtyId) -> WinSize {
    let (stored, backend) =
        with_device(id, |d| (d.winsize, d.backend)).unwrap_or((WinSize::default(), Backend::Pty));
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

/// Maximum bytes in one canonical line, terminator included (Linux
/// `MAX_CANON`).  Ordinary input past `MAX_CANON - 1` bytes is dropped until a
/// terminator arrives; the last slot is reserved so that a terminator always
/// fits, which is what keeps a full line from being unterminatable.
pub const MAX_CANON: usize = 4096;

/// Slots in a terminal's input queue (Linux `N_TTY_BUF_SIZE`).
///
/// At least `MAX_CANON`, so that any line the editor can produce fits in an
/// empty queue — otherwise a full-length line could never be delivered and the
/// master writing its terminator would wait forever.
pub const INPUT_QUEUE_CAPACITY: usize = 4096;

/// Outcome of feeding one input byte to the canonical line editor.
///
/// Signal characters are not the editor's business: [`receive`] classifies
/// them before a byte reaches [`feed`], in both modes, so there is exactly one
/// implementation of `ISIG` rather than one per read path (there used to be
/// three, and investigating a `^C` meant instrumenting all of them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineStep {
    /// Byte consumed; keep editing the current line.
    Pending,
    /// A line terminator (`\n`, `VEOL`, `VEOL2`) completed the line, which
    /// includes the terminator.
    Line,
    /// `VEOF` (`^D`): the line so far is complete *without* the `^D`. An empty
    /// line is a zero-length read — end of file.
    Eof,
}

/// A fixed-capacity in-progress line buffer for the canonical editor.
struct LineBuf {
    buf: [u8; MAX_CANON],
    len: usize,
    /// Current cursor column, tracked so that `ECHOE` can rub out the correct
    /// number of columns for a tab (1–8 depending on the tab stop the cursor
    /// was at when the tab was echoed).  Updated by [`feed`], not by the
    /// push/pop/clear methods, because the column depends on the `Termios`
    /// (control bytes are 2 columns under `ECHOCTL`, 1 otherwise).
    col: usize,
}

impl LineBuf {
    const fn new() -> Self {
        Self {
            buf: [0u8; MAX_CANON],
            len: 0,
            col: 0,
        }
    }

    /// Append an ordinary byte; `false` if the line is full.
    ///
    /// "Full" is `MAX_CANON - 1`: the last slot belongs to the terminator, so
    /// a line that has filled up can still be ended. Before 2026-09-24 the
    /// terminator's own push could fail on a full line and the line was
    /// delivered without its `\n` — a reader splitting on newlines then glued
    /// it to the next one.
    fn push(&mut self, c: u8) -> bool {
        if self.len >= MAX_CANON.saturating_sub(1) {
            return false;
        }
        self.push_terminator(c)
    }

    /// Append a line terminator, which may use the reserved last slot.
    fn push_terminator(&mut self, c: u8) -> bool {
        if let Some(slot) = self.buf.get_mut(self.len) {
            *slot = c;
            self.len = self.len.saturating_add(1);
            true
        } else {
            false
        }
    }

    /// Move up to `out.len()` bytes off the *front* of the line into `out`.
    ///
    /// Only for a hangup, where a half-typed line is delivered as it stands
    /// because nothing will ever finish it. O(len) for the shift, on a path
    /// taken once per terminal lifetime.
    fn take_front(&mut self, out: &mut [u8]) -> usize {
        let n = self.len.min(out.len());
        if let (Some(dst), Some(src)) = (out.get_mut(..n), self.buf.get(..n)) {
            dst.copy_from_slice(src);
        }
        self.buf.copy_within(n..self.len, 0);
        self.len = self.len.saturating_sub(n);
        n
    }

    fn is_empty(&self) -> bool {
        self.len == 0
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

    /// Empty the line. The cursor column goes back to 0 with it: the column is
    /// measured from where the line began, and there is no line.
    fn clear(&mut self) {
        self.len = 0;
        self.col = 0;
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
/// A control byte echoed as `^X` under `ECHOCTL` occupies two columns; a tab
/// advances to the next 8-column tab stop (1–8 columns).  Ordinary bytes are
/// one column each.
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

/// Advance a cursor column after echoing one byte.
///
/// This models what the terminal actually shows — a tab jumps to the next
/// 8-column tab stop (`(col | 7) + 1`), a control byte echoed as `^X` takes
/// two columns, `\r` resets to 0, and everything else takes one.
fn advance_col(col: usize, ch: u8, t: &Termios) -> usize {
    if ch == b'\t' {
        // Next 8-column tab stop: 0→8, 1→8, 7→8, 8→16, etc.
        (col | 7).wrapping_add(1)
    } else if ch == b'\r' {
        0
    } else if is_ctrl_echo(ch) && (t.c_lflag & lflag::ECHOCTL != 0) {
        col.wrapping_add(2) // ^X
    } else {
        col.wrapping_add(1)
    }
}

/// Recompute the cursor column by replaying the echo width of every byte in
/// the buffer from the start.  Called after an erase (`pop`) so the column
/// reflects the state *without* the erased character.  O(len) but `MAX_CANON`
/// is small (~256 bytes), and this only runs on erase — which is interactive,
/// so latency is negligible.
fn compute_col(buf: &[u8], t: &Termios) -> usize {
    let mut c = 0usize;
    for &ch in buf {
        c = advance_col(c, ch, t);
    }
    c
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
/// **Column tracking:** the line buffer carries a cursor column (`LineBuf.col`)
/// advanced by [`advance_col`] on every push, reset on clear/newline, and
/// recomputed from scratch on erase.  `ECHOE` rubs out the column *delta*
/// between the current position and the position before the erased byte, so a
/// tab at column 5 that advanced to column 8 produces three backspace-space-
/// backspace sequences.  For a pty the emulator on the far end owns the real
/// screen, so our column is an approximation — see `todo.txt` for the caveat.
fn feed(line: &mut LineBuf, ch: u8, t: &Termios) -> (LineStep, Echo) {
    let echo_on = t.c_lflag & lflag::ECHO != 0;

    // How an accepted byte is rendered: `^X` for a control byte under ECHOCTL,
    // otherwise verbatim.
    let render = |c: u8| -> Echo { render_echo(c, t) };

    if is_cc(t, cc::VEOF, ch) {
        // ^D: submit the line so far (without the EOF byte).  An empty buffer
        // becomes a zero-length read (end of file).  Not echoed: the point of
        // ^D is that it is invisible punctuation, and Linux suppresses it.
        return (LineStep::Eof, Echo::None);
    }
    if is_cc(t, cc::VERASE, ch) {
        // Erase echoes only if something was actually erased — rubbing out a
        // character that is not there would eat the prompt.
        let old_col = line.col;
        let erased = line.pop();
        if erased {
            line.col = compute_col(line.as_slice(), t);
        }
        let echo = if erased && echo_on && (t.c_lflag & lflag::ECHOE != 0) {
            // Rub out the column delta — correct for tabs (1–8 columns)
            // and control bytes (2 columns under ECHOCTL).
            Echo::Erase(old_col.saturating_sub(line.col))
        } else {
            Echo::None
        };
        return (LineStep::Pending, echo);
    }
    if is_cc(t, cc::VKILL, ch) {
        // ECHOKE rubs the whole line out in place; ECHOK (the older, weaker
        // behaviour) just starts a fresh line. ECHOKE wins when both are set,
        // matching Linux.
        let width = line.col;
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
        //
        // `push_terminator` cannot fail: `push` stops one slot short of
        // `MAX_CANON` precisely so that this slot is always free.
        let _ = line.push_terminator(b'\n');
        line.col = 0; // newline resets the cursor column
        let echo = if echo_on || (t.c_lflag & lflag::ECHONL != 0) {
            Echo::Newline
        } else {
            Echo::None
        };
        return (LineStep::Line, echo);
    }
    if is_cc(t, cc::VEOL, ch) || (t.c_lflag & lflag::IEXTEN != 0 && is_cc(t, cc::VEOL2, ch)) {
        // An alternative line terminator. Linux keeps it in the line, as it
        // keeps `\n`, and echoes it like an ordinary byte (it is not a line
        // break on screen). `VEOL2` is an extension, so it needs `IEXTEN`.
        //
        // Unhandled until 2026-09-24: `VEOL` was stored as an ordinary byte,
        // so a program that set one waited for a line that never ended.
        let _ = line.push_terminator(ch);
        line.col = 0;
        let echo = if echo_on { render(ch) } else { Echo::None };
        return (LineStep::Line, echo);
    }

    // Ordinary byte: append (silently dropped if the line is full).
    let pushed = line.push(ch);
    if pushed {
        line.col = advance_col(line.col, ch, t);
    }
    let echo = if pushed { render(ch) } else { Echo::None };
    (LineStep::Pending, echo)
}

/// Whether `ch` is the control character at `c_cc[idx]`.
///
/// A slot holding 0 is **disabled** (`_POSIX_VDISABLE`), not "the NUL
/// character": Linux builds its special-character map from `c_cc` and then
/// clears bit 0, and `stty intr undef` writes exactly that 0. Comparing
/// without this rule made a NUL byte on the wire a `^C` for any terminal whose
/// interrupt character had been turned off.
fn is_cc(t: &Termios, idx: usize, ch: u8) -> bool {
    matches!(t.c_cc.get(idx), Some(&c) if c != 0 && c == ch)
}

/// The signal a received byte raises under `ISIG`, if any.
///
/// `VINTR` → `SIGINT` (2), `VQUIT` → `SIGQUIT` (3), `VSUSP` → `SIGTSTP` (20).
/// The only `ISIG` classifier in the tree: [`receive`] asks it for both modes.
fn isig_signal(ch: u8, t: &Termios) -> Option<u8> {
    if t.c_lflag & lflag::ISIG == 0 {
        return None;
    }
    if is_cc(t, cc::VINTR, ch) {
        Some(2)
    } else if is_cc(t, cc::VQUIT, ch) {
        Some(3)
    } else if is_cc(t, cc::VSUSP, ch) {
        Some(20)
    } else {
        None
    }
}

/// Input translation (`IGNCR`, `ICRNL`, `INLCR`), in Linux's order.
///
/// `None` means the byte is discarded (`IGNCR`). Applies in raw mode as well
/// as canonical — these are input flags, independent of `ICANON`; a program
/// that wants CR delivered untouched clears `ICRNL`, which `cfmakeraw` does.
/// (The `ICRNL` half used to live inside the canonical editor, so a raw-mode
/// terminal with `ICRNL` set received CR where Linux delivers NL.)
fn translate_input(ch: u8, t: &Termios) -> Option<u8> {
    if ch == b'\r' {
        if t.c_iflag & iflag::IGNCR != 0 {
            return None;
        }
        if t.c_iflag & iflag::ICRNL != 0 {
            return Some(b'\n');
        }
    } else if ch == b'\n' && t.c_iflag & iflag::INLCR != 0 {
        return Some(b'\r');
    }
    Some(ch)
}

/// Per-slot flags in an [`InputQueue`].
mod qflag {
    /// This slot ends a canonical line: a canonical read stops after it.
    pub const LINE_END: u8 = 1;
    /// This slot is an end-of-file mark, not a byte: it ends a line (always
    /// together with [`LINE_END`]) and is consumed without being delivered.
    pub const EOF: u8 = 2;
}

/// A terminal's input queue: bytes the line discipline has finished with, in
/// arrival order, waiting for a reader.
///
/// Linux's `read_buf` + `read_flags`, and for the same reasons. In canonical
/// mode it holds complete lines only (the line being edited is in `LineBuf`),
/// each ending in a slot flagged [`qflag::LINE_END`], so a reader can take one
/// line at a time — or part of one, leaving the rest for the next `read`. A
/// `^D` needs a line end with no byte in it, so it is stored as a placeholder
/// slot flagged [`qflag::EOF`] that ends the line and is never delivered.
///
/// In raw mode the flags are simply not consulted (a placeholder left over
/// from canonical mode is skipped, never delivered as a NUL — Linux does
/// deliver it, as `__DISABLED_CHAR`, which is a quirk and not a contract).
///
/// Counts of line ends and placeholders are maintained on every push and pop,
/// so the questions a poller asks are O(1).
///
/// The two arrays are heap-allocated rather than inline: 8 KiB built by value
/// would be assembled on the caller's stack and copied into the device's box —
/// on a 64 KiB task stack, in `SYS_PTY_CREATE`, twice over in a debug build.
/// `vec![0; N]` allocates zeroed memory directly.
struct InputQueue {
    buf: Vec<u8>,
    flags: Vec<u8>,
    /// Physical index of the oldest slot.
    head: usize,
    /// Slots in use.
    len: usize,
    /// Slots flagged `LINE_END` — complete canonical lines queued.
    lines: usize,
    /// Slots flagged `EOF` — placeholders, which are not data.
    eofs: usize,
}

impl InputQueue {
    fn new() -> Self {
        Self {
            buf: alloc::vec![0u8; INPUT_QUEUE_CAPACITY],
            flags: alloc::vec![0u8; INPUT_QUEUE_CAPACITY],
            head: 0,
            len: 0,
            lines: 0,
            eofs: 0,
        }
    }

    /// Physical index of logical slot `i` (0 = oldest). Callers keep `i` below
    /// `len`, which never exceeds the capacity.
    fn phys(&self, i: usize) -> usize {
        self.head.wrapping_add(i) % INPUT_QUEUE_CAPACITY
    }

    fn free(&self) -> usize {
        INPUT_QUEUE_CAPACITY.saturating_sub(self.len)
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Complete canonical lines queued (an end-of-file mark counts as one).
    fn lines(&self) -> usize {
        self.lines
    }

    /// Deliverable bytes queued: every slot except end-of-file marks.
    fn data_len(&self) -> usize {
        self.len.saturating_sub(self.eofs)
    }

    /// Deliverable bytes in complete lines — `FIONREAD` in canonical mode.
    ///
    /// O(len), and only `FIONREAD` asks: it scans back to the last line end
    /// and counts the non-placeholder slots before it.
    fn committed_data_len(&self) -> usize {
        let mut end = None;
        for i in (0..self.len).rev() {
            if self
                .flags
                .get(self.phys(i))
                .is_some_and(|f| f & qflag::LINE_END != 0)
            {
                end = Some(i);
                break;
            }
        }
        let Some(end) = end else {
            return 0;
        };
        (0..=end)
            .filter(|&i| {
                self.flags
                    .get(self.phys(i))
                    .is_some_and(|f| f & qflag::EOF == 0)
            })
            .count()
    }

    /// Append one slot. `false`, changing nothing, if the queue is full.
    fn push(&mut self, byte: u8, flags: u8) -> bool {
        if self.len >= INPUT_QUEUE_CAPACITY {
            return false;
        }
        let p = self.phys(self.len);
        if let (Some(b), Some(f)) = (self.buf.get_mut(p), self.flags.get_mut(p)) {
            *b = byte;
            *f = flags;
        } else {
            return false;
        }
        self.len = self.len.saturating_add(1);
        if flags & qflag::LINE_END != 0 {
            self.lines = self.lines.saturating_add(1);
        }
        if flags & qflag::EOF != 0 {
            self.eofs = self.eofs.saturating_add(1);
        }
        true
    }

    /// Remove the oldest slot.
    fn pop(&mut self) -> Option<(u8, u8)> {
        if self.len == 0 {
            return None;
        }
        let p = self.head;
        let byte = self.buf.get(p).copied().unwrap_or(0);
        let flags = self.flags.get(p).copied().unwrap_or(0);
        self.head = self.phys(1);
        self.len = self.len.saturating_sub(1);
        if flags & qflag::LINE_END != 0 {
            self.lines = self.lines.saturating_sub(1);
        }
        if flags & qflag::EOF != 0 {
            self.eofs = self.eofs.saturating_sub(1);
        }
        Some((byte, flags))
    }

    /// Flags of the oldest slot, if any.
    fn front_flags(&self) -> Option<u8> {
        if self.len == 0 {
            None
        } else {
            self.flags.get(self.head).copied()
        }
    }

    fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
        self.lines = 0;
        self.eofs = 0;
    }

    /// Queue a completed canonical line.
    ///
    /// `eof` says the line was ended by `VEOF` rather than by a terminator
    /// byte: it then gets a placeholder slot to carry the line end, which is
    /// what lets `^D` on an empty line be *seen* as a zero-length line.
    ///
    /// The caller has already checked there is room for `line.len() + 1`
    /// slots — the check has to come before the editor commits to anything —
    /// so a `false` here is a broken invariant rather than a full queue, and
    /// the line is not partially queued.
    fn push_line(&mut self, line: &[u8], eof: bool) -> bool {
        let needed = line.len().saturating_add(usize::from(eof));
        if needed == 0 || self.free() < needed {
            return false;
        }
        let last = line.len().saturating_sub(1);
        for (i, &b) in line.iter().enumerate() {
            let flag = if !eof && i == last {
                qflag::LINE_END
            } else {
                0
            };
            let _ = self.push(b, flag);
        }
        if eof {
            let _ = self.push(0, qflag::LINE_END | qflag::EOF);
        }
        true
    }

    /// Remove every end-of-file mark, keeping the data slots in order.
    ///
    /// On a switch to raw mode (see `TtyDevice::set_termios`). O(len), once per
    /// mode switch. A mark's line end goes with it — raw mode has no lines, and
    /// a switch back to canonical mode makes one line of whatever is left.
    fn purge_eof_marks(&mut self) {
        if self.eofs == 0 {
            return;
        }
        let n = self.len;
        let mut kept = 0usize;
        for i in 0..n {
            let src = self.phys(i);
            let (b, f) = (
                self.buf.get(src).copied().unwrap_or(0),
                self.flags.get(src).copied().unwrap_or(0),
            );
            if f & qflag::EOF != 0 {
                continue;
            }
            // `kept <= i`, so the slot written never overtakes one not yet read.
            let dst = self.phys(kept);
            if let (Some(db), Some(df)) = (self.buf.get_mut(dst), self.flags.get_mut(dst)) {
                *db = b;
                *df = f;
            }
            kept = kept.saturating_add(1);
        }
        self.len = kept;
        self.eofs = 0;
        self.lines = (0..kept)
            .filter(|&i| {
                self.flags
                    .get(self.phys(i))
                    .is_some_and(|f| f & qflag::LINE_END != 0)
            })
            .count();
    }

    /// Make whatever is queued after the last line end into a complete line
    /// (Linux's "push", on a switch from raw to canonical mode).
    fn mark_line_end(&mut self) {
        if self.len == 0 {
            return;
        }
        let p = self.phys(self.len.saturating_sub(1));
        if let Some(f) = self.flags.get_mut(p) {
            if *f & qflag::LINE_END == 0 {
                *f |= qflag::LINE_END;
                self.lines = self.lines.saturating_add(1);
            }
        }
    }

    /// Canonical read: deliver at most one line, or as much of it as fits.
    ///
    /// Stops *after* the line end, or when `out` is full, whichever is first;
    /// a partly-delivered line stays at the front with its line end still
    /// queued, so the next read continues it. An end-of-file mark is consumed
    /// and not copied — so a line of just `^D` reads as 0 bytes, which is end
    /// of file — and one that immediately follows the bytes that filled `out`
    /// is consumed too, since it belongs to the line just delivered and would
    /// otherwise be read next time as a spurious end of file.
    ///
    /// Only meaningful when [`Self::lines`] is non-zero.
    fn read_line(&mut self, out: &mut [u8]) -> usize {
        let mut n = 0usize;
        while let Some(flags) = self.front_flags() {
            if flags & qflag::EOF != 0 {
                let _ = self.pop();
                break;
            }
            if n >= out.len() {
                break;
            }
            let Some((b, f)) = self.pop() else { break };
            if let Some(slot) = out.get_mut(n) {
                *slot = b;
            }
            n = n.saturating_add(1);
            if f & qflag::LINE_END != 0 {
                break;
            }
        }
        n
    }

    /// Raw read: deliver up to `out.len()` bytes, ignoring line ends and
    /// skipping end-of-file marks.
    fn read_raw(&mut self, out: &mut [u8]) -> usize {
        let mut n = 0usize;
        while n < out.len() {
            let Some((b, f)) = self.pop() else { break };
            if f & qflag::EOF != 0 {
                continue;
            }
            if let Some(slot) = out.get_mut(n) {
                *slot = b;
            }
            n = n.saturating_add(1);
        }
        n
    }
}

/// What [`receive`] did with one byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Received {
    /// Consumed: queued, edited into the line, or dropped because the line is
    /// full (which is what a terminal does with typing past `MAX_CANON`).
    Consumed,
    /// Consumed as a signal character: this signal is due for the terminal's
    /// foreground process group, now. The caller delivers it.
    Signal(u8),
    /// **Not** consumed: the input queue has no room for what this byte would
    /// add. Offer it again once a reader has made space — this is the
    /// terminal's flow control, and what makes a paste into a busy program
    /// wait rather than lose text.
    NoRoom,
}

/// Run one received byte through the line discipline — at the moment it
/// arrives, which is the whole point (see "When input is processed" in the
/// module docs).
///
/// In order: input translation; then `ISIG`, before anything else, so that a
/// signal character never reaches the editor or the queue; then canonical
/// editing ([`feed`]) or, in raw mode, straight into the queue. Echo goes to
/// `echo`, already rendered — `^X` for a control character, CRLF for a newline
/// under `ONLCR` — or nowhere when `echo` is `None` (the console, whose echo
/// is the keyboard driver's).
///
/// A signal character flushes the unread input unless `NOFLSH` is set, and
/// with it the echo still waiting in `echo`: pending output for input that no
/// longer exists. Linux clears its echo buffer in `isig()` for the same
/// reason, then echoes the `^C` itself.
fn receive(dev: &mut TtyDevice, raw: u8, mut echo: Option<&mut Vec<u8>>) -> Received {
    let t = dev.termios;
    let Some(ch) = translate_input(raw, &t) else {
        return Received::Consumed;
    };

    if let Some(sig) = isig_signal(ch, &t) {
        if t.c_lflag & lflag::NOFLSH == 0 {
            dev.flush_input();
            if let Some(e) = echo.as_deref_mut() {
                e.clear();
            }
        }
        if let Some(e) = echo {
            render_echo_into(e, &t, render_echo(ch, &t));
        }
        return Received::Signal(sig);
    }

    if t.is_canonical() {
        // A terminator commits the line, so it needs room for all of it, and
        // the room has to be known before the editor changes anything: once
        // `feed` has accepted a `\n` there is no way to give it back.
        let terminates = ch == b'\n'
            || is_cc(&t, cc::VEOF, ch)
            || is_cc(&t, cc::VEOL, ch)
            || (t.c_lflag & lflag::IEXTEN != 0 && is_cc(&t, cc::VEOL2, ch));
        if terminates && dev.input.free() < dev.line.len.saturating_add(1) {
            return Received::NoRoom;
        }
        let (step, e) = feed(&mut dev.line, ch, &t);
        match step {
            LineStep::Pending => {}
            LineStep::Line | LineStep::Eof => {
                let _ = dev
                    .input
                    .push_line(dev.line.as_slice(), step == LineStep::Eof);
                dev.line.clear();
            }
        }
        if let Some(out) = echo {
            render_echo_into(out, &t, e);
        }
        Received::Consumed
    } else {
        if !dev.input.push(ch, 0) {
            return Received::NoRoom;
        }
        if let Some(out) = echo {
            render_echo_into(out, &t, render_echo(ch, &t));
        }
        Received::Consumed
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
// Rendering echo
// ---------------------------------------------------------------------------

/// The letter shown after `^` when a control byte is echoed under `ECHOCTL`.
///
/// Linux (`n_tty.c`) uses `c ^ 0x40`, not `c + 0x40`. The XOR is what makes
/// the mapping run in both directions: it turns 0x03 into `'C'` *and* `DEL`
/// (0x7f) into `'?'`, whereas addition would carry 0x7f past the ASCII range
/// and print garbage where every terminal shows `^?`.
const fn caret_letter(ch: u8) -> u8 {
    ch ^ 0x40
}

/// Render the echo the line discipline decided on, appending the bytes a
/// terminal emulator should draw to `out`.
///
/// Splitting "decide" ([`feed`] / [`render_echo`]) from "render" (here) from
/// "perform" (the pty master write, which queues `out` for the master to read)
/// is what lets the discipline stay pure and unit-testable while still driving
/// a device that has no driver to delegate echo to.
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
fn render_echo_into(out: &mut Vec<u8>, t: &Termios, echo: Echo) {
    let newline: &[u8] = if t.opost_nl_is_crlf() { b"\r\n" } else { b"\n" };
    match echo {
        Echo::None => {}
        // A literal `\n` byte takes the newline path so ONLCR applies to it
        // whether it arrived as a completed line or as raw-mode input.
        Echo::Newline | Echo::Byte(b'\n') => out.extend_from_slice(newline),
        Echo::Byte(c) => out.push(c),
        Echo::Ctrl(c) => out.extend_from_slice(&[b'^', caret_letter(c)]),
        Echo::Erase(n) => {
            for _ in 0..n {
                out.extend_from_slice(b"\x08 \x08");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Waiting for input — the only device-specific part of reading
// ---------------------------------------------------------------------------

/// How long a read is prepared to wait for more input to arrive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wait {
    /// Not at all: take what has already arrived.
    Poll,
    /// Until input arrives, a signal becomes deliverable, or the terminal
    /// hangs up.
    Block,
    /// As `Block`, but no later than this monotonic time, in nanoseconds.
    Until(u64),
}

/// Why [`wait_for`] came back without its test ever passing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    /// The wait ran out: a `Poll` found nothing, or `Until`'s deadline passed.
    Timeout,
    /// A signal is deliverable to the reader: the syscall must unwind so it can
    /// run, and restart afterwards.
    Interrupted,
    /// **Console only.** A signal character was received while this reader was
    /// pulling keystrokes off the keyboard ring, and this signal is due for the
    /// foreground group. A pty decides its signals in the *writer*, so a pty
    /// reader never sees this.
    Signal(u8),
    /// The pty hung up and the test declined to deliver anything. Every test in
    /// this file answers a hangup itself, so this is a backstop against a
    /// reader parking on a terminal nothing can ever write to again.
    Hangup,
    /// The terminal no longer exists.
    Gone,
}

/// What a read's test concluded, when it concluded anything.
///
/// `ModeChanged` exists because a read is dispatched on the mode in force when
/// it started, and another process can change the mode while it waits — a
/// shell switching to raw mode under a reader still waiting for a canonical
/// line. The reader must re-dispatch rather than wait for a condition the new
/// mode can never produce. [`set_termios`] wakes pty readers for exactly this.
enum Got<T> {
    Ready(T),
    ModeChanged,
}

/// Evaluate `test` against terminal `id` until it returns `Some`, waiting for
/// more input between attempts as `wait` allows.
///
/// `test` runs with the device locked and is told whether the terminal has hung
/// up (a pty whose last master is closed — nothing more will ever arrive). It
/// both decides and *acts*: when it returns `Some` it has already taken
/// whatever it is delivering out of the queue, under the same lock that showed
/// it was there, so two readers cannot deliver the same bytes.
fn wait_for<R>(
    id: TtyId,
    backend: Backend,
    wait: Wait,
    test: impl FnMut(&mut TtyDevice, bool) -> Option<R>,
) -> Result<R, Stop> {
    match backend {
        Backend::Console => console_wait_for(wait, test),
        Backend::Pty => pty_wait_for(id, wait, test),
    }
}

/// Whether a console keystroke pulled off the keyboard ring now could be
/// received without `NoRoom`.
///
/// A key cannot be put back on the ring, so this is asked *before* taking one.
/// In canonical mode a terminator needs room for the whole line it completes;
/// in raw mode one slot is enough.
fn console_can_receive(d: &TtyDevice) -> bool {
    if d.termios.is_canonical() {
        d.input.free() > d.line.len
    } else {
        d.input.free() > 0
    }
}

/// [`wait_for`] for the console, which receives keystrokes as it pulls them.
///
/// Every keystroke already typed is received, one at a time, before this
/// waits for anything — oldest first, re-running `test` after each, so a
/// canonical read stops pulling as soon as its line is complete and leaves the
/// rest on the ring for the next reader. Only when nothing more has been typed
/// does it wait, per `wait`, for the next keystroke.
fn console_wait_for<R>(
    wait: Wait,
    mut test: impl FnMut(&mut TtyDevice, bool) -> Option<R>,
) -> Result<R, Stop> {
    use crate::keyboard::ReadOutcome;
    let pid = crate::ipc::waiters::current_user_pid();
    loop {
        // Test, then — only if the test is not satisfied and a key can be
        // stored — receive one keystroke that has already been typed.
        let step = with_device(CONSOLE, |d| {
            if let Some(r) = test(d, false) {
                return Ok(Some(r));
            }
            if !console_can_receive(d) {
                return Err(Stop::Timeout);
            }
            match crate::keyboard::try_read_char() {
                Some(b) => match receive(d, b, None) {
                    Received::Signal(sig) => Err(Stop::Signal(sig)),
                    // NoRoom is excluded by `console_can_receive` above.
                    Received::Consumed | Received::NoRoom => Ok(None),
                },
                None => Err(Stop::Timeout),
            }
        });
        match step {
            // The console is materialised on demand, so `None` cannot happen.
            None => return Err(Stop::Gone),
            Some(Ok(Some(r))) => return Ok(r),
            // Received a typed-ahead key: test again before taking another.
            Some(Ok(None)) => continue,
            Some(Err(Stop::Timeout)) => {}
            Some(Err(stop)) => return Err(stop),
        }

        // Nothing typed ahead (or no room to store it, which the test's
        // conditions make unreachable: a full canonical queue holds a complete
        // line, and a full raw queue holds more than any VMIN asks for).
        let room = with_device(CONSOLE, |d| console_can_receive(d)).unwrap_or(false);
        if !room {
            return Err(Stop::Timeout);
        }
        let key = match wait {
            Wait::Poll => return Err(Stop::Timeout),
            Wait::Block => crate::keyboard::read_char_interruptible(pid),
            Wait::Until(deadline) => {
                crate::keyboard::read_char_timeout_interruptible(deadline, pid)
            }
        };
        match key {
            ReadOutcome::Byte(b) => {
                let sig = with_device(CONSOLE, |d| match receive(d, b, None) {
                    Received::Signal(sig) => Some(sig),
                    Received::Consumed | Received::NoRoom => None,
                })
                .flatten();
                if let Some(sig) = sig {
                    return Err(Stop::Signal(sig));
                }
            }
            ReadOutcome::Interrupted => return Err(Stop::Interrupted),
            ReadOutcome::TimedOut => return Err(Stop::Timeout),
        }
    }
}

/// Wake a task parked in [`pty_wait_for`] when its deadline fires.
fn pty_deadline_wake(tid: u64) {
    if !crate::sched::try_wake(tid) {
        crate::sched::defer_wake(tid);
    }
}

/// [`wait_for`] for a pty, whose input is received by the master's write.
///
/// The test and the decision to park happen under one hold of `DEVICES`, and
/// the waiter is registered in the pty's input set while that lock is still
/// held. A writer queues input under the same lock and collects the waiters
/// under it too, so its wake cannot slip between this reader's test and its
/// registration — the lost-wakeup window the `waiters` idiom exists to close.
/// (Lock order `DEVICES` → `PTYS`, as documented at the top of this file.)
fn pty_wait_for<R>(
    id: TtyId,
    wait: Wait,
    mut test: impl FnMut(&mut TtyDevice, bool) -> Option<R>,
) -> Result<R, Stop> {
    use crate::ipc::waiters::{
        current_user_pid, deliverable_signal_pending, park_interruptible, wake_all,
    };
    let pid = current_user_pid();
    let task = crate::sched::current_task_id();

    let timer = match wait {
        Wait::Until(deadline) => {
            let now = crate::hrtimer::now_ns();
            (now < deadline).then(|| {
                crate::hrtimer::schedule_ns(deadline.saturating_sub(now), pty_deadline_wake, task)
            })
        }
        Wait::Poll | Wait::Block => None,
    };

    let result = loop {
        {
            let mut table = DEVICES.lock();
            let Some(dev) = table.get_mut(&id) else {
                break Err(Stop::Gone);
            };
            // Deregister at the top of every iteration: a wake does not clear
            // the entry, and a stale one names a task that is no longer parked.
            pty::remove_input_waiter(id, task);
            let hung = pty::master_gone(id);
            if let Some(r) = test(dev, hung) {
                // The test took input, which frees room: wake any master parked
                // on a full queue (and any other reader, which re-checks).
                let woken = pty::take_input_waiters(id);
                drop(table);
                wake_all(woken);
                break Ok(r);
            }
            if hung {
                break Err(Stop::Hangup);
            }
            match wait {
                Wait::Poll => break Err(Stop::Timeout),
                Wait::Until(deadline) if crate::hrtimer::now_ns() >= deadline => {
                    break Err(Stop::Timeout);
                }
                Wait::Until(_) | Wait::Block => {}
            }
            if deliverable_signal_pending(pid) {
                break Err(Stop::Interrupted);
            }
            pty::insert_input_waiter(id, task);
        }
        park_interruptible(pid, task);
    };

    if let Some(t) = timer {
        crate::hrtimer::cancel(t);
    }
    // No stale entry on any exit path, including the ones that broke out
    // after a registration from an earlier iteration.
    pty::remove_input_waiter(id, task);
    result
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// Outcome of a terminal [`read`].
///
/// A normal read yields [`ConsoleRead::Data`] with the number of bytes written
/// to the caller's buffer (`0` means end-of-file on a `^D` at an empty line or
/// a hangup, or nothing available in a polling raw read).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleRead {
    /// `n` bytes were written to the caller's buffer (`0` ⇒ EOF / no data).
    Data(usize),
    /// **Console only.** A terminal signal (`SIGINT`/`SIGQUIT`/`SIGTSTP`) was
    /// generated while this read was receiving keystrokes; deliver it to the
    /// foreground process group. No bytes were written to the caller's buffer.
    /// A pty never returns this: its signals are decided and delivered by the
    /// master's write (see "When input is processed" in the module docs).
    Signal(u8),
    /// A signal already pending for the *reader* cut the wait short.  No bytes
    /// were written and nothing needs delivering — the syscall layer just
    /// returns the restart sentinel so the signal checkpoint runs.  Anything
    /// typed so far is still queued or in the line editor, and will be there
    /// when the read restarts.
    Interrupted,
    /// No data is immediately available and the caller requested a non-blocking
    /// read ([`try_read`]).  The POSIX layer maps this to `EAGAIN`.
    WouldBlock,
}

/// Read from terminal `id` into `out`, blocking as the line discipline says.
///
/// In canonical mode this blocks until a complete line is queued, then returns
/// up to `out.len()` bytes of it; the rest of a long line stays queued for the
/// next call. A `^D` on an empty line returns `0` (end of file). In
/// non-canonical (raw) mode it honours `VMIN` and `VTIME` per POSIX — a pure
/// poll, a read timeout, a byte count, or an inter-byte timer — see
/// [`raw_read`].
///
/// For the console, echo is performed by the keyboard driver, which this
/// function first syncs to the termios `ECHO` bit so that raw/no-echo programs
/// (password prompts, full-screen editors) suppress echo correctly. A pty's
/// echo was done when its input arrived.
pub fn read(id: TtyId, out: &mut [u8]) -> ConsoleRead {
    read_common(id, out, false)
}

/// Non-blocking terminal read — returns [`ConsoleRead::WouldBlock`] instead
/// of waiting when nothing is ready.
///
/// This is the `O_NONBLOCK` counterpart of [`read`]:
///
/// * **Canonical mode:** a complete queued line is delivered; otherwise
///   `WouldBlock`. What has been typed of an unfinished line stays in the
///   editor.
/// * **Raw mode:** whatever is queued right now (up to `out.len()`), regardless
///   of `VMIN`/`VTIME`; `WouldBlock` if nothing is.
pub fn try_read(id: TtyId, out: &mut [u8]) -> ConsoleRead {
    read_common(id, out, true)
}

/// Body shared by [`read`] and [`try_read`]: dispatch on the mode, and again if
/// the mode changes underneath a waiting read.
fn read_common(id: TtyId, out: &mut [u8], nonblocking: bool) -> ConsoleRead {
    if out.is_empty() {
        return ConsoleRead::Data(0);
    }
    loop {
        let Some((t, backend)) = with_device(id, |d| (d.termios, d.backend)) else {
            // The pty was destroyed. A read on a terminal that no longer exists
            // is end of file, not an error: the reader's own handle is still
            // valid, and EOF is what every caller of a vanished terminal must
            // do anyway.
            return ConsoleRead::Data(0);
        };

        // The Linux read path is authoritative for console echo: keep the
        // keyboard driver's echo in sync with this terminal's ECHO bit.
        if backend == Backend::Console {
            crate::keyboard::set_echo(t.echo_enabled());
        }

        let got = if t.is_canonical() {
            canonical_read(
                id,
                backend,
                out,
                if nonblocking { Wait::Poll } else { Wait::Block },
            )
        } else if nonblocking {
            raw_try_read(id, backend, out)
        } else {
            raw_read(id, backend, &t, out)
        };
        if let Got::Ready(r) = got {
            return r;
        }
    }
}

/// Map a [`wait_for`] failure onto a read's result.
///
/// `Timeout` is the one caller-specific case: a non-blocking read reports it as
/// `WouldBlock`, a raw read with a deadline as a zero-byte read.
fn stop_to_read(stop: Stop, on_timeout: ConsoleRead) -> ConsoleRead {
    match stop {
        Stop::Timeout => on_timeout,
        Stop::Interrupted => ConsoleRead::Interrupted,
        Stop::Signal(sig) => ConsoleRead::Signal(sig),
        Stop::Hangup | Stop::Gone => ConsoleRead::Data(0),
    }
}

/// Canonical-mode read: deliver one queued line, or as much of it as fits.
///
/// A line is delivered whole or in pieces, never mixed with the next: the
/// queue marks where each line ends. On a hangup — the master is gone and the
/// line in the editor can never be finished — what was typed of it is
/// delivered as it stands (Linux does the same), and then end of file.
fn canonical_read(id: TtyId, backend: Backend, out: &mut [u8], wait: Wait) -> Got<ConsoleRead> {
    let r = wait_for(id, backend, wait, |d, hung| {
        if !d.termios.is_canonical() {
            return Some(Got::ModeChanged);
        }
        if d.input.lines() > 0 {
            return Some(Got::Ready(d.input.read_line(out)));
        }
        if hung {
            return Some(Got::Ready(d.line.take_front(out)));
        }
        None
    });
    match r {
        Ok(Got::Ready(n)) => Got::Ready(ConsoleRead::Data(n)),
        Ok(Got::ModeChanged) => Got::ModeChanged,
        Err(stop) => Got::Ready(stop_to_read(stop, ConsoleRead::WouldBlock)),
    }
}

/// Take up to `out.len()` queued raw bytes, first receiving whatever has
/// already been typed (the console) — the delivery step every raw read ends in.
///
/// Greedy on purpose: POSIX has a satisfied non-canonical read return
/// everything available up to the request, not just the `VMIN` it waited for.
fn raw_take(id: TtyId, backend: Backend, out: &mut [u8]) -> Result<Got<usize>, Stop> {
    let cap = out.len();
    // Pass 1 receives typed-ahead keystrokes until `out` could be filled; it
    // "times out" as soon as nothing more has been typed, which is the normal
    // exit. Pass 2 then takes what is there.
    // A hangup ends pass 1 as well: nothing more will arrive, and a pty
    // `wait_for` answers an unsatisfied test on a hung-up terminal with
    // `Stop::Hangup` — which would discard the bytes that ARE queued.
    match wait_for(id, backend, Wait::Poll, |d, hung| {
        if d.termios.is_canonical() {
            Some(Got::ModeChanged)
        } else if d.input.data_len() >= cap || hung {
            Some(Got::Ready(d.input.read_raw(out)))
        } else {
            None
        }
    }) {
        Ok(got) => return Ok(got),
        Err(Stop::Timeout) => {}
        Err(stop) => return Err(stop),
    }
    wait_for(id, backend, Wait::Poll, |d, _| {
        if d.termios.is_canonical() {
            Some(Got::ModeChanged)
        } else {
            Some(Got::Ready(d.input.read_raw(out)))
        }
    })
}

/// `O_NONBLOCK` raw read: whatever is queued, or `WouldBlock`.
fn raw_try_read(id: TtyId, backend: Backend, out: &mut [u8]) -> Got<ConsoleRead> {
    match raw_take(id, backend, out) {
        Ok(Got::Ready(0)) => {
            // Nothing queued. A hung-up pty is end of file rather than "try
            // again": nothing will ever arrive, and a caller told EAGAIN would
            // poll forever.
            let hung = backend == Backend::Pty && pty::master_gone(id);
            Got::Ready(if hung {
                ConsoleRead::Data(0)
            } else {
                ConsoleRead::WouldBlock
            })
        }
        Ok(Got::Ready(n)) => Got::Ready(ConsoleRead::Data(n)),
        Ok(Got::ModeChanged) => Got::ModeChanged,
        Err(stop) => Got::Ready(stop_to_read(stop, ConsoleRead::WouldBlock)),
    }
}

/// One tenth of a second — the unit of `VTIME`.
const DECISECOND_NS: u64 = 100_000_000;

/// Non-canonical (raw) read honouring both `VMIN` and `VTIME` (see [`read`]).
///
/// The four `(VMIN, VTIME)` combinations follow POSIX (`termios(3)` "Canonical
/// and noncanonical mode"):
///
/// * **`MIN==0, TIME==0`** — pure poll: return whatever is available (possibly
///   `0`), never waiting.
/// * **`MIN==0, TIME>0`** — read timeout: wait up to `TIME` deciseconds for the
///   first byte; return what has arrived, or `0` on timeout.
/// * **`MIN>0, TIME==0`** — count: wait until `MIN` bytes are queued (or as
///   many as `out` holds, if fewer).
/// * **`MIN>0, TIME>0`** — inter-byte timer: wait indefinitely for the first
///   byte, then restart a `TIME`-decisecond timer each time more arrive; return
///   when `MIN` bytes are queued or the timer expires.
///
/// Every case ends in [`raw_take`], so a satisfied read returns everything
/// available up to `out.len()`, not merely `MIN`.
///
/// Signal characters never reach here: they are handled on arrival, so the
/// bytes a signal flushed are simply not in the queue. Waiting never consumes
/// anything, so a read interrupted by a signal leaves every queued byte for
/// the restarted read — except in the inter-byte phase, where bytes have
/// provably arrived and POSIX has the read return them rather than fail.
fn raw_read(id: TtyId, backend: Backend, t: &Termios, out: &mut [u8]) -> Got<ConsoleRead> {
    let cap = out.len();
    let vmin = usize::from(t.vmin());
    let vtime_ns = u64::from(t.vtime()).saturating_mul(DECISECOND_NS);
    // How many bytes satisfy a counting read: MIN, or all of `out` if smaller.
    let want = vmin.min(cap);

    // The shape every wait below shares: re-dispatch on a mode change, succeed
    // when `ready(queued)` holds or the terminal hung up (nothing more will
    // arrive, so what is there is the answer).
    let waited = |wait: Wait, ready: &dyn Fn(usize) -> bool| -> Result<Got<usize>, Stop> {
        wait_for(id, backend, wait, |d, hung| {
            if d.termios.is_canonical() {
                return Some(Got::ModeChanged);
            }
            let queued = d.input.data_len();
            (ready(queued) || hung).then_some(Got::Ready(queued))
        })
    };

    let result = match (vmin == 0, vtime_ns == 0) {
        // MIN=0, TIME=0: pure poll.
        (true, true) => Ok(Got::Ready(0)),
        // MIN=0, TIME>0: wait up to TIME for the first byte. A timeout is not
        // an error here — MIN=0 makes a zero-byte read the defined answer.
        (true, false) => {
            let deadline = crate::hrtimer::now_ns().saturating_add(vtime_ns);
            match waited(Wait::Until(deadline), &|q| q > 0) {
                Err(Stop::Timeout) => Ok(Got::Ready(0)),
                other => other,
            }
        }
        // MIN>0, TIME=0: wait for MIN bytes.
        (false, true) => waited(Wait::Block, &|q| q >= want),
        // MIN>0, TIME>0: the first byte without a limit, then the inter-byte
        // timer, restarted whenever the queue has grown since it was armed.
        (false, false) => match waited(Wait::Block, &|q| q > 0) {
            Ok(Got::Ready(mut seen)) => loop {
                if seen >= want {
                    break Ok(Got::Ready(seen));
                }
                let deadline = crate::hrtimer::now_ns().saturating_add(vtime_ns);
                let grown = seen;
                match waited(Wait::Until(deadline), &move |q| q > grown) {
                    Ok(Got::Ready(q)) => seen = q,
                    // The timer ran out, or a signal arrived, with bytes
                    // already here: deliver them.
                    Err(Stop::Timeout | Stop::Interrupted) => break Ok(Got::Ready(seen)),
                    other => break other,
                }
            },
            other => other,
        },
    };

    match result {
        Ok(Got::Ready(_)) => match raw_take(id, backend, out) {
            Ok(Got::Ready(n)) => Got::Ready(ConsoleRead::Data(n)),
            Ok(Got::ModeChanged) => Got::ModeChanged,
            Err(stop) => Got::Ready(stop_to_read(stop, ConsoleRead::Data(0))),
        },
        Ok(Got::ModeChanged) => Got::ModeChanged,
        Err(stop) => Got::Ready(stop_to_read(stop, ConsoleRead::Data(0))),
    }
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
/// Invoked from `main` during kernel bring-up.  Verifies the wire-format sizes,
/// the canonical/echo defaults, the Linux `INIT_C_CC` control characters, byte
/// round-tripping (including raw-mode flag clearing), that `TIOCGWINSZ` reports
/// a live non-zero console size, and the line discipline: the editor
/// (erase/kill/EOF/EOL, disabled control characters, the full-line reserve),
/// echo rendering, [`receive`] (the three ISIG signals decided on arrival and
/// the flush each performs, `NOFLSH`, `ISIG` off, raw-mode `ISIG`, input
/// translation, flow control in both modes, the mode switch), the input queue
/// (partial and whole-line delivery, end-of-file marks, wraparound), and
/// foreground-pgrp ownership.
///
/// This used to be described as mirroring a `#[cfg(test)] mod tests` that
/// followed it.  Those six tests were removed on 2026-08-22: the kernel binary
/// sets `test = false` and has no lib target, so they had never compiled or run
/// (`known-issues.md` → `A-KERNEL-UNIT-TESTS-NEVER-RUN`), and every property
/// each one asserted is in fact checked here — verified case by case rather
/// than taken from the comment's word.
pub fn self_test() -> crate::error::KernelResult<()> {
    use crate::selftest;
    crate::serial_println!("[tty] Running self-test...");

    // Wire-format sizes must match the Linux kernel structs exactly.
    selftest::check_eq!(TERMIOS_BYTES, 36, "termios wire size");
    selftest::check_eq!(WINSIZE_BYTES, 8, "winsize wire size");

    // Defaults: canonical line mode with echo, VMIN=1/VTIME=0.
    let t = Termios::sane_default();
    selftest::check!(t.is_canonical(), "default should be canonical");
    selftest::check!(t.echo_enabled(), "default should echo");
    selftest::check_eq!(t.vmin(), 1, "default VMIN");
    selftest::check_eq!(t.vtime(), 0, "default VTIME");

    // Control characters mirror Linux INIT_C_CC.
    selftest::check_eq!(t.c_cc.get(cc::VINTR).copied(), Some(3), "VINTR=^C");
    selftest::check_eq!(t.c_cc.get(cc::VEOF).copied(), Some(4), "VEOF=^D");
    selftest::check_eq!(t.c_cc.get(cc::VERASE).copied(), Some(127), "VERASE=DEL");
    selftest::check_eq!(t.c_cc.get(cc::VKILL).copied(), Some(21), "VKILL=^U");

    // termios round-trips losslessly through the 36-byte wire format.
    let back = Termios::from_bytes(&t.to_bytes());
    selftest::check_eq!(t, back, "termios round-trip");
    crate::serial_println!("[tty]   termios round-trip + defaults: OK");

    // Raw mode: clearing ICANON|ECHO survives serialisation.
    let mut raw = Termios::sane_default();
    raw.c_lflag &= !(lflag::ICANON | lflag::ECHO);
    let raw_back = Termios::from_bytes(&raw.to_bytes());
    selftest::check!(!raw_back.is_canonical(), "raw clears ICANON");
    selftest::check!(!raw_back.echo_enabled(), "raw clears ECHO");
    crate::serial_println!("[tty]   raw-mode flag clearing: OK");

    // winsize round-trips, and TIOCGWINSZ reports a live non-zero size.
    let w = WinSize {
        ws_row: 24,
        ws_col: 80,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    selftest::check_eq!(WinSize::from_bytes(&w.to_bytes()), w, "winsize round-trip");
    let live = get_winsize(CONSOLE);
    selftest::check!(
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
        selftest::check_eq!(step(&mut line, b'h', &t), LineStep::Pending);
        selftest::check_eq!(step(&mut line, b'i', &t), LineStep::Pending);
        selftest::check_eq!(step(&mut line, b'\n', &t), LineStep::Line);
        selftest::check_eq!(line.as_slice(), b"hi\n", "canonical line content");

        // VERASE (DEL) erases the last byte: "ax\x7fb\n" → "ab\n".
        let mut e = LineBuf::new();
        let _ = step(&mut e, b'a', &t);
        let _ = step(&mut e, b'x', &t);
        selftest::check_eq!(step(&mut e, 127, &t), LineStep::Pending); // erase 'x'
        let _ = step(&mut e, b'b', &t);
        selftest::check_eq!(step(&mut e, b'\n', &t), LineStep::Line);
        selftest::check_eq!(e.as_slice(), b"ab\n", "VERASE erases prior byte");

        // VKILL (^U) clears the whole line.
        let mut k = LineBuf::new();
        let _ = step(&mut k, b'j', &t);
        let _ = step(&mut k, b'u', &t);
        selftest::check_eq!(step(&mut k, 21, &t), LineStep::Pending); // ^U
        selftest::check_eq!(k.as_slice(), b"", "VKILL clears the line");

        // VEOF (^D) on an empty line signals end-of-file.
        let mut eof = LineBuf::new();
        selftest::check_eq!(step(&mut eof, 4, &t), LineStep::Eof);
        selftest::check_eq!(eof.len, 0, "VEOF on empty line ⇒ EOF");

        // The editor knows nothing of signals: `receive` classifies them before
        // a byte gets here, in both modes. A ^C reaching the editor is a byte
        // that ISIG already declined — so it is stored, like any other. (The
        // signal cases themselves are in the `receive` block below.)
        let mut n = LineBuf::new();
        selftest::check_eq!(step(&mut n, 3, &t), LineStep::Pending);
        selftest::check_eq!(step(&mut n, b'\n', &t), LineStep::Line);
        selftest::check_eq!(
            n.as_slice(),
            &[3u8, b'\n'],
            "the editor stores ^C as a byte"
        );

        // VEOL is a second line terminator, kept in the line like `\n`. Until
        // 2026-09-24 it was not recognised at all, and a program that set one
        // waited for a line that never ended.
        let mut eol = Termios::sane_default();
        if let Some(c) = eol.c_cc.get_mut(cc::VEOL) {
            *c = b';';
        }
        let mut el = LineBuf::new();
        let _ = step(&mut el, b'a', &eol);
        selftest::check_eq!(
            step(&mut el, b';', &eol),
            LineStep::Line,
            "VEOL ends the line"
        );
        selftest::check_eq!(el.as_slice(), b"a;", "and stays in it");

        // A control character set to 0 is DISABLED, not "NUL": a NUL byte is
        // then an ordinary byte, not an end of file or an erase.
        let mut dis = Termios::sane_default();
        if let Some(c) = dis.c_cc.get_mut(cc::VEOF) {
            *c = 0;
        }
        let mut dl = LineBuf::new();
        selftest::check_eq!(step(&mut dl, 0, &dis), LineStep::Pending, "disabled VEOF");
        selftest::check_eq!(dl.as_slice(), &[0u8], "NUL is stored as data");
        // The default VEOL *is* 0, which is exactly why this matters: without
        // the rule every NUL byte would end a line.
        selftest::check_eq!(
            step(&mut dl, 0, &t),
            LineStep::Pending,
            "default VEOL=0 is off"
        );

        // A full line keeps its last slot for the terminator.
        let mut full = LineBuf::new();
        for _ in 0..MAX_CANON {
            let _ = step(&mut full, b'f', &t);
        }
        selftest::check_eq!(full.len, MAX_CANON - 1, "ordinary input stops one short");
        selftest::check_eq!(
            step(&mut full, b'\n', &t),
            LineStep::Line,
            "the terminator fits"
        );
        selftest::check_eq!(full.as_slice().last().copied(), Some(b'\n'), "and is there");

        crate::serial_println!(
            "[tty]   line editor (canon/erase/kill/eof/eol/vdisable/full line): OK"
        );
    }

    // `receive` — the line discipline, run as each byte ARRIVES. Driven on a
    // scratch device, so these check the decisions, and the pty self-test checks
    // the same decisions through a real master write.
    {
        let t = Termios::sane_default();
        let mut raw_t = t;
        raw_t.c_lflag &= !(lflag::ICANON | lflag::ECHO);
        let mut d = Box::new(TtyDevice::new(Backend::Pty));
        let mut echo = Vec::new();

        // ^C / ^\ / ^Z are signals, decided on arrival, and a signal flushes
        // everything typed ahead — the line being edited and complete lines
        // already queued — plus the echo still pending for it.
        for &b in b"zz" {
            selftest::check_eq!(receive(&mut d, b, Some(&mut echo)), Received::Consumed);
        }
        selftest::check_eq!(
            receive(&mut d, 3, Some(&mut echo)),
            Received::Signal(2),
            "VINTR"
        );
        selftest::check_eq!(d.line.as_slice(), b"", "VINTR flushes the line");
        selftest::check_eq!(
            echo.as_slice(),
            b"^C",
            "pending echo flushed, then ^C echoed"
        );
        selftest::check_eq!(
            receive(&mut d, 28, None),
            Received::Signal(3),
            "VQUIT is SIGQUIT"
        );
        selftest::check_eq!(
            receive(&mut d, 26, None),
            Received::Signal(20),
            "VSUSP is SIGTSTP"
        );
        for &b in b"ab\n" {
            let _ = receive(&mut d, b, None);
        }
        selftest::check_eq!(d.input.lines(), 1, "a completed line is queued");
        selftest::check_eq!(receive(&mut d, 3, None), Received::Signal(2));
        selftest::check_eq!(
            d.input.lines(),
            0,
            "^C flushes queued lines, not only the one typed"
        );

        // NOFLSH: the signal, without the flush.
        d.flush_input();
        let mut nf = t;
        nf.c_lflag |= lflag::NOFLSH;
        d.set_termios(nf);
        for &b in b"ab" {
            let _ = receive(&mut d, b, None);
        }
        selftest::check_eq!(
            receive(&mut d, 3, None),
            Received::Signal(2),
            "NOFLSH still signals"
        );
        selftest::check_eq!(d.line.as_slice(), b"ab", "NOFLSH preserves the line");
        let _ = receive(&mut d, b'\n', None);
        selftest::check_eq!(d.input.lines(), 1, "and the line completes afterwards");

        // ISIG off: ^C is data.
        d.flush_input();
        let mut noisig = t;
        noisig.c_lflag &= !lflag::ISIG;
        d.set_termios(noisig);
        selftest::check_eq!(receive(&mut d, 3, None), Received::Consumed, "ISIG off");
        let _ = receive(&mut d, b'\n', None);
        let mut out = [0u8; 8];
        selftest::check_eq!(d.input.read_line(&mut out), 2);
        selftest::check_eq!(
            out.get(..2),
            Some(&[3u8, b'\n'][..]),
            "ISIG off: ^C is literal"
        );

        // ISIG applies in raw mode too, flushing raw input.
        d.set_termios(raw_t);
        selftest::check_eq!(receive(&mut d, b'x', None), Received::Consumed);
        selftest::check_eq!(
            receive(&mut d, 3, None),
            Received::Signal(2),
            "raw keeps ISIG"
        );
        selftest::check_eq!(d.input.data_len(), 0, "and the signal flushed raw input");

        // A disabled VINTR (0) matches nothing — a NUL byte included.
        let mut dis = raw_t;
        if let Some(c) = dis.c_cc.get_mut(cc::VINTR) {
            *c = 0;
        }
        d.set_termios(dis);
        selftest::check_eq!(
            receive(&mut d, 0, None),
            Received::Consumed,
            "disabled VINTR"
        );
        selftest::check_eq!(d.input.data_len(), 1, "the NUL is data");

        // Input translation applies in raw mode as well as canonical.
        d.flush_input();
        d.set_termios(raw_t);
        let mut o = [0u8; 4];
        let _ = receive(&mut d, b'\r', None);
        selftest::check_eq!(d.input.read_raw(&mut o), 1);
        selftest::check_eq!(
            o.first().copied(),
            Some(b'\n'),
            "ICRNL maps CR to NL in raw mode"
        );
        let mut igncr = raw_t;
        igncr.c_iflag |= iflag::IGNCR;
        d.set_termios(igncr);
        let _ = receive(&mut d, b'\r', None);
        selftest::check_eq!(d.input.data_len(), 0, "IGNCR discards CR");
        let mut inlcr = raw_t;
        inlcr.c_iflag = iflag::INLCR;
        d.set_termios(inlcr);
        let _ = receive(&mut d, b'\n', None);
        selftest::check_eq!(d.input.read_raw(&mut o), 1);
        selftest::check_eq!(o.first().copied(), Some(b'\r'), "INLCR maps NL to CR");

        // Flow control, raw: a full queue refuses the byte and changes nothing.
        d.flush_input();
        d.set_termios(raw_t);
        for _ in 0..INPUT_QUEUE_CAPACITY {
            let _ = receive(&mut d, b'a', None);
        }
        selftest::check_eq!(
            receive(&mut d, b'b', None),
            Received::NoRoom,
            "full raw queue"
        );
        selftest::check_eq!(d.input.data_len(), INPUT_QUEUE_CAPACITY, "nothing dropped");
        selftest::check_eq!(
            receive(&mut d, 3, None),
            Received::Signal(2),
            "^C needs no room"
        );

        // Flow control, canonical: a terminator needs room for its whole line,
        // and without it the editor keeps the line for when there is.
        d.flush_input();
        d.set_termios(t);
        let line100 = [b'x'; 100];
        while d.input.free() > 101 {
            for &b in &line100 {
                let _ = receive(&mut d, b, None);
            }
            let _ = receive(&mut d, b'\n', None);
        }
        let free = d.input.free();
        for _ in 0..=free {
            let _ = receive(&mut d, b'y', None);
        }
        selftest::check_eq!(
            receive(&mut d, b'\n', None),
            Received::NoRoom,
            "line does not fit"
        );
        selftest::check_eq!(d.line.len, free.saturating_add(1), "the editor kept it");
        let mut drain = [0u8; 128];
        let _ = d.input.read_line(&mut drain);
        selftest::check_eq!(
            receive(&mut d, b'\n', None),
            Received::Consumed,
            "room made"
        );

        // Switching to raw mode removes end-of-file marks, which are not data.
        d.flush_input();
        d.set_termios(t);
        for &b in b"ab\x04" {
            let _ = receive(&mut d, b, None);
        }
        selftest::check_eq!(d.input.lines(), 1, "'ab^D' is one line");
        d.set_termios(raw_t);
        selftest::check_eq!(d.input.len, 2, "raw mode dropped the end-of-file mark");
        selftest::check_eq!(d.input.lines(), 0, "and the line end it carried");

        crate::serial_println!(
            "[tty]   receive: signals on arrival, flush, NOFLSH, raw ISIG, VDISABLE, input \
             translation, flow control, mode switch: OK"
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
        selftest::check_eq!(feed(&mut l, b'a', &t).1, Echo::Byte(b'a'), "printable echo");
        selftest::check_eq!(feed(&mut l, b'\n', &t).1, Echo::Newline, "newline echo");

        // ECHOCTL renders a control byte as `^X`, and the caret letter comes
        // from `caret_letter` — the XOR mapping, so DEL shows as `^?` rather
        // than as the out-of-range byte an addition would produce.
        let mut c = LineBuf::new();
        selftest::check_eq!(feed(&mut c, 1, &t).1, Echo::Ctrl(1), "^A renders as Ctrl");
        selftest::check_eq!(caret_letter(1), b'A', "caret letter for ^A");
        selftest::check_eq!(caret_letter(3), b'C', "caret letter for ^C");
        selftest::check_eq!(caret_letter(127), b'?', "caret letter for DEL is '?'");

        // A tab is exempt from ECHOCTL: it must be echoed literally or it
        // would never reach the next tab stop.
        let mut tab = LineBuf::new();
        selftest::check_eq!(feed(&mut tab, b'\t', &t).1, Echo::Byte(b'\t'), "tab echo");

        // ECHOE rubs out the erased character, two columns for a `^X`.
        let mut e = LineBuf::new();
        let _ = step(&mut e, b'a', &t);
        selftest::check_eq!(
            feed(&mut e, 127, &t).1,
            Echo::Erase(1),
            "erase a plain byte"
        );
        // ^A is only *stored* (rather than generating a signal) with ISIG
        // cleared, which is the configuration that lets us erase it.
        let mut ctrl = Termios::sane_default();
        ctrl.c_lflag &= !lflag::ISIG;
        let mut e2 = LineBuf::new();
        let _ = step(&mut e2, 1, &ctrl);
        selftest::check_eq!(
            feed(&mut e2, 127, &ctrl).1,
            Echo::Erase(2),
            "erasing a ^X-echoed byte rubs out two columns"
        );

        // Tab erase: a tab at column 0 advances to column 8, so erasing
        // it should rub out 8 columns, not 1 as the old code did.
        let mut et = LineBuf::new();
        let _ = step(&mut et, b'\t', &t);
        selftest::check_eq!(et.col, 8, "tab at col 0 advances to col 8");
        selftest::check_eq!(
            feed(&mut et, 127, &t).1,
            Echo::Erase(8),
            "erasing a tab at col 0 rubs out 8 columns"
        );

        // Tab after 3 characters: col 3 → col 8, so the tab is 5 wide.
        let mut et2 = LineBuf::new();
        let _ = step(&mut et2, b'a', &t); // col 1
        let _ = step(&mut et2, b'b', &t); // col 2
        let _ = step(&mut et2, b'c', &t); // col 3
        selftest::check_eq!(et2.col, 3, "three chars at col 3");
        let _ = step(&mut et2, b'\t', &t); // col 8
        selftest::check_eq!(et2.col, 8, "tab from col 3 advances to col 8");
        selftest::check_eq!(
            feed(&mut et2, 127, &t).1,
            Echo::Erase(5),
            "erasing a tab from col 3 rubs out 5 columns"
        );

        // ECHOKE (line kill) with a tab: "ab\t" at col 8 → erase 8.
        let mut ek = LineBuf::new();
        let _ = step(&mut ek, b'a', &t); // col 1
        let _ = step(&mut ek, b'b', &t); // col 2
        let _ = step(&mut ek, b'\t', &t); // col 8
        selftest::check_eq!(ek.col, 8, "line col before kill");
        selftest::check_eq!(
            feed(&mut ek, 21, &t).1, // ^U = VKILL
            Echo::Erase(8),
            "ECHOKE of a line with a tab rubs out 8 columns total"
        );

        // Clearing ECHO silences everything the editor would have drawn.
        let mut off = Termios::sane_default();
        off.c_lflag &= !lflag::ECHO;
        let mut q = LineBuf::new();
        selftest::check_eq!(feed(&mut q, b'a', &off).1, Echo::None, "ECHO off ⇒ silent");

        crate::serial_println!("[tty]   echo rendering (printable/^X/tab/erase/tab-erase/off): OK");
    }

    // The input queue: lines delivered whole or in pieces but never merged,
    // end-of-file marks, and the ring's wraparound.
    {
        let mut q = InputQueue::new();
        selftest::check!(
            q.push_line(b"abcdef\n", false),
            "a line fits an empty queue"
        );
        selftest::check_eq!(q.lines(), 1);
        let mut small = [0u8; 3];
        selftest::check_eq!(q.read_line(&mut small), 3);
        selftest::check_eq!(&small, b"abc");
        selftest::check_eq!(q.lines(), 1, "a partly-read line is still a line");
        let mut rest = [0u8; 16];
        selftest::check_eq!(q.read_line(&mut rest), 4);
        selftest::check_eq!(rest.get(..4), Some(&b"def\n"[..]));
        selftest::check_eq!(q.lines(), 0, "fully read");

        let _ = q.push_line(b"one\n", false);
        let _ = q.push_line(b"two\n", false);
        selftest::check_eq!(q.read_line(&mut rest), 4, "one line per read, never two");
        selftest::check_eq!(q.committed_data_len(), 4, "FIONREAD counts what remains");
        q.clear();

        // "ab^D" then "^D": two lines, two data bytes. The mark after "ab" goes
        // with the read that fills its buffer exactly, or the NEXT read would
        // see a spurious end of file.
        let _ = q.push_line(b"ab", true);
        let _ = q.push_line(b"", true);
        selftest::check_eq!(q.data_len(), 2, "marks are not data");
        selftest::check_eq!(q.lines(), 2);
        selftest::check_eq!(q.committed_data_len(), 2);
        let mut two = [0u8; 2];
        selftest::check_eq!(q.read_line(&mut two), 2, "'ab' fills the buffer exactly");
        selftest::check_eq!(q.lines(), 1, "and its end-of-file mark went with it");
        selftest::check_eq!(
            q.read_line(&mut rest),
            0,
            "then the empty line: end of file"
        );
        selftest::check!(q.is_empty(), "both marks consumed");

        // Wraparound: fill and drain across the end of the ring several times.
        let mut sink = alloc::vec![0u8; INPUT_QUEUE_CAPACITY];
        for round in 0u8..3 {
            for _ in 0..INPUT_QUEUE_CAPACITY - 10 {
                let _ = q.push(round, 0);
            }
            selftest::check_eq!(q.read_raw(&mut sink), INPUT_QUEUE_CAPACITY - 10);
            selftest::check!(
                sink.iter()
                    .take(INPUT_QUEUE_CAPACITY - 10)
                    .all(|&b| b == round)
            );
        }
        selftest::check!(q.is_empty(), "drained");
        crate::serial_println!("[tty]   input queue (lines, partial reads, EOF marks, wrap): OK");
    }

    // Foreground process group (job control).  There is nothing to set here
    // any more: the value is owned by whichever session holds the console
    // (`proc::pcb`'s controlling-terminal table), and this module only reads
    // it.  What is worth asserting is that the read agrees with that table
    // rather than caching — if this module ever reacquires storage of its
    // own, the two would drift and `^C` would go to the wrong job.
    {
        selftest::check_eq!(
            foreground_pgid(CONSOLE),
            crate::proc::pcb::ctty_fg_pgrp(CONSOLE).unwrap_or(0),
            "console foreground pgrp must be a derived read of the ctty table"
        );
        crate::serial_println!("[tty]   foreground pgrp is session-owned: OK");
    }

    crate::serial_println!("[tty] Self-test passed.");
    Ok(())
}
