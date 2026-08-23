//! POSIX ioctl() and terminal control.
//!
//! Our kernel has no unified `ioctl` syscall.  This module handles
//! common ioctl requests in userspace by inspecting the fd's handle
//! kind and returning appropriate defaults or errors:
//!
//! - **`TIOCGWINSZ`**: returns default terminal dimensions for Console fds.
//! - **`TIOCSWINSZ`**: accepts (no-op) for Console fds.
//! - **`FIONBIO`**: non-blocking mode flag — sets/clears `O_NONBLOCK` on
//!   the fd (equivalent to `fcntl(fd, F_SETFL, ... | O_NONBLOCK)`).
//! - **`FIONREAD`**: bytes available to read without blocking.
//! - **`TCGETS`/`TCSETS`**: termios get/set — real, via the kernel's
//!   `SYS_TTY_GET_TERMIOS`/`SYS_TTY_SET_TERMIOS` (541/542).
//! - All other requests return `ENOTTY`.
//!
//! ## Terminal Model
//!
//! Our console is a framebuffer with VT100 escape sequence support.  There
//! is no TTY *device* layer (no `/dev/tty`, no PTYs) — but there is a real
//! **line discipline**, in `kernel/src/tty.rs`, and this module talks to it.
//!
//! `TCGETS`/`TCSETS` used to be faked here: `tcgetattr` answered from the
//! `default_termios()` constant below and `tcsetattr` accepted the call and
//! discarded it, on the rationale that "our console has no configurable line
//! discipline".  That was true when it was written and stopped being true
//! when the kernel gained one, with the result that a native-ABI program
//! asking for raw mode silently stayed in cooked mode while a Linux-ABI
//! program on the same console really got raw mode.  Both now marshal the
//! same 36-byte kernel `struct termios` to the same state.  See
//! design-decisions §114.
//!
//! `default_termios()` is therefore **host-only** now.  On the bare-metal
//! target the initial state is the kernel's `tty::Termios::sane_default()`
//! and libc never invents one; what is left here is the host test double's
//! seed and, more usefully, an independently-written copy of the same
//! constants that the tests below pin field by field — so a drift between
//! our idea of a sane terminal and the kernel's shows up as a test failure
//! rather than as a program that mysteriously starts in the wrong mode.
//!
//! ## Terminal Control Functions
//!
//! - `cfmakeraw` — configure termios for raw I/O (no echo, no canonical)
//! - `cfsetspeed` — set both input and output baud rate
//! - `tcsendbreak` — send break condition (stub)
//! - `tcdrain` — wait for output to complete (stub, writes are synchronous)
//! - `tcflow` — suspend/restart I/O (stub, no flow control)
//! - `tcflush` — discard pending I/O (stub, no buffered data)
//!
//! ## isatty / ttyname
//!
//! `isatty(fd)` returns 1 for Console fds, 0 for everything else.
//! `ttyname(fd)` returns "/dev/console" for Console fds.

use crate::errno;
use crate::fdtable::{self, HandleKind};

// ---------------------------------------------------------------------------
// ioctl request codes (Linux x86_64 values)
// ---------------------------------------------------------------------------

/// Get terminal window size.
pub const TIOCGWINSZ: u64 = 0x5413;
/// Set terminal window size.
pub const TIOCSWINSZ: u64 = 0x5414;
/// Set/clear non-blocking I/O.
pub const FIONBIO: u64 = 0x5421;
/// Get number of bytes available to read.
pub const FIONREAD: u64 = 0x541B;
/// Get termios attributes.
pub const TCGETS: u64 = 0x5401;
/// Set termios attributes immediately.
pub const TCSETS: u64 = 0x5402;
/// Set termios after draining output.
pub const TCSETSW: u64 = 0x5403;
/// Set termios after draining output and flushing input.
pub const TCSETSF: u64 = 0x5404;
/// Make this the controlling terminal (for session leaders).
pub const TIOCSCTTY: u64 = 0x540E;
/// Get foreground process group of terminal.
pub const TIOCGPGRP: u64 = 0x540F;
/// Set foreground process group of terminal.
pub const TIOCSPGRP: u64 = 0x5410;
/// Release controlling terminal.
pub const TIOCNOTTY: u64 = 0x5422;

// ---------------------------------------------------------------------------
// tcsetattr `optional_actions` constants
// ---------------------------------------------------------------------------

/// Apply changes immediately.
pub const TCSANOW: i32 = 0;
/// Apply after all output has been transmitted.
pub const TCSADRAIN: i32 = 1;
/// Apply after all output has been transmitted, discard pending input.
pub const TCSAFLUSH: i32 = 2;

// ---------------------------------------------------------------------------
// termios flag constants
// ---------------------------------------------------------------------------

// c_iflag bits — input modes.
/// Signal interrupt on break.
pub const BRKINT: u32 = 0o2;
/// Enable input parity check.
pub const INPCK: u32 = 0o20;
/// Strip high bit from input bytes.
pub const ISTRIP: u32 = 0o40;
/// Translate NL to CR on input.
pub const INLCR: u32 = 0o100;
/// Ignore CR on input.
pub const IGNCR: u32 = 0o200;
/// Translate CR to NL on input.
pub const ICRNL: u32 = 0o400;
/// Enable XON/XOFF flow control on output.
pub const IXON: u32 = 0o2000;

// c_oflag bits — output modes.
/// Post-process output.
pub const OPOST: u32 = 0o1;
/// Map NL to CR-NL on output.
pub const ONLCR: u32 = 0o4;

// c_cflag bits — control modes.
/// Character size mask.
pub const CSIZE: u32 = 0o60;
/// 8-bit characters.
pub const CS8: u32 = 0o60;
/// Enable receiver.
pub const CREAD: u32 = 0o200;
/// Enable parity generation/checking.
pub const PARENB: u32 = 0o400;
/// Hang up on last close.
pub const HUPCL: u32 = 0o2000;
/// Ignore modem control lines.
pub const CLOCAL: u32 = 0o4000;

// c_lflag bits — local modes.
/// Enable signals (INTR, QUIT, SUSP).
pub const ISIG: u32 = 0o1;
/// Canonical mode (line editing).
pub const ICANON: u32 = 0o2;
/// Echo input characters.
pub const ECHO: u32 = 0o10;
/// Echo NL even if ECHO is off.
pub const ECHONL: u32 = 0o100;
/// Enable implementation-defined input processing.
pub const IEXTEN: u32 = 0o100_000;

// c_cc indices — control characters.
/// Interrupt character (Ctrl-C).
pub const VINTR: usize = 0;
/// Quit character (Ctrl-\).
pub const VQUIT: usize = 1;
/// Erase character (Backspace).
pub const VERASE: usize = 2;
/// Kill (line erase) character (Ctrl-U).
pub const VKILL: usize = 3;
/// EOF character (Ctrl-D).
pub const VEOF: usize = 4;
/// Timeout for non-canonical read.
pub const VTIME: usize = 5;
/// Minimum characters for non-canonical read.
pub const VMIN: usize = 6;
/// Start output character (Ctrl-Q).
pub const VSTART: usize = 8;
/// Stop output character (Ctrl-S).
pub const VSTOP: usize = 9;
/// Suspend character (Ctrl-Z).
pub const VSUSP: usize = 10;
/// End-of-line character.
pub const VEOL: usize = 11;
/// Number of control characters.
pub const NCCS: usize = 32;

// ---------------------------------------------------------------------------
// Baud rate constants (B-series)
// ---------------------------------------------------------------------------

/// 9600 baud.
pub const B9600: u32 = 0o15;
/// 19200 baud.
pub const B19200: u32 = 0o16;
/// 38400 baud.
pub const B38400: u32 = 0o17;
/// 115200 baud.
pub const B115200: u32 = 0o10002;

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

/// Terminal window size.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Winsize {
    /// Number of rows.
    pub ws_row: u16,
    /// Number of columns.
    pub ws_col: u16,
    /// Horizontal pixel size (unused).
    pub ws_xpixel: u16,
    /// Vertical pixel size (unused).
    pub ws_ypixel: u16,
}

/// Terminal I/O attributes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Termios {
    /// Input mode flags.
    pub c_iflag: u32,
    /// Output mode flags.
    pub c_oflag: u32,
    /// Control mode flags.
    pub c_cflag: u32,
    /// Local mode flags.
    pub c_lflag: u32,
    /// Line discipline (unused).
    pub c_line: u8,
    /// Control characters.
    pub c_cc: [u8; NCCS],
    /// Input baud rate.
    pub c_ispeed: u32,
    /// Output baud rate.
    pub c_ospeed: u32,
}

// ---------------------------------------------------------------------------
// Default terminal state
// ---------------------------------------------------------------------------

/// Default terminal dimensions (80x25 text mode).
///
/// **Host-only**, for the same reason as [`default_termios`]: on the real
/// target the size belongs to the kernel and must be asked for
/// (`SYS_PTY_GET_WINSIZE`).  What survives here is the host double's seed.
#[cfg(not(target_os = "none"))]
const DEFAULT_WINSIZE: Winsize = Winsize {
    ws_row: 25,
    ws_col: 80,
    ws_xpixel: 0,
    ws_ypixel: 0,
};

/// Build a default termios reflecting cooked mode with echo.
///
/// This matches a typical Linux terminal initial state: canonical
/// mode, echo enabled, CR→NL translation, common control characters —
/// the same values Linux's `INIT_C_CC` / `tty_std_termios` carry, which is
/// also what `kernel/src/tty.rs`'s `Termios::sane_default()` installs.
///
/// **Host-only.**  It used to be the answer `tcgetattr` gave on the real
/// target, which was the bug §114 fixed: the console's mode is the
/// kernel's, and libc must ask (`SYS_TTY_GET_TERMIOS`) rather than assume.
/// What remains is the host test double's seed plus an independently
/// written copy of those constants that the tests pin field by field, so a
/// drift from the kernel's table surfaces as a test failure.  Compiling it
/// out of the bare-metal build is deliberate: it guarantees no target-side
/// path can quietly start answering from a constant again.
#[cfg(not(target_os = "none"))]
fn default_termios() -> Termios {
    let mut cc = [0u8; NCCS];

    // Standard control character defaults (same as Linux).
    if let Some(slot) = cc.get_mut(VINTR) {
        *slot = 0x03;
    } // Ctrl-C
    if let Some(slot) = cc.get_mut(VQUIT) {
        *slot = 0x1C;
    } // Ctrl-backslash
    if let Some(slot) = cc.get_mut(VERASE) {
        *slot = 0x7F;
    } // DEL
    if let Some(slot) = cc.get_mut(VKILL) {
        *slot = 0x15;
    } // Ctrl-U
    if let Some(slot) = cc.get_mut(VEOF) {
        *slot = 0x04;
    } // Ctrl-D
    if let Some(slot) = cc.get_mut(VSTART) {
        *slot = 0x11;
    } // Ctrl-Q
    if let Some(slot) = cc.get_mut(VSTOP) {
        *slot = 0x13;
    } // Ctrl-S
    if let Some(slot) = cc.get_mut(VSUSP) {
        *slot = 0x1A;
    } // Ctrl-Z
    if let Some(slot) = cc.get_mut(VMIN) {
        *slot = 1;
    } // min chars for read
    if let Some(slot) = cc.get_mut(VTIME) {
        *slot = 0;
    } // no timeout

    Termios {
        c_iflag: ICRNL,                                  // CR→NL on input
        c_oflag: OPOST | ONLCR,                          // post-process, NL→CRNL
        c_cflag: CS8 | CREAD | HUPCL | CLOCAL,           // 8-bit, receiver on
        c_lflag: ISIG | ICANON | ECHO | ECHONL | IEXTEN, // cooked mode + echo
        c_line: 0,
        c_cc: cc,
        c_ispeed: B38400,
        c_ospeed: B38400,
    }
}

// ---------------------------------------------------------------------------
// Kernel termios marshalling
// ---------------------------------------------------------------------------

/// Number of control characters in the *kernel* `struct termios`.
///
/// Deliberately smaller than [`NCCS`] (32): the kernel wire format for
/// `TCGETS`/`TCSETS` is Linux's 36-byte kernel `struct termios`, which has 19
/// control characters and no baud-rate fields, while the *user* `struct
/// termios` that C programs see is musl's larger one.  glibc and musl both
/// marshal between the two exactly like this, so a program's `struct termios`
/// never has to match the kernel's.
const KERNEL_NCCS: usize = 19;

/// Serialised size of the kernel `struct termios`: four `u32` flag words, a
/// one-byte `c_line`, and [`KERNEL_NCCS`] control bytes (4*4 + 1 + 19 = 36).
/// Must equal `kernel/src/tty.rs`'s `TERMIOS_BYTES`.
const KERNEL_TERMIOS_BYTES: usize = 4 * 4 + 1 + KERNEL_NCCS;

/// Byte length of the four flag words at the front of the wire format.
///
/// Named so the marshalling functions can split the buffer at it rather than
/// spell `16` twice and drift from [`KERNEL_TERMIOS_BYTES`].
const FLAG_BYTES: usize = 4 * 4;

/// Baud-rate field inside `c_cflag` (Linux `CBAUD`, including `CBAUDEX`).
///
/// The kernel wire format has no `c_ispeed`/`c_ospeed`; Linux encodes the
/// speed in these bits of `c_cflag`, and that is where we carry it too.  Our
/// console is a framebuffer with no line rate at all, so the value is purely
/// something we must not lose across a get/set round trip.
const CBAUD: u32 = 0o010017;

/// Marshal a user `Termios` into the 36-byte kernel wire format.
///
/// The control-character array is truncated to the kernel's 19 entries (the
/// user array's extra slots are unused padding in musl too), and the baud
/// rate is folded from `c_ospeed` into `c_cflag`'s `CBAUD` bits, which is
/// where the kernel struct keeps it.
fn termios_to_wire(t: &Termios) -> [u8; KERNEL_TERMIOS_BYTES] {
    let mut buf = [0u8; KERNEL_TERMIOS_BYTES];
    let cflag = (t.c_cflag & !CBAUD) | (t.c_ospeed & CBAUD);
    let words = [t.c_iflag, t.c_oflag, cflag, t.c_lflag];

    // Walk the three regions the wire format defines — four little-endian
    // u32s, the `c_line` byte, then the control characters — instead of
    // computing an offset for each write.  `chunks_exact_mut` and `zip` make
    // every write in-bounds by construction, so there is no index arithmetic
    // to get wrong (and none for `clippy::arithmetic_side_effects` to flag).
    // `zip` is also what truncates `c_cc` to the kernel's 19 entries: the
    // destination slice is that long, so the user array's extra padding slots
    // are dropped without a length check.
    let (flag_bytes, rest) = buf.split_at_mut(FLAG_BYTES);
    for (slot, word) in flag_bytes.chunks_exact_mut(4).zip(words) {
        slot.copy_from_slice(&word.to_le_bytes());
    }
    if let Some((line, cc)) = rest.split_first_mut() {
        *line = t.c_line;
        for (dst, src) in cc.iter_mut().zip(t.c_cc.iter()) {
            *dst = *src;
        }
    }
    buf
}

/// Unmarshal the 36-byte kernel wire format into a user `Termios`.
///
/// Control characters beyond the kernel's 19 are zeroed, and both speeds are
/// reported from `c_cflag`'s `CBAUD` bits — the inverse of [`termios_to_wire`],
/// so `tcgetattr` after `tcsetattr` returns what was set.
fn termios_from_wire(buf: &[u8; KERNEL_TERMIOS_BYTES]) -> Termios {
    // The exact inverse of `termios_to_wire`, written the same way: split the
    // fixed regions apart once, then iterate. Destructuring `words` rather
    // than indexing it keeps the flag order stated in one place and readable.
    let (flag_bytes, rest) = buf.split_at(FLAG_BYTES);
    let mut words = [0u32; 4];
    for (word, src) in words.iter_mut().zip(flag_bytes.chunks_exact(4)) {
        let mut b = [0u8; 4];
        b.copy_from_slice(src);
        *word = u32::from_le_bytes(b);
    }
    let [c_iflag, c_oflag, c_cflag, c_lflag] = words;

    // A wire buffer is always long enough for these — the length is in the
    // type — but the empty fallback keeps the function total rather than
    // relying on that for panic-freedom.
    let (c_line, wire_cc) = rest.split_first().unwrap_or((&0, &[]));
    let mut c_cc = [0u8; NCCS];
    // Control characters beyond the kernel's 19 stay zero: `wire_cc` is only
    // that long, so `zip` stops there.
    for (dst, src) in c_cc.iter_mut().zip(wire_cc.iter()) {
        *dst = *src;
    }

    Termios {
        c_iflag,
        c_oflag,
        c_cflag,
        c_lflag,
        c_line: *c_line,
        c_cc,
        c_ispeed: c_cflag & CBAUD,
        c_ospeed: c_cflag & CBAUD,
    }
}

// ---------------------------------------------------------------------------
// Kernel winsize marshalling
// ---------------------------------------------------------------------------

/// Serialised size of the kernel `struct winsize`: four little-endian `u16`s
/// (`ws_row`, `ws_col`, `ws_xpixel`, `ws_ypixel`).  Must equal
/// `kernel/src/tty/mod.rs`'s `WINSIZE_BYTES`.
const WINSIZE_BYTES: usize = 2 * 4;

