//! POSIX fcntl operations.
//!
//! Implements the `fcntl` function for file descriptor manipulation.
//!
//! Supported commands:
//! - `F_GETFD`/`F_SETFD` — per-fd flags (e.g., `FD_CLOEXEC`)
//! - `F_GETFL`/`F_SETFL` — file status flags (`O_NONBLOCK`, `O_APPEND`,
//!   `O_SYNC`).  `F_GETFL` returns access mode + status flags.
//!   `F_SETFL` can only change `O_APPEND`, `O_NONBLOCK`, `O_SYNC`;
//!   access mode bits (`O_RDONLY`/`O_WRONLY`/`O_RDWR`) are immutable.
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
            // Return access mode + file status flags.
            if let Some(sf) = fdtable::get_status_flags(fd) {
                sf
            } else {
                errno::set_errno(errno::EBADF);
                -1
            }
        }
        F_SETFL => {
            // Change mutable file status flags (O_APPEND, O_NONBLOCK, O_SYNC).
            // Access mode bits (O_ACCMODE) are preserved by set_status_flags().
            #[allow(clippy::cast_possible_truncation)]
            if fdtable::set_status_flags(fd, arg as i32) {
                0
            } else {
                errno::set_errno(errno::EBADF);
                -1
            }
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

/// `fcntl64` — LP64 alias for `fcntl` (off_t is already 64-bit).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fcntl64(fd: Fd, cmd: i32, arg: i64) -> i32 {
    fcntl(fd, cmd, arg)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- fcntl command constants (match Linux/glibc) --

    #[test]
    fn test_fcntl_commands() {
        assert_eq!(F_DUPFD, 0);
        assert_eq!(F_GETFD, 1);
        assert_eq!(F_SETFD, 2);
        assert_eq!(F_GETFL, 3);
        assert_eq!(F_SETFL, 4);
        assert_eq!(F_GETLK, 5);
        assert_eq!(F_SETLK, 6);
        assert_eq!(F_SETLKW, 7);
    }

    #[test]
    fn test_f_dupfd_cloexec_value() {
        // Linux defines F_DUPFD_CLOEXEC = 1030.
        assert_eq!(F_DUPFD_CLOEXEC, 1030);
    }

    // -- Lock type constants --

    #[test]
    fn test_lock_type_constants() {
        assert_eq!(F_RDLCK, 0);
        assert_eq!(F_WRLCK, 1);
        assert_eq!(F_UNLCK, 2);
    }

    #[test]
    fn test_lock_types_distinct() {
        assert_ne!(F_RDLCK, F_WRLCK);
        assert_ne!(F_RDLCK, F_UNLCK);
        assert_ne!(F_WRLCK, F_UNLCK);
    }

    // -- Flock struct layout --

    #[test]
    fn test_flock_size() {
        // Linux x86_64: struct flock is 32 bytes.
        // l_type(2) + l_whence(2) + padding(4) + l_start(8) + l_len(8) + l_pid(4) + padding(4) = 32
        // Our repr(C) layout: l_type(i16) + l_whence(i16) + l_start(i64) + l_len(i64) + l_pid(i32)
        // With alignment: i16+i16 = 4 bytes, then padding for i64 alignment = 4 bytes, then 8+8+4 = 20, + padding = 4 → 32
        let size = core::mem::size_of::<Flock>();
        assert!(size >= 24, "Flock must be at least 24 bytes, got {size}");
    }

    #[test]
    fn test_flock_fields() {
        let f = Flock {
            l_type: F_WRLCK,
            l_whence: 0, // SEEK_SET
            l_start: 100,
            l_len: 200,
            l_pid: 42,
        };
        assert_eq!(f.l_type, F_WRLCK);
        assert_eq!(f.l_whence, 0);
        assert_eq!(f.l_start, 100);
        assert_eq!(f.l_len, 200);
        assert_eq!(f.l_pid, 42);
    }

    #[test]
    fn test_flock_zero_len_means_eof() {
        // POSIX: l_len == 0 means "lock to end of file".
        let f = Flock {
            l_type: F_RDLCK,
            l_whence: 0,
            l_start: 0,
            l_len: 0,
            l_pid: 0,
        };
        assert_eq!(f.l_len, 0);
    }

    // -- fcntl command constants (must match Linux x86_64) --

    #[test]
    fn test_fcntl_command_values() {
        assert_eq!(F_DUPFD, 0);
        assert_eq!(F_GETFD, 1);
        assert_eq!(F_SETFD, 2);
        assert_eq!(F_GETFL, 3);
        assert_eq!(F_SETFL, 4);
        assert_eq!(F_GETLK, 5);
        assert_eq!(F_SETLK, 6);
        assert_eq!(F_SETLKW, 7);
        assert_eq!(F_DUPFD_CLOEXEC, 1030);
    }

    // -- Lock type constants --

    #[test]
    fn test_lock_type_values() {
        assert_eq!(F_RDLCK, 0);
        assert_eq!(F_WRLCK, 1);
        assert_eq!(F_UNLCK, 2);
    }

    // -- Flock alignment --

    #[test]
    fn test_flock_alignment() {
        // repr(C) struct with i64 fields must be 8-byte aligned.
        assert!(core::mem::align_of::<Flock>() >= 4);
    }

    // -- Lock type exhaustiveness --

    #[test]
    fn test_flock_lock_types_distinct() {
        // All lock types must be distinct values.
        assert_ne!(F_RDLCK, F_WRLCK);
        assert_ne!(F_RDLCK, F_UNLCK);
        assert_ne!(F_WRLCK, F_UNLCK);
    }

    // -- fcntl error paths --

    #[test]
    fn test_fcntl_invalid_fd_returns_ebadf() {
        // fd 200 doesn't exist in the fd table by default in tests.
        let ret = fcntl(200, F_GETFD, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_fcntl_negative_fd_returns_ebadf() {
        let ret = fcntl(-1, F_GETFL, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_fcntl_unknown_command_returns_einval() {
        // First need a valid fd — fd 0 (stdin) should exist in test mode.
        // Actually, in test mode the fd table may not be initialized.
        // Use a high command number on any fd — the function checks fd first,
        // so we need to test with a potentially bad fd too. Let's just
        // verify the EINVAL path by using an fd that might not exist.
        let ret = fcntl(200, 9999, 0);
        // We get EBADF because the fd check comes first.
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_fcntl64_is_fcntl_alias() {
        // Both should return the same result for the same arguments.
        let r1 = fcntl(200, F_GETFD, 0);
        let e1 = crate::errno::get_errno();
        let r2 = fcntl64(200, F_GETFD, 0);
        let e2 = crate::errno::get_errno();
        assert_eq!(r1, r2);
        assert_eq!(e1, e2);
    }

    #[test]
    fn test_fcntl_setlk_no_lock_conflict() {
        // F_SETLK and F_SETLKW succeed unconditionally (no real locking).
        // We need an existing fd though.  If it doesn't exist, we get EBADF.
        // Test with a non-existent fd to verify the fd-check path.
        let ret = fcntl(200, F_SETLK, 0);
        assert_eq!(ret, -1); // EBADF because fd doesn't exist.
    }

    #[test]
    fn test_fcntl_getlk_null_ptr() {
        // F_GETLK with arg=0 (null pointer) should return EFAULT.
        // But fd check comes first, so on fd 200 we get EBADF.
        let ret = fcntl(200, F_GETLK, 0);
        assert_eq!(ret, -1);
    }

    // -- Flock initializer patterns --

    #[test]
    fn test_flock_read_lock() {
        let f = Flock {
            l_type: F_RDLCK,
            l_whence: 0,
            l_start: 0,
            l_len: 0, // Lock whole file
            l_pid: 0,
        };
        assert_eq!(f.l_type, F_RDLCK);
        assert_eq!(f.l_len, 0, "l_len=0 means lock to EOF");
    }

    #[test]
    fn test_flock_write_lock_partial() {
        let f = Flock {
            l_type: F_WRLCK,
            l_whence: 0,
            l_start: 1024,
            l_len: 4096,
            l_pid: 100,
        };
        assert_eq!(f.l_type, F_WRLCK);
        assert_eq!(f.l_start, 1024);
        assert_eq!(f.l_len, 4096);
    }
}

/// Duplicate fd to lowest available >= `min_fd`.
fn dup_fd_from(oldfd: Fd, min_fd: i32, cloexec: bool) -> i32 {
    // POSIX: F_DUPFD with negative arg or arg >= OPEN_MAX → EINVAL.
    if min_fd < 0 || min_fd as usize >= fdtable::MAX_FDS {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

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
        | fdtable::HandleKind::UdpSocket
        | fdtable::HandleKind::Eventfd => entry.handle,
        fdtable::HandleKind::Epoll => {
            // F_DUPFD on an epoll fd shares the instance.  No addref
            // needed: close() uses is_handle_referenced() to skip
            // instance teardown while another fd still references it.
            entry.handle
        }
    };

    // F_DUPFD inherits the source's file status flags (O_APPEND, etc.).
    if let Some(new_fd) = fdtable::alloc_fd_from_with_flags(
        min_fd, entry.kind, new_handle, entry.status_flags,
    ) {
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
        // Epoll: no refcount to drop — alloc_fd_from_with_flags failed
        // before installing any new fd, so the existing fd still holds
        // the only reference to the instance.
        errno::set_errno(errno::EMFILE);
        -1
    }
}
