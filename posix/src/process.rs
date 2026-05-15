//! POSIX process management functions.
//!
//! Implements `_exit`, `getpid`, `getppid`, `waitpid`, `fork`, `execve`.
//!
//! ## Fork Semantics
//!
//! Our OS uses spawn-style process creation (like Windows/Fuchsia),
//! not fork+exec.  The `fork()` function is provided for compatibility
//! but it creates a new process via `SYS_PROCESS_SPAWN` and is NOT a
//! true fork (no address space copy).  Programs that depend on fork's
//! COW semantics must be adapted.
//!
//! For `posix_spawn`-style usage (the common case), this works fine
//! since fork is immediately followed by exec anyway.

use crate::errno;
use crate::syscall::*;
use crate::types::*;

// ---------------------------------------------------------------------------
// Child PID tracking
// ---------------------------------------------------------------------------

/// Most recently spawned child PID.
///
/// Updated by `posix_spawn`/`posix_spawnp` when a child is created.
/// Used by `waitpid(-1, ...)` to return the correct child PID, since
/// our kernel's `SYS_PROCESS_WAIT` returns the exit code rather than
/// the child PID.
static mut LAST_CHILD_PID: PidT = 0;

/// Record a newly spawned child's PID.
///
/// Called from `posix_spawn` / `posix_spawnp` after a successful spawn.
pub(crate) fn record_child_pid(pid: PidT) {
    // SAFETY: Single-threaded access.
    unsafe { core::ptr::addr_of_mut!(LAST_CHILD_PID).write(pid); }
}

// ---------------------------------------------------------------------------
// waitpid flags
// ---------------------------------------------------------------------------

/// Return immediately if no child has exited.
pub const WNOHANG: i32 = 1;
/// Also report stopped (not traced) children.
pub const WUNTRACED: i32 = 2;

// ---------------------------------------------------------------------------
// Wait status macros
// ---------------------------------------------------------------------------

/// True if the child terminated normally (via exit).
#[inline]
#[must_use]
pub const fn wifexited(status: i32) -> bool {
    status.trailing_zeros() >= 7
}

/// Return the exit status of the child (only valid if WIFEXITED).
#[inline]
#[must_use]
#[allow(clippy::arithmetic_side_effects)]
pub const fn wexitstatus(status: i32) -> i32 {
    (status >> 8) & 0xff
}

/// True if the child was terminated by a signal.
#[inline]
#[must_use]
pub const fn wifsignaled(status: i32) -> bool {
    (status & 0x7f) != 0 && (status & 0x7f) != 0x7f
}

/// Return the signal number that caused the child to terminate.
#[inline]
#[must_use]
pub const fn wtermsig(status: i32) -> i32 {
    status & 0x7f
}

/// True if the child was resumed by `SIGCONT`.
///
/// Linux encoding: continued status is `0xFFFF`.
#[inline]
#[must_use]
pub const fn wifcontinued(status: i32) -> bool {
    status == 0xFFFF
}

// ---------------------------------------------------------------------------
// Process functions
// ---------------------------------------------------------------------------

/// Terminate the calling process.
///
/// This function does not return.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn _exit(status: i32) -> ! {
    let _ = syscall1(SYS_EXIT, status as u64);
    // Should never reach here, but the kernel guarantees process death.
    loop {
        // SAFETY: hlt is a valid x86_64 instruction, safe in ring 3
        // (it just waits for an interrupt, and the process is already
        // marked for exit so it won't be scheduled again).
        unsafe { core::arch::asm!("hlt", options(nostack, nomem)) };
    }
}

/// C11 `_Exit` — immediate process termination (same as POSIX `_exit`).
///
/// Unlike `exit()`, does not call atexit handlers or flush stdio buffers.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(non_snake_case, clippy::used_underscore_items)]
pub extern "C" fn _Exit(status: i32) -> ! {
    _exit(status);
}

/// Get the process ID of the calling process.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getpid() -> PidT {
    let ret = syscall0(SYS_PROCESS_ID);
    ret as PidT
}

/// Get the parent process ID of the calling process.
///
/// Note: Our kernel doesn't have a direct "get parent PID" syscall yet.
/// Returns 1 (init) as a placeholder until implemented.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getppid() -> PidT {
    // TODO: Add SYS_PROCESS_PARENT_ID syscall.
    1
}