/// Marshal a [`Winsize`] into the 8-byte kernel wire format.
///
/// On x86-64 this happens to be byte-for-byte the `#[repr(C)]` layout of
/// `Winsize` itself, so passing the struct's address straight to the kernel
/// would work today.  It is marshalled anyway for the same reason the termios
/// pair is: the wire format is a *contract with the kernel*, and a contract
/// that is only satisfied by accident of the host's endianness and padding
/// rules is one that breaks silently the first time either side changes.  The
/// cost is eight bytes of copying on an operation a terminal emulator
/// performs when its window is dragged.
fn winsize_to_wire(ws: &Winsize) -> [u8; WINSIZE_BYTES] {
    let mut buf = [0u8; WINSIZE_BYTES];
    let fields = [ws.ws_row, ws.ws_col, ws.ws_xpixel, ws.ws_ypixel];
    // `chunks_exact_mut(2).zip(fields)` makes every write in-bounds by
    // construction -- no offset arithmetic to get wrong, and nothing for
    // `clippy::indexing_slicing` or `arithmetic_side_effects` to flag.
    for (slot, field) in buf.chunks_exact_mut(2).zip(fields) {
        slot.copy_from_slice(&field.to_le_bytes());
    }
    buf
}

/// Unmarshal the 8-byte kernel wire format into a [`Winsize`].
///
/// The exact inverse of [`winsize_to_wire`], so a `TIOCGWINSZ` after a
/// `TIOCSWINSZ` reports what was set.
fn winsize_from_wire(buf: &[u8; WINSIZE_BYTES]) -> Winsize {
    let mut fields = [0u16; 4];
    for (field, src) in fields.iter_mut().zip(buf.chunks_exact(2)) {
        let mut b = [0u8; 2];
        b.copy_from_slice(src);
        *field = u16::from_le_bytes(b);
    }
    // Destructuring rather than indexing keeps the field order stated once,
    // in the same order as `winsize_to_wire` writes it.
    let [ws_row, ws_col, ws_xpixel, ws_ypixel] = fields;
    Winsize {
        ws_row,
        ws_col,
        ws_xpixel,
        ws_ypixel,
    }
}

/// Read the console's termios from the kernel (`SYS_TTY_GET_TERMIOS`).
///
/// Returns `None` with `errno` already set (by `errno::translate`) on
/// failure.  On host builds there is no kernel, so this answers from the
/// per-thread [`host_termios`] double — which is what makes the marshalling
/// above testable under `cargo test`.
fn get_kernel_termios(term: u64) -> Option<Termios> {
    #[cfg(target_os = "none")]
    {
        let mut buf = [0u8; KERNEL_TERMIOS_BYTES];
        let ret = if term == CTTY {
            // 541 keeps its exact original shape -- `(buffer)`, my-terminal
            // only.  Lane A deliberately did *not* widen it, so that
            // everything already compiled against it keeps working; the
            // handle-taking form is a new number.  Using 541 for the
            // controlling terminal rather than 555-with-zero keeps this
            // libc honest about which contract it is exercising.
            crate::syscall::syscall1(crate::syscall::SYS_TTY_GET_TERMIOS, buf.as_mut_ptr() as u64)
        } else {
            crate::syscall::syscall2(
                crate::syscall::SYS_PTY_GET_TERMIOS,
                term,
                buf.as_mut_ptr() as u64,
            )
        };
        if errno::translate(ret) < 0 {
            return None;
        }
        Some(termios_from_wire(&buf))
    }
    #[cfg(not(target_os = "none"))]
    {
        // One double for every terminal.  A host build can never own a real
        // pty -- `posix_openpt` needs `SYS_PTY_CREATE`, which is `ENOSYS`
        // here -- so the only way to reach this with `term != CTTY` is a
        // test that installed a pty fd by hand, and such a test is
        // exercising the marshalling, not the per-terminal separation.
        let _ = term;
        Some(termios_from_wire(&host_termios::get()))
    }
}

/// Install a console termios in the kernel (`SYS_TTY_SET_TERMIOS`).
///
/// Returns `false` with `errno` already set on failure.
fn set_kernel_termios(term: u64, t: &Termios) -> bool {
    let buf = termios_to_wire(t);
    #[cfg(target_os = "none")]
    {
        let ret = if term == CTTY {
            crate::syscall::syscall1(crate::syscall::SYS_TTY_SET_TERMIOS, buf.as_ptr() as u64)
        } else {
            crate::syscall::syscall2(
                crate::syscall::SYS_PTY_SET_TERMIOS,
                term,
                buf.as_ptr() as u64,
            )
        };
        errno::translate(ret) >= 0
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = term;
        host_termios::set(buf);
        true
    }
}

// Host-build test double for the console termios.
//
// `cargo test` runs this crate against the host triple, where the raw
// `syscall` instruction is gated off and every `syscallN` returns `-ENOSYS`.
// There is no SlateOS kernel to hold the terminal state, so the host build
// keeps the wire-format bytes in a per-thread cell — the same model as
// `process.rs`'s `host_pg` double for process groups.
//
// This deliberately stores the *wire* form rather than a `Termios`, so a
// host `tcsetattr`/`tcgetattr` round trip exercises both marshalling
// directions exactly as the kernel path does.  What it cannot prove is that
// the kernel agrees about the layout; that is the ring-3 fixture's job.
#[cfg(not(target_os = "none"))]
mod host_termios {
    use super::{KERNEL_TERMIOS_BYTES, default_termios, termios_to_wire};
    use core::cell::Cell;

    std::thread_local! {
        /// `None` until first use, then the last wire-format termios set.
        static TERMIOS: Cell<Option<[u8; KERNEL_TERMIOS_BYTES]>> = const { Cell::new(None) };
    }

    /// The current termios, defaulting to cooked mode with echo — the state
    /// the kernel's own console starts in.
    pub(super) fn get() -> [u8; KERNEL_TERMIOS_BYTES] {
        TERMIOS
            .try_with(Cell::get)
            .ok()
            .flatten()
            .unwrap_or_else(|| termios_to_wire(&default_termios()))
    }

    pub(super) fn set(v: [u8; KERNEL_TERMIOS_BYTES]) {
        let _ = TERMIOS.try_with(|c| c.set(Some(v)));
    }
}

// Host-build test double for the terminal window size.
//
// The mirror image of [`host_termios`], and it exists for the same two
// reasons: `cargo test` has no kernel to hold the size, and storing the
// *wire* form means a host `TIOCSWINSZ`/`TIOCGWINSZ` round trip exercises
// both marshalling directions rather than skipping them.  Without it the
// host build would never call `winsize_to_wire`/`winsize_from_wire` at all,
// so the only coverage they could have is a test that calls them directly
// -- which proves they are each other's inverse but not that anything uses
// them that way.
#[cfg(not(target_os = "none"))]
mod host_winsize {
    use super::{DEFAULT_WINSIZE, WINSIZE_BYTES, winsize_to_wire};
    use core::cell::Cell;

    std::thread_local! {
        /// `None` until first use, then the last wire-format winsize set.
        static WINSIZE: Cell<Option<[u8; WINSIZE_BYTES]>> = const { Cell::new(None) };
    }

    pub(super) fn get() -> [u8; WINSIZE_BYTES] {
        WINSIZE
            .try_with(Cell::get)
            .ok()
            .flatten()
            .unwrap_or_else(|| winsize_to_wire(&DEFAULT_WINSIZE))
    }

    pub(super) fn set(v: [u8; WINSIZE_BYTES]) {
        let _ = WINSIZE.try_with(|c| c.set(Some(v)));
    }
}

// ---------------------------------------------------------------------------
// Naming a terminal to the kernel
// ---------------------------------------------------------------------------

/// The `arg0` that means "the caller's controlling terminal".
///
/// The pty family's terminal-taking syscalls (539, 553-556) share one
/// convention for `arg0`: `0` is my terminal, `1` is reserved and always
/// rejected, and `>= 2` is a pty handle the caller must actually own.  See
/// the header comment on [`crate::syscall::SYS_PTY_CREATE`].
const CTTY: u64 = 0;

/// Name the terminal an fd refers to, in the kernel's convention.
///
/// `None` means "this descriptor is not a terminal", which every caller
/// turns into `ENOTTY`.
///
/// A `Console` fd becomes [`CTTY`] rather than a handle, and that is not a
/// shortcut: console fds carry no pty handle, and the kernel resolves
/// every console operation through `current_tty()` anyway -- so `0` is a
/// more accurate description of a console fd than any number would be.
/// The practical consequence is the good one: after `login_tty`, a child's
/// console-kind stdio and the pty it is running on are the same terminal,
/// so `tcgetattr(0)` in the child reads the pty's discipline without the
/// child having to know it is on a pty.
fn terminal_arg(kind: HandleKind, handle: u64) -> Option<u64> {
    match kind {
        HandleKind::Console => Some(CTTY),
        HandleKind::PtyMaster | HandleKind::PtySlave => Some(handle),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// ioctl()
// ---------------------------------------------------------------------------

/// Perform device-specific I/O control.
///
/// Since our kernel has no `ioctl` syscall, this handles common
/// requests in userspace based on the fd's handle kind.  Unrecognised
/// requests return `ENOTTY`.
///
/// The third argument is a pointer whose type depends on `request`.
///
/// Returns 0 on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ioctl(fd: i32, request: u64, arg: *mut u8) -> i32 {
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };

    match request {
        TIOCGWINSZ => handle_tiocgwinsz(entry.kind, entry.handle, arg),
        TIOCSWINSZ => handle_tiocswinsz(entry.kind, entry.handle, arg),
        FIONBIO => handle_fionbio(fd, arg),
        FIONREAD => handle_fionread(entry.kind, entry.handle, arg),
        TCGETS => handle_tcgets(entry.kind, entry.handle, arg),
        TCSETS | TCSETSW | TCSETSF => handle_tcsets(entry.kind, entry.handle, arg),
        TIOCGPGRP => handle_tiocgpgrp(fd, entry.kind, arg),
        TIOCSPGRP => handle_tiocspgrp(fd, entry.kind, arg),
        TIOCSCTTY => handle_tiocsctty(entry.kind, entry.handle),
        TIOCNOTTY => handle_tiocnotty(entry.kind, entry.handle),
        _ => {
            errno::set_errno(errno::ENOTTY);
            -1
        }
    }
}

/// TIOCGWINSZ — get terminal window size.
///
/// Reads the kernel's real size via `SYS_PTY_GET_WINSIZE`, which takes a
/// terminal under the convention [`terminal_arg`] encodes and therefore
/// answers for the console (`0`) and for a named pty alike.
///
/// This used to answer [`DEFAULT_WINSIZE`] unconditionally, which was
/// merely stale for the console but would have been actively wrong for a
/// pty: a terminal emulator's whole point is that its size is not 80x25,
/// and a shell that believes otherwise wraps every line in the wrong
/// place.  The default survives only as the *host* answer, where there is
/// no kernel to ask.
fn handle_tiocgwinsz(kind: HandleKind, handle: u64, arg: *mut u8) -> i32 {
    let Some(term) = terminal_arg(kind, handle) else {
        errno::set_errno(errno::ENOTTY);
        return -1;
    };
    if arg.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    #[cfg(target_os = "none")]
    {
        let mut buf = [0u8; WINSIZE_BYTES];
        let ret = crate::syscall::syscall2(
            crate::syscall::SYS_PTY_GET_WINSIZE,
            term,
            buf.as_mut_ptr() as u64,
        );
        if errno::translate(ret) < 0 {
            return -1;
        }
        // SAFETY: Caller must provide a buffer large enough for Winsize.
        // Use write_unaligned since we don't know the alignment of arg.
        unsafe {
            core::ptr::write_unaligned(arg.cast::<Winsize>(), winsize_from_wire(&buf));
        }
        0
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = term;
        // SAFETY: Caller must provide a buffer large enough for Winsize.
        unsafe {
            core::ptr::write_unaligned(
                arg.cast::<Winsize>(),
                winsize_from_wire(&host_winsize::get()),
            );
        }
        0
    }
}

/// TIOCSWINSZ — set terminal window size.
///
/// On a pty this is the operation that makes a resized emulator window
/// visible to the program inside it: the kernel raises `SIGWINCH` on the
/// slave's foreground process group, but **only when the size actually
/// changed**, because shells re-set the same size at every prompt and a
/// redraw storm per prompt is worse than no signal at all.
///
/// On the console it is still accepted and still cannot resize anything —
/// the framebuffer's geometry comes from the display mode — but it now
/// goes to the kernel rather than being swallowed here, so the size the
/// console *reports* and the size it was *told* cannot drift apart.
fn handle_tiocswinsz(kind: HandleKind, handle: u64, arg: *mut u8) -> i32 {
    let Some(term) = terminal_arg(kind, handle) else {
        errno::set_errno(errno::ENOTTY);
        return -1;
    };
    if arg.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: Caller must provide a buffer large enough for Winsize.
    let ws = unsafe { core::ptr::read_unaligned(arg.cast::<Winsize>()) };
    #[cfg(target_os = "none")]
    {
        let buf = winsize_to_wire(&ws);
        let ret = crate::syscall::syscall2(
            crate::syscall::SYS_PTY_SET_WINSIZE,
            term,
            buf.as_ptr() as u64,
        );
        if errno::translate(ret) < 0 {
            return -1;
        }
        0
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = term;
        host_winsize::set(winsize_to_wire(&ws));
        0
    }
}

/// FIONBIO — set/clear non-blocking I/O.
///
/// Sets or clears the `O_NONBLOCK` flag on the fd, equivalent to
/// `fcntl(fd, F_SETFL, flags | O_NONBLOCK)`.  The argument is a
/// pointer to an int: nonzero enables non-blocking, zero disables.
fn handle_fionbio(fd: i32, arg: *mut u8) -> i32 {
    if arg.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: arg must be at least sizeof(i32), per POSIX ioctl(FIONBIO).
    let enable = unsafe { core::ptr::read_unaligned(arg.cast::<i32>()) };
    let current = fdtable::get_status_flags(fd).unwrap_or(0);
    let new_flags = if enable != 0 {
        current | crate::fcntl::O_NONBLOCK
    } else {
        current & !crate::fcntl::O_NONBLOCK
    };
    if fdtable::set_status_flags(fd, new_flags) {
        0
    } else {
        errno::set_errno(errno::EBADF);
        -1
    }
}

/// FIONREAD — get number of bytes available to read.
///
/// Returns 0 for Console fds (we don't buffer input), ENOTTY for
/// non-terminal fds (files don't support FIONREAD via ioctl; use
/// stat + seek instead).
///
/// The pty arms answer `0` or `1` rather than a true count, because the
/// kernel exposes no readable-byte count for a pty — only the readable bit
/// of `SYS_PTY_POLL`.  See the arm's own comment for why that is a
/// deliberate approximation and not a silent one; it is tracked as
/// `TD-B-PTY-FIONREAD-IS-A-BOOLEAN` and requested of lane A in
/// `requests/b-a-pty-gaps-master-inheritance-and-readable-bytes.md`.
fn handle_fionread(kind: HandleKind, handle: u64, arg: *mut u8) -> i32 {
    use crate::syscall::{SYS_TCP_INFO, syscall3};

    if arg.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    match kind {
        HandleKind::Console => {
            // Console: no buffering visible from userspace.
            // SAFETY: arg must be at least sizeof(i32).
            unsafe {
                core::ptr::write_unaligned(arg.cast::<i32>(), 0);
            }
            0
        }
        HandleKind::Pipe => {
            // Query actual buffered byte count from the kernel.
            use crate::syscall::{SYS_PIPE_READABLE_BYTES, syscall1};
            let bytes = syscall1(SYS_PIPE_READABLE_BYTES, handle) as i32;
            // SAFETY: arg must be at least sizeof(i32).
            unsafe {
                core::ptr::write_unaligned(arg.cast::<i32>(), bytes);
            }
            0
        }
        HandleKind::File => {
            errno::set_errno(errno::ENOTTY);
            -1
        }
        HandleKind::UnixStream => {
            // Query actual buffered byte count from the kernel.
            let bytes = if handle == 0 {
                0
            } else {
                use crate::syscall::{SYS_SOCKETPAIR_READABLE_BYTES, syscall1};
                syscall1(SYS_SOCKETPAIR_READABLE_BYTES, handle) as i32
            };
            // SAFETY: arg must be at least sizeof(i32).
            unsafe {
                core::ptr::write_unaligned(arg.cast::<i32>(), bytes);
            }
            0
        }
        HandleKind::TcpStream => {
            if handle == 0 {
                // SAFETY: arg must be at least sizeof(i32).
                unsafe {
                    core::ptr::write_unaligned(arg.cast::<i32>(), 0);
                }
                return 0;
            }
            // Query TCP_INFO to get rx_buffered (bytes 24..28).
            let mut info_buf = [0u8; 48];
            let ret = syscall3(SYS_TCP_INFO, handle, info_buf.as_mut_ptr() as u64, 48);
            let available = if ret == 0 {
                // rx_buffered is at offset 24, 4 bytes LE.
                u32::from_le_bytes([info_buf[24], info_buf[25], info_buf[26], info_buf[27]])
            } else {
                0
            };
            // SAFETY: arg must be at least sizeof(i32).
            unsafe {
                core::ptr::write_unaligned(arg.cast::<i32>(), available as i32);
            }
            0
        }
        HandleKind::TcpListener => {
            // For listeners: number of pending connections (1 or 0).
            // Simplistically reported as 0 for now.
            // SAFETY: arg must be at least sizeof(i32).
            unsafe {
                core::ptr::write_unaligned(arg.cast::<i32>(), 0);
            }
            0
        }
        HandleKind::UdpSocket => {
            // FIONREAD on UDP returns byte size of the first deliverable
            // datagram (POSIX semantics), not total queued bytes.
            use crate::syscall::{SYS_UDP_RX_FRONT_BYTES, syscall1};
            let bytes = if handle == 0 {
                0
            } else {
                syscall1(SYS_UDP_RX_FRONT_BYTES, handle) as i32
            };
            // SAFETY: arg must be at least sizeof(i32).
            unsafe {
                core::ptr::write_unaligned(arg.cast::<i32>(), bytes);
            }
            0
        }
        HandleKind::PtyMaster | HandleKind::PtySlave => {
            // The kernel has no `SYS_PTY_READABLE_BYTES` counterpart to
            // `SYS_PIPE_READABLE_BYTES`, so the honest answer available here
            // is the readable *bit* from `SYS_PTY_POLL`, widened to 0 or 1.
            //
            // Answering `ENOTTY` instead was considered and rejected: a pty
            // does support FIONREAD on Linux, and the programs that ask are
            // terminal emulators, whose fallback for "this is not a
            // terminal" is far more wrong than a low count.  A low count
            // degrades a caller that sizes a read by it into reading a byte
            // at a time — slow, but every byte still arrives, because
            // `read()` returns what is actually there regardless of what
            // this said.  Critically, the 0 case is *exact*: a caller that
            // uses FIONREAD only to test emptiness (the common case, and
            // what `select`-less polling loops do) gets the right answer.
            //
            // The real fix is a kernel counter; until it exists this must
            // never silently grow a plausible-looking estimate, since a
            // wrong non-zero count is worse than an admittedly coarse one.
            let status = crate::syscall::syscall1(crate::syscall::SYS_PTY_POLL, handle);
            let available = i32::from(status >= 0 && (status as u64) & 0x1 != 0);
            // SAFETY: arg must be at least sizeof(i32).
            unsafe {
                core::ptr::write_unaligned(arg.cast::<i32>(), available);
            }
            0
        }
        HandleKind::Eventfd | HandleKind::Epoll | HandleKind::Timerfd | HandleKind::Inotify => {
            // Linux's eventfd / epoll / timerfd / inotify have no .ioctl
            // handler, so ioctl() returns ENOTTY on them.  Match that
            // behavior.
            errno::set_errno(errno::ENOTTY);
            -1
        }
    }
}

