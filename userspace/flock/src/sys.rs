//! The C library calls `flock.c` makes that std does not offer in the form
//! it needs, reached through the C ABI as the one-libc rule requires
//! (design-decisions §768): `open` without std's `O_CLOEXEC` (the lock must be
//! inherited by the command), `flock` on a descriptor number, `access`, and
//! `close`. And the two process facts: `exec` in place, and a wait status
//! as a shell reports it.
//!
//! On the Windows host the unit tests run on, none of these exist; each
//! answers "not supported", which no test reaches.

use std::ffi::OsStr;

/// Linux's numbers, which the SlateOS C library uses too.
pub const EINTR: i32 = 4;
pub const EIO: i32 = 5;
pub const EBADF: i32 = 9;
pub const ECHILD: i32 = 10;
/// `EWOULDBLOCK`, which is `EAGAIN`.
pub const EWOULDBLOCK: i32 = 11;
pub const ENOMEM: i32 = 12;
pub const EISDIR: i32 = 21;
pub const EINVAL: i32 = 22;
pub const ENFILE: i32 = 23;
pub const EMFILE: i32 = 24;
pub const ENOSPC: i32 = 28;
pub const EROFS: i32 = 30;
pub const ENOLCK: i32 = 37;

pub const O_RDONLY: i32 = 0;
pub const O_RDWR: i32 = 2;
pub const O_CREAT: i32 = 0o100;
pub const O_NOCTTY: i32 = 0o400;
pub const O_CLOEXEC: i32 = 0o2_000_000;

pub use imp::{access_rw, close, default_sigchld, exec, flock, open, status_code};

#[cfg(unix)]
mod imp {
    use super::OsStr;
    use std::ffi::{CString, c_char, c_int, c_uint};
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    use std::process::{Command, ExitStatus};

    /// `R_OK | W_OK`.
    const R_OK_W_OK: c_int = 4 | 2;

    mod ffi {
        use std::ffi::{c_char, c_int};

        unsafe extern "C" {
            pub fn open(path: *const c_char, flags: c_int, ...) -> c_int;
            pub fn flock(fd: c_int, operation: c_int) -> c_int;
            pub fn access(path: *const c_char, mode: c_int) -> c_int;
            pub fn close(fd: c_int) -> c_int;
            pub fn signal(signum: c_int, handler: usize) -> usize;
        }
    }

    /// `SIGCHLD`, and `SIG_DFL` as `sighandler_t`'s value.
    const SIGCHLD: c_int = 17;
    const SIG_DFL: usize = 0;

    /// A path as C sees it. An argv word cannot hold a NUL, so the error
    /// branch is unreachable; it answers as `open` would for a path that is
    /// not there.
    fn c_path(path: &OsStr) -> io::Result<CString> {
        CString::new(path.as_bytes()).map_err(|_| io::Error::from_raw_os_error(2))
    }

    /// `open(path, flags, mode)`: the descriptor, which is never closed by
    /// Rust -- the lock lives as long as the process, and the command
    /// inherits it.
    pub fn open(path: &OsStr, flags: i32, mode: u32) -> io::Result<i32> {
        let path = c_path(path)?;
        // SAFETY: `path` is NUL-terminated and outlives the call; the mode is
        // passed as the `mode_t` the variadic `open` reads when `O_CREAT` is
        // set, and ignored otherwise.
        let fd = unsafe { ffi::open(path.as_ptr(), flags, c_uint::from(mode)) };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(fd)
        }
    }

    /// `flock(fd, operation)`.
    pub fn flock(fd: i32, operation: i32) -> io::Result<()> {
        // SAFETY: `flock` takes plain integers; a descriptor that is not open
        // is `EBADF`, not undefined behaviour.
        if unsafe { ffi::flock(fd, operation) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// `access(path, R_OK | W_OK) == 0`.
    pub fn access_rw(path: &OsStr) -> bool {
        let Ok(path) = c_path(path) else {
            return false;
        };
        // SAFETY: `path` is NUL-terminated and outlives the call.
        unsafe { ffi::access(path.as_ptr().cast::<c_char>(), R_OK_W_OK) == 0 }
    }

    /// `close(fd)`, its result unread, as upstream leaves it.
    pub fn close(fd: i32) {
        // SAFETY: `fd` is the descriptor `open` returned, closed once, and
        // never used again: the caller replaces it at once.
        let _ = unsafe { ffi::close(fd) };
    }

    /// `signal(SIGCHLD, SIG_DFL)`: "clear any inherited settings" before the
    /// fork, as upstream does -- with `SIGCHLD` ignored, the kernel reaps the
    /// command itself and there would be no status to wait for.
    pub fn default_sigchld() {
        // SAFETY: `signal` with `SIG_DFL` installs no handler, so nothing of
        // ours can run in signal context; the previous disposition it
        // returns is not needed.
        let _ = unsafe { ffi::signal(SIGCHLD, SIG_DFL) };
    }

    /// `execvp` the command in place of this process. Returns only on
    /// failure.
    pub fn exec(command: &mut Command) -> io::Error {
        command.exec()
    }

    /// A wait status as upstream reports it: the exit status, or 128 plus
    /// the signal, or `EX_OSERR` for anything else.
    pub fn status_code(status: ExitStatus) -> u8 {
        if let Some(code) = status.code() {
            // An exit status is 0-255 already.
            return u8::try_from(code & 0xff).unwrap_or(u8::MAX);
        }
        match status.signal() {
            Some(sig) => u8::try_from(sig.saturating_add(128) & 0xff).unwrap_or(u8::MAX),
            None => super::super::EX_OSERR,
        }
    }
}

#[cfg(not(unix))]
mod imp {
    //! The Windows host has no advisory locks of this kind and no `exec`.
    use super::OsStr;
    use std::io;
    use std::process::{Command, ExitStatus};

    fn unsupported() -> io::Error {
        io::Error::from(io::ErrorKind::Unsupported)
    }

    pub fn open(_path: &OsStr, _flags: i32, _mode: u32) -> io::Result<i32> {
        Err(unsupported())
    }
    pub fn flock(_fd: i32, _operation: i32) -> io::Result<()> {
        Err(unsupported())
    }
    pub fn access_rw(_path: &OsStr) -> bool {
        false
    }
    pub fn close(_fd: i32) {}
    pub fn default_sigchld() {}
    pub fn exec(_command: &mut Command) -> io::Error {
        unsupported()
    }
    pub fn status_code(status: ExitStatus) -> u8 {
        status
            .code()
            .and_then(|c| u8::try_from(c & 0xff).ok())
            .unwrap_or(1)
    }
}
