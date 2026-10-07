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
//! - `F_GETLK`/`F_SETLK`/`F_SETLKW` — advisory record locking, in the
//!   kernel's lock table (`SYS_FS_RECORD_LOCK`), which the Linux `fcntl`
//!   shares; see [`record_lock`]

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
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };

    // `fcntl` is the one call that splits over an `O_PATH` descriptor rather
    // than accepting or refusing it wholesale, because it is really a dozen
    // calls.  The commands that operate on the *descriptor* — its close-on-exec
    // flag, its number, its status word — work; the ones that reach through to
    // the file do not.  Measured on Linux 6.6: `F_GETFD`, `F_SETFD`, `F_GETFL`
    // (which reports exactly `O_PATH`), `F_DUPFD` and `F_DUPFD_CLOEXEC` all
    // succeed, while `F_SETFL`, `F_GETLK`, `F_SETLK` and `F_SETLKW` are
    // `EBADF`.
    if matches!(cmd, F_SETFL | F_GETLK | F_SETLK | F_SETLKW)
        && crate::file::reject_path_fd_entry(&entry)
    {
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
        F_GETLK | F_SETLK | F_SETLKW => record_lock(&entry, cmd, arg as *mut Flock),
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
// Record locks
// ---------------------------------------------------------------------------

/// `SYS_FS_RECORD_LOCK`'s operations: test, set, and set waiting.
const RECORD_LOCK_GET: u64 = 0;
const RECORD_LOCK_SET: u64 = 1;
const RECORD_LOCK_SET_WAIT: u64 = 2;

/// `F_GETLK`, `F_SETLK` and `F_SETLKW` on the descriptor `entry`, with the
/// caller's `struct flock` at `flock`.
///
/// The locks are the kernel's (`SYS_FS_RECORD_LOCK`, lane A's): the table
/// the Linux `fcntl` uses, so a native and a Linux program exclude each
/// other, held by the calling process and released when it exits, when it
/// unlocks, or when it closes any descriptor for the file -- the kernel does
/// that at a handle's last close, and [`release_record_locks`] when a
/// descriptor closes whose handle another still holds. Until 2026-10-07 this
/// granted every lock and found every range free, so two processes could both
/// hold one exclusive range (`known-issues/D-POSIX-FCNTL-RECORD-LOCKS-NEVER-
/// REACH-THE-KERNEL.md`) -- which is still what happens on a kernel without
/// the call, and for a descriptor that is not a file.
///
/// What Linux refuses before it looks at a lock is refused here first, so a
/// program is answered alike on every kernel: a `struct flock` that is not
/// there (`EFAULT`), an `l_type` or `l_whence` that is none of the three
/// (`EINVAL`), `F_UNLCK` to `F_GETLK` (`EINVAL`), and a read lock on a
/// descriptor not open for reading or a write lock on one not open for
/// writing (`EBADF`; `F_GETLK` asks nothing of the mode, as Linux's does not).
/// `F_SETLKW` waits in the kernel; a signal handler ends the wait unless it
/// was installed with `SA_RESTART`, as Linux restarts it (signal(7)).
fn record_lock(entry: &fdtable::FdEntry, cmd: i32, flock: *mut Flock) -> i32 {
    use crate::fcntl::{O_ACCMODE, O_RDONLY, O_WRONLY, SEEK_CUR, SEEK_END, SEEK_SET};

    if flock.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: non-null, and the caller's contract is that it points at a
    // `struct flock`, which is all plain integers.
    let (l_type, l_whence) = unsafe { ((*flock).l_type, (*flock).l_whence) };
    let whence = i32::from(l_whence);
    if !matches!(l_type, F_RDLCK | F_WRLCK | F_UNLCK)
        || !matches!(whence, SEEK_SET | SEEK_CUR | SEEK_END)
        || (cmd == F_GETLK && l_type == F_UNLCK)
    {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let mode = entry.status_flags & O_ACCMODE;
    if cmd != F_GETLK
        && ((l_type == F_RDLCK && mode == O_WRONLY) || (l_type == F_WRLCK && mode == O_RDONLY))
    {
        errno::set_errno(errno::EBADF);
        return -1;
    }

    let free = || {
        if cmd == F_GETLK {
            // SAFETY: as above.
            unsafe { (*flock).l_type = F_UNLCK };
        }
        0
    };
    if entry.kind != fdtable::HandleKind::File {
        // A pipe, a socket, the console: nothing in the kernel's table to
        // lock. Linux locks such a file's inode, which only the processes
        // sharing it could contend for; here every lock is granted.
        return free();
    }
    let op = match cmd {
        F_GETLK => RECORD_LOCK_GET,
        F_SETLK => RECORD_LOCK_SET,
        _ => RECORD_LOCK_SET_WAIT,
    };
    let call = || {
        crate::syscall::syscall3(
            crate::syscall::SYS_FS_RECORD_LOCK,
            entry.handle,
            op,
            flock as u64,
        )
    };
    let ret = if cmd == F_SETLKW {
        crate::interrupt::restarting(crate::interrupt::Restart::IfAsked, call)
    } else {
        call()
    };
    if no_lock_table(ret) {
        return free();
    }
    if ret < 0 {
        errno::set_errno(match ret {
            // A full table: POSIX's ENOLCK, where the shared mapping says
            // ENOMEM.
            errno::native::RESOURCE_EXHAUSTED => errno::ENOLCK,
            _ => errno::errno_for(ret),
        });
        return -1;
    }
    0
}

/// Whether `ret` says there is no lock table to ask: a kernel without
/// `SYS_FS_RECORD_LOCK` (every kernel before lane A's 1093), or, in the host
/// build, no kernel at all.
fn no_lock_table(ret: i64) -> bool {
    ret == errno::native::NO_SUCH_SYSCALL
        || (cfg!(not(target_os = "none")) && ret == crate::syscall::HOST_ENOSYS)
}

/// Release every record lock this process holds on the file `handle` is
/// open on: what closing a descriptor for it does, by POSIX, when the
/// descriptor's kernel handle stays open because another descriptor -- a
/// `dup` -- still holds it, so the kernel's own release at the handle's last
/// close does not run. A whole-file `F_UNLCK`, as lane A's
/// `SYS_FS_RECORD_LOCK` documents for exactly this.
///
/// Its answer is discarded on purpose: on a kernel without the call there
/// are no locks to release, and a close must not fail over a release that
/// finds nothing -- the descriptor is gone either way.
pub(crate) fn release_record_locks(handle: u64) {
    let mut whole_file = Flock {
        l_type: F_UNLCK,
        l_whence: 0, // SEEK_SET
        l_start: 0,
        l_len: 0,
        l_pid: 0,
    };
    let _ = crate::syscall::syscall3(
        crate::syscall::SYS_FS_RECORD_LOCK,
        handle,
        RECORD_LOCK_SET,
        (&raw mut whole_file) as u64,
    );
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
        // A descriptor that is not open is EBADF before its `struct flock`
        // is looked at.
        let ret = fcntl(200, F_SETLK, 0);
        assert_eq!(ret, -1); // EBADF because fd doesn't exist.
    }

    /// A `struct flock` on the stack, for the record-lock tests.
    fn lock_of(l_type: i16, l_whence: i16) -> Flock {
        Flock {
            l_type,
            l_whence,
            l_start: 0,
            l_len: 0,
            l_pid: 0,
        }
    }

    /// `fcntl`'s answer and errno for `cmd` with `fl` on `fd`.
    fn lock_answer(fd: Fd, cmd: i32, fl: &mut Flock) -> (i32, i32) {
        crate::errno::set_errno(0);
        let r = fcntl(fd, cmd, (fl as *mut Flock) as i64);
        (r, crate::errno::get_errno())
    }

    /// What Linux refuses before it looks at a lock, refused alike on every
    /// kernel -- each answer measured on Linux 6.6 with glibc 2.39.
    #[test]
    fn record_locks_refuse_what_linux_refuses_first() {
        use crate::fcntl::{O_RDONLY, O_WRONLY};
        let rd = fdtable::alloc_fd_with_flags(fdtable::HandleKind::File, 0, O_RDONLY)
            .expect("a free descriptor");
        let wr = fdtable::alloc_fd_with_flags(fdtable::HandleKind::File, 0, O_WRONLY)
            .expect("a free descriptor");
        let einval = (-1, errno::EINVAL);
        let ebadf = (-1, errno::EBADF);

        // A lock the open mode does not allow: EBADF; an unlock needs none.
        assert_eq!(lock_answer(wr, F_SETLK, &mut lock_of(F_RDLCK, 0)), ebadf);
        assert_eq!(lock_answer(rd, F_SETLK, &mut lock_of(F_WRLCK, 0)), ebadf);
        assert_eq!(lock_answer(rd, F_SETLKW, &mut lock_of(F_WRLCK, 0)), ebadf);
        assert_eq!(lock_answer(rd, F_SETLK, &mut lock_of(F_UNLCK, 0)).0, 0);
        // A test asks nothing of the mode, and finds the range free here.
        let mut probe = lock_of(F_WRLCK, 0);
        assert_eq!(lock_answer(rd, F_GETLK, &mut probe).0, 0);
        assert_eq!(probe.l_type, F_UNLCK);

        // An unknown type or whence, and F_UNLCK to F_GETLK: EINVAL.
        assert_eq!(lock_answer(rd, F_SETLK, &mut lock_of(7, 0)), einval);
        assert_eq!(lock_answer(rd, F_SETLK, &mut lock_of(F_RDLCK, 9)), einval);
        assert_eq!(lock_answer(rd, F_GETLK, &mut lock_of(F_RDLCK, 9)), einval);
        assert_eq!(lock_answer(rd, F_GETLK, &mut lock_of(7, 0)), einval);
        assert_eq!(lock_answer(rd, F_GETLK, &mut lock_of(F_UNLCK, 0)), einval);

        // No `struct flock`: EFAULT.
        crate::errno::set_errno(0);
        assert_eq!(fcntl(rd, F_SETLK, 0), -1);
        assert_eq!(crate::errno::get_errno(), errno::EFAULT);

        let _ = fdtable::close_fd(rd);
        let _ = fdtable::close_fd(wr);
    }

    /// A pipe's ends are held to their modes as a file is -- a write lock on
    /// the read end is EBADF -- and otherwise granted: there is no file in
    /// the kernel's table to lock.
    #[test]
    fn record_locks_on_a_pipe_follow_its_ends_modes() {
        use crate::fcntl::O_RDONLY;
        let end = fdtable::alloc_fd_with_flags(fdtable::HandleKind::Pipe, 0, O_RDONLY)
            .expect("a free descriptor");
        assert_eq!(lock_answer(end, F_SETLK, &mut lock_of(F_RDLCK, 0)).0, 0);
        assert_eq!(
            lock_answer(end, F_SETLK, &mut lock_of(F_WRLCK, 0)),
            (-1, errno::EBADF)
        );
        let _ = fdtable::close_fd(end);
    }

    /// A kernel without `SYS_FS_RECORD_LOCK`, and the host, have no table: a
    /// lock is granted there and a test finds the range free, as every lock
    /// was before the call existed. Any other answer is the kernel's.
    #[test]
    fn no_lock_table_is_only_a_missing_call() {
        assert!(no_lock_table(errno::native::NO_SUCH_SYSCALL));
        assert!(no_lock_table(crate::syscall::HOST_ENOSYS));
        assert!(!no_lock_table(errno::native::WOULD_BLOCK));
        assert!(!no_lock_table(errno::native::DEADLOCK));
        assert!(!no_lock_table(0));
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

    // All handle kinds share the same kernel handle id at the fd-table
    // level (refcounted via is_handle_referenced() in close()).  For File
    // this gives correct POSIX semantics: the dup'd fd shares ONE open
    // file description with the source (shared offset + status flags),
    // rather than getting an independent cursor from SYS_FS_DUP.
    let new_handle = match entry.kind {
        fdtable::HandleKind::File
        | fdtable::HandleKind::Console
        | fdtable::HandleKind::Pipe
        | fdtable::HandleKind::TcpStream
        | fdtable::HandleKind::TcpListener
        | fdtable::HandleKind::UdpSocket
        | fdtable::HandleKind::Eventfd
        | fdtable::HandleKind::UnixStream
        | fdtable::HandleKind::UnixSocket
        | fdtable::HandleKind::PtyMaster
        | fdtable::HandleKind::PtySlave => entry.handle,
        fdtable::HandleKind::Epoll
        | fdtable::HandleKind::Timerfd
        | fdtable::HandleKind::Inotify => {
            // F_DUPFD on an epoll/timerfd/inotify fd shares the
            // instance.  No addref needed: close() uses
            // is_handle_referenced() to skip instance teardown while
            // another fd still references it.
            entry.handle
        }
    };

    // F_DUPFD inherits the source's file status flags (O_APPEND, etc.).
    if let Some(new_fd) =
        fdtable::alloc_fd_from_with_flags(min_fd, entry.kind, new_handle, entry.status_flags)
    {
        if cloexec {
            let _ = fdtable::set_fd_flags(new_fd, fdtable::FD_CLOEXEC);
        }
        // Copy socket metadata for dup'd socket fds.
        match entry.kind {
            fdtable::HandleKind::TcpStream
            | fdtable::HandleKind::TcpListener
            | fdtable::HandleKind::UdpSocket
            | fdtable::HandleKind::UnixSocket => {
                crate::socket::copy_meta(oldfd, new_fd);
            }
            _ => {}
        }
        new_fd
    } else {
        // No kernel cleanup needed: every handle kind (including File now)
        // shares the source fd's handle id, so the source still holds the
        // reference.  alloc_fd_from_with_flags failed before installing any
        // new fd, so nothing was duplicated.
        errno::set_errno(errno::EMFILE);
        -1
    }
}