/// Wait for a child process to change state.
///
/// # Parameters
///
/// - `pid`: Process ID to wait for, or -1 for any child.
/// - `status`: Pointer to status buffer (set on return).
/// - `options`: `WNOHANG` for non-blocking.
///
/// Returns the PID of the child, 0 if WNOHANG and no child changed
/// state, or -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn waitpid(pid: PidT, status: *mut i32, options: i32) -> PidT {
    // Use non-blocking or blocking wait based on options.
    let sys_nr = if options & WNOHANG != 0 {
        SYS_PROCESS_TRY_WAIT
    } else {
        SYS_PROCESS_WAIT
    };

    let ret = syscall1(sys_nr, pid as u64);

    if ret < 0 {
        // POSIX: WNOHANG with no child state change returns 0, not -1.
        // The kernel signals "nothing ready" with WouldBlock (-4).
        if (options & WNOHANG) != 0 && ret == errno::native::WOULD_BLOCK {
            return 0;
        }
        return errno::translate(ret) as PidT;
    }

    // The kernel returns the exit code in rax for blocking wait.
    // Pack it into a wait status: (exit_code << 8) | 0 (normal exit).
    if !status.is_null() {
        let exit_code = ret as i32;
        // SAFETY: Caller guarantees status is valid or null (checked above).
        unsafe {
            #[allow(clippy::arithmetic_side_effects)]
            let packed = (exit_code & 0xff) << 8;
            *status = packed;
        }
    }

    // The kernel returns the exit code, not the child PID.  For
    // positive pid arguments, the caller already knows which child
    // they waited for.  For pid < 0 (wait-for-any), use the most
    // recently spawned child PID recorded by posix_spawn, or
    // fallback to 1 if unknown.
    if pid > 0 {
        pid
    } else {
        let child = unsafe { core::ptr::addr_of!(LAST_CHILD_PID).read() };
        if child > 0 { child } else { 1 }
    }
}

/// Wait for any child process (convenience wrapper).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn wait(status: *mut i32) -> PidT {
    waitpid(-1, status, 0)
}

/// Wait for a child process with resource usage.
///
/// Like `waitpid(-1, status, options)` but also fills `rusage` with
/// resource usage data (zeroed — no kernel accounting yet).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn wait3(
    status: *mut i32,
    options: i32,
    rusage: *mut crate::resource::Rusage,
) -> PidT {
    // Zero the rusage if provided.
    if !rusage.is_null() {
        // SAFETY: Caller guarantees rusage is valid.
        unsafe { core::ptr::write_bytes(rusage, 0, 1); }
    }
    waitpid(-1, status, options)
}

/// Wait for a specific child process with resource usage.
///
/// Like `waitpid` but also fills `rusage`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn wait4(
    pid: PidT,
    status: *mut i32,
    options: i32,
    rusage: *mut crate::resource::Rusage,
) -> PidT {
    if !rusage.is_null() {
        // SAFETY: Caller guarantees rusage is valid.
        unsafe { core::ptr::write_bytes(rusage, 0, 1); }
    }
    waitpid(pid, status, options)
}

// ---------------------------------------------------------------------------
// waitid
// ---------------------------------------------------------------------------

/// Identifer type for `waitid`.
pub const P_PID: i32 = 1;
/// Wait for any child.
pub const P_ALL: i32 = 0;
/// Wait for a process group.
pub const P_PGID: i32 = 2;

/// Extended wait for a child process.
///
/// Stub: delegates to `waitpid` internally.  The `infop` parameter
/// is not filled in (would need `siginfo_t` support).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn waitid(
    idtype: i32,
    id: PidT,
    _infop: *mut core::ffi::c_void,
    options: i32,
) -> i32 {
    let pid = match idtype {
        P_PID => id,
        P_ALL => -1,
        P_PGID => {
            // We don't really support process groups.
            errno::set_errno(errno::ENOSYS);
            return -1;
        }
        _ => {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
    };

    let ret = waitpid(pid, core::ptr::null_mut(), options);
    if ret < 0 { -1 } else { 0 }
}

// ---------------------------------------------------------------------------
// fork / exec
// ---------------------------------------------------------------------------

/// Create a child process.
///
/// **WARNING**: This is NOT a true fork.  Our OS uses spawn-style process
/// creation.  `fork()` here returns -1 with `ENOSYS` because a true fork
/// (address space copy) is not yet implemented.  Use `posix_spawn()` or
/// the native `SYS_PROCESS_SPAWN` instead.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fork() -> PidT {
    // True fork requires address space duplication, which is complex
    // and not yet implemented.  Return ENOSYS for now.
    //
    // Programs should use posix_spawn() or vfork()+exec() patterns,
    // which can be implemented via SYS_PROCESS_SPAWN.
    errno::set_errno(errno::ENOSYS);
    -1
}