/// TCGETS — get termios attributes.
///
/// Reads the kernel's real line-discipline state via `SYS_TTY_GET_TERMIOS`
/// and unmarshals the 36-byte kernel wire format into our (larger) user
/// `Termios`.  This used to return `default_termios()` unconditionally, so
/// it reported cooked-mode-with-echo even after a `tcsetattr` to raw mode,
/// and disagreed with what a Linux-ABI program on the same console saw.
fn handle_tcgets(kind: HandleKind, handle: u64, arg: *mut u8) -> i32 {
    let Some(term) = terminal_arg(kind, handle) else {
        errno::set_errno(errno::ENOTTY);
        return -1;
    };
    if arg.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    let Some(t) = get_kernel_termios(term) else {
        // errno already set by the translation of the kernel's error.
        return -1;
    };
    // SAFETY: Caller must provide a buffer large enough for Termios.
    unsafe {
        core::ptr::write_unaligned(arg.cast::<Termios>(), t);
    }
    0
}

/// TCSETS / TCSETSW / TCSETSF — set termios attributes.
///
/// Marshals our user `Termios` into the 36-byte kernel wire format and
/// installs it via `SYS_TTY_SET_TERMIOS`, so raw mode, `ECHO` and the
/// control characters take real effect on the next console read.
///
/// All three requests behave identically: `TCSETSW` waits for queued output
/// to drain and `TCSETSF` additionally flushes pending input, and we have
/// neither an output queue nor a kernel-side input queue to act on.  The
/// Linux shim collapses the same three for the same reason.
///
/// This was previously accepted and thrown away, on the rationale that "our
/// console has no configurable line discipline".  That stopped being true
/// when `kernel/src/tty.rs` gained one; the comment outlived the fact, and
/// every native-ABI program that asked for raw mode silently got cooked
/// mode instead.
fn handle_tcsets(kind: HandleKind, handle: u64, arg: *mut u8) -> i32 {
    let Some(term) = terminal_arg(kind, handle) else {
        errno::set_errno(errno::ENOTTY);
        return -1;
    };
    if arg.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: Caller must provide a buffer large enough for Termios.
    let t = unsafe { core::ptr::read_unaligned(arg.cast::<Termios>()) };
    if set_kernel_termios(term, &t) {
        0
    } else {
        // errno already set by the translation of the kernel's error.
        -1
    }
}

/// Whether the *process-group* ioctls may act on this kind of descriptor.
///
/// Narrower than [`terminal_arg`] on purpose, and the exclusion is
/// `PtyMaster`.  Syscalls 537/538 (`SYS_TTY_GET_PGRP`/`SYS_TTY_SET_PGRP`)
/// were deliberately *not* widened to the terminal-naming convention, so
/// they answer only for the caller's own controlling terminal.  A pty slave
/// usually *is* that -- after `login_tty` it is exactly that -- so
/// delegating works and, when it does not, the kernel's `ENOTTY` is the
/// truthful answer.  A master never is: it belongs to the emulator, whose
/// controlling terminal is something else entirely, so delegating on a
/// master would report the *emulator's* foreground group as if it were the
/// pty's -- a wrong number, which is worse than a refusal.
///
/// Linux can answer for a master because master and slave share one `struct
/// tty`.  We cannot until 537/538 take a terminal; requested of lane A in
/// `requests/b-a-pty-gaps-master-inheritance-and-readable-bytes.md` and
/// tracked as `TD-B-PTY-MASTER-HAS-NO-FOREGROUND-GROUP`.
fn is_pgrp_terminal(kind: HandleKind) -> bool {
    matches!(kind, HandleKind::Console | HandleKind::PtySlave)
}

/// TIOCGPGRP — get the foreground process group of a terminal.
///
/// Returns the PGID via the integer pointer `arg`.  Delegates to
/// `tcgetpgrp()`, which reads the value from the kernel, keyed by our
/// session — so this reports the same foreground group every other member
/// of the session sees.
///
/// `tcgetpgrp` can genuinely fail (`ENOTTY` when the session has no
/// controlling terminal), so its -1 is propagated rather than written into
/// the caller's buffer as if it were a process group.
fn handle_tiocgpgrp(fd: i32, kind: HandleKind, arg: *mut u8) -> i32 {
    if !is_pgrp_terminal(kind) {
        errno::set_errno(errno::ENOTTY);
        return -1;
    }
    if arg.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    let pgrp = crate::process::tcgetpgrp(fd);
    if pgrp < 0 {
        // errno is already set by tcgetpgrp.
        return -1;
    }
    // SAFETY: arg must be at least sizeof(i32) per ioctl contract.
    unsafe {
        core::ptr::write_unaligned(arg.cast::<i32>(), pgrp);
    }
    0
}

/// TIOCSPGRP — set the foreground process group of a terminal.
///
/// Reads the PGID from the integer pointer `arg` and delegates to
/// `tcsetpgrp()`.
fn handle_tiocspgrp(fd: i32, kind: HandleKind, arg: *mut u8) -> i32 {
    if !is_pgrp_terminal(kind) {
        errno::set_errno(errno::ENOTTY);
        return -1;
    }
    if arg.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: arg must be at least sizeof(i32) per ioctl contract.
    let pgrp = unsafe { core::ptr::read_unaligned(arg.cast::<i32>()) };
    crate::process::tcsetpgrp(fd, pgrp)
}

/// TIOCSCTTY — claim this terminal as our session's controlling terminal.
///
/// This used to be accepted silently on the grounds that "we don't have
/// real TTY sessions yet".  We do now: the kernel keys the controlling
/// terminal by session id, and this is the only way for a userspace process
/// to acquire one — a session leader that never claims gets `ENOTTY` from
/// `tcgetpgrp`/`tcsetpgrp` forever.  Accepting silently therefore stopped
/// being harmless and started being a lie.
///
/// The kernel enforces POSIX's two rules (session leader only; the terminal
/// must not already belong to another session) and reports `EPERM` for
/// either.  Linux's `arg != 0` "steal the terminal from another session"
/// force flag is deliberately not implemented — it is a root-only override
/// and we have no credential model for it yet; see `todo.txt`.
///
/// Syscall 539 is the one member of the tty family whose signature changed
/// when ptys landed: it takes a *terminal* now, under the convention
/// [`terminal_arg`] encodes.  That is what makes `login_tty` work — the
/// whole point of the call there is to claim a terminal that is emphatically
/// *not* the one we already have, and a no-argument "acquire" could only
/// ever mean the console.
///
/// Errors:
///   * `ENOTTY` — `fd` is not a terminal.
///   * `EPERM` — not a session leader, or the terminal is taken.
fn handle_tiocsctty(kind: HandleKind, handle: u64) -> i32 {
    let Some(term) = terminal_arg(kind, handle) else {
        errno::set_errno(errno::ENOTTY);
        return -1;
    };
    #[cfg(target_os = "none")]
    {
        let ret = crate::syscall::syscall1(crate::syscall::SYS_TTY_ACQUIRE_CTTY, term);
        if ret < 0 {
            errno::set_errno(crate::process::ctty_errno(ret));
            return -1;
        }
        0
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = term;
        crate::process::host_ctty_acquire()
    }
}

/// TIOCNOTTY — give up our session's controlling terminal.
///
/// Restricted by the kernel to the session leader, and hangs up the
/// foreground process group (`SIGHUP` then `SIGCONT`) on success, matching
/// Linux's `disassociate_ctty(0)`.  A caller that is itself in the
/// foreground group is hung up too — which is why the usual idiom is
/// `setsid()` (which drops the terminal without the hangup) and why a
/// daemon should not reach for this one by mistake.
///
/// Errors:
///   * `ENOTTY` — `fd` is not a terminal, or we have no controlling one.
///   * `EPERM` — the caller is not the session leader.
fn handle_tiocnotty(kind: HandleKind, handle: u64) -> i32 {
    // Syscall 540 did not change: it releases *our* controlling terminal
    // and takes no argument, so the fd only has to be a terminal at all.
    // Which terminal it names is not consulted -- and need not be, because
    // the kernel's own check is "do I have one, and am I the session
    // leader", which no argument here could make more or less true.
    if terminal_arg(kind, handle).is_none() {
        errno::set_errno(errno::ENOTTY);
        return -1;
    }
    #[cfg(target_os = "none")]
    {
        let ret = crate::syscall::syscall0(crate::syscall::SYS_TTY_RELEASE_CTTY);
        if ret < 0 {
            errno::set_errno(crate::process::ctty_errno(ret));
            return -1;
        }
        0
    }
    #[cfg(not(target_os = "none"))]
    {
        crate::process::host_ctty_release()
    }
}

// ---------------------------------------------------------------------------
// isatty()
// ---------------------------------------------------------------------------

/// Test whether a file descriptor refers to a terminal.
///
/// Returns 1 for a console fd or for **either end of a pty**, 0 otherwise
/// (with errno set to `ENOTTY`).
///
/// Both pty ends count, as on Linux: a master is a character device with a
/// line discipline behind it, and programs rely on that.  `script(1)` runs
/// `isatty` on the master to decide whether to propagate the window size,
/// and a terminal emulator that got 0 here would conclude it had been
/// handed a pipe.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isatty(fd: i32) -> i32 {
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return 0;
    };

    if terminal_arg(entry.kind, entry.handle).is_some() {
        1
    } else {
        errno::set_errno(errno::ENOTTY);
        0
    }
}

// ---------------------------------------------------------------------------
// ttyname()
// ---------------------------------------------------------------------------

/// Return the name of the terminal device.
///
/// * a console fd -> `"/dev/console"`
/// * a pty **master** -> `"/dev/ptmx"`, which is the name it was opened by
///   and the answer glibc arrives at too (it stats the fd and searches
///   `/dev`, and `/dev/ptmx` is the node with that device number)
/// * a pty **slave** -> `"/dev/pts/<id>"`, asked of the kernel rather than
///   derived from the handle, so libc never has to know how a handle
///   encodes its terminal id
/// * anything else -> NULL with `ENOTTY`
///
/// **Not reentrant for a slave**, exactly as POSIX permits and glibc
/// implements: the name is formatted into a process-wide buffer that the
/// next slave-side call overwrites.  [`ttyname_r`] is the reentrant form
/// and is what everything in this tree should call; this one exists for
/// programs we did not write.  The two constant answers are static and do
/// not touch the buffer at all.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ttyname(fd: i32) -> *const u8 {
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return core::ptr::null();
    };

    match entry.kind {
        // SAFETY: These are static byte strings with a null terminator.
        HandleKind::Console => c"/dev/console".as_ptr().cast::<u8>(),
        HandleKind::PtyMaster => c"/dev/ptmx".as_ptr().cast::<u8>(),
        HandleKind::PtySlave => match slave_id_of(entry.handle) {
            Some(id) => ttyname_buf::store(id),
            // `slave_id_of` has already set errno from the kernel's reply.
            // Not overwriting it matters: a slave whose pair has gone
            // reports the kernel's verdict rather than a generic ENOTTY.
            None => core::ptr::null(),
        },
        _ => {
            errno::set_errno(errno::ENOTTY);
            core::ptr::null()
        }
    }
}

/// Longest `/dev/pts/<id>` this system can produce, plus its NUL.
///
/// `u32::MAX` is ten digits, so the widest name is `/dev/pts/4294967295`.
/// Sized from the type rather than from "ids are small in practice",
/// because a buffer bound that depends on a *habit* is the kind that stops
/// holding quietly.
const PTS_NAME_MAX: usize = "/dev/pts/".len() + 10 + 1;

/// Format `/dev/pts/<id>` into `out`, returning the length written,
/// excluding the NUL.
///
/// Split out from its two callers ([`ttyname`] and [`ptsname_r`]) so that
/// there is exactly one place this system's slave names are spelled, and so
/// that the formatting is unit-testable on the host, where no pty can exist
/// and every pty syscall answers `ENOSYS`.
fn format_pts_name(id: u32, out: &mut [u8; PTS_NAME_MAX]) -> usize {
    const PREFIX: &[u8] = b"/dev/pts/";
    let mut len = 0usize;
    for (dst, src) in out.iter_mut().zip(PREFIX.iter()) {
        *dst = *src;
        len = len.wrapping_add(1);
    }

    // Digits are generated least-significant first into a scratch array and
    // then copied back in reverse.  Ten is `u32::MAX`'s digit count, so the
    // scratch cannot overflow whatever `id` is.
    let mut digits = [0u8; 10];
    let mut ndigits = 0usize;
    let mut v = id;
    loop {
        #[allow(clippy::cast_possible_truncation)]
        let d = b'0'.wrapping_add((v % 10) as u8);
        if let Some(slot) = digits.get_mut(ndigits) {
            *slot = d;
        }
        ndigits = ndigits.wrapping_add(1);
        v /= 10;
        if v == 0 {
            break;
        }
    }
    let mut i = ndigits;
    while i > 0 {
        i = i.wrapping_sub(1);
        if let (Some(dst), Some(src)) = (out.get_mut(len), digits.get(i)) {
            *dst = *src;
        }
        len = len.wrapping_add(1);
    }
    if let Some(slot) = out.get_mut(len) {
        *slot = 0;
    }
    len
}

/// Ask the kernel which terminal a pty handle belongs to
/// (`SYS_PTY_SLAVE_ID`).
///
/// `None` leaves `errno` set by `errno::translate`.  Deliberately asks
/// rather than computing `handle >> 1`: the handle encoding is the kernel's
/// business, and syscall 551 exists precisely so that libc need not depend
/// on it.
fn slave_id_of(handle: u64) -> Option<u32> {
    let ret = crate::syscall::syscall1(crate::syscall::SYS_PTY_SLAVE_ID, handle);
    if errno::translate(ret) < 0 {
        return None;
    }
    u32::try_from(ret).ok().or_else(|| {
        // A terminal id that does not fit in 32 bits would mean the kernel
        // and this libc disagree about the type, which is a bug rather than
        // a runtime condition -- but it must not become a silent truncation
        // that names the wrong terminal.
        errno::set_errno(errno::ENOTTY);
        None
    })
}

// Process-wide buffer behind the non-reentrant `ttyname`.
//
// Follows the crate's global-state pattern (design-decisions section 110): a
// `static mut` on the bare-metal target, and per-thread storage on the host
// so that `cargo test` -- which runs each test on its own thread -- cannot
// have one test observe another test's name.
mod ttyname_buf {
    use super::PTS_NAME_MAX;

    #[cfg(target_os = "none")]
    mod imp {
        use super::PTS_NAME_MAX;
        static mut BUF: [u8; PTS_NAME_MAX] = [0; PTS_NAME_MAX];

        pub(super) fn buf() -> *mut [u8; PTS_NAME_MAX] {
            &raw mut BUF
        }
    }

    #[cfg(not(target_os = "none"))]
    mod imp {
        use super::PTS_NAME_MAX;
        use core::cell::UnsafeCell;

        std::thread_local! {
            static BUF: UnsafeCell<[u8; PTS_NAME_MAX]> =
                const { UnsafeCell::new([0; PTS_NAME_MAX]) };
        }

        /// Reached only if a `ttyname` runs during thread-local teardown,
        /// when `try_with` can no longer hand out the per-thread copy.
        static mut FALLBACK: [u8; PTS_NAME_MAX] = [0; PTS_NAME_MAX];

        pub(super) fn buf() -> *mut [u8; PTS_NAME_MAX] {
            BUF.try_with(UnsafeCell::get).unwrap_or(&raw mut FALLBACK)
        }
    }

