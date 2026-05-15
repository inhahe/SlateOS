//! POSIX pipe functions.
//!
//! Implements `pipe`, `pipe2`.
//!
//! Our kernel provides `SYS_PIPE_CREATE` which returns a pair of handles
//! (read end, write end).  This module wraps them into POSIX fd semantics
//! via the fd table.

use crate::errno;
use crate::fdtable::{self, HandleKind};
use crate::syscall::*;
use crate::types::*;

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// Create a unidirectional data channel (pipe).
///
/// On success, `pipefd[0]` is the read end and `pipefd[1]` is the write end.
/// Returns 0 on success, -1 on error (errno set).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pipe(pipefd: *mut Fd) -> i32 {
    pipe2(pipefd, 0)
}

/// Create a pipe with flags.
///
/// Supported flags: `O_CLOEXEC`, `O_NONBLOCK` (stored but not yet
/// enforced by the kernel).
///
/// Returns 0 on success, -1 on error (errno set).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pipe2(pipefd: *mut Fd, flags: i32) -> i32 {
    if pipefd.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // Create the kernel pipe.
    // SYS_PIPE_CREATE returns two handles via ok2: read in rax, write in rdx.
    let read_handle: u64;
    let write_handle: u64;

    // SAFETY: SYSCALL is the defined kernel entry.  RCX/R11 are clobbered.
    // SYS_PIPE_CREATE returns read handle in RAX, write handle in RDX.
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") SYS_PIPE_CREATE,
            lateout("rax") read_handle,
            lateout("rdx") write_handle,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }

    // Check for error (negative rax).
    #[allow(clippy::cast_possible_wrap)]
    let ret_signed = read_handle as i64;
    if ret_signed < 0 {
        let _ = errno::translate(ret_signed);
        return -1;
    }

    // Register both handles in the fd table.
    // Pipe read end is O_RDONLY, write end is O_WRONLY, plus any
    // O_NONBLOCK from the flags argument.
    let nonblock_bit = flags & crate::fcntl::O_NONBLOCK;
    let read_status = crate::fcntl::O_RDONLY | nonblock_bit;
    let write_status = crate::fcntl::O_WRONLY | nonblock_bit;

    let Some(read_fd) = fdtable::alloc_fd_with_flags(
        HandleKind::Pipe, read_handle, read_status,
    ) else {
        // Table full — close the kernel handles.
        let _ = syscall1(SYS_PIPE_CLOSE, read_handle);
        let _ = syscall1(SYS_PIPE_CLOSE, write_handle);
        errno::set_errno(errno::EMFILE);
        return -1;
    };

    let Some(write_fd) = fdtable::alloc_fd_with_flags(
        HandleKind::Pipe, write_handle, write_status,
    ) else {
        // Table full — close both.
        let _ = fdtable::close_fd(read_fd);
        let _ = syscall1(SYS_PIPE_CLOSE, read_handle);
        let _ = syscall1(SYS_PIPE_CLOSE, write_handle);
        errno::set_errno(errno::EMFILE);
        return -1;
    };

    // Set FD_CLOEXEC if O_CLOEXEC was requested.
    if flags & crate::fcntl::O_CLOEXEC != 0 {
        let _ = fdtable::set_fd_flags(read_fd, fdtable::FD_CLOEXEC);
        let _ = fdtable::set_fd_flags(write_fd, fdtable::FD_CLOEXEC);
    }

    // SAFETY: Caller guarantees pipefd points to at least 2 ints.
    unsafe {
        *pipefd = read_fd;
        *pipefd.add(1) = write_fd;
    }

    0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Null pointer checks (don't require kernel) --

    #[test]
    fn pipe_null_returns_efault() {
        let ret = pipe(core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn pipe2_null_returns_efault() {
        let ret = pipe2(core::ptr::null_mut(), 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn pipe2_null_with_flags_returns_efault() {
        let ret = pipe2(core::ptr::null_mut(), crate::fcntl::O_CLOEXEC);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn pipe_delegates_to_pipe2() {
        // pipe(pipefd) == pipe2(pipefd, 0) — both should fail with
        // null the same way, confirming pipe delegates.
        let r1 = pipe(core::ptr::null_mut());
        let e1 = errno::get_errno();
        let r2 = pipe2(core::ptr::null_mut(), 0);
        let e2 = errno::get_errno();
        assert_eq!(r1, r2);
        assert_eq!(e1, e2);
    }

    // -- pipe2 null with O_NONBLOCK --

    #[test]
    fn pipe2_null_with_nonblock() {
        let ret = pipe2(core::ptr::null_mut(), crate::fcntl::O_NONBLOCK);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    // -- pipe2 null with combined flags --

    #[test]
    fn pipe2_null_with_combined_flags() {
        let flags = crate::fcntl::O_CLOEXEC | crate::fcntl::O_NONBLOCK;
        let ret = pipe2(core::ptr::null_mut(), flags);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    // -- pipe null clears previous errno --

    #[test]
    fn pipe_null_sets_efault_not_previous() {
        errno::set_errno(errno::ENOENT);
        let ret = pipe(core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    // -- pipe2 null clears previous errno --

    #[test]
    fn pipe2_null_sets_efault_not_previous() {
        errno::set_errno(errno::ENOENT);
        let ret = pipe2(core::ptr::null_mut(), 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    // -- pipe2 returns consistent results for same null input --

    #[test]
    fn pipe2_null_idempotent() {
        for _ in 0..3 {
            let ret = pipe2(core::ptr::null_mut(), 0);
            assert_eq!(ret, -1);
            assert_eq!(errno::get_errno(), errno::EFAULT);
        }
    }
}