// execve is implemented in spawn.rs with real ELF loading.

/// Equivalent to `fork()` (stub — returns -1 with `ENOSYS`).
///
/// In a proper implementation, `vfork()` would suspend the parent until
/// the child calls `exec*()` or `_exit()`.  Since we don't have fork at
/// all, this has the same behavior as our `fork()` stub.
///
/// Programs should use `posix_spawn()` instead.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn vfork() -> PidT {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Get the task/thread ID (Linux-specific, but commonly used).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn gettid() -> PidT {
    let ret = syscall0(SYS_TASK_ID);
    ret as PidT
}

// ---------------------------------------------------------------------------
// Process groups / sessions (stubs)
// ---------------------------------------------------------------------------
//
// Our kernel doesn't have process groups or sessions yet.  These stubs
// return the process's own PID as its group/session ID, making every
// process appear to be its own group and session leader.  This is
// sufficient for programs that query but don't rely on job control.

// ---------------------------------------------------------------------------
// Process group / session tracking
// ---------------------------------------------------------------------------
//
// Without kernel support for multi-process job control, we track the
// calling process's own PGID and SID in static variables.  Queries for
// other PIDs fall back to returning the PID itself (each process is its
// own group leader).  This gives consistent behavior for programs that
// call setpgid/setsid and later query their own group/session.

/// Process group ID of the calling process.  Initialized to our PID
/// on first call (lazy init via 0 sentinel; real PIDs are ≥ 1).
static mut OUR_PGID: PidT = 0;

/// Session ID of the calling process.  Same lazy-init pattern.
static mut OUR_SID: PidT = 0;

/// Foreground process group of the terminal (set by tcsetpgrp).
static mut FG_PGRP: PidT = 0;

/// Ensure our PGID/SID are initialized (called before any getter).
///
/// On first call, sets both to our PID (process is its own group/session
/// leader at startup, matching POSIX semantics for the initial process).
fn ensure_pg_init() {
    // SAFETY: single-address-space process, no concurrency.
    unsafe {
        if core::ptr::addr_of!(OUR_PGID).read() == 0 {
            core::ptr::addr_of_mut!(OUR_PGID).write(getpid());
        }
        if core::ptr::addr_of!(OUR_SID).read() == 0 {
            core::ptr::addr_of_mut!(OUR_SID).write(getpid());
        }
        if core::ptr::addr_of!(FG_PGRP).read() == 0 {
            core::ptr::addr_of_mut!(FG_PGRP).write(getpid());
        }
    }
}

/// Get the process group ID of the calling process.
///
/// Returns the PGID set by `setpgid()` or `setpgrp()`, defaulting
/// to our own PID (we are our own group leader at startup).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getpgrp() -> PidT {
    ensure_pg_init();
    // SAFETY: initialized above.
    unsafe { core::ptr::addr_of!(OUR_PGID).read() }
}

/// Get the process group ID of a specific process.
///
/// For `pid` == 0 or our own PID, returns the stored PGID.
/// For other PIDs, returns the PID itself (no kernel visibility into
/// other processes' groups).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getpgid(pid: PidT) -> PidT {
    ensure_pg_init();
    let us = getpid();
    if pid == 0 || pid == us {
        // SAFETY: initialized.
        return unsafe { core::ptr::addr_of!(OUR_PGID).read() };
    }
    // Without kernel support, each other process is assumed to be its
    // own group leader.
    pid
}

/// Set the process group ID of a process.
///
/// `pid` == 0 means the calling process.  `pgid` == 0 means set the
/// PGID to the target PID.  Only our own PGID can actually be changed
/// (no kernel support for modifying other processes).
///
/// Returns 0 on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::similar_names)] // POSIX parameter names: pid and pgid.
pub extern "C" fn setpgid(pid: PidT, pgid: PidT) -> i32 {
    ensure_pg_init();
    let us = getpid();
    let target = if pid == 0 { us } else { pid };
    if target != us {
        // Can't change other processes — succeed silently to avoid
        // breaking programs that call setpgid(child, ...) after spawn.
        return 0;
    }
    let new_pgid = if pgid == 0 { us } else { pgid };
    // SAFETY: single process.
    unsafe { core::ptr::addr_of_mut!(OUR_PGID).write(new_pgid); }
    0
}

