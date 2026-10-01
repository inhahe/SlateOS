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

/// `ENOTTY`, and `ERANGE`: what upstream's terminal probe leaves in `errno`
/// on a host whose failure has no number, and a `strtol` overflow's.
const ENOTTY: i32 = 25;
const ERANGE: i32 = 34;

/// What `get_terminal_dimension` leaves in `errno`, from `errno` before it:
/// the `ioctl`'s failure (`ENOTTY` off a terminal), then -- for each size
/// asked for (`cols`, `lines`) and not given by the terminal -- 0 if the
/// variable (`COLUMNS`, `LINES`) is set, as `get_env_int` clears `errno`
/// before its `strtol`, or `ERANGE` if that overflows.
///
/// Upstream never looks at the result, but callers read `errno` later
/// without clearing it -- libsmartcols' `width=` column property does -- so
/// a port reproducing them has to know. `scols_new_table` asks for both
/// sizes; `get_terminal_width` for the columns.
#[must_use]
pub fn dimension_errno(errno: i32, cols: bool, lines: bool) -> i32 {
    let mut errno = errno;
    let (c, l) = match winsize() {
        Ok(size) => size,
        Err(e) => {
            errno = e.raw_os_error().unwrap_or(ENOTTY);
            (0, 0)
        }
    };
    for (wanted, size, name) in [(cols, c, "COLUMNS"), (lines, l, "LINES")] {
        if wanted
            && size == 0
            && let Some(value) = std::env::var_os(name)
        {
            errno = 0;
            let value = quoting::os_bytes(&value);
            if let Some(sc) = ulstrutils::scan_integer(&value, 10) {
                let limit: u128 = if sc.negative { 1 << 63 } else { (1 << 63) - 1 };
                if sc.saturated || sc.magnitude > limit {
                    errno = ERANGE;
                }
            }
        }
    }
    errno
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

/// What `setlocale(LC_ALL, "")` leaves in `errno`: `ENOENT` when some
/// category's locale has to be loaded -- a name other than `C` and `POSIX`,
/// from `LC_ALL`, the category's own variable or `LANG`, the first set and
/// not empty -- and 0 otherwise. Measured on glibc 2.39, where loading a
/// locale leaves the `ENOENT` of a failed lookup behind even when it
/// succeeds; a util-linux program that later reports an `errno` nothing had
/// set prints it -- `lsirq` on an empty `/proc/interrupts` says `cannot
/// read /proc/interrupts: No such file or directory` in C.UTF-8 and
/// `...: Success` in C.
#[must_use]
pub fn setlocale_errno() -> i32 {
    const ENOENT: i32 = 2;
    const CATEGORIES: [&str; 12] = [
        "LC_CTYPE",
        "LC_NUMERIC",
        "LC_TIME",
        "LC_COLLATE",
        "LC_MONETARY",
        "LC_MESSAGES",
        "LC_PAPER",
        "LC_NAME",
        "LC_ADDRESS",
        "LC_TELEPHONE",
        "LC_MEASUREMENT",
        "LC_IDENTIFICATION",
    ];
    let get = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    let loads = CATEGORIES.iter().any(|&category| {
        get("LC_ALL")
            .or_else(|| get(category))
            .or_else(|| get("LANG"))
            .is_some_and(|name| name != "C" && name != "POSIX")
    });
    if loads { ENOENT } else { 0 }
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
