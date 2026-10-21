//! The descriptor table itself: putting a descriptor at a given number
//! (`dup2`), copying one above the standard three (`F_DUPFD_CLOEXEC`), whether
//! it survives an `exec` (`FD_CLOEXEC`), and closing one by number -- through
//! the linked C library. And two questions asked of a descriptor: whether it
//! is a terminal (`isatty`), and whether it will take a write within a time
//! (`poll` for `POLLOUT`), which a program writing to other people's
//! terminals -- journald's broadcast -- asks of each. And `poll` itself, for
//! a program that waits on several descriptors, each for events of its own.
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

/// `poll`'s events: something to read, room to write, an error, a hang-up,
/// and a descriptor that is not open. The last three are reported whether
/// asked for or not.
pub const POLLIN: i16 = 0x0001;
/// Room to write.
pub const POLLOUT: i16 = 0x0004;
/// An error on the descriptor -- for a pipe's write end, no reader left.
pub const POLLERR: i16 = 0x0008;
/// The other end has gone: a pipe with no writer, a terminal hung up.
pub const POLLHUP: i16 = 0x0010;
/// The descriptor is not open.
pub const POLLNVAL: i16 = 0x0020;

/// One descriptor for [`poll`] to watch: C's `struct pollfd`. `events` is
/// what to wait for -- 0 for only what `poll` always reports, an error or a
/// hang-up -- and `revents` what came.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PollFd {
    /// The descriptor; a negative one is not watched.
    pub fd: i32,
    /// The events asked for.
    pub events: i16,
    /// The events that came, written by [`poll`].
    pub revents: i16,
}

/// Wait at most `timeout_ms` milliseconds (`-1` for ever) for any of `fds`
/// to have an event it asked for, or one `poll` always reports: `poll`. How
/// many have one -- each one's `revents` says which -- and 0 when the time
/// ran out.
///
/// For a program that waits on several descriptors, each for its own events
/// -- `rfkill event`'s device for `POLLIN | POLLHUP` and its standard output
/// for nothing, so that only a reader going away ends the wait.
///
/// # Errors
///
/// What `poll` reports: `EINTR` when a signal came first, `EINVAL`, `ENOMEM`;
/// [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn poll(fds: &mut [PollFd], timeout_ms: i32) -> Result<usize, i32> {
    poll_impl(fds, timeout_ms)
}

#[cfg(unix)]
fn poll_impl(fds: &mut [PollFd], timeout_ms: i32) -> Result<usize, i32> {
    let nfds = u64::try_from(fds.len()).map_err(|_| crate::EINVAL)?;
    // SAFETY: `fds` is `nfds` live `struct pollfd`s in a row -- `PollFd` is
    // `repr(C)` with C's fields -- which the call reads and writes `revents`
    // into, and keeps no pointer to.
    let rc = unsafe { sys::poll(fds.as_mut_ptr(), nfds, timeout_ms) };
    usize::try_from(rc).map_err(|_| last_errno())
}

#[cfg(not(unix))]
fn poll_impl(_fds: &mut [PollFd], _timeout_ms: i32) -> Result<usize, i32> {
    Err(ENOSYS)
}
/// `EBADF`.
#[cfg(unix)]
const EBADF: i32 = 9;

#[cfg(unix)]
mod sys {
    pub use super::PollFd;

    unsafe extern "C" {
        pub fn dup2(old: i32, new: i32) -> i32;
        pub fn fcntl(fd: i32, cmd: i32, ...) -> i32;
        pub fn close(fd: i32) -> i32;
        pub fn isatty(fd: i32) -> i32;
        pub fn poll(fds: *mut PollFd, nfds: u64, timeout: i32) -> i32;
        // `void *`, as the C library and Rust's runtime declare them.
        pub fn write(fd: i32, buf: *const core::ffi::c_void, count: usize) -> isize;
        pub fn read(fd: i32, buf: *mut core::ffi::c_void, count: usize) -> isize;
        pub fn pipe2(fds: *mut i32, flags: i32) -> i32;
        pub fn open(path: *const core::ffi::c_char, flags: i32, ...) -> i32;
        // Two `struct timespec`s, as four 64-bit words; only ever passed
        // null here.
        pub fn futimens(fd: i32, times: *const [i64; 4]) -> i32;
    }