/// Set the process group ID of the calling process to its own PID.
///
/// Equivalent to `setpgid(0, 0)`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setpgrp() -> i32 {
    setpgid(0, 0)
}

/// Get the session ID of a process.
///
/// For `pid` == 0 or our own PID, returns the stored SID.
/// For other PIDs, returns the PID itself.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getsid(pid: PidT) -> PidT {
    ensure_pg_init();
    let us = getpid();
    if pid == 0 || pid == us {
        // SAFETY: initialized.
        return unsafe { core::ptr::addr_of!(OUR_SID).read() };
    }
    pid
}

/// Create a new session.
///
/// Sets our SID and PGID to our own PID (new session leader is its
/// own process group leader).  Returns the new SID.
///
/// POSIX requires the caller not be a process group leader already,
/// but since we track only our own state and have no kernel enforcement,
/// we always succeed.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setsid() -> PidT {
    let us = getpid();
    // SAFETY: single process.
    unsafe {
        core::ptr::addr_of_mut!(OUR_SID).write(us);
        core::ptr::addr_of_mut!(OUR_PGID).write(us);
        core::ptr::addr_of_mut!(FG_PGRP).write(us);
    }
    us
}

/// Get the foreground process group ID of a terminal.
///
/// Returns the PGID last set by `tcsetpgrp()`, defaulting to our
/// own PID.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tcgetpgrp(_fd: crate::types::Fd) -> PidT {
    ensure_pg_init();
    // SAFETY: initialized.
    unsafe { core::ptr::addr_of!(FG_PGRP).read() }
}

/// Set the foreground process group ID of a terminal.
///
/// Stores the value for later retrieval by `tcgetpgrp()`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tcsetpgrp(_fd: crate::types::Fd, pgrp: PidT) -> i32 {
    ensure_pg_init();
    if pgrp <= 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // SAFETY: single process.
    unsafe { core::ptr::addr_of_mut!(FG_PGRP).write(pgrp); }
    0
}

// ===========================================================================
// Linux-specific process control stubs
// ===========================================================================

/// Linux `clone` — create a new process/thread.
///
/// Stub: returns -1 with ENOSYS.  Our OS uses `posix_spawn` for
/// process creation and doesn't support Linux-style clone flags.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn clone(
    _fn_ptr: *const u8,
    _child_stack: *mut u8,
    _flags: i32,
    _arg: *mut u8,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Linux `unshare` — disassociate parts of the execution context.