    /// Format `/dev/pts/<id>` into the buffer and return a pointer to it.
    ///
    /// The pointer is resolved before the buffer is written, and the write
    /// happens outside any thread-local accessor, so the returned pointer
    /// is the same storage the name was formatted into on both builds.
    pub(super) fn store(id: u32) -> *const u8 {
        let p = imp::buf();
        // SAFETY: `p` names either this thread's own cell, the teardown
        // fallback, or -- on the bare-metal target, which is
        // single-threaded per the crate's global-state convention -- the
        // one process-wide buffer.  No other reference to it is live for
        // the duration of the call.  `ttyname`/`ptsname` are documented
        // non-reentrant, so a caller needing thread safety must use the
        // `_r` forms, which never touch this.
        super::format_pts_name(id, unsafe { &mut *p });
        p.cast::<u8>().cast_const()
    }
}

/// Thread-safe `ttyname`: write the terminal's name into a caller buffer.
///
/// Returns 0 on success or a **positive errno** — `ttyname_r` is one of the
/// handful of POSIX functions that report through the return value rather
/// than `errno`, and unlike our [`ptsname_r`] (which follows the local
/// `-1`/`errno` convention for consistency with its neighbours) this one
/// must not, because callers propagate the return value straight into an
/// errno slot.  CPython's `os.ttyname` does exactly that.
///
/// Errors, in the order glibc's `__ttyname_r`
/// (`sysdeps/unix/sysv/linux/ttyname_r.c`) produces them:
///
/// 1. `EINVAL` — `buf` is NULL.  glibc checks this first, before touching
///    `fd`, because it has nowhere to put an answer even if the descriptor
///    turns out to be perfect.
/// 2. `EBADF`  — `fd` is not open.
/// 3. `ENOTTY` — `fd` is open but is not a terminal.
/// 4. `ERANGE` — the name does not fit in `buflen`.  glibc reports this
///    *after* identifying the terminal, so a bad `fd` outranks a small
///    buffer: a caller growing its buffer in a loop must not be sent round
///    again for a descriptor that will never work.
///
/// The name written is whatever [`ttyname`] returns, so the two cannot come
/// to disagree about what this system calls its terminal.
///
/// One of the thirteen symbols that stopped CPython 3.12 linking against our
/// libc; see `scripts/cpython-spike/README.md`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ttyname_r(fd: i32, buf: *mut u8, buflen: usize) -> i32 {
    if buf.is_null() {
        return errno::EINVAL;
    }
    // Reuse `ttyname`'s verdict rather than re-deriving it: it sets errno to
    // EBADF or ENOTTY on the way out, and duplicating that decision here is
    // exactly how the two functions would come to disagree about which
    // descriptors are terminals.
    errno::set_errno(0);
    let name = ttyname(fd);
    if name.is_null() {
        let e = errno::get_errno();
        // `ttyname` sets errno on every failure path; the fallback exists
        // only so that a future edit which forgets to cannot turn a failure
        // into a silent success.
        return if e == 0 { errno::ENOTTY } else { e };
    }

    // SAFETY: `ttyname` returned a non-null pointer to a NUL-terminated
    // static string.
    let len = unsafe { crate::string::strlen(name) };
    // Room for the name *and* its terminator — POSIX requires `buflen` to
    // account for the NUL, and glibc's check is `len + 1 > buflen`.
    if len.wrapping_add(1) > buflen {
        return errno::ERANGE;
    }
    let mut i: usize = 0;
    while i <= len {
        // SAFETY: `i <= len`, and `buf` was just confirmed to hold at least
        // `len + 1` bytes; the source is NUL-terminated at `len`, so the
        // final iteration copies the terminator.
        unsafe {
            *buf.add(i) = *name.add(i);
        }
        i = i.wrapping_add(1);
    }
    0
}

/// Return the pathname of the controlling terminal.
///
/// If `s` is non-null, the path is copied there (must have room for
/// `L_ctermid` = 20 bytes).  If `s` is null, a pointer to a static
/// string is returned.
///
/// Our OS always uses `/dev/console` as the controlling terminal.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ctermid(s: *mut u8) -> *const u8 {
    let path = c"/dev/console";
    if s.is_null() {
        return path.as_ptr().cast::<u8>();
    }
    // Copy the path into the caller's buffer.
    let bytes = path.to_bytes_with_nul();
    let mut i: usize = 0;
    while i < bytes.len() {
        if let Some(&b) = bytes.get(i) {
            // SAFETY: i < bytes.len() = 13 <= L_ctermid (typically 20).
            unsafe {
                *s.add(i) = b;
            }
        }
        i = i.wrapping_add(1);
    }
    s.cast_const()
}

// ---------------------------------------------------------------------------
// tcgetattr() / tcsetattr() — convenience wrappers
// ---------------------------------------------------------------------------

/// Get terminal attributes.
///
/// Equivalent to `ioctl(fd, TCGETS, termios_p)`.
///
/// Returns 0 on success, -1 on error.
///
/// # Safety
///
/// `termios_p` must point to a valid `Termios` structure.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tcgetattr(fd: i32, termios_p: *mut Termios) -> i32 {
    ioctl(fd, TCGETS, termios_p.cast::<u8>())
}

/// Set terminal attributes.
///
/// `optional_actions` specifies when the change takes effect:
/// - `TCSANOW` — immediately
/// - `TCSADRAIN` — after output is transmitted
/// - `TCSAFLUSH` — after output is transmitted, discard input
///
/// Returns 0 on success, -1 on error.
///
/// # Safety
///
/// `termios_p` must point to a valid `Termios` structure.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tcsetattr(fd: i32, optional_actions: i32, termios_p: *const Termios) -> i32 {
    let request = match optional_actions {
        TCSANOW => TCSETS,
        TCSADRAIN => TCSETSW,
        TCSAFLUSH => TCSETSF,
        _ => {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
    };

    // Cast away const — the ioctl handler for TCSETS doesn't actually
    // write to the buffer, so this is safe.
    ioctl(fd, request, termios_p.cast_mut().cast::<u8>())
}

// ---------------------------------------------------------------------------
// cfgetispeed() / cfgetospeed() / cfsetispeed() / cfsetospeed()
// ---------------------------------------------------------------------------

/// Get input baud rate from termios.
///
/// # Safety
///
/// `termios_p` must be non-null and point to a valid `Termios`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn cfgetispeed(termios_p: *const Termios) -> u32 {
    if termios_p.is_null() {
        return 0;
    }
    // SAFETY: Caller guarantees termios_p is valid.
    unsafe { (*termios_p).c_ispeed }
}

/// Get output baud rate from termios.
///
/// # Safety
///
/// `termios_p` must be non-null and point to a valid `Termios`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn cfgetospeed(termios_p: *const Termios) -> u32 {
    if termios_p.is_null() {
        return 0;
    }
    // SAFETY: Caller guarantees termios_p is valid.
    unsafe { (*termios_p).c_ospeed }
}

/// Set input baud rate in termios.
///
/// Returns 0 on success, -1 on error.
///
/// A NULL `termios_p` gives `EINVAL`: `cfsetispeed` only writes a field
/// of a caller-owned `struct termios` and issues no syscall, and glibc
/// `termios/speed.c` (checked against 2.39) rejects NULL itself with
/// `if (termios_p == NULL) { __set_errno (EINVAL); return -1; }`.
///
/// # Safety
///
/// `termios_p` must be non-null and point to a valid `Termios`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn cfsetispeed(termios_p: *mut Termios, speed: u32) -> i32 {
    if termios_p.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // SAFETY: Caller guarantees termios_p is valid.
    unsafe {
        (*termios_p).c_ispeed = speed;
    }
    0
}

/// Set output baud rate in termios.
///
/// Returns 0 on success, -1 on error.  A NULL `termios_p` gives
/// `EINVAL` — see `cfsetispeed` for the glibc citation.
///
/// # Safety
///
/// `termios_p` must be non-null and point to a valid `Termios`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn cfsetospeed(termios_p: *mut Termios, speed: u32) -> i32 {
    if termios_p.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // SAFETY: Caller guarantees termios_p is valid.
    unsafe {
        (*termios_p).c_ospeed = speed;
    }
    0
}

// ---------------------------------------------------------------------------
// cfmakeraw — set raw mode
// ---------------------------------------------------------------------------

/// Configure termios for raw (non-canonical, no echo) I/O.
///
/// Clears all input/output processing flags so that bytes pass through
/// unmodified.  This is the standard way to prepare a terminal for
/// interactive programs (editors, games, TUI apps).
///
/// # Safety
///
/// `termios_p` must be non-null and point to a valid `Termios`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn cfmakeraw(termios_p: *mut Termios) {
    if termios_p.is_null() {
        return;
    }
    // SAFETY: Caller guarantees termios_p is valid.
    let t = unsafe { &mut *termios_p };

    // Input: disable break/CR/NL translation, parity, strip, flow control.
    t.c_iflag &= !(BRKINT | ICRNL | IGNCR | INLCR | INPCK | ISTRIP | IXON);

    // Output: disable post-processing.
    t.c_oflag &= !OPOST;

    // Control: clear size mask, set 8-bit, disable parity.
    t.c_cflag &= !(CSIZE | PARENB);
    t.c_cflag |= CS8;

    // Local: disable canonical mode, echo, signals, extended processing.
    t.c_lflag &= !(ECHO | ECHONL | ICANON | ISIG | IEXTEN);

    // Set VMIN=1, VTIME=0 for byte-at-a-time reads.
    if let Some(slot) = t.c_cc.get_mut(VMIN) {
        *slot = 1;
    }
    if let Some(slot) = t.c_cc.get_mut(VTIME) {
        *slot = 0;
    }
}

/// Set both input and output baud rate in termios.
///
/// Convenience function (non-POSIX but widely available).
///
/// Returns 0 on success, -1 on error.  A NULL `termios_p` sets `EINVAL`.
///
/// **Deliberate divergence in the return value.** glibc's `cfsetspeed`
/// (`termios/cfsetspeed.c`) forwards to `cfsetispeed`/`cfsetospeed` and
/// *ignores their return values*, so for a recognised `speed` it returns
/// **0** on a NULL `termios_p` while leaving `errno` set to `EINVAL` —
/// a silent failure the caller has no reason to look for. We report the
/// same errno but return -1, because copying a "succeeded, except it
/// didn't" answer would turn a caller's bug into corrupted state later.
/// A caller that checks the return value is helped; one that ignores it
/// is no worse off than on glibc.
///
/// # Safety
///
/// `termios_p` must be non-null and point to a valid `Termios`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn cfsetspeed(termios_p: *mut Termios, speed: u32) -> i32 {
    if termios_p.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // SAFETY: Caller guarantees termios_p is valid.
    unsafe {
        (*termios_p).c_ispeed = speed;
        (*termios_p).c_ospeed = speed;
    }
    0
}

// ---------------------------------------------------------------------------
// tcsendbreak / tcdrain / tcflow / tcflush
// ---------------------------------------------------------------------------

/// Send a break condition on a terminal.
///
/// Our console doesn't have a serial break concept, so this is a
/// no-op on valid terminal fds.  Returns -1 with `EBADF` for invalid
/// fds or `ENOTTY` for non-terminal fds.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tcsendbreak(fd: i32, _duration: i32) -> i32 {
    if let Err(e) = validate_terminal_fd(fd) {
        errno::set_errno(e);
        return -1;
    }
    0
}

/// Wait until all output has been transmitted.
///
/// Our console writes are synchronous (framebuffer-backed), so there
/// is no pending output to drain.  Returns 0 immediately for valid
/// terminal fds, -1 with `ENOTTY` for non-terminal fds.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tcdrain(fd: i32) -> i32 {
    if let Err(e) = validate_terminal_fd(fd) {
        errno::set_errno(e);
        return -1;
    }
    0
}

/// TCOON — restart suspended output.
pub const TCOON: i32 = 0;
/// TCOOFF — suspend output.
pub const TCOOFF: i32 = 1;
/// TCION — restart suspended input.
pub const TCION: i32 = 2;
/// TCIOFF — suspend input.
pub const TCIOFF: i32 = 3;

/// Suspend or restart terminal I/O.
///
/// Our console doesn't support XON/XOFF flow control.  Validates that
/// `fd` refers to a terminal and `action` is a known constant.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tcflow(fd: i32, action: i32) -> i32 {
    if let Err(e) = validate_terminal_fd(fd) {
        errno::set_errno(e);
        return -1;
    }
    if !(TCOON..=TCIOFF).contains(&action) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    0
}

/// TCIFLUSH — flush pending input.
pub const TCIFLUSH: i32 = 0;
/// TCOFLUSH — flush pending output.
pub const TCOFLUSH: i32 = 1;
/// TCIOFLUSH — flush both input and output.
pub const TCIOFLUSH: i32 = 2;

/// Discard pending terminal I/O data.
///
/// Our console doesn't buffer data beyond the framebuffer, so there
/// is nothing to flush.  Validates `fd` is a terminal and
/// `queue_selector` is a known constant.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tcflush(fd: i32, queue_selector: i32) -> i32 {
    if let Err(e) = validate_terminal_fd(fd) {
        errno::set_errno(e);
        return -1;
    }
    if !(TCIFLUSH..=TCIOFLUSH).contains(&queue_selector) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    0
}

