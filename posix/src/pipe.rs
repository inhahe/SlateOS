//! POSIX pipe functions.
//!
//! Implements `pipe`, `pipe2`.
//!
//! Our kernel provides `SYS_PIPE_CREATE` which returns a pair of handles
//! (read end, write end).  This module wraps them into POSIX fd semantics
//! via the fd table.

use crate::errno;
use crate::fdtable::{self, HandleKind};
// Only the OS-target arms of `pipe_kernel_create` / `pipe_kernel_close`
// talk to the kernel; on a host build those arms compile out and the
// simulator takes over, so the import would be unused there.
#[cfg(target_os = "none")]
use crate::syscall::{SYS_PIPE_CLOSE, SYS_PIPE_CREATE, syscall0_ok2, syscall1};
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

// ---------------------------------------------------------------------------
// Kernel interface (and its host stand-in)
// ---------------------------------------------------------------------------

/// Host-side stand-in for the kernel's pipe objects.
///
/// On a host build there is no SlateOS kernel to create a pipe, and the
/// raw `SYSCALL` that would ask for one is gated off (see the host-build
/// safety gate in `syscall.rs`).  Without a stand-in, `pipe2` would fail
/// with `ENOSYS` on host and every test that merely needs *an open fd of
/// kind `Pipe`* to reach the logic it actually cares about would fail
/// with it — which is exactly what happened to seventeen tests in
/// `file.rs` and `dirent.rs`.
///
/// So the simulator hands out handles and remembers them, and does not
/// pretend to move data: nothing on the host side needs a pipe to
/// *carry* bytes, and `SYS_PIPE_READ`/`SYS_PIPE_WRITE` already return the
/// `ENOSYS` sentinel through the gated `syscall3`.  Modelling a buffer
/// here would be inventing semantics no test asks for, and would make
/// host runs of `read`/`write` diverge from the target instead of
/// failing honestly.  Same shape and same reasoning as
/// `host_eventfd_sim` in `epoll.rs`.
#[cfg(not(target_os = "none"))]
mod host_pipe_sim {
    extern crate std;
    use std::sync::Mutex;
    use std::vec::Vec;

    /// Live simulated handles.  A pipe contributes two (read and write).
    static LIVE: Mutex<Vec<u64>> = Mutex::new(Vec::new());

    /// Distinct from `host_eventfd_sim`'s `0x4000_0000` base so a handle
    /// seen in a debugger names its own simulator.  The fd table keys on
    /// (kind, handle), so the ranges need not be disjoint for
    /// correctness — only for legibility.
    static NEXT_HANDLE: Mutex<u64> = Mutex::new(0x5000_0000);

    /// Allocate the read/write handle pair for one simulated pipe.
    pub fn create() -> (u64, u64) {
        let mut next = NEXT_HANDLE.lock().unwrap_or_else(|e| e.into_inner());
        let read = *next;
        let write = next.wrapping_add(1);
        *next = next.wrapping_add(2);
        drop(next);

        let mut live = LIVE.lock().unwrap_or_else(|e| e.into_inner());
        live.push(read);
        live.push(write);
        (read, write)
    }

    /// Release one handle.  Returns 0 if it was live, `BAD_HANDLE`
    /// otherwise — the same discrimination the kernel makes, so a
    /// double close is caught on host too.
    pub fn close(handle: u64) -> i64 {
        let mut live = LIVE.lock().unwrap_or_else(|e| e.into_inner());
        match live.iter().position(|&h| h == handle) {
            Some(i) => {
                live.swap_remove(i);
                0
            }
            None => crate::errno::native::BAD_HANDLE,
        }
    }
}

/// Ask the kernel for a pipe's read/write handle pair.
///
/// Returns `(rax, write_handle)` — a negative `rax` is an error, and the
/// second value is then meaningless.  On success `rax` is the read
/// handle.
#[inline]
fn pipe_kernel_create() -> (i64, u64) {
    #[cfg(target_os = "none")]
    {
        syscall0_ok2(SYS_PIPE_CREATE)
    }
    #[cfg(not(target_os = "none"))]
    {
        let (read, write) = host_pipe_sim::create();
        // Cast: handles start at 0x5000_0000 and step by 2, so they stay
        // far below i64::MAX and can never be mistaken for an error.
        #[allow(clippy::cast_possible_wrap)]
        let read_signed = read as i64;
        (read_signed, write)
    }
}

