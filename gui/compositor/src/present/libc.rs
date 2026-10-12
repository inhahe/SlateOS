//! The C library's calls the device modules make, declared by hand.
//!
//! **Through the C library, never as a `syscall` instruction.** Which table a
//! `syscall` reaches is decided for each process when its binary is loaded
//! (`kernel/src/proc/elf.rs`, `detect_linux_abi`), and a compositor built for
//! SlateOS is a *native* program, whose table numbers the calls differently:
//! Linux's `open` (2) is SlateOS's `task_id`, `ioctl` (16) its
//! `clock_adjtime`, `munmap` (11) its `sleep`. [`super::drm`] and
//! [`super::evdev`] used to issue Linux's numbers themselves, so the first
//! card they opened on SlateOS would have taken a task id for a descriptor
//! (`requests/c-f-linux-syscall-numbers-in-a-native-program-reach-other-calls.md`).
//! The C library is right in either kind of process: glibc on a Linux host,
//! where these modules drive a real card and keyboard, and lane D's on
//! SlateOS -- whose `ioctl` answers `ENOTTY` for a request it does not
//! handle, so a device SlateOS cannot drive yet is a device not found rather
//! than a clock set.
//!
//! The workspace carries no `libc` crate, and these are the only calls the
//! two modules make; each is declared once, here (two declarations that
//! disagreed would be undefined behaviour).
//!
//! `open` and `ioctl` are variadic in C, and declared so: calling a variadic
//! function through a fixed prototype is undefined, and glibc's are variadic.
//! Lane D's are not, which a variadic call reaches correctly -- the arguments
//! land in the same registers.

use core::ffi::{c_char, c_int, c_uint, c_ulong, c_void};

unsafe extern "C" {
    /// `int open(const char *path, int flags, ...)`; the mode only matters
    /// with `O_CREAT`, which nothing here passes, and is passed as 0.
    pub(super) fn open(path: *const c_char, flags: c_int, ...) -> c_int;
    /// `int close(int fd)`.
    pub(super) fn close(fd: c_int) -> c_int;
    /// `ssize_t read(int fd, void *buf, size_t count)`.
    pub(super) fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
    /// `int ioctl(int fd, unsigned long request, ...)`: here always with one
    /// pointer argument, the request's structure.
    pub(super) fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
    /// `void *mmap(void *addr, size_t length, int prot, int flags, int fd,
    /// off_t offset)`; `MAP_FAILED`, all ones, on failure.
    pub(super) fn mmap(
        addr: *mut c_void,
        length: usize,
        prot: c_int,
        flags: c_int,
        fd: c_int,
        offset: i64,
    ) -> *mut c_void;
    /// `int munmap(void *addr, size_t length)`.
    pub(super) fn munmap(addr: *mut c_void, length: usize) -> c_int;
}

/// `open`'s mode, for a call that creates nothing.
pub(super) const NO_MODE: c_uint = 0;

/// The `errno` the C library left after a call that failed, as a positive
/// number; `fallback` if none can be read.
pub(super) fn last_errno(fallback: i32) -> i32 {
    std::io::Error::last_os_error()
        .raw_os_error()
        .filter(|&e| e > 0)
        .unwrap_or(fallback)
}