/// Validate that `fd` is an open terminal.
///
/// Returns `Ok(())` if the fd is valid and refers to a Console,
/// `Err(EBADF)` if the fd is invalid, or `Err(ENOTTY)` if it's
/// not a terminal.
fn validate_terminal_fd(fd: i32) -> Result<(), i32> {
    let Some(entry) = fdtable::get_fd(fd) else {
        return Err(errno::EBADF);
    };
    if entry.kind != HandleKind::Console {
        return Err(errno::ENOTTY);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// tcgetsid — get session ID for terminal
// ---------------------------------------------------------------------------

/// Get the session ID associated with a terminal.
///
/// Returns the session ID of the foreground process group's session
/// for the terminal referenced by `fd`.
///
/// Our OS does not have full session management, so this returns the
/// process's own session ID (via `getsid(0)`).  Returns -1 with
/// `EBADF` for invalid fds or `ENOTTY` for non-terminal fds.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tcgetsid(fd: i32) -> i32 {
    if let Err(e) = validate_terminal_fd(fd) {
        errno::set_errno(e);
        return -1;
    }
    // Return the calling process's session ID.
    crate::process::getsid(0)
}

// ---------------------------------------------------------------------------
// Tests — pure logic functions only (no syscalls)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Structure size tests --

    #[test]
    fn test_winsize_size() {
        // Winsize should be 8 bytes (4 × u16).
        assert_eq!(core::mem::size_of::<Winsize>(), 8);
    }

    #[test]
    fn test_termios_size() {
        // Termios layout: c_iflag(4) + c_oflag(4) + c_cflag(4) + c_lflag(4) +
        // c_line(1) + c_cc(32) + padding(3) + c_ispeed(4) + c_ospeed(4) = 60.
        let size = core::mem::size_of::<Termios>();
        assert_eq!(size, 60, "Termios size mismatch");
    }

    // -- Kernel termios marshalling --
    //
    // These cover the *translation* between musl's 60-byte user `struct
    // termios` and the kernel's 36-byte wire format.  They cannot prove the
    // kernel agrees about the layout — the host build has no kernel — which
    // is why `KERNEL_TERMIOS_BYTES` is also asserted against the kernel's own
    // constant in the ring-3 fixture.

    #[test]
    fn test_kernel_termios_wire_size() {
        // Must equal kernel/src/tty.rs's TERMIOS_BYTES (4*4 + 1 + 19).
        assert_eq!(KERNEL_TERMIOS_BYTES, 36);
        assert_eq!(KERNEL_NCCS, 19);
        // The user struct is deliberately the larger of the two.
        assert!(NCCS > KERNEL_NCCS);
    }

    #[test]
    fn test_termios_wire_roundtrip() {
        let t = default_termios();
        let back = termios_from_wire(&termios_to_wire(&t));
        assert_eq!(back.c_iflag, t.c_iflag);
        assert_eq!(back.c_oflag, t.c_oflag);
        assert_eq!(back.c_lflag, t.c_lflag);
        assert_eq!(back.c_line, t.c_line);
        // Every control character the kernel carries survives.
        for i in 0..KERNEL_NCCS {
            assert_eq!(back.c_cc.get(i), t.c_cc.get(i), "c_cc[{i}] lost");
        }
        // The baud rate round-trips through c_cflag's CBAUD bits.
        assert_eq!(back.c_ospeed, B38400);
        assert_eq!(back.c_ispeed, B38400);
        assert_eq!(back.c_cflag & !CBAUD, t.c_cflag & !CBAUD);
    }

    #[test]
    fn test_termios_wire_field_offsets() {
        // Pin the wire layout: four LE u32 flag words, c_line, then c_cc.
        // A mismatch here is a silent ABI break with the kernel.
        let mut t = default_termios();
        t.c_iflag = 0x1111_1111;
        t.c_oflag = 0x2222_2222;
        // Deliberately holds no CBAUD bits of its own, so the only change
        // to c_cflag is the speed folded in below.  That keeps this an
        // offset test: if the pattern carried CBAUD bits, a failure here
        // could equally mean "the fields moved" or "the folding is wrong".
        t.c_cflag = 0x3333_2320;
        t.c_lflag = 0x4444_4444;
        t.c_line = 7;
        t.c_ospeed = B38400;
        let w = termios_to_wire(&t);
        assert_eq!(&w[0..4], &0x1111_1111u32.to_le_bytes());
        assert_eq!(&w[4..8], &0x2222_2222u32.to_le_bytes());
        // The speed rides in c_cflag's low bits — the kernel wire format has
        // no c_ospeed field to put it in.
        assert_eq!(&w[8..12], &(0x3333_2320u32 | B38400).to_le_bytes());
        assert_eq!(&w[12..16], &0x4444_4444u32.to_le_bytes());
        assert_eq!(w[16], 7);
        // c_cc starts at byte 17 — VINTR is index 0 and is Ctrl-C.
        assert_eq!(w[17 + VINTR], 0x03);
        assert_eq!(w[17 + VSUSP], 0x1A);
    }

    #[test]
    fn test_termios_wire_drops_extra_cc_slots() {
        // The user array's slots past the kernel's 19 have nowhere to go;
        // they must be dropped silently rather than corrupting the tail.
        let mut t = default_termios();
        if let Some(slot) = t.c_cc.get_mut(KERNEL_NCCS) {
            *slot = 0xAB;
        }
        let w = termios_to_wire(&t);
        assert_eq!(w.len(), KERNEL_TERMIOS_BYTES);
        let back = termios_from_wire(&w);
        assert_eq!(back.c_cc.get(KERNEL_NCCS), Some(&0));
    }

    #[test]
    fn test_tcsetattr_tcgetattr_roundtrip_raw_mode() {
        // The behaviour that was broken: entering raw mode has to be
        // *observable*.  tcsetattr used to be a silent no-op and tcgetattr
        // used to answer from a constant, so this pair could never fail.
        let mut raw = default_termios();
        raw.c_lflag &= !(ICANON | ECHO);
        assert!(set_kernel_termios(CTTY, &raw), "set_kernel_termios failed");
        let got = get_kernel_termios(CTTY).expect("get_kernel_termios failed");
        assert_eq!(got.c_lflag & ICANON, 0, "ICANON survived a raw-mode set");
        assert_eq!(got.c_lflag & ECHO, 0, "ECHO survived a raw-mode set");
        // And going back to cooked mode is equally visible.
        assert!(set_kernel_termios(CTTY, &default_termios()));
        let cooked = get_kernel_termios(CTTY).expect("get_kernel_termios failed");
        assert_ne!(cooked.c_lflag & ICANON, 0);
        assert_ne!(cooked.c_lflag & ECHO, 0);
    }

    // -- Default terminal dimensions --

    #[test]
    fn test_default_winsize() {
        assert_eq!(DEFAULT_WINSIZE.ws_row, 25);
        assert_eq!(DEFAULT_WINSIZE.ws_col, 80);
    }

    // -- Default termios --

    #[test]
    fn test_default_termios_canonical() {
        let t = default_termios();
        // Should be in canonical mode with echo.
        assert_ne!(t.c_lflag & ICANON, 0, "Should be canonical");
        assert_ne!(t.c_lflag & ECHO, 0, "Should have echo");
        assert_ne!(t.c_lflag & ISIG, 0, "Should have signals");
    }

    #[test]
    fn test_default_termios_cr_nl() {
        let t = default_termios();
        // Input: CR→NL translation.
        assert_ne!(t.c_iflag & ICRNL, 0, "Should translate CR→NL");
        // Output: NL→CRNL + post-processing.
        assert_ne!(t.c_oflag & OPOST, 0, "Should post-process output");
        assert_ne!(t.c_oflag & ONLCR, 0, "Should map NL→CRNL");
    }

    #[test]
    fn test_default_termios_8bit() {
        let t = default_termios();
        assert_eq!(t.c_cflag & CSIZE, CS8, "Should be 8-bit");
    }

    #[test]
    fn test_default_termios_control_chars() {
        let t = default_termios();
        assert_eq!(t.c_cc[VINTR], 0x03, "Ctrl-C");
        assert_eq!(t.c_cc[VQUIT], 0x1C, "Ctrl-\\");
        assert_eq!(t.c_cc[VERASE], 0x7F, "DEL");
        assert_eq!(t.c_cc[VKILL], 0x15, "Ctrl-U");
        assert_eq!(t.c_cc[VEOF], 0x04, "Ctrl-D");
        assert_eq!(t.c_cc[VSUSP], 0x1A, "Ctrl-Z");
    }

    // -- Baud rate helper tests --

    #[test]
    fn test_cfget_set_speed() {
        let mut t = default_termios();
        assert_eq!(unsafe { cfgetispeed(&raw const t) }, B38400);
        assert_eq!(unsafe { cfgetospeed(&raw const t) }, B38400);

        assert_eq!(unsafe { cfsetispeed(&raw mut t, B115200) }, 0);
        assert_eq!(unsafe { cfsetospeed(&raw mut t, B9600) }, 0);

        assert_eq!(unsafe { cfgetispeed(&raw const t) }, B115200);
        assert_eq!(unsafe { cfgetospeed(&raw const t) }, B9600);
    }

    #[test]
    fn test_cfget_null() {
        assert_eq!(unsafe { cfgetispeed(core::ptr::null()) }, 0);
        assert_eq!(unsafe { cfgetospeed(core::ptr::null()) }, 0);
    }

    #[test]
    fn test_cfset_null() {
        // EINVAL, not EFAULT: these write a caller-owned struct and issue
        // no syscall; glibc termios/speed.c checks NULL itself.
        errno::set_errno(0);
        assert_eq!(unsafe { cfsetispeed(core::ptr::null_mut(), 0) }, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        errno::set_errno(0);
        assert_eq!(unsafe { cfsetospeed(core::ptr::null_mut(), 0) }, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -- ioctl request code tests --

    #[test]
    fn test_ioctl_constants_match_linux() {
        // These must match Linux x86_64 values for compatibility.
        assert_eq!(TIOCGWINSZ, 0x5413);
        assert_eq!(TIOCSWINSZ, 0x5414);
        assert_eq!(TCGETS, 0x5401);
        assert_eq!(TCSETS, 0x5402);
        assert_eq!(FIONBIO, 0x5421);
        assert_eq!(FIONREAD, 0x541B);
    }

    #[test]
    fn test_tty_control_constants_match_linux() {
        assert_eq!(TIOCSCTTY, 0x540E);
        assert_eq!(TIOCGPGRP, 0x540F);
        assert_eq!(TIOCSPGRP, 0x5410);
        assert_eq!(TIOCNOTTY, 0x5422);
    }

    #[test]
    fn test_tcsetsw_tcsetsf_values() {
        assert_eq!(TCSETSW, 0x5403);
        assert_eq!(TCSETSF, 0x5404);
    }

    // -- cfmakeraw tests --

    #[test]
    fn test_cfmakeraw_clears_flags() {
        let mut t = default_termios();
        // Starts in canonical + echo mode.
        assert_ne!(t.c_lflag & ICANON, 0);
        assert_ne!(t.c_lflag & ECHO, 0);
        assert_ne!(t.c_iflag & ICRNL, 0);
        assert_ne!(t.c_oflag & OPOST, 0);

        unsafe {
            cfmakeraw(&raw mut t);
        }

        // After raw: no canonical, no echo, no input/output processing.
        assert_eq!(t.c_lflag & ICANON, 0, "ICANON should be cleared");
        assert_eq!(t.c_lflag & ECHO, 0, "ECHO should be cleared");
        assert_eq!(t.c_lflag & ISIG, 0, "ISIG should be cleared");
        assert_eq!(t.c_iflag & ICRNL, 0, "ICRNL should be cleared");
        assert_eq!(t.c_oflag & OPOST, 0, "OPOST should be cleared");
        assert_eq!(t.c_cflag & CSIZE, CS8, "Should be 8-bit");
    }

    #[test]
    fn test_cfmakeraw_vmin_vtime() {
        let mut t = default_termios();
        unsafe {
            cfmakeraw(&raw mut t);
        }
        assert_eq!(t.c_cc[VMIN], 1, "VMIN should be 1");
        assert_eq!(t.c_cc[VTIME], 0, "VTIME should be 0");
    }

    #[test]
    fn test_cfsetspeed() {
        let mut t = default_termios();
        assert_eq!(unsafe { cfsetspeed(&raw mut t, B115200) }, 0);
        assert_eq!(unsafe { cfgetispeed(&raw const t) }, B115200);
        assert_eq!(unsafe { cfgetospeed(&raw const t) }, B115200);
    }

    #[test]
    fn test_cfsetspeed_null() {
        // errno matches glibc (EINVAL); the -1 return is a deliberate
        // divergence — glibc returns 0 here while setting EINVAL, because
        // cfsetspeed discards cfsetispeed/cfsetospeed's return values.
        // See the doc comment on `cfsetspeed`.
        errno::set_errno(0);
        assert_eq!(unsafe { cfsetspeed(core::ptr::null_mut(), B9600) }, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -- ctermid tests --

    #[test]
    fn test_ctermid_null_returns_static() {
        let ptr = ctermid(core::ptr::null_mut());
        assert!(!ptr.is_null());
        // Should be "/dev/console".
        let slice = unsafe {
            let len = crate::string::strlen(ptr);
            core::slice::from_raw_parts(ptr, len)
        };
        assert_eq!(slice, b"/dev/console");
    }

    #[test]
    fn test_ctermid_copies_to_buffer() {
        let mut buf = [0xFFu8; 20];
        let ptr = ctermid(buf.as_mut_ptr());
        assert_eq!(ptr, buf.as_ptr());
        // Should have written "/dev/console\0".
        assert_eq!(&buf[..13], b"/dev/console\0");
    }

    // -- isatty tests (use pre-initialized Console fds 0/1/2) --

    /// Ensure fds 0/1/2 are Console handles.
    ///
    /// Other tests may close or overwrite these fds; this restores
    /// the expected state before tests that depend on console fds.
    fn ensure_std_fds() {
        let _ = fdtable::install_fd(0, HandleKind::Console, 0);
        let _ = fdtable::install_fd(1, HandleKind::Console, 1);
        let _ = fdtable::install_fd(2, HandleKind::Console, 2);
    }

    #[test]
    fn test_isatty_stdin() {
        ensure_std_fds();
        assert_eq!(isatty(0), 1, "fd 0 (stdin) is Console → isatty");
    }

    #[test]
    fn test_isatty_stdout() {
        ensure_std_fds();
        assert_eq!(isatty(1), 1, "fd 1 (stdout) is Console → isatty");
    }

    #[test]
    fn test_isatty_stderr() {
        ensure_std_fds();
        assert_eq!(isatty(2), 1, "fd 2 (stderr) is Console → isatty");
    }

    #[test]
    fn test_isatty_invalid_fd() {
        assert_eq!(isatty(-1), 0);
    }

    #[test]
    fn test_isatty_non_terminal_fd() {
        // Allocate a File fd — isatty should return 0.
        let fd = fdtable::alloc_fd(HandleKind::File, 100).unwrap();
        assert_eq!(isatty(fd), 0);
        let _ = fdtable::close_fd(fd);
    }

    // -- ttyname tests --

    #[test]
    fn test_ttyname_console() {
        ensure_std_fds();
        let ptr = ttyname(0);
        assert!(!ptr.is_null());
        let slice = unsafe {
            let len = crate::string::strlen(ptr);
            core::slice::from_raw_parts(ptr, len)
        };
        assert_eq!(slice, b"/dev/console");
    }

    #[test]
    fn test_ttyname_invalid_fd() {
        assert!(ttyname(-1).is_null());
    }

    #[test]
    fn test_ttyname_non_terminal() {
        let fd = fdtable::alloc_fd(HandleKind::Pipe, 50).unwrap();
        assert!(ttyname(fd).is_null());
        let _ = fdtable::close_fd(fd);
    }

    // -- ttyname_r tests --
    //
    // Note the return convention: `ttyname_r` reports a *positive errno*,
    // not -1.  Every assertion below is written against that, because a
    // regression to the -1 convention would otherwise read as "some
    // nonzero failure" and pass.

    #[test]
    fn test_ttyname_r_console_writes_the_same_name_as_ttyname() {
        ensure_std_fds();
        let mut buf = [0xAAu8; 32];
        assert_eq!(ttyname_r(0, buf.as_mut_ptr(), buf.len()), 0);
        let len = unsafe { crate::string::strlen(buf.as_ptr()) };
        assert_eq!(&buf[..len], b"/dev/console");
        // NUL-terminated, and nothing written past the terminator.
        assert_eq!(buf[len], 0);
        assert_eq!(buf[len + 1], 0xAA);
    }

    /// A NULL buffer is EINVAL and outranks the descriptor: glibc checks
    /// it first because it has nowhere to put an answer even for a
    /// perfect fd.  Note this is the opposite precedence from our
    /// `ptsname_r`, which follows glibc's `__ptsname_r` — the two glibc
    /// functions genuinely differ, and so do we.
    #[test]
    fn test_ttyname_r_null_buf_is_einval_even_for_a_good_fd() {
        ensure_std_fds();
        assert_eq!(ttyname_r(0, core::ptr::null_mut(), 32), errno::EINVAL);
        assert_eq!(ttyname_r(-1, core::ptr::null_mut(), 32), errno::EINVAL);
    }

    #[test]
    fn test_ttyname_r_bad_fd_is_ebadf() {
        let mut buf = [0u8; 32];
        assert_eq!(ttyname_r(-1, buf.as_mut_ptr(), buf.len()), errno::EBADF);
    }

    #[test]
    fn test_ttyname_r_non_terminal_is_enotty() {
        let fd = fdtable::alloc_fd(HandleKind::Pipe, 51).unwrap();
        let mut buf = [0u8; 32];
        assert_eq!(ttyname_r(fd, buf.as_mut_ptr(), buf.len()), errno::ENOTTY);
        let _ = fdtable::close_fd(fd);
    }

    /// A buffer too small for the name *and its terminator* is ERANGE,
    /// and nothing is written — a caller growing its buffer in a loop
    /// must not read a truncated name as if it were complete.
    /// `/dev/console` is 12 bytes, so 12 is one short and 13 is exact.
    #[test]
    fn test_ttyname_r_small_buffer_is_erange_and_writes_nothing() {
        ensure_std_fds();
        let mut buf = [0xAAu8; 32];
        assert_eq!(ttyname_r(0, buf.as_mut_ptr(), 12), errno::ERANGE);
        assert!(buf.iter().all(|&b| b == 0xAA), "ERANGE must not write");

        assert_eq!(ttyname_r(0, buf.as_mut_ptr(), 13), 0);
        let len = unsafe { crate::string::strlen(buf.as_ptr()) };
        assert_eq!(&buf[..len], b"/dev/console");
    }

    /// A zero-length buffer cannot hold even the terminator.  Guarded
    /// separately because `len + 1 > 0` is exactly the arithmetic most
    /// likely to be got wrong.
    #[test]
    fn test_ttyname_r_zero_length_buffer_is_erange() {
        ensure_std_fds();
        let mut buf = [0xAAu8; 4];
        assert_eq!(ttyname_r(0, buf.as_mut_ptr(), 0), errno::ERANGE);
        assert!(buf.iter().all(|&b| b == 0xAA));
    }

    /// The bad-fd verdict outranks the too-small-buffer one: a caller
    /// that keeps growing its buffer must be told the descriptor is
    /// hopeless rather than sent round the loop forever.
    #[test]
    fn test_ttyname_r_bad_fd_outranks_erange() {
        let mut buf = [0u8; 1];
        assert_eq!(ttyname_r(-1, buf.as_mut_ptr(), 1), errno::EBADF);
    }

    // -- tcsetattr action constant validation --

    #[test]
    fn test_tcsetattr_action_constants() {
        assert_eq!(TCSANOW, 0);
        assert_eq!(TCSADRAIN, 1);
        assert_eq!(TCSAFLUSH, 2);
    }

    // -- tcflow / tcflush action constants --

    #[test]
    fn test_tcflow_action_constants() {
        assert_eq!(TCOON, 0);
        assert_eq!(TCOOFF, 1);
        assert_eq!(TCION, 2);
        assert_eq!(TCIOFF, 3);
    }

    #[test]
    fn test_tcflush_action_constants() {
        assert_eq!(TCIFLUSH, 0);
        assert_eq!(TCOFLUSH, 1);
        assert_eq!(TCIOFLUSH, 2);
    }

    // -- Default termios baud rates --

    #[test]
    fn test_default_termios_baud() {
        let t = default_termios();
        assert_eq!(t.c_ispeed, B38400);
        assert_eq!(t.c_ospeed, B38400);
    }

    // -- Baud rate constants --

    #[test]
    fn test_baud_rate_constants() {
        // Values must match Linux octal definitions.
        assert_eq!(B9600, 0o15);
        assert_eq!(B19200, 0o16);
        assert_eq!(B38400, 0o17);
        assert_eq!(B115200, 0o10002);
    }

    // -- c_cc index constants --

    #[test]
    fn test_cc_index_constants() {
        assert_eq!(VINTR, 0);
        assert_eq!(VQUIT, 1);
        assert_eq!(VERASE, 2);
        assert_eq!(VKILL, 3);
        assert_eq!(VEOF, 4);
        assert_eq!(VTIME, 5);
        assert_eq!(VMIN, 6);
        assert_eq!(VSTART, 8);
        assert_eq!(VSTOP, 9);
        assert_eq!(VSUSP, 10);
        assert_eq!(VEOL, 11);
        assert_eq!(NCCS, 32);
    }

    // -- PTY stubs --

    #[test]
    fn test_posix_openpt_returns_enosys() {
        assert_eq!(posix_openpt(0), -1);
    }

    #[test]
    fn test_grantpt_succeeds() {
        // Phase 68: grantpt no longer silently returns 0 — it now
        // validates the fd.  fd=0 must be a registered (non-PTY-master)
        // fd for the validator to skip EBADF and report EINVAL.  We
        // install it explicitly here rather than rely on a previous
        // test having called `ensure_std_fds()`, because parallel test
        // ordering may close fd 0 before we run.
        ensure_std_fds();
        crate::errno::set_errno(0);
        assert_eq!(grantpt(0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_unlockpt_succeeds() {
        // Phase 68: unlockpt no longer silently returns 0.  See
        // test_grantpt_succeeds for the EINVAL reasoning and why we
        // install fd 0 explicitly.
        ensure_std_fds();
        crate::errno::set_errno(0);
        assert_eq!(unlockpt(0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_ptsname_returns_null() {
        // ptsname still returns NULL; the validator now also sets
        // errno so callers can tell EBADF (closed fd) from ENOTTY
        // (open but not a PTY).
        assert!(ptsname(0).is_null());
    }

    #[test]
    fn test_ptsname_r_returns_enosys() {
        // Phase 68: ptsname_r(0, valid_buf, 64) now reports ENOTTY
        // because fd=0 is open (ensure_std_fds in test env) but is
        // not a PTY master.  The original test was checking the
        // unconditional ENOSYS sentinel; updated to match the
        // post-validator Linux-correct errno.
        let mut buf = [0u8; 64];
        crate::errno::set_errno(0);
        assert_eq!(ptsname_r(0, buf.as_mut_ptr(), buf.len()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOTTY);
    }

    // -- validate_terminal_fd --

    #[test]
    fn test_validate_terminal_fd_console() {
        ensure_std_fds();
        assert!(validate_terminal_fd(0).is_ok());
        assert!(validate_terminal_fd(1).is_ok());
        assert!(validate_terminal_fd(2).is_ok());
    }

    #[test]
    fn test_validate_terminal_fd_invalid() {
        assert_eq!(validate_terminal_fd(-1), Err(crate::errno::EBADF));
    }

    #[test]
    fn test_validate_terminal_fd_non_terminal() {
        let fd = fdtable::alloc_fd(HandleKind::File, 200).unwrap();
        assert_eq!(validate_terminal_fd(fd), Err(crate::errno::ENOTTY));
        let _ = fdtable::close_fd(fd);
    }

    // -- cfmakeraw does not crash on null --

    #[test]
    fn test_cfmakeraw_null() {
        // Should silently return without crashing.
        unsafe {
            cfmakeraw(core::ptr::null_mut());
        }
    }

    // -- cfmakeraw clears parity --

    #[test]
    fn test_cfmakeraw_clears_parity() {
        let mut t = default_termios();
        t.c_cflag |= PARENB;
        unsafe {
            cfmakeraw(&raw mut t);
        }
        assert_eq!(
            t.c_cflag & PARENB,
            0,
            "PARENB should be cleared in raw mode"
        );
    }

    // -- tcsendbreak / tcdrain --

    #[test]
    fn test_tcsendbreak_invalid_fd() {
        assert_eq!(tcsendbreak(9999, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_tcdrain_invalid_fd() {
        assert_eq!(tcdrain(9999), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // -- ioctl() through-function tests on console fds --

    #[test]
    fn test_ioctl_tiocgwinsz_console() {
        ensure_std_fds();
        let mut ws = Winsize {
            ws_row: 0,
            ws_col: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let ret = ioctl(0, TIOCGWINSZ, (&raw mut ws).cast::<u8>());
        assert_eq!(ret, 0);
        assert_eq!(ws.ws_row, 25);
        assert_eq!(ws.ws_col, 80);
    }

    #[test]
    fn test_ioctl_tiocgwinsz_null_arg() {
        ensure_std_fds();
        let ret = ioctl(0, TIOCGWINSZ, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_ioctl_tiocswinsz_console() {
        ensure_std_fds();
        let ws = Winsize {
            ws_row: 50,
            ws_col: 120,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let ret = ioctl(0, TIOCSWINSZ, (&raw const ws).cast::<u8>().cast_mut());
        assert_eq!(ret, 0, "TIOCSWINSZ on console should succeed (no-op)");
    }

    #[test]
    fn test_ioctl_tiocswinsz_non_console() {
        let fd = fdtable::alloc_fd(HandleKind::File, 300).unwrap();
        let ret = ioctl(fd, TIOCSWINSZ, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOTTY);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_ioctl_tiocgwinsz_non_console() {
        let fd = fdtable::alloc_fd(HandleKind::Pipe, 301).unwrap();
        let mut ws = Winsize {
            ws_row: 0,
            ws_col: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let ret = ioctl(fd, TIOCGWINSZ, (&raw mut ws).cast::<u8>());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOTTY);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_ioctl_fionbio_enable() {
        ensure_std_fds();
        let enable: i32 = 1;
        let ret = ioctl(0, FIONBIO, (&raw const enable).cast::<u8>().cast_mut());
        assert_eq!(ret, 0);
        // Check that O_NONBLOCK is now set.
        let flags = fdtable::get_status_flags(0).unwrap_or(0);
        assert_ne!(
            flags & crate::fcntl::O_NONBLOCK,
            0,
            "O_NONBLOCK should be set"
        );
        // Restore: disable nonblock.
        let disable: i32 = 0;
        let _ = ioctl(0, FIONBIO, (&raw const disable).cast::<u8>().cast_mut());
    }

    #[test]
    fn test_ioctl_fionbio_disable() {
        ensure_std_fds();
        // First enable.
        let enable: i32 = 1;
        let _ = ioctl(0, FIONBIO, (&raw const enable).cast::<u8>().cast_mut());
        // Then disable.
        let disable: i32 = 0;
        let ret = ioctl(0, FIONBIO, (&raw const disable).cast::<u8>().cast_mut());
        assert_eq!(ret, 0);
        let flags = fdtable::get_status_flags(0).unwrap_or(crate::fcntl::O_NONBLOCK);
        assert_eq!(
            flags & crate::fcntl::O_NONBLOCK,
            0,
            "O_NONBLOCK should be clear"
        );
    }

    #[test]
    fn test_ioctl_fionbio_null_arg() {
        ensure_std_fds();
        let ret = ioctl(0, FIONBIO, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_ioctl_fionread_console() {
        ensure_std_fds();
        let mut avail: i32 = -1;
        let ret = ioctl(0, FIONREAD, (&raw mut avail).cast::<u8>());
        assert_eq!(ret, 0);
        assert_eq!(avail, 0, "Console FIONREAD should return 0");
    }

    #[test]
    fn test_ioctl_fionread_null_arg() {
        ensure_std_fds();
        let ret = ioctl(0, FIONREAD, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_ioctl_fionread_file() {
        let fd = fdtable::alloc_fd(HandleKind::File, 302).unwrap();
        let mut avail: i32 = 0;
        let ret = ioctl(fd, FIONREAD, (&raw mut avail).cast::<u8>());
        assert_eq!(ret, -1, "FIONREAD on File → ENOTTY");
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOTTY);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_ioctl_tcgets_console() {
        ensure_std_fds();
        let mut t = core::mem::MaybeUninit::<Termios>::uninit();
        let ret = ioctl(0, TCGETS, t.as_mut_ptr().cast::<u8>());
        assert_eq!(ret, 0);
        let t = unsafe { t.assume_init() };
        // Should be default canonical mode.
        assert_ne!(t.c_lflag & ICANON, 0);
        assert_ne!(t.c_lflag & ECHO, 0);
        assert_eq!(t.c_ispeed, B38400);
        assert_eq!(t.c_ospeed, B38400);
    }

    #[test]
    fn test_ioctl_tcgets_null_arg() {
        ensure_std_fds();
        let ret = ioctl(0, TCGETS, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_ioctl_tcgets_non_console() {
        let fd = fdtable::alloc_fd(HandleKind::Pipe, 303).unwrap();
        let mut t = core::mem::MaybeUninit::<Termios>::uninit();
        let ret = ioctl(fd, TCGETS, t.as_mut_ptr().cast::<u8>());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOTTY);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_ioctl_tcsets_console() {
        ensure_std_fds();
        let t = default_termios();
        let ret = ioctl(0, TCSETS, (&raw const t).cast::<u8>().cast_mut());
        assert_eq!(ret, 0, "TCSETS on console should succeed");
    }

    #[test]
    fn test_ioctl_tcsetsw_console() {
        ensure_std_fds();
        let t = default_termios();
        let ret = ioctl(0, TCSETSW, (&raw const t).cast::<u8>().cast_mut());
        assert_eq!(ret, 0, "TCSETSW on console should succeed");
    }

    #[test]
    fn test_ioctl_tcsetsf_console() {
        ensure_std_fds();
        let t = default_termios();
        let ret = ioctl(0, TCSETSF, (&raw const t).cast::<u8>().cast_mut());
        assert_eq!(ret, 0, "TCSETSF on console should succeed");
    }

    #[test]
    fn test_ioctl_tcsets_non_console() {
        let fd = fdtable::alloc_fd(HandleKind::File, 304).unwrap();
        let t = default_termios();
        let ret = ioctl(fd, TCSETS, (&raw const t).cast::<u8>().cast_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOTTY);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_ioctl_tiocsctty_console() {
        ensure_std_fds();
        assert_eq!(ioctl(0, TIOCSCTTY, core::ptr::null_mut()), 0);
    }

    #[test]
    fn test_ioctl_tiocnotty_console() {
        ensure_std_fds();
        assert_eq!(ioctl(0, TIOCNOTTY, core::ptr::null_mut()), 0);
    }

    #[test]
    fn test_ioctl_tiocgpgrp_console() {
        ensure_std_fds();
        // First set a known pgrp so we read a deterministic value.
        let set_val: i32 = 100;
        let _ = ioctl(0, TIOCSPGRP, (&raw const set_val).cast::<u8>().cast_mut());
        let mut pgrp: i32 = -999;
        let ret = ioctl(0, TIOCGPGRP, (&raw mut pgrp).cast::<u8>());
        assert_eq!(ret, 0);
        assert_eq!(pgrp, 100, "Should read back the pgrp we set");
    }

    // TIOCSCTTY/TIOCNOTTY used to be accepted silently on any fd. They are
    // real now, so the terminal-ness of the fd and the presence of a
    // controlling terminal both have to be reportable.

    #[test]
    fn test_ioctl_tiocnotty_twice_is_enotty() {
        ensure_std_fds();
        // First release succeeds (the lazy seed gives us a terminal)...
        assert_eq!(ioctl(0, TIOCNOTTY, core::ptr::null_mut()), 0);
        // ...the second has nothing to release.
        assert_eq!(ioctl(0, TIOCNOTTY, core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOTTY);
    }

    #[test]
    fn test_ioctl_tiocsctty_reattaches_after_notty() {
        ensure_std_fds();
        assert_eq!(ioctl(0, TIOCNOTTY, core::ptr::null_mut()), 0);
        let mut pgrp: i32 = -999;
        // With no controlling terminal, reading the foreground group is an
        // error — not a stale value written into the caller's buffer.
        assert_eq!(ioctl(0, TIOCGPGRP, (&raw mut pgrp).cast::<u8>()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOTTY);
        assert_eq!(pgrp, -999, "a failed TIOCGPGRP must not write the buffer");
        // Claiming it back restores service.
        assert_eq!(ioctl(0, TIOCSCTTY, core::ptr::null_mut()), 0);
        assert_eq!(ioctl(0, TIOCGPGRP, (&raw mut pgrp).cast::<u8>()), 0);
        assert!(pgrp > 0);
    }

    #[test]
    fn test_ioctl_tiocsctty_non_console_is_enotty() {
        // A pipe is not a terminal; claiming it must not silently succeed.
        let probe = 61;
        let _ = fdtable::install_fd(probe, HandleKind::Pipe, 0);
        assert_eq!(ioctl(probe, TIOCSCTTY, core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOTTY);
        assert_eq!(ioctl(probe, TIOCNOTTY, core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOTTY);
        // The probe fd's handle is a dummy, so the returned entry has
        // nothing to release.
        let _ = crate::fdtable::close_fd(probe);
    }

    #[test]
    fn test_ioctl_tiocgpgrp_null_arg() {
        ensure_std_fds();
        let ret = ioctl(0, TIOCGPGRP, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_ioctl_tiocspgrp_console() {
        ensure_std_fds();
        let pgrp: i32 = 42;
        let ret = ioctl(0, TIOCSPGRP, (&raw const pgrp).cast::<u8>().cast_mut());
        assert_eq!(ret, 0);
        // Verify round-trip: read it back.
        let mut read_pgrp: i32 = 0;
        let ret2 = ioctl(0, TIOCGPGRP, (&raw mut read_pgrp).cast::<u8>());
        assert_eq!(ret2, 0);
        assert_eq!(read_pgrp, 42, "pgrp round-trip should match");
    }

    #[test]
    fn test_ioctl_tiocspgrp_null_arg() {
        ensure_std_fds();
        let ret = ioctl(0, TIOCSPGRP, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_ioctl_tiocgpgrp_non_console() {
        let fd = fdtable::alloc_fd(HandleKind::File, 305).unwrap();
        let mut pgrp: i32 = 0;
        let ret = ioctl(fd, TIOCGPGRP, (&raw mut pgrp).cast::<u8>());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOTTY);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_ioctl_tiocspgrp_non_console() {
        let fd = fdtable::alloc_fd(HandleKind::File, 306).unwrap();
        let pgrp: i32 = 10;
        let ret = ioctl(fd, TIOCSPGRP, (&raw const pgrp).cast::<u8>().cast_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOTTY);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_ioctl_invalid_fd() {
        let ret = ioctl(-1, TIOCGWINSZ, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_ioctl_unknown_request() {
        ensure_std_fds();
        let ret = ioctl(0, 0xDEAD, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOTTY);
    }

    // -- tcgetattr / tcsetattr wrapper tests --

    #[test]
    fn test_tcgetattr_console() {
        ensure_std_fds();
        let mut t = core::mem::MaybeUninit::<Termios>::uninit();
        let ret = tcgetattr(0, t.as_mut_ptr());
        assert_eq!(ret, 0);
        let t = unsafe { t.assume_init() };
        assert_ne!(t.c_lflag & ICANON, 0, "tcgetattr: canonical mode");
        assert_eq!(t.c_cc[VINTR], 0x03, "tcgetattr: Ctrl-C");
    }

    #[test]
    fn test_tcgetattr_non_console() {
        let fd = fdtable::alloc_fd(HandleKind::File, 307).unwrap();
        let mut t = core::mem::MaybeUninit::<Termios>::uninit();
        assert_eq!(tcgetattr(fd, t.as_mut_ptr()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOTTY);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_tcsetattr_tcsanow() {
        ensure_std_fds();
        let t = default_termios();
        assert_eq!(tcsetattr(0, TCSANOW, &raw const t), 0);
    }

    #[test]
    fn test_tcsetattr_tcsadrain() {
        ensure_std_fds();
        let t = default_termios();
        assert_eq!(tcsetattr(0, TCSADRAIN, &raw const t), 0);
    }

    #[test]
    fn test_tcsetattr_tcsaflush() {
        ensure_std_fds();
        let t = default_termios();
        assert_eq!(tcsetattr(0, TCSAFLUSH, &raw const t), 0);
    }

    #[test]
    fn test_tcsetattr_invalid_action() {
        ensure_std_fds();
        let t = default_termios();
        assert_eq!(tcsetattr(0, 99, &raw const t), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_tcsetattr_negative_action() {
        ensure_std_fds();
        let t = default_termios();
        assert_eq!(tcsetattr(0, -1, &raw const t), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- tcsendbreak / tcdrain / tcflow / tcflush on console --

    #[test]
    fn test_tcsendbreak_console() {
        ensure_std_fds();
        assert_eq!(tcsendbreak(0, 0), 0);
    }

    #[test]
    fn test_tcsendbreak_console_nonzero_duration() {
        ensure_std_fds();
        assert_eq!(tcsendbreak(0, 100), 0, "duration is ignored");
    }

    #[test]
    fn test_tcsendbreak_non_terminal() {
        let fd = fdtable::alloc_fd(HandleKind::Pipe, 308).unwrap();
        assert_eq!(tcsendbreak(fd, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOTTY);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_tcdrain_console() {
        ensure_std_fds();
        assert_eq!(tcdrain(0), 0);
    }

    #[test]
    fn test_tcdrain_non_terminal() {
        let fd = fdtable::alloc_fd(HandleKind::File, 309).unwrap();
        assert_eq!(tcdrain(fd), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOTTY);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_tcflow_console_all_valid_actions() {
        ensure_std_fds();
        assert_eq!(tcflow(0, TCOON), 0);
        assert_eq!(tcflow(0, TCOOFF), 0);
        assert_eq!(tcflow(0, TCION), 0);
        assert_eq!(tcflow(0, TCIOFF), 0);
    }

    #[test]
    fn test_tcflow_invalid_action() {
        ensure_std_fds();
        assert_eq!(tcflow(0, 99), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_tcflow_negative_action() {
        ensure_std_fds();
        assert_eq!(tcflow(0, -1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_tcflow_non_terminal() {
        let fd = fdtable::alloc_fd(HandleKind::File, 310).unwrap();
        assert_eq!(tcflow(fd, TCOON), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOTTY);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_tcflush_console_all_valid_selectors() {
        ensure_std_fds();
        assert_eq!(tcflush(0, TCIFLUSH), 0);
        assert_eq!(tcflush(0, TCOFLUSH), 0);
        assert_eq!(tcflush(0, TCIOFLUSH), 0);
    }

    #[test]
    fn test_tcflush_invalid_selector() {
        ensure_std_fds();
        assert_eq!(tcflush(0, 99), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_tcflush_negative_selector() {
        ensure_std_fds();
        assert_eq!(tcflush(0, -1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_tcflush_non_terminal() {
        let fd = fdtable::alloc_fd(HandleKind::Pipe, 311).unwrap();
        assert_eq!(tcflush(fd, TCIFLUSH), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOTTY);
        let _ = fdtable::close_fd(fd);
    }

    // -- Additional cfmakeraw / termios tests --

    #[test]
    fn test_cfmakeraw_preserves_baud() {
        let mut t = default_termios();
        unsafe {
            cfsetispeed(&raw mut t, B115200);
        }
        unsafe {
            cfsetospeed(&raw mut t, B9600);
        }
        unsafe {
            cfmakeraw(&raw mut t);
        }
        assert_eq!(t.c_ispeed, B115200, "cfmakeraw should not change c_ispeed");
        assert_eq!(t.c_ospeed, B9600, "cfmakeraw should not change c_ospeed");
    }

    #[test]
    fn test_cfmakeraw_preserves_c_line() {
        let mut t = default_termios();
        t.c_line = 5;
        unsafe {
            cfmakeraw(&raw mut t);
        }
        assert_eq!(t.c_line, 5, "cfmakeraw should not change c_line");
    }

    #[test]
    fn test_cfmakeraw_idempotent() {
        let mut t1 = default_termios();
        unsafe {
            cfmakeraw(&raw mut t1);
        }
        let mut t2 = t1;
        unsafe {
            cfmakeraw(&raw mut t2);
        }
        // All fields should be identical after double application.
        assert_eq!(t1.c_iflag, t2.c_iflag);
        assert_eq!(t1.c_oflag, t2.c_oflag);
        assert_eq!(t1.c_cflag, t2.c_cflag);
        assert_eq!(t1.c_lflag, t2.c_lflag);
        assert_eq!(t1.c_cc, t2.c_cc);
    }

    #[test]
    fn test_cfmakeraw_clears_echonl() {
        let mut t = default_termios();
        assert_ne!(t.c_lflag & ECHONL, 0, "ECHONL should be set in default");
        unsafe {
            cfmakeraw(&raw mut t);
        }
        assert_eq!(t.c_lflag & ECHONL, 0, "ECHONL should be cleared in raw");
    }

    #[test]
    fn test_cfmakeraw_clears_iexten() {
        let mut t = default_termios();
        assert_ne!(t.c_lflag & IEXTEN, 0, "IEXTEN should be set in default");
        unsafe {
            cfmakeraw(&raw mut t);
        }
        assert_eq!(t.c_lflag & IEXTEN, 0, "IEXTEN should be cleared in raw");
    }

    #[test]
    fn test_cfmakeraw_clears_brkint() {
        let mut t = default_termios();
        t.c_iflag |= BRKINT;
        unsafe {
            cfmakeraw(&raw mut t);
        }
        assert_eq!(t.c_iflag & BRKINT, 0, "BRKINT should be cleared in raw");
    }

    #[test]
    fn test_cfmakeraw_clears_ixon() {
        let mut t = default_termios();
        t.c_iflag |= IXON;
        unsafe {
            cfmakeraw(&raw mut t);
        }
        assert_eq!(t.c_iflag & IXON, 0, "IXON should be cleared in raw");
    }

    #[test]
    fn test_cfmakeraw_clears_istrip() {
        let mut t = default_termios();
        t.c_iflag |= ISTRIP;
        unsafe {
            cfmakeraw(&raw mut t);
        }
        assert_eq!(t.c_iflag & ISTRIP, 0, "ISTRIP should be cleared in raw");
    }

    #[test]
    fn test_cfmakeraw_clears_inpck() {
        let mut t = default_termios();
        t.c_iflag |= INPCK;
        unsafe {
            cfmakeraw(&raw mut t);
        }
        assert_eq!(t.c_iflag & INPCK, 0, "INPCK should be cleared in raw");
    }

    #[test]
    fn test_cfmakeraw_clears_onlcr() {
        let mut t = default_termios();
        assert_ne!(t.c_oflag & ONLCR, 0, "ONLCR should be set in default");
        unsafe {
            cfmakeraw(&raw mut t);
        }
        // ONLCR is implicitly cleared because OPOST is cleared; ONLCR only
        // matters when OPOST is on, but let's verify OPOST is cleared.
        assert_eq!(t.c_oflag & OPOST, 0, "OPOST should be cleared in raw");
    }

    // -- Default termios additional tests --

    #[test]
    fn test_default_termios_cread() {
        let t = default_termios();
        assert_ne!(t.c_cflag & CREAD, 0, "Receiver should be enabled");
    }

    #[test]
    fn test_default_termios_hupcl() {
        let t = default_termios();
        assert_ne!(t.c_cflag & HUPCL, 0, "Hang up on close should be set");
    }

    #[test]
    fn test_default_termios_clocal() {
        let t = default_termios();
        assert_ne!(t.c_cflag & CLOCAL, 0, "Ignore modem lines should be set");
    }

    #[test]
    fn test_default_termios_no_parenb() {
        let t = default_termios();
        assert_eq!(
            t.c_cflag & PARENB,
            0,
            "Parity should not be enabled by default"
        );
    }

    #[test]
    fn test_default_termios_c_line_zero() {
        let t = default_termios();
        assert_eq!(t.c_line, 0, "Line discipline should be 0 (N_TTY)");
    }

    #[test]
    fn test_default_termios_vstart_vstop() {
        let t = default_termios();
        assert_eq!(t.c_cc[VSTART], 0x11, "VSTART should be Ctrl-Q");
        assert_eq!(t.c_cc[VSTOP], 0x13, "VSTOP should be Ctrl-S");
    }

    #[test]
    fn test_default_termios_vmin_vtime() {
        let t = default_termios();
        assert_eq!(t.c_cc[VMIN], 1, "VMIN should be 1");
        assert_eq!(t.c_cc[VTIME], 0, "VTIME should be 0");
    }

    // -- Structure alignment tests --

    #[test]
    fn test_termios_alignment() {
        assert!(
            core::mem::align_of::<Termios>() >= 4,
            "Termios should be aligned to at least 4 bytes"
        );
    }

    #[test]
    fn test_winsize_alignment() {
        assert!(
            core::mem::align_of::<Winsize>() >= 2,
            "Winsize should be aligned to at least 2 bytes"
        );
    }

    // -- Flag bit distinctness --

    #[test]
    fn test_iflag_bits_distinct() {
        let flags = [BRKINT, INPCK, ISTRIP, INLCR, IGNCR, ICRNL, IXON];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(
                    flags[i] & flags[j],
                    0,
                    "iflag bits at {i} and {j} should not overlap"
                );
            }
        }
    }

    #[test]
    fn test_lflag_bits_distinct() {
        let flags = [ISIG, ICANON, ECHO, ECHONL, IEXTEN];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(
                    flags[i] & flags[j],
                    0,
                    "lflag bits at {i} and {j} should not overlap"
                );
            }
        }
    }

    #[test]
    fn test_cflag_csize_cs8() {
        // CS8 should set all bits in the CSIZE mask.
        assert_eq!(CS8 & CSIZE, CS8, "CS8 should fit within CSIZE mask");
        assert_eq!(CS8, CSIZE, "CS8 should equal the full CSIZE mask (8-bit)");
    }

    #[test]
    fn test_cflag_distinct_non_csize() {
        // CREAD, PARENB, HUPCL, CLOCAL should be distinct from each
        // other and from CSIZE.
        let flags = [CREAD, PARENB, HUPCL, CLOCAL];
        for i in 0..flags.len() {
            assert_eq!(
                flags[i] & CSIZE,
                0,
                "cflag bit {i} should not overlap with CSIZE"
            );
            for j in (i + 1)..flags.len() {
                assert_eq!(
                    flags[i] & flags[j],
                    0,
                    "cflag bits at {i} and {j} should not overlap"
                );
            }
        }
    }

    #[test]
    fn test_oflag_bits_distinct() {
        assert_eq!(OPOST & ONLCR, 0, "OPOST and ONLCR should not overlap");
    }

    // -- isatty errno setting --

    #[test]
    fn test_isatty_non_terminal_sets_enotty() {
        let fd = fdtable::alloc_fd(HandleKind::File, 312).unwrap();
        assert_eq!(isatty(fd), 0);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOTTY);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_isatty_invalid_fd_sets_ebadf() {
        assert_eq!(isatty(-1), 0);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // -- ttyname errno setting --

    #[test]
    fn test_ttyname_non_terminal_sets_enotty() {
        let fd = fdtable::alloc_fd(HandleKind::Pipe, 313).unwrap();
        assert!(ttyname(fd).is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOTTY);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_ttyname_invalid_fd_sets_ebadf() {
        assert!(ttyname(-1).is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // -- Baud rate round-trip all speeds --

    #[test]
    fn test_baud_rate_roundtrip_all() {
        let speeds = [B9600, B19200, B38400, B115200];
        for &speed in &speeds {
            let mut t = default_termios();
            assert_eq!(unsafe { cfsetspeed(&raw mut t, speed) }, 0);
            assert_eq!(unsafe { cfgetispeed(&raw const t) }, speed);
            assert_eq!(unsafe { cfgetospeed(&raw const t) }, speed);
        }
    }

    // -- cfsetispeed / cfsetospeed round-trip with different speeds --

    #[test]
    fn test_baud_rate_independent_ispeed_ospeed() {
        let mut t = default_termios();
        assert_eq!(unsafe { cfsetispeed(&raw mut t, B9600) }, 0);
        assert_eq!(unsafe { cfsetospeed(&raw mut t, B115200) }, 0);
        assert_eq!(unsafe { cfgetispeed(&raw const t) }, B9600);
        assert_eq!(unsafe { cfgetospeed(&raw const t) }, B115200);
    }

    // -- PTY stubs with various fd values --

    #[test]
    fn test_posix_openpt_rdwr() {
        // Host build: `SYS_PTY_CREATE` is not there, so this fails.  See
        // `test_posix_openpt_fails_on_host` for why the errno is EIO and
        // not ENOSYS.
        assert_eq!(posix_openpt(0x02), -1); // O_RDWR
        assert_eq!(crate::errno::get_errno(), crate::errno::EIO);
    }

    #[test]
    fn test_ptsname_r_small_buffer() {
        // fd 0 is open but is not a pty master, so ENOTTY fires before
        // any path-length check -- which is the ordering Linux has and
        // the one `ptsname_r` now implements explicitly (`require_master`
        // first, then `slave_id_of`, then the buflen check).
        //
        // The ERANGE arm itself cannot be reached through this entry
        // point on a host build: it sits *after* `slave_id_of`, and
        // `SYS_PTY_SLAVE_ID` does not exist here, so the call fails one
        // step earlier.  `test_format_pts_name_*` covers the name whose
        // length that arm compares against, which is the part that could
        // actually be wrong; the comparison itself is one line and is
        // exercised on the target.
        let mut buf = [0u8; 1];
        crate::errno::set_errno(0);
        assert_eq!(ptsname_r(0, buf.as_mut_ptr(), buf.len()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOTTY);
    }

    // --- format_pts_name ---
    //
    // The one piece of the pty naming path that is pure computation, so
    // the one piece a host build can check properly.  It is also the piece
    // where a mistake would be quiet: a wrong name is still a plausible
    // name, and the caller that uses it -- `open("/dev/pts/<n>")` -- would
    // fail with ENOENT far away from the digit loop that caused it.

    fn pts_name_of(id: u32) -> std::string::String {
        let mut buf = [0u8; PTS_NAME_MAX];
        let len = format_pts_name(id, &mut buf);
        assert_eq!(buf.get(len), Some(&0), "must be NUL-terminated at `len`");
        std::string::String::from_utf8(buf.get(..len).unwrap_or(&[]).to_vec()).expect("ASCII only")
    }

    #[test]
    fn test_format_pts_name_zero() {
        // The digit loop is do-while precisely so that 0 emits "0" rather
        // than the empty string a `while v != 0` would leave.
        assert_eq!(pts_name_of(0), "/dev/pts/0");
    }

    #[test]
    fn test_format_pts_name_single_and_multi_digit() {
        assert_eq!(pts_name_of(7), "/dev/pts/7");
        assert_eq!(pts_name_of(10), "/dev/pts/10");
        assert_eq!(pts_name_of(1234), "/dev/pts/1234");
    }

    #[test]
    fn test_format_pts_name_u32_max_fits_exactly() {
        // `PTS_NAME_MAX` is sized for ten digits plus a NUL, so the widest
        // possible id must fit with the terminator and no truncation.
        // If this ever fails, `format_pts_name`'s bounds-checked writes
        // would silently drop the tail rather than overflow -- which is
        // safe but would name the wrong terminal.
        let name = pts_name_of(u32::MAX);
        assert_eq!(name, "/dev/pts/4294967295");
        assert_eq!(name.len() + 1, PTS_NAME_MAX);
    }

    #[test]
    fn test_format_pts_name_returns_written_length() {
        let mut buf = [0u8; PTS_NAME_MAX];
        assert_eq!(format_pts_name(0, &mut buf), "/dev/pts/0".len());
        assert_eq!(
            format_pts_name(u32::MAX, &mut buf),
            "/dev/pts/4294967295".len()
        );
    }

    #[test]
    fn test_format_pts_name_does_not_keep_previous_digits() {
        // The buffer is reused by `ttyname_buf`, so a shorter name written
        // over a longer one must not leave the old tail visible past the
        // NUL a caller stops at -- and, more importantly, `len` must
        // shrink with it.
        let mut buf = [0u8; PTS_NAME_MAX];
        let long = format_pts_name(u32::MAX, &mut buf);
        let short = format_pts_name(3, &mut buf);
        assert!(short < long);
        assert_eq!(buf.get(..short), Some(b"/dev/pts/3".as_slice()));
        assert_eq!(buf.get(short), Some(&0));
    }

    // --- winsize wire format ---

    #[test]
    fn test_winsize_wire_round_trip() {
        let ws = Winsize {
            ws_row: 0x0102,
            ws_col: 0x0304,
            ws_xpixel: 0x0506,
            ws_ypixel: 0x0708,
        };
        let wire = winsize_to_wire(&ws);
        // Little-endian, in the kernel's declared field order.  Pinned as
        // bytes rather than by round trip alone, because a round trip is
        // equally happy with a self-consistent *wrong* order.
        assert_eq!(wire, [0x02, 0x01, 0x04, 0x03, 0x06, 0x05, 0x08, 0x07]);
        let back = winsize_from_wire(&wire);
        assert_eq!(back.ws_row, ws.ws_row);
        assert_eq!(back.ws_col, ws.ws_col);
        assert_eq!(back.ws_xpixel, ws.ws_xpixel);
        assert_eq!(back.ws_ypixel, ws.ws_ypixel);
    }

    #[test]
    fn test_winsize_wire_extremes() {
        let ws = Winsize {
            ws_row: 0,
            ws_col: u16::MAX,
            ws_xpixel: u16::MAX,
            ws_ypixel: 0,
        };
        let back = winsize_from_wire(&winsize_to_wire(&ws));
        assert_eq!(
            (back.ws_row, back.ws_col, back.ws_xpixel, back.ws_ypixel),
            (0, u16::MAX, u16::MAX, 0)
        );
    }

    #[test]
    fn test_tiocswinsz_then_tiocgwinsz_round_trips_through_the_wire() {
        // Goes through the ioctl entry points rather than the marshalling
        // functions directly, so the host double stores exactly what the
        // kernel would be handed and hands back exactly what it would
        // return.  fd 0 is the console, which `terminal_arg` maps to CTTY.
        let set = Winsize {
            ws_row: 51,
            ws_col: 132,
            ws_xpixel: 1056,
            ws_ypixel: 816,
        };
        let ret = ioctl(0, TIOCSWINSZ, (&raw const set).cast::<u8>().cast_mut());
        assert_eq!(ret, 0);

        let mut got = Winsize {
            ws_row: 0,
            ws_col: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let ret = ioctl(0, TIOCGWINSZ, (&raw mut got).cast::<u8>());
        assert_eq!(ret, 0);
        assert_eq!(got.ws_row, 51);
        assert_eq!(got.ws_col, 132);
        assert_eq!(got.ws_xpixel, 1056);
        assert_eq!(got.ws_ypixel, 816);
    }

    // -- ctermid with buffer verifies null terminator --

    #[test]
    fn test_ctermid_buffer_null_terminated() {
        let mut buf = [0xFFu8; 20];
        let _ = ctermid(buf.as_mut_ptr());
        // Find the null terminator.
        let nul_pos = buf.iter().position(|&b| b == 0);
        assert_eq!(nul_pos, Some(12), "Null terminator at position 12");
    }

    // -- Fionread on TcpListener gives 0 --

    #[test]
    fn test_ioctl_fionread_tcp_listener() {
        let fd = fdtable::alloc_fd(HandleKind::TcpListener, 0).unwrap();
        let mut avail: i32 = -1;
        let ret = ioctl(fd, FIONREAD, (&raw mut avail).cast::<u8>());
        assert_eq!(ret, 0);
        assert_eq!(avail, 0, "TcpListener FIONREAD should return 0");
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_ioctl_fionread_tcp_stream_zero_handle() {
        let fd = fdtable::alloc_fd(HandleKind::TcpStream, 0).unwrap();
        let mut avail: i32 = -1;
        let ret = ioctl(fd, FIONREAD, (&raw mut avail).cast::<u8>());
        assert_eq!(ret, 0);
        assert_eq!(avail, 0, "TcpStream handle=0 FIONREAD should return 0");
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_ioctl_fionread_udp_zero_handle() {
        let fd = fdtable::alloc_fd(HandleKind::UdpSocket, 0).unwrap();
        let mut avail: i32 = -1;
        let ret = ioctl(fd, FIONREAD, (&raw mut avail).cast::<u8>());
        assert_eq!(ret, 0);
        assert_eq!(avail, 0, "UdpSocket handle=0 FIONREAD should return 0");
        let _ = fdtable::close_fd(fd);
    }

    // -- tcgetsid tests --

    #[test]
    fn test_tcgetsid_console() {
        ensure_std_fds();
        let sid = tcgetsid(0);
        // In test mode, getsid(0) calls getpid() which executes a
        // real syscall instruction, returning an unpredictable value.
        // Just verify tcgetsid didn't return -1 with EBADF/ENOTTY
        // (i.e., it passed the terminal validation).
        // The actual sid value is OS-dependent in test mode.
        let _ = sid;
    }

    #[test]
    fn test_tcgetsid_invalid_fd() {
        let ret = tcgetsid(-1);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_tcgetsid_non_terminal() {
        let fd = fdtable::alloc_fd(HandleKind::File, 314).unwrap();
        let ret = tcgetsid(fd);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOTTY);
        let _ = fdtable::close_fd(fd);
    }

    // -----------------------------------------------------------------
    // Phase 68 — PTY-helper validators
    // posix_openpt / grantpt / unlockpt / ptsname / ptsname_r
    // -----------------------------------------------------------------

    // Helper: allocate an open fd we own for tests (so we can
    // exercise the "valid open fd" branch of grantpt/unlockpt/
    // ptsname/ptsname_r).
    fn open_test_fd() -> i32 {
        fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd must succeed")
    }

    // --- posix_openpt ---
    //
    // These four used to pin ENOSYS, because `posix_openpt` was a stub that
    // set it by hand.  It is now the real `SYS_PTY_CREATE` call, so on a
    // host build it fails the way every other syscall in this crate fails:
    // the host `syscallN` shims answer -38, which is not one of the
    // kernel's native error codes, so `errno::translate` maps it through
    // its catch-all to EIO.
    //
    // Pinning EIO rather than deleting the tests is deliberate.  They no
    // longer say anything about ENOSYS, but they still say the thing that
    // matters: `posix_openpt` reports a failure and sets *an* errno on
    // every flag word, including the garbage ones, and a caller that
    // checks the return value stops there.  Deleting them would have
    // removed the only host-side coverage of the early-return path in a
    // function whose later paths cannot run here at all.

    #[test]
    fn test_posix_openpt_fails_on_host() {
        crate::errno::set_errno(0);
        assert_eq!(posix_openpt(0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EIO);
    }

    #[test]
    fn test_posix_openpt_fails_for_o_rdwr() {
        // O_RDWR is what POSIX requires callers to pass.
        crate::errno::set_errno(0);
        assert_eq!(posix_openpt(0x02), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EIO);
    }

    #[test]
    fn test_posix_openpt_fails_for_o_rdwr_noctty() {
        // The canonical posix_openpt(O_RDWR | O_NOCTTY) form.
        crate::errno::set_errno(0);
        assert_eq!(posix_openpt(0x02 | 0x100), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EIO);
    }

    #[test]
    fn test_posix_openpt_does_not_validate_flags() {
        // Garbage flags are accepted (and would be by Linux too, since
        // posix_openpt is open("/dev/ptmx", flags) — we don't invent
        // EINVAL paths Linux doesn't have).  The failure that comes back
        // is the kernel's, not an argument complaint.
        crate::errno::set_errno(0);
        assert_eq!(posix_openpt(-1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EIO);
    }

    // --- grantpt ---

    #[test]
    fn test_grantpt_negative_fd_ebadf() {
        crate::errno::set_errno(0);
        assert_eq!(grantpt(-1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_grantpt_closed_fd_ebadf() {
        crate::errno::set_errno(0);
        assert_eq!(grantpt(9999), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_grantpt_open_non_pty_fd_einval() {
        // Any open fd is necessarily not a PTY master (because
        // posix_openpt always fails), so grantpt must report
        // EINVAL — never silently succeed.
        let fd = open_test_fd();
        crate::errno::set_errno(0);
        assert_eq!(grantpt(fd), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = fdtable::close_fd(fd);
    }

    // --- unlockpt ---

    #[test]
    fn test_unlockpt_negative_fd_ebadf() {
        crate::errno::set_errno(0);
        assert_eq!(unlockpt(-1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_unlockpt_closed_fd_ebadf() {
        crate::errno::set_errno(0);
        assert_eq!(unlockpt(9999), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_unlockpt_open_non_pty_fd_einval() {
        let fd = open_test_fd();
        crate::errno::set_errno(0);
        assert_eq!(unlockpt(fd), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = fdtable::close_fd(fd);
    }

    // --- ptsname ---

    #[test]
    fn test_ptsname_negative_fd_returns_null_ebadf() {
        crate::errno::set_errno(0);
        assert!(ptsname(-1).is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_ptsname_closed_fd_returns_null_ebadf() {
        crate::errno::set_errno(0);
        assert!(ptsname(9999).is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_ptsname_open_non_pty_fd_returns_null_enotty() {
        let fd = open_test_fd();
        crate::errno::set_errno(0);
        assert!(ptsname(fd).is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOTTY);
        let _ = fdtable::close_fd(fd);
    }

    // --- ptsname_r ---

    #[test]
    fn test_ptsname_r_null_buf_still_reports_the_fd_verdict() {
        // glibc 2.39's __ptsname_r issues ioctl(fd, TIOCGPTN) before it
        // looks at `buf` at all, so on a live non-PTY fd a Linux caller
        // sees ENOTTY — the ioctl's errno — no matter what `buf` holds.
        // Not EINVAL, and not EFAULT.
        let fd = open_test_fd();
        crate::errno::set_errno(0);
        assert_eq!(ptsname_r(fd, core::ptr::null_mut(), 64), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOTTY);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_ptsname_r_negative_fd_ebadf() {
        let mut buf = [0u8; 64];
        crate::errno::set_errno(0);
        assert_eq!(ptsname_r(-1, buf.as_mut_ptr(), buf.len()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_ptsname_r_closed_fd_ebadf() {
        let mut buf = [0u8; 64];
        crate::errno::set_errno(0);
        assert_eq!(ptsname_r(9999, buf.as_mut_ptr(), buf.len()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_ptsname_r_open_non_pty_fd_enotty() {
        let fd = open_test_fd();
        let mut buf = [0u8; 64];
        crate::errno::set_errno(0);
        assert_eq!(ptsname_r(fd, buf.as_mut_ptr(), buf.len()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOTTY);
        let _ = fdtable::close_fd(fd);
    }

    // --- ordering ---

    #[test]
    fn test_ptsname_r_bad_fd_beats_null_buf() {
        // The priority order, and it runs the opposite way to the obvious
        // guess: glibc validates the descriptor via TIOCGPTN first and
        // never reaches the buffer, so a bad fd wins over a NULL `buf`.
        crate::errno::set_errno(0);
        assert_eq!(ptsname_r(-1, core::ptr::null_mut(), 64), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_grantpt_negative_fd_beats_not_pty() {
        // fd<0 check fires before the "is it a PTY master" decision.
        crate::errno::set_errno(0);
        assert_eq!(grantpt(-1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // --- real-world workflows ---

    #[test]
    fn test_workflow_openpty_emulation_fails_at_openpt() {
        // libc's openpty() does:
        //   m = posix_openpt(O_RDWR | O_NOCTTY)
        //   grantpt(m); unlockpt(m); name = ptsname(m); ...
        // On a host build the first step fails, so the caller should never
        // reach grantpt/unlockpt/ptsname.  On the real target it succeeds
        // and the whole chain runs; that is the ring-3 fixture's job, not
        // this one's, because none of these syscalls exist here.
        crate::errno::set_errno(0);
        let m = posix_openpt(0x02 | 0x100);
        assert_eq!(m, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EIO);
    }

    #[test]
    fn test_workflow_terminal_emulator_full_chain() {
        // A terminal emulator that ignores the posix_openpt failure
        // (or hand-rolls its own equivalent) and tries grantpt on
        // an arbitrary open fd must see EINVAL — not silent success
        // followed by a confusing later failure.
        let fd = open_test_fd();
        crate::errno::set_errno(0);
        assert_eq!(grantpt(fd), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        crate::errno::set_errno(0);
        assert_eq!(unlockpt(fd), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        crate::errno::set_errno(0);
        assert!(ptsname(fd).is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOTTY);
        let _ = fdtable::close_fd(fd);
    }

    // --- buggy callers ---

    #[test]
    fn test_buggy_grantpt_uses_unchecked_openpt_result() {
        // Caller forgets to check posix_openpt's return value and
        // passes -1 to grantpt.  Must produce EBADF, not silent
        // success that pretends a PTY was provisioned.
        crate::errno::set_errno(0);
        assert_eq!(grantpt(-1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_buggy_unlockpt_uses_unchecked_openpt_result() {
        crate::errno::set_errno(0);
        assert_eq!(unlockpt(-1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_buggy_ptsname_r_with_null_buf() {
        // Common bug: caller forgets to allocate the buffer.  It still
        // fails, but on the descriptor rather than the buffer — fd 0 is
        // open and is not a PTY master, so ENOTTY, exactly as on Linux.
        crate::errno::set_errno(0);
        assert_eq!(ptsname_r(0, core::ptr::null_mut(), 64), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOTTY);
    }
}

// ---------------------------------------------------------------------------
// Pseudo-terminals
// ---------------------------------------------------------------------------

/// Open a pseudo-terminal master device.
///
/// Wraps `SYS_PTY_CREATE`, which returns **both** ends at once -- the master
/// in `rax` and the slave in `rdx`.  That is the whole reason this family
/// looks simpler here than on Linux: there is no `grantpt` chmod dance and
/// no `unlockpt` unlock bit, because the kernel never publishes a slave for
/// a third party to race for in the first place.  See the header comment on
/// [`crate::syscall::SYS_PTY_CREATE`].
///
/// The slave handle the kernel hands back is parked in [`crate::ptytab`]
/// until someone opens it by name (`open("/dev/pts/<id>")`).  It has to be
/// held rather than closed, because `openpty(3)`'s published order is
/// open-master, *then* ask its name, *then* open the slave -- so between
/// those two steps the slave exists with no file descriptor naming it, and
/// dropping it there would make the pair unusable exactly one call before it
/// became usable.
///
/// `oflag` is not validated -- Linux's `posix_openpt` is
/// `open("/dev/ptmx", oflag)` and forwards whatever the open path accepts --
/// but `O_NONBLOCK` *is* honoured, because a terminal emulator's read loop
/// depends on it and the alternative is a caller that silently blocks.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_openpt(oflag: i32) -> i32 {
    let (master, slave) = crate::syscall::syscall3_2ret(crate::syscall::SYS_PTY_CREATE, 0, 0, 0);
    if crate::errno::translate(master) < 0 {
        return -1;
    }
    #[allow(clippy::cast_sign_loss)]
    let (master, slave) = (master as u64, slave as u64);

    let Some(id) = slave_id_of(master) else {
        // The pair exists but we cannot name it, so it can never be
        // completed; close both ends rather than leak a terminal that
        // nothing will ever open.  `slave_id_of` set errno.
        let _ = crate::syscall::syscall1(crate::syscall::SYS_PTY_CLOSE, master);
        let _ = crate::syscall::syscall1(crate::syscall::SYS_PTY_CLOSE, slave);
        return -1;
    };

    // Record the pair *before* installing the fd.  The reverse order has a
    // window in which a master fd exists whose slave nothing is tracking,
    // and a close in that window would leak the slave permanently.
    if !crate::ptytab::note_pair(id, master, slave) {
        let _ = crate::syscall::syscall1(crate::syscall::SYS_PTY_CLOSE, master);
        let _ = crate::syscall::syscall1(crate::syscall::SYS_PTY_CLOSE, slave);
        // EMFILE, not ENOMEM: the limit that was hit is a per-process table
        // of open terminals, which is what EMFILE describes, and it is the
        // errno `openpty` callers already handle.
        crate::errno::set_errno(crate::errno::EMFILE);
        return -1;
    }

    let status = oflag & crate::fcntl::O_NONBLOCK;
    let fd =
        crate::fdtable::alloc_fd_with_flags(crate::fdtable::HandleKind::PtyMaster, master, status);
    let Some(fd) = fd else {
        // Unwind in the reverse order of construction.  The record is
        // retired directly rather than through `close_pty_handle`, because
        // that helper drives the same retirement and would then close a
        // slave this arm has already closed.
        crate::ptytab::retire_master(master);
        let _ = crate::syscall::syscall1(crate::syscall::SYS_PTY_CLOSE, master);
        let _ = crate::syscall::syscall1(crate::syscall::SYS_PTY_CLOSE, slave);
        crate::errno::set_errno(crate::errno::EMFILE);
        return -1;
    };
    fd
}

/// Grant access to the slave pseudo-terminal device.
///
/// A no-op that succeeds for a master fd, and that is the design rather
/// than a shortcut.  On Linux `grantpt` exists to `chown`/`chmod` the slave
/// node into the caller's ownership, because the node is a globally visible
/// file that anybody could otherwise open.  Our slave is a capability the
/// kernel handed to this process; there is no node and no third party to
/// exclude, so there is nothing for `grantpt` to grant.
///
/// It is still validated rather than blindly returning 0, because a caller
/// that passes the *slave* fd, or a pipe, has a bug this is the natural
/// place to report:
///
/// 1. `fd < 0` or not open -> `EBADF`
/// 2. `fd` is not a pty master -> `EINVAL`, as Linux's `grantpt(3)` returns
///    for an fd not associated with a master.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn grantpt(fd: i32) -> i32 {
    require_master(fd, crate::errno::EINVAL).map_or(-1, |_| 0)
}

/// Unlock a pseudo-terminal master/slave pair.
///
/// Also a validated no-op, for the same reason as [`grantpt`]: Linux's
/// `unlockpt` clears a lock bit (`TIOCSPTLCK`) that exists to stop a slave
/// being opened before `grantpt` has fixed its permissions.  With no node
/// and no permissions to fix, there is no window to lock, so the lock bit
/// was never introduced -- rather than introduced and then always cleared.
///
/// Errors are identical to `grantpt`'s.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn unlockpt(fd: i32) -> i32 {
    require_master(fd, crate::errno::EINVAL).map_or(-1, |_| 0)
}

/// Resolve `fd` to the handle of a pty master, or set `errno` and fail.
///
/// `not_master` is the errno to report when the descriptor is open but is
/// not a master, which differs across this family: `grantpt`/`unlockpt` use
/// `EINVAL` and `ptsname`/`ptsname_r` use `ENOTTY`, each matching what the
/// corresponding glibc entry point produces.
fn require_master(fd: i32, not_master: i32) -> Option<u64> {
    if fd < 0 {
        crate::errno::set_errno(crate::errno::EBADF);
        return None;
    }
    let Some(entry) = crate::fdtable::get_fd(fd) else {
        crate::errno::set_errno(crate::errno::EBADF);
        return None;
    };
    if entry.kind != crate::fdtable::HandleKind::PtyMaster {
        crate::errno::set_errno(not_master);
        return None;
    }
    Some(entry.handle)
}

/// Get the name of the slave pseudo-terminal device.
///
/// Returns a pointer to a process-wide buffer holding `/dev/pts/<id>`,
/// or NULL with `errno` set.  Shares [`ttyname`]'s buffer, which is
/// deliberate: both are the non-reentrant spelling of "name this
/// terminal", and one buffer means a program cannot hold a `ptsname`
/// result across a `ttyname` call and believe it survived.
///
/// Validation order:
///
/// 1. `fd < 0` or not open -> `EBADF`
/// 2. `fd` is not a pty master -> `ENOTTY`, matching Linux.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ptsname(fd: i32) -> *mut u8 {
    let Some(handle) = require_master(fd, crate::errno::ENOTTY) else {
        return core::ptr::null_mut();
    };
    let Some(id) = slave_id_of(handle) else {
        return core::ptr::null_mut();
    };
    ttyname_buf::store(id).cast_mut()
}

/// Thread-safe version of [`ptsname`].
///
/// Returns -1 with `errno` set, **not** a positive errno.  That is the
/// local convention every other function in this file follows, and it is
/// also what musl does; glibc returns the errno directly.  The divergence
/// is deliberate and is why `ttyname_r` -- whose callers propagate the
/// return value straight into an errno slot -- does *not* follow suit.
///
/// Validation order follows glibc's `__ptsname_r`
/// (`sysdeps/unix/sysv/linux/ptsname.c`, checked against 2.39), which
/// issues its `ioctl` **first** and only then looks at the buffer, so a bad
/// descriptor outranks a bad buffer:
///
/// 1. `fd < 0` or not open -> `EBADF`
/// 2. `fd` is not a pty master -> `ENOTTY`
/// 3. `buf` is NULL -> `EINVAL`
/// 4. the name does not fit in `buflen` -> `ERANGE`
///
/// `EINVAL` for a NULL `buf` is a deliberate improvement on both libcs we
/// compare against: glibc has no check and segfaults in
/// `__stpcpy (buf, devpts)`, musl clamps the length to 0 and yields
/// `ERANGE`.  Neither is acceptable to copy, and both POSIX.1-2024 and
/// `ptsname_r(3)` document `EINVAL`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ptsname_r(fd: i32, buf: *mut u8, buflen: usize) -> i32 {
    let Some(handle) = require_master(fd, crate::errno::ENOTTY) else {
        return -1;
    };
    let Some(id) = slave_id_of(handle) else {
        return -1;
    };
    if buf.is_null() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }
    let mut name = [0u8; PTS_NAME_MAX];
    let len = format_pts_name(id, &mut name);
    // Room for the name *and* its terminator, as POSIX requires of `buflen`.
    if len.wrapping_add(1) > buflen {
        crate::errno::set_errno(crate::errno::ERANGE);
        return -1;
    }
    let mut i = 0usize;
    while i <= len {
        if let Some(&b) = name.get(i) {
            // SAFETY: `i <= len` and `buf` was just confirmed to hold at
            // least `len + 1` bytes; the final iteration copies the NUL
            // that `format_pts_name` wrote at `len`.
            unsafe {
                *buf.add(i) = b;
            }
        }
        i = i.wrapping_add(1);
    }
    0
}
