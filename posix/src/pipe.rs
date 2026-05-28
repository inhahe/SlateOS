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

/// `O_DIRECT` value as it appears in `pipe2(2)` flags.
///
/// Matches Linux/x86_64's `0o40000` (`0x4000`).  On Linux, setting this
/// bit on a pipe creates a packetized pipe (each write becomes a single
/// readable packet).  Our kernel only supports stream-mode pipes, so we
/// accept the bit for source compatibility but produce stream-mode
/// semantics — the same data is delivered, just not framed.  This
/// matches the behaviour of a Linux kernel built without packetized
/// pipe support (the bit is accepted and ignored).
pub const PIPE2_O_DIRECT: i32 = 0o40000;

/// Mask of `pipe2(2)` flag bits accepted by Linux.
///
/// Linux's `fs/pipe.c::do_pipe2` rejects any bit outside the set
/// `O_CLOEXEC | O_NONBLOCK | O_DIRECT | O_NOTIFICATION_PIPE` with
/// `EINVAL` before allocating any pipe fds.  We don't model
/// `O_NOTIFICATION_PIPE` (it's used by Linux's keyring change-notify
/// subsystem which we don't have), so the accepted set is the three
/// common bits — same as Linux ≤ 5.7 and as every existing pipe2
/// caller (glibc, musl, Bionic, sandbox helpers).
pub const PIPE2_VALID_FLAGS: i32 =
    crate::fcntl::O_CLOEXEC | crate::fcntl::O_NONBLOCK | PIPE2_O_DIRECT;

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
/// enforced by the kernel), `O_DIRECT` (accepted for source
/// compatibility; pipes remain stream-mode regardless).  Any other
/// bit in `flags` yields `EINVAL`.
///
/// # Validation order (Linux-matching)
///
/// 1. `flags & ~PIPE2_VALID_FLAGS != 0` → `EINVAL`.  Matches Linux's
///    `fs/pipe.c::do_pipe2` which rejects unknown bits *before*
///    `pipefd` is ever touched — even a NULL `pipefd` will see
///    `EINVAL` first if the flags are also wrong.
/// 2. `pipefd == NULL` → `EFAULT`.
///
/// Returns 0 on success, -1 on error (errno set).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pipe2(pipefd: *mut Fd, flags: i32) -> i32 {
    if flags & !PIPE2_VALID_FLAGS != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
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

    // -- Phase 99: pipe2 flag-mask validation --

    /// `PIPE2_VALID_FLAGS` is the OR of the three accepted bits.
    #[test]
    fn test_pipe2_valid_flags_is_or_of_known_bits() {
        assert_eq!(
            PIPE2_VALID_FLAGS,
            crate::fcntl::O_CLOEXEC | crate::fcntl::O_NONBLOCK | PIPE2_O_DIRECT,
        );
    }

    /// `O_DIRECT` for pipe2 matches the Linux/x86_64 numeric value.
    #[test]
    fn test_pipe2_o_direct_matches_linux_value() {
        assert_eq!(PIPE2_O_DIRECT, 0o40000);
        assert_eq!(PIPE2_O_DIRECT, 0x4000);
    }

    /// Unknown high bit (`0x8000_0000`, i.e. `i32::MIN`) → `EINVAL`.
    /// This is the canonical "garbage flags" attack — must be rejected.
    #[test]
    fn test_pipe2_high_bit_rejected() {
        let mut fds: [Fd; 2] = [-1, -1];
        errno::set_errno(0);
        let ret = pipe2(fds.as_mut_ptr(), i32::MIN);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        // pipefd must not have been touched.
        assert_eq!(fds, [-1, -1], "pipefd must not be written on EINVAL");
    }

    /// An arbitrary unknown bit (here `O_APPEND`, which is not a pipe2
    /// flag in Linux) is rejected.
    #[test]
    fn test_pipe2_unknown_bit_rejected() {
        let mut fds: [Fd; 2] = [-1, -1];
        errno::set_errno(0);
        let ret = pipe2(fds.as_mut_ptr(), crate::fcntl::O_APPEND);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    /// `O_RDWR` (a valid open(2) bit but not a pipe2 bit) is rejected.
    #[test]
    fn test_pipe2_o_rdwr_rejected() {
        let mut fds: [Fd; 2] = [-1, -1];
        errno::set_errno(0);
        let ret = pipe2(fds.as_mut_ptr(), crate::fcntl::O_RDWR);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    /// Validation order: `EINVAL` (bad flags) fires before `EFAULT`
    /// (null pipefd).  A buggy caller that passes both errors at once
    /// sees the flag error, matching Linux's `do_pipe2` prologue.
    #[test]
    fn test_pipe2_einval_wins_over_efault() {
        errno::set_errno(0);
        let ret = pipe2(core::ptr::null_mut(), i32::MIN);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL,
            "bad flags must beat null pipefd");
    }

    /// All three valid bits individually pass the mask check.
    /// (The test uses NULL pipefd so the call still fails — but with
    /// EFAULT, proving the flag check accepted the bit.)
    #[test]
    fn test_pipe2_o_cloexec_alone_passes_mask() {
        errno::set_errno(0);
        let ret = pipe2(core::ptr::null_mut(), crate::fcntl::O_CLOEXEC);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_pipe2_o_nonblock_alone_passes_mask() {
        errno::set_errno(0);
        let ret = pipe2(core::ptr::null_mut(), crate::fcntl::O_NONBLOCK);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_pipe2_o_direct_alone_passes_mask() {
        errno::set_errno(0);
        let ret = pipe2(core::ptr::null_mut(), PIPE2_O_DIRECT);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    /// All three valid bits combined pass the mask check.
    #[test]
    fn test_pipe2_all_valid_bits_pass_mask() {
        errno::set_errno(0);
        let ret = pipe2(
            core::ptr::null_mut(),
            crate::fcntl::O_CLOEXEC | crate::fcntl::O_NONBLOCK | PIPE2_O_DIRECT,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    /// Valid bits combined with one unknown bit → EINVAL.  A common
    /// "I added one more flag" bug shape.
    #[test]
    fn test_pipe2_valid_plus_unknown_rejected() {
        let mut fds: [Fd; 2] = [-1, -1];
        errno::set_errno(0);
        let ret = pipe2(
            fds.as_mut_ptr(),
            crate::fcntl::O_CLOEXEC | crate::fcntl::O_APPEND,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    /// Buggy-caller workflow: EINVAL on bad flags, retry with the
    /// flags fixed — same null pipefd, second call surfaces EFAULT.
    #[test]
    fn test_pipe2_recovery_after_einval() {
        errno::set_errno(0);
        assert_eq!(pipe2(core::ptr::null_mut(), 0xDEAD), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        errno::set_errno(0);
        assert_eq!(pipe2(core::ptr::null_mut(), crate::fcntl::O_CLOEXEC), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    /// `pipe(pipefd)` is documented as `pipe2(pipefd, 0)` — and flags=0
    /// must pass the mask check (no bits set means no unknown bits).
    #[test]
    fn test_pipe_zero_flags_passes_mask() {
        errno::set_errno(0);
        let ret = pipe(core::ptr::null_mut());
        assert_eq!(ret, -1);
        // Hit the null check, not the flag check.
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }
}
