//! POSIX unistd.h equivalents — miscellaneous functions.
//!
//! Functions that don't fit neatly into another category: `getcwd`,
//! `chdir`, `isatty`, `getuid`, `getgid`, `sysconf`, `write` to
//! stdout/stderr.

use crate::errno;
use crate::syscall::*;
use crate::types::*;

// ---------------------------------------------------------------------------
// Standard file descriptors
// ---------------------------------------------------------------------------

/// Standard input.
pub const STDIN_FILENO: Fd = 0;
/// Standard output.
pub const STDOUT_FILENO: Fd = 1;
/// Standard error.
pub const STDERR_FILENO: Fd = 2;

// ---------------------------------------------------------------------------
// sysconf names
// ---------------------------------------------------------------------------

/// Page size.
pub const _SC_PAGESIZE: i32 = 30;
/// Number of configured processors.
pub const _SC_NPROCESSORS_CONF: i32 = 83;
/// Number of online processors.
pub const _SC_NPROCESSORS_ONLN: i32 = 84;
/// Open max (max file descriptors).
pub const _SC_OPEN_MAX: i32 = 4;

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// Get current working directory.
///
/// Returns `buf` on success, NULL on error (errno set).
///
/// Note: Our kernel doesn't track per-process CWD yet.  This returns
/// "/" as a placeholder until per-process working directories are
/// implemented.
#[unsafe(no_mangle)]
pub extern "C" fn getcwd(buf: *mut u8, size: SizeT) -> *mut u8 {
    if buf.is_null() || size == 0 {
        errno::set_errno(errno::EINVAL);
        return core::ptr::null_mut();
    }

    // TODO: Implement proper CWD tracking per-process.
    // For now, return "/" as the default working directory.
    if size < 2 {
        errno::set_errno(errno::ERANGE);
        return core::ptr::null_mut();
    }

    unsafe {
        *buf = b'/';
        *buf.add(1) = 0;
    }
    buf
}

/// Change the current working directory.
///
/// Note: Our kernel doesn't track per-process CWD yet.
/// Returns -1 with ENOSYS.
#[unsafe(no_mangle)]
pub extern "C" fn chdir(_path: *const u8) -> i32 {
    // TODO: Implement per-process CWD tracking in the kernel.
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Test whether a file descriptor refers to a terminal.
///
/// Returns 1 if `fd` is a terminal, 0 otherwise.
#[unsafe(no_mangle)]
pub extern "C" fn isatty(fd: Fd) -> i32 {
    // Our console (fd 0, 1, 2) is always a terminal.
    if fd == STDIN_FILENO || fd == STDOUT_FILENO || fd == STDERR_FILENO {
        1
    } else {
        errno::set_errno(errno::ENOTTY);
        0
    }
}

/// Get the real user ID of the calling process.
///
/// Returns 0 (root) since we don't have multi-user support in
/// userspace yet.
#[unsafe(no_mangle)]
pub extern "C" fn getuid() -> UidT {
    0
}

/// Get the effective user ID of the calling process.
#[unsafe(no_mangle)]
pub extern "C" fn geteuid() -> UidT {
    0
}

/// Get the real group ID of the calling process.
#[unsafe(no_mangle)]
pub extern "C" fn getgid() -> GidT {
    0
}

/// Get the effective group ID of the calling process.
#[unsafe(no_mangle)]
pub extern "C" fn getegid() -> GidT {
    0
}

/// Get configurable system variables.
///
/// Returns the value of the named system variable, or -1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn sysconf(name: i32) -> i64 {
    match name {
        _SC_PAGESIZE => 16384, // Our OS uses 16 KiB pages.
        _SC_NPROCESSORS_CONF | _SC_NPROCESSORS_ONLN => {
            // TODO: Query actual CPU count from kernel.
            1
        }
        _SC_OPEN_MAX => 256, // Reasonable default.
        _ => {
            errno::set_errno(errno::EINVAL);
            -1
        }
    }
}

/// Write a message to standard error and abort.
///
/// Not exactly POSIX, but commonly needed by C runtime init code.
#[unsafe(no_mangle)]
pub extern "C" fn abort() -> ! {
    // Write "Aborted\n" to stderr (console).
    let msg = b"Aborted\n";
    let _ = syscall2(SYS_CONSOLE_WRITE, msg.as_ptr() as u64, msg.len() as u64);
    #[allow(clippy::used_underscore_items)] // _exit is the POSIX name.
    crate::process::_exit(134); // 128 + SIGABRT(6)
}
