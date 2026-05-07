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

/// Get the task/thread ID (Linux-specific, but commonly used).
#[unsafe(no_mangle)]
pub extern "C" fn gettid() -> PidT {
    let ret = syscall0(SYS_TASK_ID);
    ret as PidT
}
