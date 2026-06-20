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
pub fn set_termios(new: Termios) {
    *CONSOLE_TERMIOS.lock() = new;
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
}
