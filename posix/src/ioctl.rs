//! POSIX ioctl() and terminal control.
//!
//! Our kernel has no unified `ioctl` syscall.  This module handles
//! common ioctl requests in userspace by inspecting the fd's handle
//! kind and returning appropriate defaults or errors:
//!
//! - **`TIOCGWINSZ`**: returns default terminal dimensions for Console fds.
//! - **`TIOCSWINSZ`**: accepts (no-op) for Console fds.
//! - **`FIONBIO`**: non-blocking mode flag — accepted silently (our kernel
//!   I/O is currently all blocking).
//! - **`FIONREAD`**: bytes available to read — returns 0.
//! - **`TCGETS`/`TCSETS`**: termios get/set — returns/accepts defaults for
//!   Console fds.
//! - All other requests return `ENOTTY`.
//!
//! ## Terminal Model
//!
//! Our console is a framebuffer with VT100 escape sequence support.
//! There is no TTY device layer yet.  We fake enough termios to let
//! programs like `less`, `vim`, and `readline` function.  The default
//! termios reflects a cooked-mode terminal with echo enabled.
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
/// Translate NL to CR on input.
pub const INLCR: u32 = 0o100;
/// Ignore CR on input.
pub const IGNCR: u32 = 0o200;
/// Translate CR to NL on input.
pub const ICRNL: u32 = 0o400;

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
const DEFAULT_WINSIZE: Winsize = Winsize {
    ws_row: 25,
    ws_col: 80,
    ws_xpixel: 0,
    ws_ypixel: 0,
};

