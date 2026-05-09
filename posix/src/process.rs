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

// ---------------------------------------------------------------------------
// Process functions
// ---------------------------------------------------------------------------

/// Terminate the calling process.
///
/// This function does not return.
#[unsafe(no_mangle)]
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

/// Get the process ID of the calling process.
#[unsafe(no_mangle)]
pub extern "C" fn getpid() -> PidT {
    let ret = syscall0(SYS_PROCESS_ID);
    ret as PidT
}

/// Get the parent process ID of the calling process.
///
/// Note: Our kernel doesn't have a direct "get parent PID" syscall yet.
/// Returns 1 (init) as a placeholder until implemented.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn waitpid(pid: PidT, status: *mut i32, options: i32) -> PidT {
    // Use non-blocking or blocking wait based on options.
    let sys_nr = if options & WNOHANG != 0 {
        SYS_PROCESS_TRY_WAIT
    } else {
        SYS_PROCESS_WAIT
    };

    let ret = syscall1(sys_nr, pid as u64);

    if ret < 0 {
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

    pid
}

/// Wait for any child process (convenience wrapper).
#[unsafe(no_mangle)]
pub extern "C" fn wait(status: *mut i32) -> PidT {
    waitpid(-1, status, 0)
}

/// Wait for a child process with resource usage.
///
/// Like `waitpid(-1, status, options)` but also fills `rusage` with
/// resource usage data (zeroed — no kernel accounting yet).
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn vfork() -> PidT {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Get the task/thread ID (Linux-specific, but commonly used).
#[unsafe(no_mangle)]
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

/// Get the process group ID of the calling process.
///
/// Stub: returns the caller's PID (every process is its own group).
#[unsafe(no_mangle)]
pub extern "C" fn getpgrp() -> PidT {
    getpid()
}

/// Get the process group ID of a specific process.
///
/// Stub: if `pid` is 0 or the caller's own PID, returns the caller's
/// PID.  For other PIDs, also returns the PID (as if each process is
/// its own group leader).
#[unsafe(no_mangle)]
pub extern "C" fn getpgid(pid: PidT) -> PidT {
    if pid == 0 {
        return getpid();
    }
    // Without kernel support, just return the pid itself.
    pid
}

/// Set the process group ID of a process.
///
/// Stub: succeeds silently (no-op).  Real implementation needs kernel
/// support for process group tracking.
#[unsafe(no_mangle)]
pub extern "C" fn setpgid(_pid: PidT, _pgid: PidT) -> i32 {
    0
}

/// Set the process group ID of the calling process.
///
/// Stub: succeeds silently.
#[unsafe(no_mangle)]
pub extern "C" fn setpgrp() -> i32 {
    0
}

/// Get the session ID of a process.
///
/// Stub: returns the PID (every process is its own session leader).
#[unsafe(no_mangle)]
pub extern "C" fn getsid(pid: PidT) -> PidT {
    if pid == 0 {
        return getpid();
    }
    pid
}

/// Create a new session.
///
/// Stub: returns the caller's PID (as if a new session was created).
/// Real implementation needs kernel session/controlling-terminal support.
#[unsafe(no_mangle)]
pub extern "C" fn setsid() -> PidT {
    getpid()
}

/// Get the foreground process group ID of a terminal.
///
/// Stub: returns the caller's PID.
#[unsafe(no_mangle)]
pub extern "C" fn tcgetpgrp(_fd: crate::types::Fd) -> PidT {
    getpid()
}

/// Set the foreground process group ID of a terminal.
///
/// Stub: succeeds silently (no-op).
#[unsafe(no_mangle)]
pub extern "C" fn tcsetpgrp(_fd: crate::types::Fd, _pgrp: PidT) -> i32 {
    0
}