///
/// Stub: returns -1 with ENOSYS (namespaces not implemented).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn unshare(_flags: i32) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Linux `setns` — reassociate a thread with a namespace.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setns(_fd: i32, _nstype: i32) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Mount a filesystem.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mount(
    _source: *const u8,
    _target: *const u8,
    _fstype: *const u8,
    _flags: u64,
    _data: *const u8,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Unmount a filesystem.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn umount(_target: *const u8) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Unmount a filesystem with flags.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn umount2(_target: *const u8, _flags: i32) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Wait flag constants match Linux --

    #[test]
    fn test_wnohang_value() {
        assert_eq!(WNOHANG, 1);
    }

    #[test]
    fn test_wuntraced_value() {
        assert_eq!(WUNTRACED, 2);
    }

    // -- waitid id type constants --

    #[test]
    fn test_waitid_idtype_constants() {
        assert_eq!(P_ALL, 0);
        assert_eq!(P_PID, 1);
        assert_eq!(P_PGID, 2);
    }

    // -- wifexited: normal exit has low 7 bits zero --

    #[test]
    fn test_wifexited_normal_exit_code_0() {
        // Normal exit with code 0: status = (0 << 8) | 0 = 0
        let status = 0;
        assert!(wifexited(status));
        assert_eq!(wexitstatus(status), 0);
    }

    #[test]
    fn test_wifexited_normal_exit_code_1() {
        // Normal exit with code 1: status = (1 << 8) | 0 = 256
        let status = 1 << 8;
        assert!(wifexited(status));
        assert_eq!(wexitstatus(status), 1);
    }

    #[test]
    fn test_wifexited_normal_exit_code_42() {
        let status = 42 << 8;
        assert!(wifexited(status));
        assert_eq!(wexitstatus(status), 42);
    }

    #[test]
    fn test_wifexited_normal_exit_code_255() {
        let status = 255 << 8;
        assert!(wifexited(status));
        assert_eq!(wexitstatus(status), 255);
    }

    // -- wifsignaled: signal death has low 7 bits = signal number --

    #[test]
    fn test_wifsignaled_signal_2() {
        // Killed by signal 2 (SIGINT): status = 2
        let status = 2;
        assert!(wifsignaled(status));
        assert!(!wifexited(status));
        assert_eq!(wtermsig(status), 2);
    }

    #[test]
    fn test_wifsignaled_signal_9() {
        // Killed by signal 9 (SIGKILL): status = 9
        let status = 9;
        assert!(wifsignaled(status));
        assert_eq!(wtermsig(status), 9);
    }

    #[test]
    fn test_wifsignaled_signal_15() {
        // Killed by signal 15 (SIGTERM): status = 15
        let status = 15;
        assert!(wifsignaled(status));
        assert_eq!(wtermsig(status), 15);
    }

    // -- stopped status: low byte = 0x7f --

    #[test]
    fn test_stopped_not_exited_or_signaled() {
        // Stopped by SIGSTOP (19): status = (19 << 8) | 0x7f = 4991
        let status = (19 << 8) | 0x7f;
        assert!(!wifexited(status));
        assert!(!wifsignaled(status)); // 0x7f is excluded from signaled
    }

    // -- Edge cases --

    #[test]
    fn test_wifexited_status_zero_all_bits() {
        // Status 0: exit(0) — wifexited must be true.
        assert!(wifexited(0));
        assert_eq!(wexitstatus(0), 0);
    }

    #[test]
    fn test_wifexited_false_for_signal_1() {
        // Signal 1 (SIGHUP): low 7 bits = 1
        assert!(!wifexited(1));
    }

    #[test]
    fn test_wtermsig_masks_low_7_bits() {
        // Ensure only low 7 bits are returned
        let status = 0xFF; // low 7 = 0x7F, bit 7 = 1 (core dump)
        assert_eq!(wtermsig(status), 0x7f);
    }

    #[test]
    fn test_wexitstatus_masks_byte() {
        // Bits 15:8 is the exit code
        let status = 0xAB_00; // exit code 0xAB = 171
        assert_eq!(wexitstatus(status), 0xAB);
    }

    #[test]
    fn test_wexitstatus_ignores_high_bits() {
        // Only bits 15:8 matter
        let status: i32 = 0x12_34_00u32 as i32;
        assert_eq!(wexitstatus(status), 0x34);
    }

    // -- Stub functions --

    #[test]
    fn test_fork_returns_enosys() {
        assert_eq!(fork(), -1);
    }

    #[test]
    fn test_vfork_returns_enosys() {
        assert_eq!(vfork(), -1);
    }

    #[test]
    fn test_getppid_returns_1() {
        assert_eq!(getppid(), 1);
    }

    #[test]
    fn test_clone_returns_enosys() {
        assert_eq!(clone(core::ptr::null(), core::ptr::null_mut(), 0, core::ptr::null_mut()), -1);
    }

    #[test]
    fn test_unshare_returns_enosys() {
        assert_eq!(unshare(0), -1);
    }

    #[test]
    fn test_setns_returns_enosys() {
        assert_eq!(setns(0, 0), -1);
    }

    #[test]
    fn test_mount_returns_enosys() {
        assert_eq!(mount(core::ptr::null(), core::ptr::null(), core::ptr::null(), 0, core::ptr::null()), -1);
    }

    #[test]
    fn test_umount_returns_enosys() {
        assert_eq!(umount(core::ptr::null()), -1);
    }

    #[test]
    fn test_umount2_returns_enosys() {
        assert_eq!(umount2(core::ptr::null(), 0), -1);
    }

    // -- wifcontinued: continued status is 0xFFFF --

    #[test]
    fn test_wifcontinued_true() {
        // Linux continued status encoding.
        assert!(wifcontinued(0xFFFF));
    }

    #[test]
    fn test_wifcontinued_false_for_normal_exit() {
        assert!(!wifcontinued(42 << 8));
        assert!(!wifcontinued(0));
    }

    #[test]
    fn test_wifcontinued_false_for_signal() {
        assert!(!wifcontinued(9));
        assert!(!wifcontinued(11));
    }

    #[test]
    fn test_wifcontinued_false_for_stopped() {
        assert!(!wifcontinued((19 << 8) | 0x7F));
    }

    #[test]
    fn test_wifcontinued_mutually_exclusive() {
        // Continued status must not be recognized as exited/signaled/stopped.
        let status = 0xFFFF;
        assert!(wifcontinued(status));
        assert!(!wifexited(status));
        assert!(!wifsignaled(status));
        // WIFSTOPPED: low byte = 0xFF != 0x7F, so not stopped.
    }
}
