//! The C library's calls this crate makes, declared by hand.
//!
//! **Through the C library, never as a `syscall` instruction.** Which table a
//! `syscall` reaches is decided for each process when its binary is loaded,
//! from the binary's markers (`kernel/src/proc/elf.rs`,
//! `detect_linux_abi`). A program built for SlateOS is a *native* program,
//! whose table numbers the calls differently: Linux's `write` (1) is
//! SlateOS's `exit`, `read` (0) its `yield`, `poll` (7) and `pipe2` (293)
//! other calls again. This crate used to issue Linux's numbers itself, so the
//! first frame a native program sent would have ended it
//! (`requests/c-f-linux-syscall-numbers-in-a-native-program-reach-other-calls.md`).
//! The C library is the one layer that is right in either kind of process:
//! glibc on a Linux host, lane D's (`posix/`) on SlateOS, which issues the
//! native calls a native program needs.
//!
//! Declared here rather than taken from a `libc` crate, which the workspace
//! does not carry, and in one place, so that each function has exactly one
//! declaration in the crate (two that disagreed would be undefined
//! behaviour, which `clashing_extern_declarations` exists to catch).
//!
//! Every function keeps the C library's convention: `-1` (or a null pointer)
//! on failure, with the reason in `errno` -- [`last_errno`] reads it.

use core::ffi::{c_int, c_ulong, c_void};

unsafe extern "C" {
    /// `ssize_t read(int fd, void *buf, size_t count)`.
    pub(crate) fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
    /// `ssize_t write(int fd, const void *buf, size_t count)`.
    pub(crate) fn write(fd: c_int, buf: *const c_void, count: usize) -> isize;
    /// `int close(int fd)`.
    pub(crate) fn close(fd: c_int) -> c_int;
    /// `int poll(struct pollfd *fds, nfds_t nfds, int timeout)`, `nfds_t`
    /// being `unsigned long`. `fds` points to `nfds` eight-byte
    /// `struct pollfd { int fd; short events; short revents; }`.
    pub(crate) fn poll(fds: *mut c_void, nfds: c_ulong, timeout: c_int) -> c_int;
    /// `int pipe2(int pipefd[2], int flags)`.
    pub(crate) fn pipe2(pipefd: *mut c_int, flags: c_int) -> c_int;
    /// `int fstat(int fd, struct stat *buf)`: x86-64's 144-byte
    /// `struct stat`, which lane D's `Stat` lays out as Linux does.
    #[cfg_attr(not(target_vendor = "slateos"), allow(dead_code))]
    pub(crate) fn fstat(fd: c_int, buf: *mut c_void) -> c_int;
}

/// The `errno` the C library left after a call that failed, as a positive
/// number; 0 if none can be read.
pub(crate) fn last_errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// `EINTR`: a signal cut the call short before it did anything.
pub(crate) const EINTR: i32 = 4;

/// `O_NONBLOCK`: Linux's value, which lane D's C library shares.
pub(crate) const O_NONBLOCK: c_int = 0o4000;

/// `O_CLOEXEC`: Linux's value, which lane D's C library shares.
pub(crate) const O_CLOEXEC: c_int = 0o2_000_000;