/// Build a default termios reflecting cooked mode with echo.
///
/// This matches a typical Linux terminal initial state: canonical
/// mode, echo enabled, CR→NL translation, common control characters.
fn default_termios() -> Termios {
    let mut cc = [0u8; NCCS];

    // Standard control character defaults (same as Linux).
    if let Some(slot) = cc.get_mut(VINTR) { *slot = 0x03; }   // Ctrl-C
    if let Some(slot) = cc.get_mut(VQUIT) { *slot = 0x1C; }   // Ctrl-backslash
    if let Some(slot) = cc.get_mut(VERASE) { *slot = 0x7F; }  // DEL
    if let Some(slot) = cc.get_mut(VKILL) { *slot = 0x15; }   // Ctrl-U
    if let Some(slot) = cc.get_mut(VEOF) { *slot = 0x04; }    // Ctrl-D
    if let Some(slot) = cc.get_mut(VSTART) { *slot = 0x11; }  // Ctrl-Q
    if let Some(slot) = cc.get_mut(VSTOP) { *slot = 0x13; }   // Ctrl-S
    if let Some(slot) = cc.get_mut(VSUSP) { *slot = 0x1A; }   // Ctrl-Z
    if let Some(slot) = cc.get_mut(VMIN) { *slot = 1; }       // min chars for read
    if let Some(slot) = cc.get_mut(VTIME) { *slot = 0; }      // no timeout

    Termios {
        c_iflag: ICRNL,                            // CR→NL on input
        c_oflag: OPOST | ONLCR,                    // post-process, NL→CRNL
        c_cflag: CS8 | CREAD | HUPCL | CLOCAL,     // 8-bit, receiver on
        c_lflag: ISIG | ICANON | ECHO | ECHONL | IEXTEN, // cooked mode + echo
        c_line: 0,
        c_cc: cc,
        c_ispeed: B38400,
        c_ospeed: B38400,
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
#[unsafe(no_mangle)]
pub extern "C" fn ioctl(fd: i32, request: u64, arg: *mut u8) -> i32 {
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };

    match request {
        TIOCGWINSZ => handle_tiocgwinsz(entry.kind, arg),
        TIOCSWINSZ => handle_tiocswinsz(entry.kind),
        FIONBIO => handle_fionbio(arg),
        FIONREAD => handle_fionread(entry.kind, arg),
        TCGETS => handle_tcgets(entry.kind, arg),
        TCSETS | TCSETSW | TCSETSF => handle_tcsets(entry.kind),
        _ => {
            errno::set_errno(errno::ENOTTY);
            -1
        }
    }
}

/// TIOCGWINSZ — get terminal window size.
fn handle_tiocgwinsz(kind: HandleKind, arg: *mut u8) -> i32 {
    if kind != HandleKind::Console {
        errno::set_errno(errno::ENOTTY);
        return -1;
    }
    if arg.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: Caller must provide a buffer large enough for Winsize.
    // Use write_unaligned since we don't know the alignment of arg.
    unsafe {
        core::ptr::write_unaligned(arg.cast::<Winsize>(), DEFAULT_WINSIZE);
    }
    0
}

/// TIOCSWINSZ — set terminal window size.
///
/// Accepted as a no-op for Console fds.  Our framebuffer console has a
/// fixed size determined by the display resolution.
fn handle_tiocswinsz(kind: HandleKind) -> i32 {
    if kind != HandleKind::Console {
        errno::set_errno(errno::ENOTTY);
        return -1;
    }
    // Accept silently — we can't resize the framebuffer console.
    0
}

/// FIONBIO — set/clear non-blocking I/O.
///
/// Accepted as a no-op for all fds.  Our kernel I/O is currently
/// all blocking; when non-blocking I/O is added, this will need
/// to set a per-fd flag.
fn handle_fionbio(arg: *mut u8) -> i32 {
    if arg.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // Accept silently — non-blocking mode is not yet implemented.
    // The argument is a pointer to an int (0 = blocking, nonzero = non-blocking).
    0
}

/// FIONREAD — get number of bytes available to read.
///
/// Returns 0 for Console fds (we don't buffer input), ENOTTY for
/// non-terminal fds (files don't support FIONREAD via ioctl; use
/// stat + seek instead).
fn handle_fionread(kind: HandleKind, arg: *mut u8) -> i32 {
    if arg.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    match kind {
        HandleKind::Console | HandleKind::Pipe => {
            // Return 0 bytes available.  A proper implementation would
            // query the kernel's input buffer.
            // SAFETY: arg must be at least sizeof(i32).
            unsafe {
                core::ptr::write_unaligned(arg.cast::<i32>(), 0);
            }
            0
        }
        _ => {
            errno::set_errno(errno::ENOTTY);
            -1
        }
    }
}

/// TCGETS — get termios attributes.
fn handle_tcgets(kind: HandleKind, arg: *mut u8) -> i32 {
    if kind != HandleKind::Console {
        errno::set_errno(errno::ENOTTY);
        return -1;
    }
    if arg.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    let t = default_termios();
    // SAFETY: Caller must provide a buffer large enough for Termios.
    unsafe {
        core::ptr::write_unaligned(arg.cast::<Termios>(), t);
    }
    0
}

/// TCSETS / TCSETSW / TCSETSF — set termios attributes.
///
/// Accepted as a no-op for Console fds.  We don't actually change
/// terminal behaviour (e.g., switching to raw mode) because our
/// console has no configurable line discipline.  Programs that set
/// raw mode will work via VT100 escape sequences instead.
fn handle_tcsets(kind: HandleKind) -> i32 {
    if kind != HandleKind::Console {
        errno::set_errno(errno::ENOTTY);
        return -1;
    }
    // Accept silently.
    0
}

// ---------------------------------------------------------------------------
// isatty()
// ---------------------------------------------------------------------------

/// Test whether a file descriptor refers to a terminal.
///
/// Returns 1 if `fd` is a Console fd, 0 otherwise (with errno set
/// to `ENOTTY`).
#[unsafe(no_mangle)]
pub extern "C" fn isatty(fd: i32) -> i32 {
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return 0;
    };

    if entry.kind == HandleKind::Console {
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
/// Returns "/dev/console\0" for Console fds, NULL otherwise.
#[unsafe(no_mangle)]
pub extern "C" fn ttyname(fd: i32) -> *const u8 {
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return core::ptr::null();
    };

    if entry.kind == HandleKind::Console {
        // SAFETY: This is a static byte string with a null terminator.
        c"/dev/console".as_ptr().cast::<u8>()
    } else {
        errno::set_errno(errno::ENOTTY);
        core::ptr::null()
    }
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
/// # Safety
///
/// `termios_p` must be non-null and point to a valid `Termios`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cfsetispeed(termios_p: *mut Termios, speed: u32) -> i32 {
    if termios_p.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // SAFETY: Caller guarantees termios_p is valid.
    unsafe { (*termios_p).c_ispeed = speed; }
    0
}

/// Set output baud rate in termios.
///
/// Returns 0 on success, -1 on error.
///
/// # Safety
///
/// `termios_p` must be non-null and point to a valid `Termios`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cfsetospeed(termios_p: *mut Termios, speed: u32) -> i32 {
    if termios_p.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // SAFETY: Caller guarantees termios_p is valid.
    unsafe { (*termios_p).c_ospeed = speed; }
    0
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
        // Exact size depends on alignment/padding; just verify it's reasonable.
        assert!(size >= 44 + NCCS, "Termios too small: {size}");
        assert!(size <= 64, "Termios too large: {size}");
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
        assert_eq!(unsafe { cfsetispeed(core::ptr::null_mut(), 0) }, -1);
        assert_eq!(unsafe { cfsetospeed(core::ptr::null_mut(), 0) }, -1);
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
}

// ---------------------------------------------------------------------------
// Pseudo-terminal stubs
// ---------------------------------------------------------------------------

/// Open a pseudo-terminal master device.
///
/// Stub: returns -1 with ENOSYS.  PTY support requires kernel /dev/ptmx.
#[unsafe(no_mangle)]
pub extern "C" fn posix_openpt(_oflag: i32) -> i32 {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

/// Grant access to the slave pseudo-terminal device.
///
/// Stub: returns 0 (success) since we don't enforce PTY permissions.
#[unsafe(no_mangle)]
pub extern "C" fn grantpt(_fd: i32) -> i32 {
    0
}

/// Unlock a pseudo-terminal master/slave pair.
///
/// Stub: returns 0 (success).
#[unsafe(no_mangle)]
pub extern "C" fn unlockpt(_fd: i32) -> i32 {
    0
}

/// Get the name of the slave pseudo-terminal device.
///
/// Stub: returns null (no PTY support).
#[unsafe(no_mangle)]
pub extern "C" fn ptsname(_fd: i32) -> *mut u8 {
    core::ptr::null_mut()
}

/// Thread-safe version of `ptsname`.
///
/// Stub: returns ENOSYS.
#[unsafe(no_mangle)]
pub extern "C" fn ptsname_r(_fd: i32, _buf: *mut u8, _buflen: usize) -> i32 {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}
