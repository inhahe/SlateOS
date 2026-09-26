//! What libsmartcols asks of the terminal and the locale: util-linux's
//! `get_terminal_dimension` (`lib/ttyutils.c`), `isatty(STDOUT_FILENO)`, and
//! the `nl_langinfo(CODESET) == "UTF-8"` that chooses the tree's symbols.
//!
//! Public because a program ported on top of the table sometimes has to know
//! what the table asked: `lsmem` reports `errno` in a message, and what is in
//! it by then depends on whether [`Table::new`](crate::Table::new)'s
//! [`winsize`] failed.

use std::io::{self, IsTerminal};

/// `get_terminal_dimension(&cols, &lines)`: `TIOCGWINSZ` on stdout, and for a
/// size it does not give, `COLUMNS` / `LINES` -- each `None` when neither
/// says.
pub fn terminal_dimension() -> (Option<usize>, Option<usize>) {
    let (cols, rows) = winsize().unwrap_or((0, 0));
    let cols = (cols > 0)
        .then_some(usize::from(cols))
        .or_else(|| env_int("COLUMNS"));
    let rows = (rows > 0)
        .then_some(usize::from(rows))
        .or_else(|| env_int("LINES"));
    (cols, rows)
}

/// `ioctl(STDOUT_FILENO, TIOCGWINSZ)`: the terminal's columns and rows, or
/// why stdout has none -- `ENOTTY` when it is not a terminal, `EBADF` when it
/// is closed.
///
/// # Errors
///
/// The `ioctl`'s own. On a host without the call, `Unsupported`.
pub fn winsize() -> io::Result<(u16, u16)> {
    imp::winsize()
}

/// `get_env_int(name)`: the variable as a whole decimal `strtol` number,
/// positive and within `int`, else nothing.
pub fn env_int(name: &str) -> Option<usize> {
    let value = std::env::var_os(name)?;
    let n = ulstrutils::ul_strtos64(&quoting::os_bytes(&value), 10).ok()?;
    if n > 0 && n <= i64::from(i32::MAX) {
        usize::try_from(n).ok()
    } else {
        None
    }
}

/// `isatty(STDOUT_FILENO)`.
#[must_use]
pub fn stdout_is_tty() -> bool {
    std::io::stdout().is_terminal()
}

/// Whether `setlocale(LC_ALL, "")` would leave a UTF-8 codeset: the first
/// of `LC_ALL`, `LC_CTYPE` and `LANG` that is set and not empty names the
/// locale, and its codeset -- after the `.`, before any `@` -- is UTF-8 in
/// any of its spellings (`UTF-8`, `utf8`). No variable, `C` and `POSIX` are
/// the C locale, whose codeset is ASCII.
pub fn codeset_is_utf8() -> bool {
    let name = ["LC_ALL", "LC_CTYPE", "LANG"]
        .iter()
        .filter_map(std::env::var_os)
        .find(|v| !v.is_empty());
    let Some(name) = name else {
        return false;
    };
    let name = quoting::os_bytes(&name).into_owned();
    let Some(dot) = name.iter().position(|&b| b == b'.') else {
        return false;
    };
    let codeset: Vec<u8> = name
        .get(dot.saturating_add(1)..)
        .unwrap_or_default()
        .iter()
        .take_while(|&&b| b != b'@')
        .filter(|&&b| b != b'-')
        .map(u8::to_ascii_lowercase)
        .collect();
    codeset == b"utf8"
}

#[cfg(unix)]
mod imp {
    use std::ffi::{c_int, c_ulong};
    use std::io;

    /// `struct winsize`.
    #[repr(C)]
    #[derive(Default)]
    struct Winsize {
        ws_row: u16,
        ws_col: u16,
        ws_xpixel: u16,
        ws_ypixel: u16,
    }

    /// `TIOCGWINSZ`, on Linux and in the SlateOS C library.
    const TIOCGWINSZ: c_ulong = 0x5413;

    mod ffi {
        use std::ffi::{c_int, c_ulong};

        unsafe extern "C" {
            pub fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
        }
    }

    /// `ioctl(STDOUT_FILENO, TIOCGWINSZ, &w)`: columns and rows.
    pub fn winsize() -> io::Result<(u16, u16)> {
        let mut w = Winsize::default();
        let stdout: c_int = 1;
        // SAFETY: `TIOCGWINSZ` writes one `struct winsize` through the
        // pointer, which is valid, writable and of that type; on a descriptor
        // that is not a terminal the call fails and writes nothing.
        let rc = unsafe { ffi::ioctl(stdout, TIOCGWINSZ, &raw mut w) };
        if rc == 0 {
            Ok((w.ws_col, w.ws_row))
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

#[cfg(not(unix))]
mod imp {
    use std::io;

    /// The Windows host the unit tests run on: no terminal size, so the
    /// environment or the defaults decide.
    pub fn winsize() -> io::Result<(u16, u16)> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}
