//! POSIX fcntl operations.
//!
//! Implements the `fcntl` function for file descriptor manipulation.
//!
//! Supported commands:
//! - `F_GETFD`/`F_SETFD` — fd-level flags (e.g., `FD_CLOEXEC`)
//! - `F_GETFL`/`F_SETFL` — file status flags (O_NONBLOCK, O_APPEND)
//! - `F_DUPFD`/`F_DUPFD_CLOEXEC` — duplicate fd to lowest >= arg
//! - `F_GETLK`/`F_SETLK`/`F_SETLKW` — advisory record locking
//!   (stub: no kernel-level locking, always succeeds)

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
/// Get advisory record lock (test if lock can be placed).
pub const F_GETLK: i32 = 5;
/// Set advisory record lock (non-blocking).
pub const F_SETLK: i32 = 6;
/// Set advisory record lock (blocking — waits if conflicting lock exists).
pub const F_SETLKW: i32 = 7;
/// Duplicate fd with close-on-exec.
pub const F_DUPFD_CLOEXEC: i32 = 1030;

// ---------------------------------------------------------------------------
// Advisory lock types (l_type in struct flock)
// ---------------------------------------------------------------------------

/// Shared (read) lock.
pub const F_RDLCK: i16 = 0;
/// Exclusive (write) lock.
pub const F_WRLCK: i16 = 1;
/// Unlock.
pub const F_UNLCK: i16 = 2;

// ---------------------------------------------------------------------------
// struct flock — advisory record locking
// ---------------------------------------------------------------------------

/// POSIX advisory record lock descriptor.
///
/// Used with `fcntl(fd, F_GETLK/F_SETLK/F_SETLKW, &flock)` for
/// byte-range advisory file locking.
///
/// Layout matches Linux x86_64 for binary compatibility.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Flock {
    /// Lock type: `F_RDLCK`, `F_WRLCK`, or `F_UNLCK`.
    pub l_type: i16,
    /// How to interpret `l_start`: `SEEK_SET`, `SEEK_CUR`, or `SEEK_END`.
    pub l_whence: i16,
    /// Starting offset of the lock region.
    pub l_start: i64,
    /// Number of bytes to lock.  0 means "to EOF".
    pub l_len: i64,
    /// PID of the process holding a conflicting lock (output for `F_GETLK`).
    pub l_pid: i32,
}

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
        F_GETLK => {
            // Test if an advisory lock can be placed.
            //
            // Our kernel doesn't implement file locking, so we always
            // report "no conflicting lock" by setting l_type = F_UNLCK.
            // This tells the caller that their desired lock would succeed.
            if arg == 0 {
                errno::set_errno(errno::EFAULT);
                return -1;
            }
            // SAFETY: arg is a pointer to a Flock struct (caller contract).
            // We only write to the struct, never read uninit memory.
            let flock_ptr = arg as *mut Flock;
            unsafe {
                (*flock_ptr).l_type = F_UNLCK;
                (*flock_ptr).l_pid = 0;
            }
            0
        }
        F_SETLK | F_SETLKW => {
            // Set or clear an advisory lock (non-blocking or blocking).
            //
            // Our kernel doesn't implement file locking, so we always
            // succeed.  Programs that use fcntl locking (editors,
            // databases, package managers) will proceed as if the lock
            // was acquired.  Since all user processes run in separate
            // address spaces and we're currently single-process, there
            // are no real lock conflicts to worry about.
            let _ = arg;
            0
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
    // For Console/Pipe/Socket, share the same handle (refcounted
    // via is_handle_referenced() in close()).
    let new_handle = match entry.kind {
        fdtable::HandleKind::File => {
            let ret = crate::syscall::syscall1(crate::syscall::SYS_FS_DUP, entry.handle);
            if ret < 0 {
                return crate::errno::translate(ret) as i32;
            }
            ret as u64
        }
        fdtable::HandleKind::Console
        | fdtable::HandleKind::Pipe
        | fdtable::HandleKind::TcpStream
        | fdtable::HandleKind::TcpListener
        | fdtable::HandleKind::UdpSocket => entry.handle,
    };

    if let Some(new_fd) = fdtable::alloc_fd_from(min_fd, entry.kind, new_handle) {
        if cloexec {
            let _ = fdtable::set_fd_flags(new_fd, fdtable::FD_CLOEXEC);
        }
        // Copy socket metadata for dup'd socket fds.
        match entry.kind {
            fdtable::HandleKind::TcpStream
            | fdtable::HandleKind::TcpListener
            | fdtable::HandleKind::UdpSocket => {
                crate::socket::copy_meta(oldfd, new_fd);
            }
            _ => {}
        }
        new_fd
    } else {
        // Clean up the kernel handle if it's a file (has independent handle).
        if entry.kind == fdtable::HandleKind::File {
            let _ = crate::syscall::syscall1(crate::syscall::SYS_FS_CLOSE, new_handle);
        }
        errno::set_errno(errno::EMFILE);
        -1
    }
}