    /// `O_RDONLY`.
    pub const O_RDONLY: i32 = 0;
    /// `O_CLOEXEC`, on Linux and in SlateOS's C library.
    pub const O_CLOEXEC: i32 = 0o2_000_000;
}

/// Open `path` for reading, once: `open (path, O_RDONLY | O_CLOEXEC)`. The
/// descriptor, the caller's to [`close`].
///
/// Once: an `open` that waits -- a FIFO with no writer yet -- and is
/// interrupted by a signal's handler answers `EINTR` rather than waiting
/// again, as the standard library's would, so that a program acting on its
/// signals where it waits can act on that one.
///
/// # Errors
///
/// What `open` reports: `ENOENT`, `EACCES`, `EINTR` and the rest;
/// [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn open_read(path: &core::ffi::CStr) -> Result<i32, i32> {
    open_read_one(path)
}

#[cfg(unix)]
fn open_read_one(path: &core::ffi::CStr) -> Result<i32, i32> {
    // SAFETY: a NUL-terminated path the call only reads, and two flags; no
    // third argument, which only `O_CREAT` and `O_TMPFILE` read.
    let fd = unsafe { sys::open(path.as_ptr(), sys::O_RDONLY | sys::O_CLOEXEC) };
    if fd >= 0 { Ok(fd) } else { Err(last_errno()) }
}

#[cfg(not(unix))]
fn open_read_one(_path: &core::ffi::CStr) -> Result<i32, i32> {
    Err(ENOSYS)
}

/// `O_CREAT`.
pub const O_CREAT: i32 = 0o100;

/// Open `path` for reading, creating it -- empty, with permissions `mode`
/// -- if it is not there: `open (path, O_CREAT | O_CLOEXEC | O_RDONLY,
/// mode)`, which the standard library will not do (it creates only for
/// writing). The descriptor, the caller's to [`close`].
///
/// # Errors
///
/// What `open` reports: `EACCES`, `ENOENT` for a missing directory, `EROFS`
/// and the rest; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn open_create_read(path: &core::ffi::CStr, mode: u32) -> Result<i32, i32> {
    #[cfg(unix)]
    {
        let flags = O_CREAT | sys::O_CLOEXEC | sys::O_RDONLY;
        // SAFETY: a NUL-terminated path the call only reads, the flags, and
        // the mode `O_CREAT` reads as the third argument, an `unsigned int`.
        let fd = unsafe { sys::open(path.as_ptr(), flags, mode) };
        if fd >= 0 { Ok(fd) } else { Err(last_errno()) }
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Err(ENOSYS)
    }
}

/// Set the file on `fd`'s access and change times to now: `futimens (fd,
/// NULL)` -- which, unlike setting given times (all the standard library
/// offers), anyone who may write the file may do, its owner or not.
///
/// # Errors
///
/// `EACCES` or `EPERM` without write permission, `EBADF`, `EROFS`;
/// [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn touch_now(fd: i32) -> Result<(), i32> {
    #[cfg(unix)]
    {
        // SAFETY: a null `times` asks for "now" for both times; nothing of
        // ours is read or written.
        if unsafe { sys::futimens(fd, core::ptr::null()) } == 0 {
            Ok(())
        } else {
            Err(last_errno())
        }
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
        Err(ENOSYS)
    }
}

/// `O_WRONLY`.
pub const O_WRONLY: i32 = 0o1;

/// Open `path` for writing, creating it -- empty, with permissions `mode`
/// -- if it is not there, close-on-exec: `open (path, O_CREAT | O_CLOEXEC |
/// O_WRONLY, mode)`. Unlike the standard library's, this does not ask for
/// `O_TRUNC` or anything else. The descriptor, the caller's to [`close`].
///
/// # Errors
///
/// What `open` reports; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn open_create_write(path: &core::ffi::CStr, mode: u32) -> Result<i32, i32> {
    #[cfg(unix)]
    {
        let flags = O_CREAT | sys::O_CLOEXEC | O_WRONLY;
        // SAFETY: as in `open_create_read`.
        let fd = unsafe { sys::open(path.as_ptr(), flags, mode) };
        if fd >= 0 { Ok(fd) } else { Err(last_errno()) }
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Err(ENOSYS)
    }
}

