//! The two C library calls `findmnt` makes that Rust's standard library does
//! not: `statvfs` for the SIZE, AVAIL, USED and USE% columns, and `poll` on
//! the mount table for `--poll`.
//!
//! On a host that is not Unix neither exists: `statvfs` fails with `ENOSYS`
//! (the columns are then empty, as upstream's are when `statvfs` fails), and
//! `poll` reports a timeout, so `--poll` ends as if nothing had changed. The
//! differential harness runs under WSL, where `cfg(unix)` holds.

use std::io;

/// What `findmnt` reads of `struct statvfs`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Statvfs {
    pub f_frsize: u64,
    pub f_blocks: u64,
    pub f_bfree: u64,
    pub f_bavail: u64,
}

/// `struct statvfs` as glibc and SlateOS's C library lay it out on x86_64:
/// the four fields read here come first but for `f_bsize`, and the tail is
/// generous, since `statvfs` takes no length and a buffer shorter than the
/// C library's struct would be written past.
#[cfg(unix)]
#[repr(C)]
#[derive(Default)]
struct CStatvfs {
    f_bsize: u64,
    f_frsize: u64,
    f_blocks: u64,
    f_bfree: u64,
    f_bavail: u64,
    _tail: [u64; 16],
}

/// `struct pollfd`.
#[cfg(unix)]
#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

#[cfg(unix)]
unsafe extern "C" {
    fn statvfs(path: *const u8, buf: *mut CStatvfs) -> i32;
    fn poll(fds: *mut PollFd, nfds: u64, timeout: i32) -> i32;
}

/// `POLLPRI`: what the kernel signals on a mount table that changed.
#[cfg(unix)]
const POLLPRI: i16 = 0x002;

/// `statvfs(path, &buf)`.
///
/// # Errors
///
/// `statvfs`'s `errno`; `EINVAL` for a path holding a NUL, which C would
/// have cut there -- no mount point holds one.
pub fn stat_vfs(path: &[u8]) -> io::Result<Statvfs> {
    if path.contains(&0) {
        return Err(io::Error::from_raw_os_error(22));
    }
    #[cfg(unix)]
    {
        let mut c = path.to_vec();
        c.push(0);
        let mut buf = CStatvfs::default();
        // SAFETY: `c` is NUL-terminated and outlives the call; `buf` is a live,
        // writable allocation at least as large as the C library's `struct
        // statvfs` (see `CStatvfs`'s `_tail`).
        let rc = unsafe { statvfs(c.as_ptr(), &raw mut buf) };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Statvfs {
            f_frsize: buf.f_frsize,
            f_blocks: buf.f_blocks,
            f_bfree: buf.f_bfree,
            f_bavail: buf.f_bavail,
        })
    }
    #[cfg(not(unix))]
    {
        Err(io::Error::from_raw_os_error(38))
    }
}

/// `poll({fd, POLLPRI}, 1, timeout)`: `Ok(0)` on the timeout, the number of
/// ready descriptors otherwise.
///
/// # Errors
///
/// `poll`'s `errno`.
#[cfg_attr(
    not(unix),
    allow(clippy::unnecessary_wraps, reason = "only the unix half can fail")
)]
pub fn poll_pri(file: &std::fs::File, timeout: i32) -> io::Result<i32> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let mut fds = PollFd {
            fd: file.as_raw_fd(),
            events: POLLPRI,
            revents: 0,
        };
        // SAFETY: `fds` is one live, writable `struct pollfd`, and the count
        // says one; the descriptor belongs to `file`, which outlives the call.
        let rc = unsafe { poll(&raw mut fds, 1, timeout) };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(rc)
    }
    #[cfg(not(unix))]
    {
        let _ = (file, timeout);
        Ok(0)
    }
}