/// Release a pipe handle previously returned by [`pipe_kernel_create`].
///
/// Shared with `file.rs`'s `close` path so that a pipe closed through
/// the fd table and one closed on `pipe2`'s own error path go to the
/// same place — on host, that keeps the simulator's live-handle list
/// from growing for the length of the test binary.
#[inline]
pub(crate) fn pipe_kernel_close(handle: u64) -> i64 {
    #[cfg(target_os = "none")]
    {
        syscall1(SYS_PIPE_CLOSE, handle)
    }
    #[cfg(not(target_os = "none"))]
    {
        host_pipe_sim::close(handle)
    }
}

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

    // Create the kernel pipe.  SYS_PIPE_CREATE answers with two handles
    // via the `ok2` convention: read in rax, write in rdx.
    let (ret_signed, write_handle) = pipe_kernel_create();

    // Check for error (negative rax).
    if ret_signed < 0 {
        let _ = errno::translate(ret_signed);
        return -1;
    }
    // Non-negative, so this is the read handle rather than an error.
    #[allow(clippy::cast_sign_loss)]
    let read_handle = ret_signed as u64;

    // Register both handles in the fd table.
    // Pipe read end is O_RDONLY, write end is O_WRONLY, plus any
    // O_NONBLOCK from the flags argument.
    let nonblock_bit = flags & crate::fcntl::O_NONBLOCK;
    let read_status = crate::fcntl::O_RDONLY | nonblock_bit;
    let write_status = crate::fcntl::O_WRONLY | nonblock_bit;

    let Some(read_fd) = fdtable::alloc_fd_with_flags(HandleKind::Pipe, read_handle, read_status)
    else {
        // Table full — close the kernel handles.
        let _ = pipe_kernel_close(read_handle);
        let _ = pipe_kernel_close(write_handle);
        errno::set_errno(errno::EMFILE);
        return -1;
    };

    let Some(write_fd) = fdtable::alloc_fd_with_flags(HandleKind::Pipe, write_handle, write_status)
    else {
        // Table full — close both.
        let _ = fdtable::close_fd(read_fd);
        let _ = pipe_kernel_close(read_handle);
        let _ = pipe_kernel_close(write_handle);
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
        assert_eq!(
            errno::get_errno(),
            errno::EINVAL,
            "bad flags must beat null pipefd"
        );
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

    // -- The success path.  Until 2026-08-26 nothing here reached it: every
    //    test above passes NULL precisely so that it stops before the
    //    syscall.  That left `pipe2`'s only interesting half — the half
    //    that talks to the kernel and populates the fd table — with no host
    //    coverage at all, which is how an ungated raw SYSCALL lived in it
    //    undetected.  See known-issues
    //    `B-POSIX-PIPE2-ISSUES-AN-UNGATED-RAW-SYSCALL-ON-HOST-BUILDS`.

    /// A successful `pipe()` yields two distinct, open fds of kind `Pipe`.
    #[test]
    fn test_pipe_success_yields_two_distinct_pipe_fds() {
        let mut fds: [Fd; 2] = [-1, -1];
        assert_eq!(pipe(fds.as_mut_ptr()), 0, "pipe() must succeed");
        assert_ne!(fds[0], fds[1], "read and write ends must be distinct fds");
        for fd in fds {
            let entry = fdtable::get_fd(fd).expect("both ends must be open");
            assert!(
                matches!(entry.kind, HandleKind::Pipe),
                "fd {fd} must be registered as a pipe",
            );
        }
        assert_eq!(crate::file::close(fds[0]), 0);
        assert_eq!(crate::file::close(fds[1]), 0);
    }

    /// The read end opens `O_RDONLY` and the write end `O_WRONLY`, and
    /// `O_NONBLOCK` is carried onto both when asked for.
    #[test]
    fn test_pipe2_sets_access_mode_and_nonblock_per_end() {
        let mut fds: [Fd; 2] = [-1, -1];
        assert_eq!(pipe2(fds.as_mut_ptr(), crate::fcntl::O_NONBLOCK), 0);
        let read = fdtable::get_fd(fds[0]).expect("read end open");
        let write = fdtable::get_fd(fds[1]).expect("write end open");
        assert_eq!(read.status_flags & 0o3, crate::fcntl::O_RDONLY);
        assert_eq!(write.status_flags & 0o3, crate::fcntl::O_WRONLY);
        assert_ne!(read.status_flags & crate::fcntl::O_NONBLOCK, 0);
        assert_ne!(write.status_flags & crate::fcntl::O_NONBLOCK, 0);
        let _ = crate::file::close(fds[0]);
        let _ = crate::file::close(fds[1]);
    }

    /// Two pipes handed out at once never share a handle.
    ///
    /// This is the assertion that would have caught the ungated syscall on
    /// Windows, where it was silently *green*: the raw instruction returned
    /// the NTSTATUS `STATUS_INVALID_HANDLE` (`0xC000_0008`) in EAX, which is
    /// positive once widened to 64 bits, so `pipe2`'s `ret < 0` error check
    /// read an NT error as success and registered `(0xC000_0008, 0)` as a
    /// handle pair.  Every pipe got that same fabricated pair, so any test
    /// needing merely "an fd of kind `Pipe`" passed — for entirely the wrong
    /// reason.  Linux was the honest host: syscall 220 there is
    /// `semtimedop`, whose `-errno` really is negative, so `pipe2` failed
    /// and took seventeen tests with it.
    #[test]
    fn test_two_pipes_do_not_share_handles() {
        let mut a: [Fd; 2] = [-1, -1];
        let mut b: [Fd; 2] = [-1, -1];
        assert_eq!(pipe(a.as_mut_ptr()), 0);
        assert_eq!(pipe(b.as_mut_ptr()), 0);

        let handles: [u64; 4] = [a[0], a[1], b[0], b[1]]
            .map(|fd| fdtable::get_fd(fd).expect("all four ends open").handle);
        for i in 0..handles.len() {
            for j in (i + 1)..handles.len() {
                assert_ne!(
                    handles[i], handles[j],
                    "handles {i} and {j} collide — every pipe would be sharing \
                     one fabricated handle, which is what an ungated raw \
                     syscall on the host produces",
                );
            }
        }
        for fd in a.into_iter().chain(b) {
            let _ = crate::file::close(fd);
        }
    }

    /// Closing a pipe end twice is an error the second time — the handle is
    /// genuinely released rather than merely forgotten.
    #[test]
    fn test_double_close_of_a_pipe_end_is_rejected() {
        let mut fds: [Fd; 2] = [-1, -1];
        assert_eq!(pipe(fds.as_mut_ptr()), 0);
        assert_eq!(crate::file::close(fds[0]), 0, "first close succeeds");
        assert_eq!(crate::file::close(fds[0]), -1, "second close must fail");
        assert_eq!(errno::get_errno(), errno::EBADF);
        let _ = crate::file::close(fds[1]);
    }
}