/// `O_RDWR`.
pub const O_RDWR: i32 = 0o2;
/// `O_NOCTTY`: opening a terminal does not make it the controlling one.
pub const O_NOCTTY: i32 = 0o400;

/// Open a terminal line for reading and writing as a getty opens it --
/// `open (path, O_RDWR | O_NOCTTY | O_NONBLOCK)`: not as the controlling
/// terminal, without waiting for carrier, and *not* closed on `exec`, since
/// it is to be the login's standard input. The lowest free descriptor, the
/// caller's to [`close`].
///
/// # Errors
///
/// What `open` reports: `ENOENT`, `EACCES`, `EBUSY` and the rest;
/// [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn open_line(path: &core::ffi::CStr) -> Result<i32, i32> {
    #[cfg(unix)]
    {
        let flags = O_RDWR | O_NOCTTY | crate::termios::O_NONBLOCK;
        // SAFETY: a NUL-terminated path the call only reads, and flags; no
        // third argument, which only `O_CREAT` and `O_TMPFILE` read.
        let fd = unsafe { sys::open(path.as_ptr(), flags) };
        if fd >= 0 { Ok(fd) } else { Err(last_errno()) }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(ENOSYS)
    }
}

/// Write `buf` to `fd` once: `write`. How many bytes went, which may be
/// fewer than `buf` holds.
///
/// For a caller that must write without the standard library's stream --
/// a curses screen, whose output has to be the bytes it means and nothing
/// else, at the moment it flushes them.
///
/// # Errors
///
/// What `write` reports: `EINTR`, `EAGAIN`, `EBADF`, `EPIPE`, `EIO` and the
/// rest; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn write(fd: i32, buf: &[u8]) -> Result<usize, i32> {
    write_one(fd, buf)
}

#[cfg(unix)]
fn write_one(fd: i32, buf: &[u8]) -> Result<usize, i32> {
    // SAFETY: `buf` is readable for its whole length, which is what the
    // call is told.
    let n = unsafe { sys::write(fd, buf.as_ptr().cast(), buf.len()) };
    usize::try_from(n).map_err(|_| last_errno())
}

#[cfg(not(unix))]
fn write_one(_fd: i32, _buf: &[u8]) -> Result<usize, i32> {
    Err(ENOSYS)
}

