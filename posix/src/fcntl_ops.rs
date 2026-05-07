//! POSIX fcntl operations.
//!
//! Implements the `fcntl` function for file descriptor manipulation.
//! Currently supports `F_GETFD`/`F_SETFD` (fd-level flags like `FD_CLOEXEC`)
//! and `F_GETFL`/`F_SETFL` (file status flags).

use crate::errno;
use crate::fdtable;
use crate::types::*;

// ---------------------------------------------------------------------------
// fcntl commands
// ---------------------------------------------------------------------------

/// Duplicate fd (same as dup).
pub const F_DUPFD: i32 = 0;
/// Get fd flags (FD_CLOEXEC).
pub const F_GETFD: i32 = 1;
/// Set fd flags.
pub const F_SETFD: i32 = 2;
/// Get file status flags (O_RDONLY, O_NONBLOCK, etc.).
pub const F_GETFL: i32 = 3;
/// Set file status flags.
pub const F_SETFL: i32 = 4;
/// Duplicate fd with close-on-exec.
pub const F_DUPFD_CLOEXEC: i32 = 1030;

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// File control operations.
///
/// Performs various operations on open file descriptors.
///
/// Returns the result of the command, or -1 on error (errno set).
#[unsafe(no_mangle)]
pub extern "C" fn fcntl(fd: Fd, cmd: i32, arg: i64) -> i32 {
    // Verify the fd exists.
    if fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }

    match cmd {
        F_GETFD => {
            // Get per-fd flags.
            if let Some(flags) = fdtable::get_fd_flags(fd) {
                flags as i32
            } else {
                errno::set_errno(errno::EBADF);
                -1
            }
        }
        F_SETFD => {
            // Set per-fd flags (typically FD_CLOEXEC).
            if fdtable::set_fd_flags(fd, arg as u32) {
                0
            } else {
                errno::set_errno(errno::EBADF);
                -1
            }
        }
        F_GETFL => {
            // TODO: Track per-fd file status flags (O_NONBLOCK, O_APPEND).
            // For now, return 0 (read-only, no special flags).
            0
        }
        F_SETFL => {
            // TODO: Implement file status flag changes.
            // For now, silently accept (many programs set O_NONBLOCK).
            let _ = arg;
            0
        }
        F_DUPFD => {
            // Duplicate fd to lowest available >= arg.
            dup_fd_from(fd, arg as i32, false)
        }
        F_DUPFD_CLOEXEC => {
            // Duplicate fd to lowest available >= arg, with FD_CLOEXEC.
            dup_fd_from(fd, arg as i32, true)
        }
        _ => {
            errno::set_errno(errno::EINVAL);
            -1
        }
    }
}

/// Duplicate fd to lowest available >= `min_fd`.
fn dup_fd_from(oldfd: Fd, min_fd: i32, cloexec: bool) -> i32 {
    let Some(entry) = fdtable::get_fd(oldfd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };

    // For File handles, create a kernel-level duplicate.
    let new_handle = match entry.kind {
        fdtable::HandleKind::File => {
            let ret = crate::syscall::syscall1(crate::syscall::SYS_FS_DUP, entry.handle);
            if ret < 0 {
                return crate::errno::translate(ret) as i32;
            }
            ret as u64
        }
        fdtable::HandleKind::Console => entry.handle,
        fdtable::HandleKind::Pipe => {
            errno::set_errno(errno::ENOSYS);
            return -1;
        }
    };

    if let Some(new_fd) = fdtable::alloc_fd_from(min_fd, entry.kind, new_handle) {
        if cloexec {
            let _ = fdtable::set_fd_flags(new_fd, fdtable::FD_CLOEXEC);
        }
        new_fd
    } else {
        // Clean up the kernel handle if it's a file.
        if entry.kind == fdtable::HandleKind::File {
            let _ = crate::syscall::syscall1(crate::syscall::SYS_FS_CLOSE, new_handle);
        }
        errno::set_errno(errno::EMFILE);
        -1
    }
}
