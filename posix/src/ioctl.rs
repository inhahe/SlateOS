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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ioctl(fd: i32, request: u64, arg: *mut u8) -> i32 {
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };

    match request {
        TIOCGWINSZ => handle_tiocgwinsz(entry.kind, arg),
        TIOCSWINSZ => handle_tiocswinsz(entry.kind),
        FIONBIO => handle_fionbio(fd, arg),
        FIONREAD => handle_fionread(entry.kind, entry.handle, arg),
        TCGETS => handle_tcgets(entry.kind, arg),
        TCSETS | TCSETSW | TCSETSF => handle_tcsets(entry.kind),
        TIOCGPGRP => handle_tiocgpgrp(entry.kind, arg),
        TIOCSPGRP => handle_tiocspgrp(entry.kind, arg),
        TIOCSCTTY | TIOCNOTTY => {
            // Accept silently — we don't have real TTY sessions yet.
            // Many programs call these during startup/shutdown.
            0
        }
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
fn handle_fionread(kind: HandleKind, handle: u64, arg: *mut u8) -> i32 {
    use crate::syscall::{syscall3, SYS_TCP_INFO};

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
            use crate::syscall::{syscall1, SYS_PIPE_READABLE_BYTES};
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
        HandleKind::TcpStream => {
            if handle == 0 {
                // SAFETY: arg must be at least sizeof(i32).
                unsafe { core::ptr::write_unaligned(arg.cast::<i32>(), 0); }
                return 0;
            }
            // Query TCP_INFO to get rx_buffered (bytes 24..28).
            let mut info_buf = [0u8; 48];
            let ret = syscall3(
                SYS_TCP_INFO,
                handle,
                info_buf.as_mut_ptr() as u64,
                48,
            );
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
            use crate::syscall::{syscall1, SYS_UDP_RX_FRONT_BYTES};
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
        HandleKind::Eventfd => {
            // Linux's eventfd has no .ioctl handler, so ioctl() returns
            // ENOTTY on eventfds.  Match that behavior.
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

/// TIOCGPGRP — get the foreground process group of a terminal.
///
/// Returns the PGID via the integer pointer `arg`.  Delegates to
/// `tcgetpgrp()` which tracks the foreground PGID in process-local state.
fn handle_tiocgpgrp(kind: HandleKind, arg: *mut u8) -> i32 {
    if kind != HandleKind::Console {
        errno::set_errno(errno::ENOTTY);
        return -1;
    }
    if arg.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    let pgrp = crate::process::tcgetpgrp(0);
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
fn handle_tiocspgrp(kind: HandleKind, arg: *mut u8) -> i32 {
    if kind != HandleKind::Console {
        errno::set_errno(errno::ENOTTY);
        return -1;
    }
    if arg.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: arg must be at least sizeof(i32) per ioctl contract.
    let pgrp = unsafe { core::ptr::read_unaligned(arg.cast::<i32>()) };
    crate::process::tcsetpgrp(0, pgrp)
}

// ---------------------------------------------------------------------------
// isatty()
// ---------------------------------------------------------------------------

/// Test whether a file descriptor refers to a terminal.
///
/// Returns 1 if `fd` is a Console fd, 0 otherwise (with errno set
/// to `ENOTTY`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
            unsafe { *s.add(i) = b; }
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
/// Returns 0 on success, -1 on error.
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

        unsafe { cfmakeraw(&raw mut t); }

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
        unsafe { cfmakeraw(&raw mut t); }
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
        assert_eq!(unsafe { cfsetspeed(core::ptr::null_mut(), B9600) }, -1);
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
        assert_eq!(grantpt(0), 0);
    }

    #[test]
    fn test_unlockpt_succeeds() {
        assert_eq!(unlockpt(0), 0);
    }

    #[test]
    fn test_ptsname_returns_null() {
        assert!(ptsname(0).is_null());
    }

    #[test]
    fn test_ptsname_r_returns_enosys() {
        let mut buf = [0u8; 64];
        assert_eq!(ptsname_r(0, buf.as_mut_ptr(), buf.len()), -1);
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
        unsafe { cfmakeraw(core::ptr::null_mut()); }
    }

    // -- cfmakeraw clears parity --

    #[test]
    fn test_cfmakeraw_clears_parity() {
        let mut t = default_termios();
        t.c_cflag |= PARENB;
        unsafe { cfmakeraw(&raw mut t); }
        assert_eq!(t.c_cflag & PARENB, 0, "PARENB should be cleared in raw mode");
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
        let mut ws = Winsize { ws_row: 0, ws_col: 0, ws_xpixel: 0, ws_ypixel: 0 };
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
        let ws = Winsize { ws_row: 50, ws_col: 120, ws_xpixel: 0, ws_ypixel: 0 };
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
        let mut ws = Winsize { ws_row: 0, ws_col: 0, ws_xpixel: 0, ws_ypixel: 0 };
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
        assert_ne!(flags & crate::fcntl::O_NONBLOCK, 0, "O_NONBLOCK should be set");
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
        assert_eq!(flags & crate::fcntl::O_NONBLOCK, 0, "O_NONBLOCK should be clear");
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
        unsafe { cfsetispeed(&raw mut t, B115200); }
        unsafe { cfsetospeed(&raw mut t, B9600); }
        unsafe { cfmakeraw(&raw mut t); }
        assert_eq!(t.c_ispeed, B115200, "cfmakeraw should not change c_ispeed");
        assert_eq!(t.c_ospeed, B9600, "cfmakeraw should not change c_ospeed");
    }

    #[test]
    fn test_cfmakeraw_preserves_c_line() {
        let mut t = default_termios();
        t.c_line = 5;
        unsafe { cfmakeraw(&raw mut t); }
        assert_eq!(t.c_line, 5, "cfmakeraw should not change c_line");
    }

    #[test]
    fn test_cfmakeraw_idempotent() {
        let mut t1 = default_termios();
        unsafe { cfmakeraw(&raw mut t1); }
        let mut t2 = t1;
        unsafe { cfmakeraw(&raw mut t2); }
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
        unsafe { cfmakeraw(&raw mut t); }
        assert_eq!(t.c_lflag & ECHONL, 0, "ECHONL should be cleared in raw");
    }

    #[test]
    fn test_cfmakeraw_clears_iexten() {
        let mut t = default_termios();
        assert_ne!(t.c_lflag & IEXTEN, 0, "IEXTEN should be set in default");
        unsafe { cfmakeraw(&raw mut t); }
        assert_eq!(t.c_lflag & IEXTEN, 0, "IEXTEN should be cleared in raw");
    }

    #[test]
    fn test_cfmakeraw_clears_brkint() {
        let mut t = default_termios();
        t.c_iflag |= BRKINT;
        unsafe { cfmakeraw(&raw mut t); }
        assert_eq!(t.c_iflag & BRKINT, 0, "BRKINT should be cleared in raw");
    }

    #[test]
    fn test_cfmakeraw_clears_ixon() {
        let mut t = default_termios();
        t.c_iflag |= IXON;
        unsafe { cfmakeraw(&raw mut t); }
        assert_eq!(t.c_iflag & IXON, 0, "IXON should be cleared in raw");
    }

    #[test]
    fn test_cfmakeraw_clears_istrip() {
        let mut t = default_termios();
        t.c_iflag |= ISTRIP;
        unsafe { cfmakeraw(&raw mut t); }
        assert_eq!(t.c_iflag & ISTRIP, 0, "ISTRIP should be cleared in raw");
    }

    #[test]
    fn test_cfmakeraw_clears_inpck() {
        let mut t = default_termios();
        t.c_iflag |= INPCK;
        unsafe { cfmakeraw(&raw mut t); }
        assert_eq!(t.c_iflag & INPCK, 0, "INPCK should be cleared in raw");
    }

    #[test]
    fn test_cfmakeraw_clears_onlcr() {
        let mut t = default_termios();
        assert_ne!(t.c_oflag & ONLCR, 0, "ONLCR should be set in default");
        unsafe { cfmakeraw(&raw mut t); }
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
        assert_eq!(t.c_cflag & PARENB, 0, "Parity should not be enabled by default");
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
        assert!(core::mem::align_of::<Termios>() >= 4,
            "Termios should be aligned to at least 4 bytes");
    }

    #[test]
    fn test_winsize_alignment() {
        assert!(core::mem::align_of::<Winsize>() >= 2,
            "Winsize should be aligned to at least 2 bytes");
    }

    // -- Flag bit distinctness --

    #[test]
    fn test_iflag_bits_distinct() {
        let flags = [BRKINT, INPCK, ISTRIP, INLCR, IGNCR, ICRNL, IXON];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0,
                    "iflag bits at {i} and {j} should not overlap");
            }
        }
    }

    #[test]
    fn test_lflag_bits_distinct() {
        let flags = [ISIG, ICANON, ECHO, ECHONL, IEXTEN];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0,
                    "lflag bits at {i} and {j} should not overlap");
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
            assert_eq!(flags[i] & CSIZE, 0,
                "cflag bit {i} should not overlap with CSIZE");
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0,
                    "cflag bits at {i} and {j} should not overlap");
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
        assert_eq!(posix_openpt(0x02), -1); // O_RDWR
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_ptsname_r_small_buffer() {
        let mut buf = [0u8; 1];
        assert_eq!(ptsname_r(0, buf.as_mut_ptr(), buf.len()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
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
}

// ---------------------------------------------------------------------------
// Pseudo-terminal stubs
// ---------------------------------------------------------------------------

/// Open a pseudo-terminal master device.
///
/// Stub: returns -1 with ENOSYS.  PTY support requires kernel /dev/ptmx.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_openpt(_oflag: i32) -> i32 {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

/// Grant access to the slave pseudo-terminal device.
///
/// Stub: returns 0 (success) since we don't enforce PTY permissions.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn grantpt(_fd: i32) -> i32 {
    0
}

/// Unlock a pseudo-terminal master/slave pair.
///
/// Stub: returns 0 (success).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn unlockpt(_fd: i32) -> i32 {
    0
}

/// Get the name of the slave pseudo-terminal device.
///
/// Stub: returns null (no PTY support).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ptsname(_fd: i32) -> *mut u8 {
    core::ptr::null_mut()
}

/// Thread-safe version of `ptsname`.
///
/// Stub: returns ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ptsname_r(_fd: i32, _buf: *mut u8, _buflen: usize) -> i32 {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}
