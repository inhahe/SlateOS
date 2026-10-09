//! The descriptor table itself: putting a descriptor at a given number
//! (`dup2`), copying one above the standard three (`F_DUPFD_CLOEXEC`), whether
//! it survives an `exec` (`FD_CLOEXEC`), and closing one by number -- through
//! the linked C library.
//!
//! What a program needs when it arranges another program's standard
//! descriptors and then becomes that program, as `systemd-cat` does: std
//! opens everything close-on-exec and owns what it opens, and has no word for
//! "this socket is descriptor 1 from now on".
//!
//! On a host that is not unix every call answers [`ENOSYS`](crate::ENOSYS).

#[cfg(not(unix))]
use crate::ENOSYS;
#[cfg(unix)]
use crate::last_errno;

/// `fcntl`'s command for a copy at the lowest free number at or above the
/// argument, close-on-exec.
#[cfg(any(unix, test))]
const F_DUPFD_CLOEXEC: i32 = 1030;
/// `fcntl`'s commands for the descriptor's own flags.
#[cfg(any(unix, test))]
const F_GETFD: i32 = 1;
#[cfg(any(unix, test))]
const F_SETFD: i32 = 2;
/// The one descriptor flag: close on `exec`.
#[cfg(any(unix, test))]
const FD_CLOEXEC: i32 = 1;

#[cfg(unix)]
mod sys {
    unsafe extern "C" {
        pub fn dup2(old: i32, new: i32) -> i32;
        pub fn fcntl(fd: i32, cmd: i32, ...) -> i32;
        pub fn close(fd: i32) -> i32;
    }
}

/// Make descriptor `new` refer to what `old` refers to, closing whatever
/// `new` was first: `dup2`. `new` is not close-on-exec afterwards. When the
/// two are the same number nothing changes, as `dup2` itself does nothing.
///
/// # Errors
///
/// `EBADF` when `old` is not open or `new` is out of range, `EMFILE`;
/// [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn dup2(old: i32, new: i32) -> Result<(), i32> {
    dup2_one(old, new)
}

#[cfg(unix)]
fn dup2_one(old: i32, new: i32) -> Result<(), i32> {
    // SAFETY: two numbers; the call acts on the descriptor table only.
    let rc = unsafe { sys::dup2(old, new) };
    if rc < 0 { Err(last_errno()) } else { Ok(()) }
}

#[cfg(not(unix))]
fn dup2_one(_old: i32, _new: i32) -> Result<(), i32> {
    Err(ENOSYS)
}

/// A copy of `fd` at the lowest free number at or above `min`,
/// close-on-exec: `fcntl (fd, F_DUPFD_CLOEXEC, min)`. The copy is the
/// caller's to close.
///
/// # Errors
///
/// `EBADF` when `fd` is not open, `EINVAL` for a `min` out of range,
/// `EMFILE`; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn dup_above(fd: i32, min: i32) -> Result<i32, i32> {
    dup_above_one(fd, min)
}

#[cfg(unix)]
fn dup_above_one(fd: i32, min: i32) -> Result<i32, i32> {
    // SAFETY: numbers only; `F_DUPFD_CLOEXEC` reads its third argument as an
    // `int` and touches no memory.
    let copy = unsafe { sys::fcntl(fd, F_DUPFD_CLOEXEC, min) };
    if copy < 0 {
        Err(last_errno())
    } else {
        Ok(copy)
    }
}

#[cfg(not(unix))]
fn dup_above_one(_fd: i32, _min: i32) -> Result<i32, i32> {
    Err(ENOSYS)
}

/// Whether `fd` is closed by an `exec`: `FD_CLOEXEC` set or cleared with
/// `F_SETFD`, the descriptor's other flags (there are none today) kept.
///
/// # Errors
///
/// `EBADF` when `fd` is not open; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn set_close_on_exec(fd: i32, on: bool) -> Result<(), i32> {
    set_close_on_exec_one(fd, on)
}

#[cfg(unix)]
fn set_close_on_exec_one(fd: i32, on: bool) -> Result<(), i32> {
    // SAFETY: `F_GETFD` takes no third argument and touches no memory.
    let flags = unsafe { sys::fcntl(fd, F_GETFD) };
    if flags < 0 {
        return Err(last_errno());
    }
    let wanted = if on {
        flags | FD_CLOEXEC
    } else {
        flags & !FD_CLOEXEC
    };
    if wanted == flags {
        return Ok(());
    }
    // SAFETY: `F_SETFD` reads its third argument as an `int` and touches no
    // memory.
    let rc = unsafe { sys::fcntl(fd, F_SETFD, wanted) };
    if rc < 0 { Err(last_errno()) } else { Ok(()) }
}

#[cfg(not(unix))]
fn set_close_on_exec_one(_fd: i32, _on: bool) -> Result<(), i32> {
    Err(ENOSYS)
}

/// Close descriptor `fd`: `close`.
///
/// For a descriptor the caller knows only by number -- one inherited, or one
/// another call installed -- which nothing in the caller owns. Closing a
/// descriptor something else owns makes that owner close whatever is given
/// the number next, so the caller must be the only one that knows it.
///
/// # Errors
///
/// `EBADF` when `fd` is not open, `EINTR` or `EIO` from the close itself
/// (the descriptor is gone either way, as Linux defines it);
/// [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn close(fd: i32) -> Result<(), i32> {
    close_one(fd)
}

#[cfg(unix)]
fn close_one(fd: i32) -> Result<(), i32> {
    // SAFETY: a number; the caller's contract (above) is that nothing else
    // owns it.
    let rc = unsafe { sys::close(fd) };
    if rc < 0 { Err(last_errno()) } else { Ok(()) }
}

#[cfg(not(unix))]
fn close_one(_fd: i32) -> Result<(), i32> {
    Err(ENOSYS)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    /// The restated numbers are the library's.
    #[test]
    fn the_numbers_are_the_librarys() {
        assert_eq!(F_DUPFD_CLOEXEC, posix::fcntl_ops::F_DUPFD_CLOEXEC);
        assert_eq!(F_GETFD, posix::fcntl_ops::F_GETFD);
        assert_eq!(F_SETFD, posix::fcntl_ops::F_SETFD);
        assert_eq!(FD_CLOEXEC, 1);
    }

    #[cfg(unix)]
    #[test]
    fn a_copy_lands_above_its_floor_and_closes_on_exec() {
        extern crate std;
        use std::os::fd::AsRawFd;

        let file = std::fs::File::open("/dev/null").unwrap();
        let copy = dup_above(file.as_raw_fd(), 100).unwrap();
        assert!(copy >= 100);
        // Cleared and set again, each answered.
        set_close_on_exec(copy, false).unwrap();
        set_close_on_exec(copy, true).unwrap();
        // dup2 onto a number of our choosing, then both closed.
        let target = copy.checked_add(1).unwrap();
        dup2(copy, target).unwrap();
        close(target).unwrap();
        close(copy).unwrap();
        assert!(close(copy).is_err(), "closed twice");
    }

    #[cfg(not(unix))]
    #[test]
    fn off_unix_every_call_declines() {
        assert_eq!(dup2(0, 1), Err(ENOSYS));
        assert_eq!(dup_above(0, 3), Err(ENOSYS));
        assert_eq!(set_close_on_exec(0, true), Err(ENOSYS));
        assert_eq!(close(0), Err(ENOSYS));
    }
}