/// Write all of `buf` to `fd`: `write` again after a short write and after
/// one a signal interrupted -- and, with `retry_would_block`, after
/// `EAGAIN` as well, as ncurses' `_nc_flush` does -- until all of it is out.
/// A write of nothing, or any other failure, abandons the rest.
///
/// For a program that writes a terminal byte by byte through the C library
/// and has nowhere to report a terminal that has gone: `telnet`, `screen`,
/// a curses screen.
///
/// # Errors
///
/// The `errno` of the write that failed; 0 for a write of nothing.
pub fn write_all(fd: i32, buf: &[u8], retry_would_block: bool) -> Result<(), i32> {
    let mut rest = buf;
    while !rest.is_empty() {
        match write(fd, rest) {
            Ok(0) => return Err(0),
            Ok(n) => rest = rest.get(n..).unwrap_or_default(),
            Err(crate::EINTR) => {}
            Err(crate::EAGAIN) if retry_would_block => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// A pipe: `pipe`, as the read end and the write end. Neither is
/// close-on-exec, as `pipe` leaves them -- what a program handing one end to
/// a child it forks wants.
///
/// # Errors
///
/// `EMFILE`, `ENFILE`; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn pipe() -> Result<(i32, i32), i32> {
    pipe_one()
}

#[cfg(unix)]
fn pipe_one() -> Result<(i32, i32), i32> {
    let mut fds = [-1i32; 2];
    // SAFETY: `fds` is two writable `int`s, which is what the call fills.
    if unsafe { sys::pipe2(fds.as_mut_ptr(), 0) } == 0 {
        Ok((fds[0], fds[1]))
    } else {
        Err(last_errno())
    }
}

#[cfg(not(unix))]
fn pipe_one() -> Result<(i32, i32), i32> {
    Err(ENOSYS)
}

/// Read from `fd` once into `buf`: `read`. How many bytes came, 0 at the
/// end; never more than asked for, and nothing read ahead -- what a program
/// reading keys one at a time between `select`s needs, which a buffered
/// stream would take from under the next `select`.
///
/// # Errors
///
/// What `read` reports: `EINTR`, `EAGAIN`, `EBADF`, `EIO` and the rest;
/// [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn read(fd: i32, buf: &mut [u8]) -> Result<usize, i32> {
    read_one(fd, buf)
}

#[cfg(unix)]
fn read_one(fd: i32, buf: &mut [u8]) -> Result<usize, i32> {
    // SAFETY: `buf` is writable for its whole length, which is what the call
    // is told.
    let n = unsafe { sys::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
    usize::try_from(n).map_err(|_| last_errno())
}

#[cfg(not(unix))]
fn read_one(_fd: i32, _buf: &mut [u8]) -> Result<usize, i32> {
    Err(ENOSYS)
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

/// `fcntl`'s command for a copy at the lowest free number at or above the
/// argument, kept across `exec`.
#[cfg(any(unix, test))]
const F_DUPFD: i32 = 0;

/// `dup (fd)`: a copy of `fd` at the lowest free number, *kept* across
/// `exec` -- what a program arranging another's standard descriptors needs,
/// where [`dup_above`]'s copy would be gone in the program it runs. The
/// copy is the caller's to close.
///
/// # Errors
///
/// `EBADF` when `fd` is not open, `EMFILE`; [`ENOSYS`](crate::ENOSYS) off
/// Unix.
pub fn dup(fd: i32) -> Result<i32, i32> {
    #[cfg(unix)]
    {
        // SAFETY: numbers only; `F_DUPFD` reads its third argument as an
        // `int` and touches no memory.
        let copy = unsafe { sys::fcntl(fd, F_DUPFD, 0) };
        if copy < 0 {
            Err(last_errno())
        } else {
            Ok(copy)
        }
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
        Err(ENOSYS)
    }
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

/// Whether `fd` is a terminal: `isatty`. False for anything else, a
/// descriptor that is not open included, and off Unix.
#[must_use]
pub fn is_terminal(fd: i32) -> bool {
    is_terminal_one(fd)
}

#[cfg(unix)]
fn is_terminal_one(fd: i32) -> bool {
    // SAFETY: a number; the call asks the terminal layer about it and touches
    // no memory of ours.
    unsafe { sys::isatty(fd) == 1 }
}

#[cfg(not(unix))]
fn is_terminal_one(_fd: i32) -> bool {
    false
}

/// Wait at most `timeout_ms` milliseconds (`-1` for ever) for `fd` to have
/// something to read: `poll` for `POLLIN`. `Ok(true)` when it has -- or has
/// reached its end, failed or hung up, which is when `select` too calls a
/// descriptor readable and the read then says which -- and `Ok(false)` when
/// the time ran out.
///
/// # Errors
///
/// `EBADF` when `fd` is not open (`POLLNVAL`), `EINTR` when a signal came
/// first, `EINVAL`, `ENOMEM`; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn wait_readable(fd: i32, timeout_ms: i32) -> Result<bool, i32> {
    wait_readable_one(fd, timeout_ms)
}

/// The most descriptors [`wait_readable_among`] waits on at once.
pub const WAIT_AMONG_MAX: usize = 8;

/// [`wait_readable`] for several descriptors at once: one `poll` for
/// `POLLIN` on each of `fds`, which ends as soon as any of them has
/// something to read. Which ones have, as a mask -- bit `k` for `fds[k]` --
/// and 0 when the time ran out.
///
/// For a program that waits on its terminal and on a pipe its signal
/// handlers write to, so that a signal ends the wait however close to its
/// start it came.
///
/// # Errors
///
/// As [`wait_readable`]: `EBADF` when one of `fds` is not open, `EINTR`
/// when a signal came first, `ENOMEM`; `EINVAL` for more than
/// [`WAIT_AMONG_MAX`] descriptors; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn wait_readable_among(fds: &[i32], timeout_ms: i32) -> Result<u32, i32> {
    wait_readable_among_impl(fds, timeout_ms)
}

#[cfg(unix)]
fn wait_readable_among_impl(fds: &[i32], timeout_ms: i32) -> Result<u32, i32> {
    if fds.len() > WAIT_AMONG_MAX {
        return Err(crate::EINVAL);
    }
    let mut polls = [const {
        sys::PollFd {
            fd: -1,
            events: POLLIN,
            revents: 0,
        }
    }; WAIT_AMONG_MAX];
    for (p, &fd) in polls.iter_mut().zip(fds) {
        p.fd = fd;
    }
    let nfds = u64::try_from(fds.len()).unwrap_or(0);
    // SAFETY: `polls` holds `WAIT_AMONG_MAX` live `struct pollfd`s in a
    // row, of which the call reads and writes the first `nfds`.
    let rc = unsafe { sys::poll(polls.as_mut_ptr(), nfds, timeout_ms) };
    if rc < 0 {
        return Err(last_errno());
    }
    let mut mask = 0u32;
    for (k, p) in polls.iter().take(fds.len()).enumerate() {
        if p.revents & POLLNVAL != 0 {
            return Err(EBADF);
        }
        if p.revents != 0 {
            mask |= 1 << k;
        }
    }
    Ok(mask)
}

#[cfg(not(unix))]
fn wait_readable_among_impl(_fds: &[i32], _timeout_ms: i32) -> Result<u32, i32> {
    Err(ENOSYS)
}

#[cfg(unix)]
fn wait_readable_one(fd: i32, timeout_ms: i32) -> Result<bool, i32> {
    let mut p = sys::PollFd {
        fd,
        events: POLLIN,
        revents: 0,
    };
    // SAFETY: `p` is one live `struct pollfd`, which the call reads and
    // writes `revents` into; `nfds` says one.
    let rc = unsafe { sys::poll(&raw mut p, 1, timeout_ms) };
    if rc < 0 {
        return Err(last_errno());
    }
    if rc == 0 {
        return Ok(false);
    }
    if p.revents & POLLNVAL != 0 {
        return Err(EBADF);
    }
    Ok(true)
}

#[cfg(not(unix))]
fn wait_readable_one(_fd: i32, _timeout_ms: i32) -> Result<bool, i32> {
    Err(ENOSYS)
}

/// Wait at most `timeout_ms` milliseconds (`-1` for ever) for `fd` to take a
/// write: `poll` for `POLLOUT`, as systemd's `fd_wait_for_event` asks it.
/// `Ok(true)` when it will -- or when it has failed or hung up, which the
/// write itself then reports -- and `Ok(false)` when the time ran out.
///
/// # Errors
///
/// `EBADF` when `fd` is not open (`POLLNVAL`), `EINTR` when a signal came
/// first, `EINVAL`, `ENOMEM`; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn wait_writable(fd: i32, timeout_ms: i32) -> Result<bool, i32> {
    wait_writable_one(fd, timeout_ms)
}

#[cfg(unix)]
fn wait_writable_one(fd: i32, timeout_ms: i32) -> Result<bool, i32> {
    let mut p = sys::PollFd {
        fd,
        events: POLLOUT,
        revents: 0,
    };
    // SAFETY: `p` is one live `struct pollfd`, which the call reads and
    // writes `revents` into; `nfds` says one.
    let rc = unsafe { sys::poll(&raw mut p, 1, timeout_ms) };
    if rc < 0 {
        return Err(last_errno());
    }
    if rc == 0 {
        return Ok(false);
    }
    if p.revents & POLLNVAL != 0 {
        return Err(EBADF);
    }
    Ok(true)
}

#[cfg(not(unix))]
fn wait_writable_one(_fd: i32, _timeout_ms: i32) -> Result<bool, i32> {
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
        assert_eq!(F_DUPFD, posix::fcntl_ops::F_DUPFD);
        assert_eq!(F_GETFD, posix::fcntl_ops::F_GETFD);
        assert_eq!(F_SETFD, posix::fcntl_ops::F_SETFD);
        assert_eq!(FD_CLOEXEC, 1);
        assert_eq!(POLLIN, posix::poll::POLLIN);
        assert_eq!(POLLOUT, posix::poll::POLLOUT);
        assert_eq!(POLLNVAL, posix::poll::POLLNVAL);
        assert_eq!(POLLERR, posix::poll::POLLERR);
        assert_eq!(POLLHUP, posix::poll::POLLHUP);
        assert_eq!(core::mem::size_of::<PollFd>(), 8, "C's struct pollfd");
        assert_eq!(O_RDWR, posix::fcntl::O_RDWR);
        assert_eq!(O_NOCTTY, posix::fcntl::O_NOCTTY);
        assert_eq!(O_CREAT, posix::fcntl::O_CREAT);
        assert_eq!(O_WRONLY, posix::fcntl::O_WRONLY);
    }

    /// Each descriptor is woken by what it asked for, and by an error or a
    /// hang-up whether it asked or not.
    #[cfg(unix)]
    #[test]
    fn poll_reports_each_descriptor_s_own_events() {
        let (r, w) = pipe().unwrap();
        let mut fds = [
            PollFd {
                fd: r,
                events: POLLIN | POLLHUP,
                revents: 0,
            },
            PollFd {
                fd: w,
                events: 0,
                revents: 0,
            },
        ];
        assert_eq!(poll(&mut fds, 0), Ok(0), "nothing to read, nothing wrong");
        write(w, b"x").unwrap();
        assert_eq!(poll(&mut fds, 0), Ok(1));
        assert_eq!(fds[0].revents, POLLIN);
        assert_eq!(fds[1].revents, 0, "room to write was not asked about");
        close(r).unwrap();
        fds[0].fd = -1;
        assert_eq!(poll(&mut fds, 0), Ok(1));
        assert_eq!(fds[1].revents, POLLERR, "no reader left");
        close(w).unwrap();
    }

    /// A file opened for writing is created as asked, not emptied when it
    /// is there, and its times set to now.
    #[cfg(unix)]
    #[test]
    fn a_file_is_touched_to_now() {
        extern crate std;
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(std::format!("libcall-touch-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("trigger");
        std::fs::write(&path, b"kept").unwrap();
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(old)
            .unwrap();
        let c = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
        let fd = open_create_write(&c, 0o600).unwrap();
        touch_now(fd).unwrap();
        let _ = close(fd);
        let meta = std::fs::metadata(&path).unwrap();
        assert_eq!(meta.len(), 4);
        assert!(meta.modified().unwrap() > old);
        std::fs::remove_file(&path).unwrap();
        let fd = open_create_write(&c, 0o640).unwrap();
        let _ = close(fd);
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640 & !umask()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The process's umask, which a created file's mode passes through:
    /// read, not set, since setting it would race the other tests' threads.
    #[cfg(unix)]
    fn umask() -> u32 {
        extern crate std;
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find_map(|l| l.strip_prefix("Umask:"))
                    .and_then(|v| u32::from_str_radix(v.trim(), 8).ok())
            })
            .unwrap_or(0o022)
    }

    /// A file opened for reading is created if need be, with the mode
    /// asked for, and opened again as it is.
    #[cfg(unix)]
    #[test]
    fn a_file_is_created_to_be_read() {
        extern crate std;
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(std::format!("libcall-create-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("trigger");
        let c = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
        let fd = open_create_read(&c, 0o600).unwrap();
        let _ = close(fd);
        let meta = std::fs::metadata(&path).unwrap();
        assert_eq!(
            (meta.len(), meta.permissions().mode() & 0o777),
            (0, 0o600 & !umask())
        );
        let fd = open_create_read(&c, 0o644).unwrap();
        let _ = close(fd);
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600 & !umask()
        );
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            open_create_read(c"/nonexistent/dir/trigger", 0o600),
            Err(crate::ENOENT)
        );
    }

    /// A line opened as a getty opens it is read-write and survives `exec`;
    /// a path that is not there is refused.
    #[cfg(unix)]
    #[test]
    fn a_line_is_opened_read_write_and_kept_across_exec() {
        let fd = open_line(c"/dev/null").unwrap();
        let fl = crate::termios::status_flags(fd).unwrap();
        assert_eq!(fl & 0o3, O_RDWR);
        assert_ne!(fl & crate::termios::O_NONBLOCK, 0);
        // SAFETY: `F_GETFD` takes no third argument.
        let fdflags = unsafe { sys::fcntl(fd, F_GETFD) };
        assert_eq!(fdflags & FD_CLOEXEC, 0);
        let _ = close(fd);
        assert_eq!(open_line(c"/nonexistent/line"), Err(crate::ENOENT));
    }

    #[cfg(unix)]
    #[test]
    fn a_file_opened_for_reading_reads_and_a_missing_one_says_so() {
        assert_eq!(sys::O_CLOEXEC, posix::fcntl::O_CLOEXEC);
        assert_eq!(sys::O_RDONLY, posix::fcntl::O_RDONLY);
        let fd = open_read(c"/proc/self/stat").unwrap();
        let mut buf = [0u8; 64];
        let n = read(fd, &mut buf).unwrap();
        assert!(n > 0, "something read");
        // SAFETY: `F_GETFD` takes no third argument and touches no memory.
        let flags = unsafe { sys::fcntl(fd, F_GETFD) };
        assert_eq!(flags & FD_CLOEXEC, FD_CLOEXEC, "closed on exec");
        close(fd).unwrap();
        assert_eq!(open_read(c"/nonexistent/at/all"), Err(2), "ENOENT");
    }

    #[cfg(not(unix))]
    #[test]
    fn nothing_opens_off_unix() {
        assert_eq!(open_read(c"anything"), Err(crate::ENOSYS));
    }

    #[cfg(unix)]
    #[test]
    fn a_file_is_no_terminal_and_takes_a_write_at_once() {
        extern crate std;
        use std::os::fd::AsRawFd;

        let file = std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/null")
            .unwrap();
        assert!(!is_terminal(file.as_raw_fd()));
        assert!(!is_terminal(-1));
        assert_eq!(wait_writable(file.as_raw_fd(), 0), Ok(true));
        // A number nothing has open. Taking one and closing it would race
        // the test above, which `dup2`s onto the number after its own copy
        // -- tests share one descriptor table and run at once.
        assert_eq!(wait_writable(1 << 20, 0), Err(EBADF), "not open");
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

    /// A plain copy is kept across `exec`, where `dup_above`'s is not.
    #[cfg(unix)]
    #[test]
    fn a_plain_copy_is_kept_across_exec() {
        extern crate std;
        use std::os::fd::AsRawFd;

        let file = std::fs::File::open("/dev/null").unwrap();
        let copy = dup(file.as_raw_fd()).unwrap();
        // SAFETY: `F_GETFD` takes no third argument.
        let flags = unsafe { sys::fcntl(copy, F_GETFD) };
        assert_eq!(flags & FD_CLOEXEC, 0);
        let above = dup_above(file.as_raw_fd(), 0).unwrap();
        // SAFETY: as above.
        let flags = unsafe { sys::fcntl(above, F_GETFD) };
        assert_eq!(flags & FD_CLOEXEC, FD_CLOEXEC);
        close(copy).unwrap();
        close(above).unwrap();
        assert_eq!(dup(-1), Err(EBADF));
    }

    #[cfg(unix)]
    #[test]
    fn a_wait_among_says_which_one_has_something() {
        let (r1, w1) = pipe().unwrap();
        let (r2, w2) = pipe().unwrap();
        assert_eq!(wait_readable_among(&[r1, r2], 0), Ok(0));
        assert_eq!(write(w2, b"x"), Ok(1));
        assert_eq!(wait_readable_among(&[r1, r2], -1), Ok(0b10));
        assert_eq!(wait_readable_among(&[r1, 1 << 20], 0), Err(EBADF));
        assert_eq!(wait_readable_among(&[r1; 9], 0), Err(crate::EINVAL));
        for fd in [r1, w1, r2, w2] {
            close(fd).unwrap();
        }
    }

    #[cfg(not(unix))]
    #[test]
    fn off_unix_every_call_declines() {
        assert_eq!(wait_readable_among(&[0], 0), Err(ENOSYS));
        assert_eq!(dup2(0, 1), Err(ENOSYS));
        assert_eq!(dup_above(0, 3), Err(ENOSYS));
        assert_eq!(set_close_on_exec(0, true), Err(ENOSYS));
        assert_eq!(close(0), Err(ENOSYS));
        assert!(!is_terminal(0));
        assert_eq!(wait_writable(1, 0), Err(ENOSYS));
    }
}
