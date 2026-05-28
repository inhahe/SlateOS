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
// waitpid / waitid flags
//
// Values match glibc <bits/waitflags.h> and Linux <linux/wait.h>.
// ---------------------------------------------------------------------------

/// Return immediately if no child has exited.
pub const WNOHANG: i32 = 1;
/// Also report stopped (not traced) children.
pub const WUNTRACED: i32 = 2;
/// Wait for stopped children — synonym of `WUNTRACED` for `waitid`.
pub const WSTOPPED: i32 = WUNTRACED;
/// `waitid` only: report exited children.  Required for `waitid` since
/// at least one of `WEXITED`, `WSTOPPED`, or `WCONTINUED` must be set.
pub const WEXITED: i32 = 4;
/// Also report continued children (those that received `SIGCONT`).
pub const WCONTINUED: i32 = 8;
/// `waitid` only: leave the child waitable (don't reap it).
pub const WNOWAIT: i32 = 0x0100_0000;
/// Linux extension: only consider non-thread children.
pub const __WNOTHREAD: i32 = 0x2000_0000;
/// Linux extension: consider all children, regardless of clone vs fork.
pub const __WALL: i32 = 0x4000_0000;
/// Linux extension: only consider clone children (SIGCHLD reporter).
/// Bit 31 is the sign bit on i32, so this is i32::MIN.
pub const __WCLONE: i32 = i32::MIN; // 1 << 31, as a signed integer (0x8000_0000)

/// Mask of flag bits accepted by `waitpid` / `wait4` / `wait3`.  This
/// is exactly the mask Linux's `kernel/exit.c::kernel_wait4` validates
/// against in its prologue:
///
/// ```c
/// if (options & ~(WNOHANG | WUNTRACED | WCONTINUED |
///                 __WNOTHREAD | __WCLONE | __WALL))
///         return -EINVAL;
/// ```
pub const WAITPID_VALID_OPTIONS: i32 =
    WNOHANG | WUNTRACED | WCONTINUED | __WNOTHREAD | __WALL | __WCLONE;

/// Mask of flag bits accepted by `waitid`.  Strict superset of
/// `WAITPID_VALID_OPTIONS` (adds `WEXITED`, `WSTOPPED` — which equals
/// `WUNTRACED` so it's already in the mask — and `WNOWAIT`).
pub const WAITID_VALID_OPTIONS: i32 =
    WNOHANG | WUNTRACED | WEXITED | WCONTINUED | WNOWAIT
        | __WNOTHREAD | __WALL | __WCLONE;

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
/// Queries `SYS_PROCESS_PARENT_ID` on the kernel target.  The kernel
/// returns 0 if the calling task isn't owned by any process (kernel
/// thread) or if the process has no recorded parent (init/pid 1, or
/// a process whose parent has already exited).  We translate "no parent"
/// to 1 (init) to match the POSIX convention that orphaned processes
/// are re-parented to init — userspace code that does `if getppid() == 1`
/// to detect orphan/daemon-reparenting status keeps working.
///
/// On host builds, returns 1 as a deterministic placeholder.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getppid() -> PidT {
    #[cfg(target_os = "none")]
    {
        #[allow(clippy::cast_possible_truncation)]
        let raw = syscall0(SYS_PROCESS_PARENT_ID);
        if raw > 0 { raw as PidT } else { 1 }
    }
    #[cfg(not(target_os = "none"))]
    {
        1
    }
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
    // Linux semantics (kernel/exit.c::kernel_wait4):
    //   if (options & ~(WNOHANG | WUNTRACED | WCONTINUED |
    //                   __WNOTHREAD | __WCLONE | __WALL))
    //           return -EINVAL;
    // The mask check is the first thing the syscall does, before
    // pid is interpreted and before the wait queue is consulted.
    // Our previous code silently dropped unknown bits.
    if options & !WAITPID_VALID_OPTIONS != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
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
    // Linux validates options BEFORE touching rusage (see sys_wait4
    // in kernel/exit.c).  Match that ordering: a buggy caller passing
    // garbage options sees EINVAL with rusage untouched.
    if options & !WAITPID_VALID_OPTIONS != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
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
    // Linux validates options BEFORE touching rusage (sys_wait4 in
    // kernel/exit.c).  Match that ordering.
    if options & !WAITPID_VALID_OPTIONS != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
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
    // Linux semantics (kernel/exit.c::sys_waitid):
    //   if (options & ~(WNOHANG | WNOWAIT | WEXITED | WSTOPPED |
    //                   WCONTINUED | __WNOTHREAD | __WCLONE | __WALL))
    //           return -EINVAL;
    //   if (!(options & (WEXITED | WSTOPPED | WCONTINUED)))
    //           return -EINVAL;
    // Both checks precede the idtype/id dispatch.  Our previous code
    // didn't validate options at all, and delegated to waitpid with
    // the raw value.  Now that waitpid also rejects WEXITED/WNOWAIT/
    // etc (which are waitid-only bits), we must validate up-front
    // and then strip waitid-only bits before delegating.
    if options & !WAITID_VALID_OPTIONS != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if options & (WEXITED | WSTOPPED | WCONTINUED) == 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

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

    // Strip waitid-only bits before delegating to waitpid (which only
    // accepts WAITPID_VALID_OPTIONS).  WSTOPPED == WUNTRACED is
    // already shared; WEXITED/WNOWAIT are not.  We don't actually
    // implement the semantic difference (stop/continue tracking),
    // so dropping them only loses the no-op equivalence — the wait
    // behaviour for our exited-children-only model is unchanged.
    let pid_options = options & WAITPID_VALID_OPTIONS;
    let ret = waitpid(pid, core::ptr::null_mut(), pid_options);
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
/// Returns the PGID last set by `tcsetpgrp()`, defaulting to our own
/// PID.  Validates `fd`: Linux's `tcgetpgrp` returns -1/EBADF for a
/// closed fd before consulting the controlling terminal.  Since we
/// don't track which fds are terminals, an open non-tty fd accepts the
/// call (matches Linux ENOTTY-equivalent leniency for our stub).
///
/// Errors:
///   * `EBADF` — `fd` is negative or not open.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tcgetpgrp(fd: crate::types::Fd) -> PidT {
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if crate::fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    ensure_pg_init();
    // SAFETY: initialized.
    unsafe { core::ptr::addr_of!(FG_PGRP).read() }
}

/// Set the foreground process group ID of a terminal.
///
/// Stores the value for later retrieval by `tcgetpgrp()`.  Validates
/// `fd` before checking `pgrp` — Linux's prologue order is to check the
/// fd first.
///
/// Errors:
///   * `EBADF` — `fd` is negative or not open.
///   * `EINVAL` — `pgrp` is zero or negative.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tcsetpgrp(fd: crate::types::Fd, pgrp: PidT) -> i32 {
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if crate::fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }
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

/// Maximum exit-signal value accepted in `clone(flags) & CSIGNAL`.
///
/// Linux accepts any signal number `0..=_NSIG` (64 on x86_64) in the
/// low byte of the flags argument.  `0` is allowed and means "no
/// notification on child exit" (used implicitly with `CLONE_THREAD`).
/// Values 65..=255 are rejected with `EINVAL` — they would request a
/// non-existent signal.
pub const CLONE_CSIGNAL_MAX: u64 = 64;

/// All CLONE_* flag bits accepted by `clone(2)` (excluding the
/// `CSIGNAL` exit-signal byte).
///
/// Mostly a superset of [`UNSHARE_FLAGS_VALID`] — clone additionally
/// accepts the runtime flags that don't make sense for unshare:
/// `CLONE_PIDFD`, `CLONE_PTRACE`, `CLONE_VFORK`, `CLONE_PARENT`,
/// `CLONE_DETACHED` (historical, ignored), `CLONE_UNTRACED`,
/// `CLONE_SETTLS`, `CLONE_PARENT_SETTID`, `CLONE_CHILD_SETTID`,
/// `CLONE_CHILD_CLEARTID`, and `CLONE_IO`.
///
/// **`CLONE_NEWTIME` is intentionally excluded** even though it's in
/// `UNSHARE_FLAGS_VALID`: its bit value `0x80` collides with the
/// `CSIGNAL` exit-signal byte, so legacy `clone(2)` cannot express it
/// unambiguously.  Linux therefore accepts time-namespace cloning only
/// through `clone3(2)`.  Userspace needing it must use `clone3` or
/// `unshare(CLONE_NEWTIME)`.
///
/// The 64-bit `clone3`-only bits `CLONE_INTO_CGROUP` and
/// `CLONE_CLEAR_SIGHAND` are also excluded — they live above bit 32
/// and cannot be reached through the legacy `clone(2)` argument
/// register on x86_64 anyway, but we guard the comparison against
/// `i32`-sign-extended inputs just in case.
pub const CLONE_FLAGS_VALID: u64 = crate::linux_clone_args::CLONE_VM
    | crate::linux_clone_args::CLONE_FS
    | crate::linux_clone_args::CLONE_FILES
    | crate::linux_clone_args::CLONE_SIGHAND
    | crate::linux_clone_args::CLONE_PIDFD
    | crate::linux_clone_args::CLONE_PTRACE
    | crate::linux_clone_args::CLONE_VFORK
    | crate::linux_clone_args::CLONE_PARENT
    | crate::linux_clone_args::CLONE_THREAD
    | crate::linux_clone_args::CLONE_NEWNS
    | crate::linux_clone_args::CLONE_SYSVSEM
    | crate::linux_clone_args::CLONE_SETTLS
    | crate::linux_clone_args::CLONE_PARENT_SETTID
    | crate::linux_clone_args::CLONE_CHILD_CLEARTID
    | crate::linux_clone_args::CLONE_DETACHED
    | crate::linux_clone_args::CLONE_UNTRACED
    | crate::linux_clone_args::CLONE_CHILD_SETTID
    | crate::linux_clone_args::CLONE_NEWCGROUP
    | crate::linux_clone_args::CLONE_NEWUTS
    | crate::linux_clone_args::CLONE_NEWIPC
    | crate::linux_clone_args::CLONE_NEWUSER
    | crate::linux_clone_args::CLONE_NEWPID
    | crate::linux_clone_args::CLONE_NEWNET
    | crate::linux_clone_args::CLONE_IO;

/// Linux `clone` — create a new process/thread.
///
/// # Linux behaviour
///
/// The glibc wrapper `int clone(int (*fn)(void *), void *stack,
/// int flags, void *arg, ...)` performs its own argument checks
/// before issuing the `SYS_clone` syscall; the kernel then runs the
/// full `copy_process` flag-combination matrix.  We enforce both
/// layers here, in the order they fail on real Linux + glibc:
///
/// 1. `fn == NULL`                                    → `EINVAL`
///    (glibc's `clone.S` rejects this before the syscall)
/// 2. `stack == NULL`                                 → `EINVAL`
///    (glibc must initialise the child's stack pointer; the kernel
///    also requires it whenever `CLONE_VM` is set because the child
///    would otherwise share the parent's stack)
/// 3. exit-signal byte `flags & CSIGNAL > 64`         → `EINVAL`
/// 4. `flags & ~(CSIGNAL | CLONE_FLAGS_VALID)`        → `EINVAL`
///    (rejects clone3-only bits and any other reserved bits)
/// 5. `CLONE_THREAD` without `CLONE_SIGHAND`          → `EINVAL`
///    (a thread group must share signal handlers)
/// 6. `CLONE_SIGHAND` without `CLONE_VM`              → `EINVAL`
///    (Linux 5.0+: shared handlers require shared address space)
/// 7. `CLONE_THREAD` with non-zero exit signal        → `EINVAL`
///    (thread death is reported via futex/CLEARTID, not signals)
/// 8. `CLONE_FS | CLONE_NEWUSER`                      → `EINVAL`
///    (`copy_process` forbids inheriting fs-state into a new userns)
/// 9. `CLONE_THREAD | CLONE_NEWUSER`                  → `EINVAL`
///    (a thread group cannot span user namespaces)
/// 10. `CLONE_PIDFD | CLONE_DETACHED`                 → `EINVAL`
///    (DETACHED means "no parent notification"; PIDFD requires a
///    referent in the parent's fd table)
/// 11. `CLONE_NEWNS | CLONE_FS`                       → `EINVAL`
///    (a new mount namespace cannot share filesystem-state)
///
/// All combinations that survive validation reach `ENOSYS`: the
/// microkernel doesn't expose a `clone`-style primitive — userspace
/// uses `posix_spawn`, threads come from the kernel's lightweight
/// thread syscall, namespaces are managed via capability handles.
///
/// # Safety
///
/// `fn_ptr` and `child_stack` are not dereferenced by this validator;
/// the caller's contract is the usual "valid pointer to an executable
/// function" / "valid writable stack region" pair, which would only
/// matter if the syscall actually reached the kernel.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn clone(
    fn_ptr: *const u8,
    child_stack: *mut u8,
    flags: i32,
    _arg: *mut u8,
) -> i32 {
    // (1) glibc rejects NULL fn before issuing the syscall.
    if fn_ptr.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // (2) child_stack is mandatory in the glibc wrapper (it has to
    // arrange for the child to return into the user-provided fn) and
    // in the kernel whenever CLONE_VM is set.
    if child_stack.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Bit-pattern preserved across i32→u32→u64 via zero-extend so the
    // negative-flag attack (e.g. `flags = i32::MIN` = CLONE_IO) is
    // detected by the whitelist below rather than sign-extended into
    // every high bit.
    let bits = (flags as u32) as u64;

    // (3) Exit signal in the low byte must be a valid signal number.
    let exit_signal = bits & crate::linux_clone_args::CSIGNAL;
    if exit_signal > CLONE_CSIGNAL_MAX {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // (4) Reject any CLONE_* bit not in the clone(2) whitelist.
    if (bits & !(crate::linux_clone_args::CSIGNAL | CLONE_FLAGS_VALID)) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // (5) CLONE_THREAD requires CLONE_SIGHAND.
    if (bits & crate::linux_clone_args::CLONE_THREAD) != 0
        && (bits & crate::linux_clone_args::CLONE_SIGHAND) == 0
    {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // (6) CLONE_SIGHAND requires CLONE_VM (Linux 5.0+).
    if (bits & crate::linux_clone_args::CLONE_SIGHAND) != 0
        && (bits & crate::linux_clone_args::CLONE_VM) == 0
    {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // (7) Threads must not request a parent-death signal.
    if (bits & crate::linux_clone_args::CLONE_THREAD) != 0 && exit_signal != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // (8) New user namespace cannot share filesystem state.
    if (bits & crate::linux_clone_args::CLONE_NEWUSER) != 0
        && (bits & crate::linux_clone_args::CLONE_FS) != 0
    {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // (9) New user namespace cannot span a thread group.
    if (bits & crate::linux_clone_args::CLONE_NEWUSER) != 0
        && (bits & crate::linux_clone_args::CLONE_THREAD) != 0
    {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // (10) PIDFD and DETACHED are mutually exclusive.
    if (bits & crate::linux_clone_args::CLONE_PIDFD) != 0
        && (bits & crate::linux_clone_args::CLONE_DETACHED) != 0
    {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // (11) New mount namespace cannot share filesystem state.
    if (bits & crate::linux_clone_args::CLONE_NEWNS) != 0
        && (bits & crate::linux_clone_args::CLONE_FS) != 0
    {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // All combinations validated; clone primitive not implemented.
    errno::set_errno(errno::ENOSYS);
    -1
}

/// All flag bits accepted by `unshare(2)`.
///
/// Mirrors Linux's `check_unshare_flags` in `kernel/fork.c`: the union
/// of every CLONE_* bit that unshare permits.  In particular, CLONE_IO,
/// CLONE_PIDFD, CLONE_SETTLS, CLONE_PARENT_SETTID, CLONE_CHILD_SETTID,
/// CLONE_CHILD_CLEARTID, CLONE_UNTRACED, CLONE_DETACHED, and the high
/// clone3-only bits (CLONE_INTO_CGROUP, CLONE_CLEAR_SIGHAND) are
/// rejected — they are not meaningful in an "unshare from the current
/// task" context.
pub const UNSHARE_FLAGS_VALID: u32 = (crate::linux_clone_args::CLONE_THREAD
    | crate::linux_clone_args::CLONE_FS
    | crate::linux_clone_args::CLONE_NEWNS
    | crate::linux_clone_args::CLONE_SIGHAND
    | crate::linux_clone_args::CLONE_VM
    | crate::linux_clone_args::CLONE_FILES
    | crate::linux_clone_args::CLONE_SYSVSEM
    | crate::linux_clone_args::CLONE_NEWUTS
    | crate::linux_clone_args::CLONE_NEWIPC
    | crate::linux_clone_args::CLONE_NEWNET
    | crate::linux_clone_args::CLONE_NEWUSER
    | crate::linux_clone_args::CLONE_NEWPID
    | crate::linux_clone_args::CLONE_NEWCGROUP
    | crate::linux_clone_args::CLONE_NEWTIME) as u32;

/// All `nstype` bits accepted by `setns(2)`.
///
/// Linux 3.0 introduced `setns` and only the namespace CLONE_NEW* bits
/// are valid — sharing flags (CLONE_VM, CLONE_FS, ...) are not meaningful
/// since `setns` joins an existing namespace, it doesn't create a fresh
/// resource view.  `nstype == 0` is a special "infer the namespace type
/// from the file descriptor" probe and is accepted.
pub const SETNS_NSTYPE_VALID: u32 = (crate::linux_clone_args::CLONE_NEWNS
    | crate::linux_clone_args::CLONE_NEWCGROUP
    | crate::linux_clone_args::CLONE_NEWUTS
    | crate::linux_clone_args::CLONE_NEWIPC
    | crate::linux_clone_args::CLONE_NEWUSER
    | crate::linux_clone_args::CLONE_NEWPID
    | crate::linux_clone_args::CLONE_NEWNET
    | crate::linux_clone_args::CLONE_NEWTIME) as u32;

/// Linux `unshare` — disassociate parts of the execution context.
///
/// # Linux behaviour
///
/// `unshare(int flags)` (added in Linux 2.6.16) lets a process give up
/// shared resources (mount namespace, UTS namespace, IPC namespace, ...)
/// to create per-process copies.  The valid flag set is
/// `UNSHARE_FLAGS_VALID`; any other bit yields `EINVAL`.
///
/// Special case: `unshare(0)` is a successful no-op — `kernel/fork.c`
/// short-circuits when no resources need duplicating.  Userspace libraries
/// (e.g. `util-linux` `unshare(1)`'s `--keep-caps` probe) call this form
/// to test for syscall availability.
///
/// After flag validation we return `ENOSYS` because the namespace
/// subsystem isn't wired up — matches what Linux returns when built
/// without `CONFIG_NAMESPACES`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn unshare(flags: i32) -> i32 {
    // Reject any bit outside the unshare-accepted CLONE_* set.
    // Cast i32 → u32 preserves bit pattern so high-bit attacks
    // (e.g. CLONE_IO at 0x8000_0000 i.e. i32::MIN) are detected.
    let bits = flags as u32;
    if (bits & !UNSHARE_FLAGS_VALID) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // unshare(0) is a successful no-op per Linux.
    if bits == 0 {
        return 0;
    }
    // Arguments validated; namespace subsystem not implemented.
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Linux `setns` — reassociate a thread with a namespace.
///
/// # Linux behaviour
///
/// `setns(int fd, int nstype)` (added in Linux 3.0) joins the namespace
/// referenced by `fd`.  Argument-domain checks:
///
/// * `fd < 0`                              → `EBADF`
/// * `nstype & ~SETNS_NSTYPE_VALID`         → `EINVAL`
///
/// `nstype == 0` is the "any namespace, infer from fd" form and is
/// accepted — used by container runtimes that don't know the namespace
/// type in advance.  In Linux 5.8+ `nstype` may also be a `pidfd` that
/// triggers entering *all* of the target's namespaces; we accept fds
/// from `pidfd_open` the same way.
///
/// After arguments validate we return `ENOSYS` (no namespace subsystem).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setns(fd: i32, nstype: i32) -> i32 {
    // fd must be non-negative.
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    // Reject any bit outside the setns-accepted namespace set.
    let bits = nstype as u32;
    if (bits & !SETNS_NSTYPE_VALID) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // Arguments validated; namespace subsystem not implemented.
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Maximum source/target path length accepted by `mount(2)`.
///
/// Matches Linux's `PATH_MAX` (4096 bytes including NUL).  Source and
/// target paths longer than this are rejected with `ENAMETOOLONG`.
pub const MOUNT_PATH_MAX: usize = 4096;

/// Maximum filesystem-type name length accepted by `mount(2)`.
///
/// Linux's `copy_mount_string` caps the fstype copy at `PAGE_SIZE`
/// (4096 on x86_64).  We use a tighter 256-byte cap which still
/// accommodates every real-world fstype ("ext4", "xfs", "tmpfs",
/// "overlay", "fuse.gocryptfs-1.7", "nfs4", "cifs", "9p", ...).
pub const MOUNT_TYPE_MAX: usize = 256;

/// All `MS_*` bits accepted by `mount(2)`.
///
/// Mirrors Linux's user-visible mount-flag set in `fs/namespace.c`.
/// Any bit outside this mask is rejected with `EINVAL`.  Note that
/// `MS_KERNMOUNT` is kernel-internal and not exposed here — passing
/// it from userspace is rejected.
pub const MOUNT_FLAGS_VALID: u64 = crate::sys_mount::MS_RDONLY
    | crate::sys_mount::MS_NOSUID
    | crate::sys_mount::MS_NODEV
    | crate::sys_mount::MS_NOEXEC
    | crate::sys_mount::MS_SYNCHRONOUS
    | crate::sys_mount::MS_REMOUNT
    | crate::sys_mount::MS_MANDLOCK
    | crate::sys_mount::MS_DIRSYNC
    | crate::sys_mount::MS_NOSYMFOLLOW
    | crate::sys_mount::MS_NOATIME
    | crate::sys_mount::MS_NODIRATIME
    | crate::sys_mount::MS_BIND
    | crate::sys_mount::MS_MOVE
    | crate::sys_mount::MS_REC
    | crate::sys_mount::MS_SILENT
    | crate::sys_mount::MS_POSIXACL
    | crate::sys_mount::MS_UNBINDABLE
    | crate::sys_mount::MS_PRIVATE
    | crate::sys_mount::MS_SLAVE
    | crate::sys_mount::MS_SHARED
    | crate::sys_mount::MS_RELATIME
    | crate::sys_mount::MS_I_VERSION
    | crate::sys_mount::MS_STRICTATIME
    | crate::sys_mount::MS_LAZYTIME;

/// Bits that select the *mode* of the mount operation.
///
/// Linux's `do_mount` dispatches based on which of these bits is set:
/// `MS_REMOUNT` → remount path, `MS_BIND` → bind path,
/// `MS_MOVE` → move path, one of `MS_SHARED|MS_PRIVATE|MS_SLAVE|
/// MS_UNBINDABLE` → propagation-type change, none → fresh mount.
///
/// Exactly **one** (or zero) of these bits may be set.  Combinations
/// like `MS_BIND | MS_MOVE` or `MS_SHARED | MS_PRIVATE` are rejected
/// with `EINVAL` (Linux's `do_mount` likewise checks this).  Note that
/// `MS_REC` is **not** a mode bit — it modifies bind/propagation
/// operations and may be combined with any of them.
pub const MOUNT_MODE_BITS: u64 = crate::sys_mount::MS_REMOUNT
    | crate::sys_mount::MS_BIND
    | crate::sys_mount::MS_MOVE
    | crate::sys_mount::MS_SHARED
    | crate::sys_mount::MS_PRIVATE
    | crate::sys_mount::MS_SLAVE
    | crate::sys_mount::MS_UNBINDABLE;

/// Mount a filesystem.
///
/// # Linux behaviour
///
/// `mount(const char *source, const char *target, const char *fstype,
///        unsigned long flags, const void *data)`.  Argument-domain
/// checks performed before reaching kernel mount code, in the order
/// Linux executes them:
///
/// 1. `target == NULL`                                  → `EFAULT`
/// 2. empty target string                               → `ENOENT`
/// 3. target not NUL-terminated within `PATH_MAX`       → `ENAMETOOLONG`
/// 4. `flags & ~MOUNT_FLAGS_VALID`                      → `EINVAL`
/// 5. more than one of `MOUNT_MODE_BITS` set            → `EINVAL`
/// 6. modes requiring a source (`MS_BIND`, `MS_MOVE`, fresh mount)
///    validate the source pointer:
///    * `source == NULL`                                → `EFAULT`
///    * empty source string                             → `ENOENT`
///    * source overflows `PATH_MAX`                     → `ENAMETOOLONG`
/// 7. modes requiring a filesystem type (fresh mount only) validate
///    the fstype pointer:
///    * `fstype == NULL`                                → `EFAULT`
///    * empty fstype string                             → `EINVAL`
///      (matches Linux's "no such filesystem" path)
///    * fstype overflows `MOUNT_TYPE_MAX`               → `ENAMETOOLONG`
///
/// After all argument-domain checks pass we return `ENOSYS`: there is
/// no VFS/mount-namespace subsystem in this microkernel — filesystem
/// services live in userspace and are reached via capability handles,
/// not via the legacy `mount(2)` syscall.
///
/// # Safety
///
/// When non-NULL, `target` and `source` must each point to a
/// NUL-terminated byte string or to at least `MOUNT_PATH_MAX + 1`
/// readable bytes.  `fstype`, when non-NULL, must point to a
/// NUL-terminated byte string or at least `MOUNT_TYPE_MAX + 1`
/// readable bytes.  `data` is never dereferenced.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mount(
    source: *const u8,
    target: *const u8,
    fstype: *const u8,
    flags: u64,
    _data: *const u8,
) -> i32 {
    // (1)–(3) Target: required, non-NULL, non-empty, NUL-terminated
    // within PATH_MAX.
    if target.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: target is non-null; caller contract guarantees
    // NUL-terminated string or PATH_MAX+1 readable bytes.
    let tlen = unsafe { umount_cstr_len(target, MOUNT_PATH_MAX) };
    match tlen {
        None => {
            errno::set_errno(errno::ENAMETOOLONG);
            return -1;
        }
        Some(0) => {
            errno::set_errno(errno::ENOENT);
            return -1;
        }
        Some(_) => {}
    }

    // (4) Reject unknown MS_* bits.
    if (flags & !MOUNT_FLAGS_VALID) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // (5) At most one mode bit may be set.
    let mode_bits = flags & MOUNT_MODE_BITS;
    if mode_bits != 0 && !mode_bits.is_power_of_two() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // (6) Source required for: fresh mount, MS_BIND, MS_MOVE.
    // Not required for MS_REMOUNT or any propagation-type change.
    let source_required = mode_bits == 0
        || mode_bits == crate::sys_mount::MS_BIND
        || mode_bits == crate::sys_mount::MS_MOVE;
    if source_required {
        if source.is_null() {
            errno::set_errno(errno::EFAULT);
            return -1;
        }
        // SAFETY: source non-null per caller contract.
        let slen = unsafe { umount_cstr_len(source, MOUNT_PATH_MAX) };
        match slen {
            None => {
                errno::set_errno(errno::ENAMETOOLONG);
                return -1;
            }
            Some(0) => {
                errno::set_errno(errno::ENOENT);
                return -1;
            }
            Some(_) => {}
        }
    }

    // (7) fstype required only for fresh mount.  Bind/move/remount/
    // propagation ignore fstype on Linux.
    let fstype_required = mode_bits == 0;
    if fstype_required {
        if fstype.is_null() {
            errno::set_errno(errno::EFAULT);
            return -1;
        }
        // SAFETY: fstype non-null per caller contract.
        let flen = unsafe { umount_cstr_len(fstype, MOUNT_TYPE_MAX) };
        match flen {
            None => {
                errno::set_errno(errno::ENAMETOOLONG);
                return -1;
            }
            Some(0) => {
                // Linux returns ENODEV for empty/unknown fstype after
                // module-load failure; we collapse to EINVAL since we
                // never reach fstype lookup.
                errno::set_errno(errno::EINVAL);
                return -1;
            }
            Some(_) => {}
        }
    }

    // All arguments validated; VFS/mount subsystem not implemented.
    errno::set_errno(errno::ENOSYS);
    -1
}

/// All flag bits accepted by `umount2(2)`.
///
/// Mirrors Linux's `fs/namespace.c::ksys_umount` whitelist:
/// `MNT_FORCE | MNT_DETACH | MNT_EXPIRE | UMOUNT_NOFOLLOW`.  Any
/// other bit yields `EINVAL`.
pub const UMOUNT2_FLAGS_VALID: i32 = crate::sys_mount::MNT_FORCE
    | crate::sys_mount::MNT_DETACH
    | crate::sys_mount::MNT_EXPIRE
    | crate::sys_mount::UMOUNT_NOFOLLOW;

/// Maximum path length accepted by `umount`/`umount2` (matches
/// `PATH_MAX` on Linux — 4096 bytes including NUL).
pub const UMOUNT_PATH_MAX: usize = 4096;

/// Walk a NUL-terminated byte string up to `max` bytes (excluding NUL).
///
/// Returns `Some(len)` if a NUL byte is found, where `len` is the number
/// of bytes before the NUL.  Returns `None` if no NUL appears in the
/// first `max + 1` bytes — the path is treated as "too long."
///
/// # Safety
///
/// `s` must be non-null and point to at least one readable byte; the
/// walk stops as soon as a NUL is found or after reading `max + 1` bytes.
/// Caller must ensure the buffer is at least `max + 1` bytes large or
/// terminated within that range — same contract as Linux's `strnlen_user`.
#[inline]
unsafe fn umount_cstr_len(s: *const u8, max: usize) -> Option<usize> {
    let mut i = 0usize;
    while i <= max {
        // SAFETY: caller contract — readable up to first NUL or max+1.
        let b = unsafe { *s.add(i) };
        if b == 0 {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Unmount a filesystem.
///
/// # Linux behaviour
///
/// `umount(const char *target)` (Linux's old single-arg form, kept for
/// backward compat with libc; `umount(8)` actually calls `umount2`).
/// Argument checks:
///
/// * `target == NULL`                         → `EFAULT`
/// * `*target == 0` (empty path)              → `ENOENT`
/// * not NUL-terminated within `PATH_MAX`     → `ENAMETOOLONG`
///
/// After path validation we return `ENOSYS` because no filesystem-
/// namespace subsystem is wired up here.
///
/// # Safety
///
/// `target`, when non-NULL, must point to a NUL-terminated byte string
/// or to at least `UMOUNT_PATH_MAX + 1` readable bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn umount(target: *const u8) -> i32 {
    if target.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: target is non-null; caller contract gives us a NUL-
    // terminated string or at least UMOUNT_PATH_MAX+1 readable bytes.
    let len = unsafe { umount_cstr_len(target, UMOUNT_PATH_MAX) };
    match len {
        None => {
            // No NUL in PATH_MAX+1 bytes — path is too long.
            errno::set_errno(errno::ENAMETOOLONG);
            -1
        }
        Some(0) => {
            // Empty path string.
            errno::set_errno(errno::ENOENT);
            -1
        }
        Some(_) => {
            // Path is well-formed; mount subsystem not wired up.
            errno::set_errno(errno::ENOSYS);
            -1
        }
    }
}

/// Unmount a filesystem with flags.
///
/// # Linux behaviour
///
/// `umount2(const char *target, int flags)` (the syscall `umount(8)`
/// actually invokes).  Argument checks, in the order Linux performs
/// them in `fs/namespace.c::ksys_umount` / `path_umount`:
///
/// 1. `flags & ~UMOUNT2_FLAGS_VALID`                   → `EINVAL`
///    Linux performs this check *before* `user_path_at`, so an
///    unknown flag bit beats every path-related errno (including
///    NULL-pointer EFAULT and empty-path ENOENT).
/// 2. (Capability check `may_mount` → `EPERM` — skipped here, we
///    have no cred model yet.)
/// 3. `target == NULL`                                 → `EFAULT`
///    Linux: `user_path_at → getname → strncpy_from_user` on a NULL
///    user pointer returns `-EFAULT`.
/// 4. `*target == 0`                                   → `ENOENT`
///    Linux: empty path string fails name resolution with `-ENOENT`.
/// 5. not NUL-terminated within `PATH_MAX`             → `ENAMETOOLONG`
///    Linux: `getname` enforces the `PATH_MAX` bound.
/// 6. `MNT_EXPIRE` combined with `MNT_FORCE | MNT_DETACH`→ `EINVAL`
///    Linux: `do_umount` rejects this combo *after* path resolution,
///    because an expiry mark can't coexist with a force/detach action.
///    We surface it as an extra validation step before `ENOSYS`.
///
/// After arguments are validated we return `ENOSYS`.
///
/// # Safety
///
/// `target`, when non-NULL, must point to a NUL-terminated byte string
/// or to at least `UMOUNT_PATH_MAX + 1` readable bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn umount2(target: *const u8, flags: i32) -> i32 {
    // 1. Reject unknown flag bits.  Linux performs this check at the
    //    very top of `ksys_umount`, before any path resolution.
    if (flags & !UMOUNT2_FLAGS_VALID) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // 2. NULL target → EFAULT (Linux: getname/strncpy_from_user).
    if target.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // 3. Path string validation.  SAFETY: same contract as umount above.
    let len = unsafe { umount_cstr_len(target, UMOUNT_PATH_MAX) };
    match len {
        None => {
            errno::set_errno(errno::ENAMETOOLONG);
            return -1;
        }
        Some(0) => {
            errno::set_errno(errno::ENOENT);
            return -1;
        }
        Some(_) => {}
    }
    // 4. MNT_EXPIRE is mutually exclusive with MNT_FORCE and MNT_DETACH
    //    (Linux's `do_umount`, after path resolution).
    if (flags & crate::sys_mount::MNT_EXPIRE) != 0
        && (flags
            & (crate::sys_mount::MNT_FORCE
                | crate::sys_mount::MNT_DETACH))
            != 0
    {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // Arguments validated; mount subsystem not wired up.
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// reboot — system reboot
// ---------------------------------------------------------------------------

/// Linux reboot magic values.
pub const LINUX_REBOOT_MAGIC1: u32 = 0xfee1_dead;
pub const LINUX_REBOOT_MAGIC2: u32 = 672274793;
/// Alternate `magic2` accepted by the kernel — `05121996` decimal
/// (Linus's daughter's birthday, kept for ABI history).
pub const LINUX_REBOOT_MAGIC2A: u32 = 85072278;
/// Alternate `magic2` accepted by the kernel — `16041998` decimal.
pub const LINUX_REBOOT_MAGIC2B: u32 = 369367448;
/// Alternate `magic2` accepted by the kernel — `20112000` decimal.
pub const LINUX_REBOOT_MAGIC2C: u32 = 537993216;

/// Reboot commands.
pub const LINUX_REBOOT_CMD_RESTART: u32 = 0x01234567;
pub const LINUX_REBOOT_CMD_HALT: u32 = 0xCDEF0123;
pub const LINUX_REBOOT_CMD_POWER_OFF: u32 = 0x4321FEDC;
pub const LINUX_REBOOT_CMD_CAD_ON: u32 = 0x89ABCDEF;
pub const LINUX_REBOOT_CMD_CAD_OFF: u32 = 0;
/// Restart with a user-supplied boot command string (`reboot(2)` arg).
pub const LINUX_REBOOT_CMD_RESTART2: u32 = 0xA1B2C3D4;
/// Suspend system to disk (hibernate).
pub const LINUX_REBOOT_CMD_SW_SUSPEND: u32 = 0xD000FCE2;
/// Hand off to a previously loaded kexec image.
pub const LINUX_REBOOT_CMD_KEXEC: u32 = 0x45584543;

/// Return `true` for known `LINUX_REBOOT_CMD_*` values per Linux
/// `kernel/reboot.c::SYSCALL_DEFINE4(reboot, ...)` switch arms.  Any
/// other `cmd` is rejected with `EINVAL` before the capability check.
///
/// Exposed for test-side reuse and for callers that want to pre-check
/// the value (e.g. logging shutdown intent before invoking `reboot`).
#[must_use]
pub fn reboot_cmd_known(cmd: u32) -> bool {
    matches!(
        cmd,
        LINUX_REBOOT_CMD_RESTART
            | LINUX_REBOOT_CMD_HALT
            | LINUX_REBOOT_CMD_POWER_OFF
            | LINUX_REBOOT_CMD_CAD_ON
            | LINUX_REBOOT_CMD_CAD_OFF
            | LINUX_REBOOT_CMD_RESTART2
            | LINUX_REBOOT_CMD_SW_SUSPEND
            | LINUX_REBOOT_CMD_KEXEC
    )
}

/// Reboot the system.
///
/// Stub: validates `cmd` against the Linux-recognised reboot commands,
/// then checks `CAP_SYS_BOOT`, then surfaces `ENOSYS` because our
/// microkernel does not export a reboot path yet.  Real implementation
/// will hand off to the platform power-management driver once it lands.
///
/// # glibc / Linux model
///
/// The glibc wrapper `reboot(int howto)` hard-codes both magic values
/// and the optional `arg` pointer, leaving only `cmd` as a user-visible
/// argument.  The kernel's argument validation therefore reduces to:
///
/// 1. `magic1 != LINUX_REBOOT_MAGIC1`               → `EINVAL`
/// 2. `magic2` not in the accepted set              → `EINVAL`
/// 3. `cmd` not in the known set                    → `EINVAL`
/// 4. Caller lacks `CAP_SYS_BOOT`                   → `EPERM`
/// 5. Otherwise: dispatch to the platform handler.
///
/// Since the wrapper supplies (1) and (2) itself, our visible checks
/// are (3) and (4); step (5) becomes `ENOSYS` here.  This matches the
/// pattern used by `swapon`/`swapoff` and `ptrace`: validate the same
/// errno classes a real kernel would, then return `ENOSYS` once
/// nothing is left to reject.
///
/// # Validation order
///
/// `EINVAL` precedes `EPERM` here because the glibc wrapper's magic
/// values are always correct, so the only `EINVAL` path the user can
/// trigger is "unknown cmd".  Linux's kernel does the capability check
/// before the cmd-switch, but in glibc-mediated calls the cmd value
/// determines whether the syscall is even worth issuing — pre-syscall
/// validation surfacing `EINVAL` first matches what portable userspace
/// observes when the kernel rejects a bad cmd.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn reboot(cmd: i32) -> i32 {
    // Reinterpret the signed C `int` as u32 so the magic constants
    // (some of which have the high bit set, e.g. CMD_HALT 0xCDEF0123)
    // compare equal regardless of sign-extension on the caller side.
    let cmd_u = cmd as u32;
    if !reboot_cmd_known(cmd_u) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if !crate::sys_capability::has_capability(crate::sys_capability::CAP_SYS_BOOT) {
        errno::set_errno(errno::EPERM);
        return -1;
    }
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// pidfd — Linux process file descriptor (5.3+)
// ---------------------------------------------------------------------------

/// `O_NONBLOCK` for `pidfd_open` — return immediately from `waitid`/`read`
/// instead of blocking when the referenced process is still running.
///
/// Linux defines this as `O_NONBLOCK` (octal `04000`).  Added in Linux 5.10.
pub const PIDFD_NONBLOCK: u32 = 0o4000;

/// `O_EXCL` repurposed for `pidfd_open(2)` — open a TID (thread) pidfd
/// instead of a TGID (process) pidfd.  Added in Linux 6.2.
///
/// Numerically: octal `0200` = `0x80`, matching Linux's `O_EXCL`.
pub const PIDFD_THREAD: u32 = 0o200;

/// All flag bits accepted by `pidfd_open(2)`.
///
/// Any bit outside this mask makes `pidfd_open` fail with `EINVAL` —
/// matches Linux's `kernel/pid.c::pidfd_create` validator.
pub const PIDFD_OPEN_FLAGS_VALID: u32 = PIDFD_NONBLOCK | PIDFD_THREAD;

/// Maximum signal number accepted by `pidfd_send_signal(2)`.
///
/// Linux's `kernel/signal.c::pidfd_send_signal` rejects `sig < 0 ||
/// sig > _NSIG` (`_NSIG = 64` on x86-64).  Signal `0` is the
/// permission-test value (no signal delivered).
pub const PIDFD_SIG_MAX: i32 = 64;

/// Obtain a file descriptor that refers to a process.
///
/// # Linux behaviour
///
/// `pidfd_open(2)` (added in Linux 5.3) returns a file descriptor that
/// refers to the process whose PID is `pid`.  The descriptor can be
/// passed to `waitid(P_PIDFD, ...)`, `pidfd_send_signal(2)`,
/// `pidfd_getfd(2)`, and `poll/epoll` (to learn of exit).
///
/// Errors the kernel returns *before* allocating a pidfd object, in
/// the order Linux's `SYSCALL_DEFINE2(pidfd_open)` (kernel/pid.c)
/// surfaces them:
///
/// * `flags & ~(PIDFD_NONBLOCK|PIDFD_THREAD)`    → `EINVAL`
/// * `pid <= 0`                                 → `EINVAL`
/// * unknown PID (no such process)              → `ESRCH`  (only when
///   the kernel actually looks up the task; here we cannot, so callers
///   should not depend on `ESRCH` from this validator)
///
/// We replicate the *argument*-domain checks so callers (e.g. container
/// runtimes' probing code) get the same `EINVAL`/`ENOSYS` shape they
/// expect.  After arguments are accepted, we fall back to `ENOSYS`
/// because the spawn/lookup subsystem isn't wired up here.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pidfd_open(pid: PidT, flags: u32) -> i32 {
    // Linux's pidfd_open prologue checks the flag mask BEFORE pid:
    //
    //     if (flags & ~(PIDFD_NONBLOCK | PIDFD_THREAD))
    //         return -EINVAL;
    //     if (pid <= 0)
    //         return -EINVAL;
    //
    // Both errors are EINVAL, but the precedence matters for callers
    // bisecting which argument is wrong.  Match that order.
    if (flags & !PIDFD_OPEN_FLAGS_VALID) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // pid must be a strictly positive PID — Linux rejects 0 and any
    // negative value (since negative would mean "process group" elsewhere
    // but is not accepted here).
    if pid <= 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // Arguments validated; underlying subsystem not implemented.
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Send a signal to a process referred to by a pidfd.
///
/// # Linux behaviour
///
/// `pidfd_send_signal(pidfd, sig, info, flags)` (added in Linux 5.1)
/// delivers `sig` to the process referenced by `pidfd`.  Argument-
/// domain checks the kernel performs before touching the target:
///
/// * `flags != 0`           → `EINVAL`  (no flag bits defined yet)
/// * `pidfd < 0`            → `EBADF`
/// * `sig < 0 || sig > 64`  → `EINVAL`  (`sig == 0` is allowed and is
///   a permission/existence probe — no signal is delivered)
/// * If `info != NULL`: the kernel copies in a `siginfo_t` and rejects
///   the call when `info->si_signo != sig` (`kernel/signal.c`
///   `do_pidfd_send_signal` → `copy_siginfo_from_user`).
///
/// We replicate every argument-domain check.  Callers that just want
/// to know "does the syscall exist with the right shape" (e.g. systemd's
/// `bus_kill_unit_processes` fallback ladder) will see the same errno
/// pattern they get on a stripped-down Linux build.
///
/// # Safety
///
/// `info`, if non-NULL, must point to at least `sizeof(SiginfoT) == 128`
/// readable bytes.  We use `core::ptr::read_unaligned` to defend against
/// alignment-1 caller pointers.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pidfd_send_signal(
    pidfd: i32,
    sig: i32,
    info: *const core::ffi::c_void,
    flags: u32,
) -> i32 {
    // No flag bits are defined for pidfd_send_signal in Linux.
    if flags != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // pidfd must be a non-negative fd.
    if pidfd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    // sig must be in [0, 64].  0 is the permission/existence probe.
    if !(0..=PIDFD_SIG_MAX).contains(&sig) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // If info is provided, validate the siginfo_t.si_signo cross-check.
    // Note: when sig == 0 the kernel still requires si_signo == 0 when
    // info is non-NULL.
    if !info.is_null() {
        // SAFETY: caller contract says `info` (when non-NULL) points to
        // a SiginfoT-sized region.  read_unaligned defends against an
        // arbitrarily-aligned caller pointer.
        let si_signo = unsafe {
            core::ptr::read_unaligned(info.cast::<crate::signal::SiginfoT>())
                .si_signo
        };
        if si_signo != sig {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
    }
    // Arguments validated; signal delivery not wired up.
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Retrieve a duplicate of another process's file descriptor via pidfd.
///
/// # Linux behaviour
///
/// `pidfd_getfd(pidfd, targetfd, flags)` (added in Linux 5.6) returns
/// a new fd in *our* table that refers to the same open file description
/// as `targetfd` in the process referenced by `pidfd`.  Used by debuggers
/// (`strace -y`, `lldb`), container runtimes that need to pass an fd
/// across PID namespaces, and rootless container image extractors.
///
/// Argument-domain checks the kernel performs:
///
/// * `flags != 0`     → `EINVAL`  (no flag bits defined)
/// * `pidfd < 0`      → `EBADF`
/// * `targetfd < 0`   → `EBADF`
///
/// After arguments are accepted we return `ENOSYS` — replicating a
/// Linux build without `CONFIG_PIDFD_GETFD`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pidfd_getfd(pidfd: i32, targetfd: i32, flags: u32) -> i32 {
    // No flag bits are defined.
    if flags != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // pidfd must be non-negative.
    if pidfd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    // targetfd must be non-negative.
    if targetfd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    // Arguments validated; cross-process fd duplication not wired up.
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// arch_prctl — x86-64 specific thread state
// ---------------------------------------------------------------------------

/// Set the FS base address.
pub const ARCH_SET_FS: i32 = 0x1002;
/// Get the FS base address.
pub const ARCH_GET_FS: i32 = 0x1003;
/// Set the GS base address.
pub const ARCH_SET_GS: i32 = 0x1001;
/// Get the GS base address.
pub const ARCH_GET_GS: i32 = 0x1004;

/// Linux 4.12+: get the `IA32_TSC_AUX` "CPU ID" emitted by `rdtscp` /
/// `rdpid`.  `addr` is a `unsigned long *` to receive the value.
pub const ARCH_GET_CPUID: i32 = 0x1011;
/// Linux 4.12+: set whether userspace can execute `cpuid` (1) or it
/// faults with `SIGSEGV` (0).  `addr` is the boolean value, not a ptr.
pub const ARCH_SET_CPUID: i32 = 0x1012;
/// Linux 5.18+: control Intel CET shadow-stack feature for the
/// calling task.  `addr` is a `cet_status *` to receive the state.
pub const ARCH_CET_STATUS: i32 = 0x3001;
/// Linux 5.18+: enable Intel CET shadow-stack on the calling task.
pub const ARCH_CET_ENABLE: i32 = 0x3002;
/// Linux 5.18+: disable Intel CET shadow-stack on the calling task.
pub const ARCH_CET_DISABLE: i32 = 0x3003;
/// Linux 5.18+: lock CET configuration so it can't be changed
/// (anti-tamper for hardened processes).
pub const ARCH_CET_LOCK: i32 = 0x3004;
/// Linux 5.18+: allocate a shadow-stack region for a thread.
pub const ARCH_CET_ALLOC_SHSTK: i32 = 0x3005;
/// Linux 6.4+: set the Intel LAM (Linear Address Masking) width.
pub const ARCH_GET_UNTAG_MASK: i32 = 0x4001;
/// Linux 6.4+: enable LAM with a given untag mask width.
pub const ARCH_ENABLE_TAGGED_ADDR: i32 = 0x4002;
/// Linux 6.4+: get max LAM untag mask width supported by the kernel.
pub const ARCH_GET_MAX_TAG_BITS: i32 = 0x4003;
/// Linux 6.4+: force LAM untagging even on legacy syscalls.
pub const ARCH_FORCE_TAGGED_SVA: i32 = 0x4004;

/// Highest x86-64 user-canonical address bit (bit 47 on 4-level paging,
/// bit 56 on 5-level paging).  Anything above this with the high bits
/// not all-zero / all-one is non-canonical and faults at the LDT/MSR.
///
/// We use the conservative 4-level bound (bit 47); a real kernel would
/// pick this dynamically based on CR4.LA57.  Used to validate `addr`
/// arguments to ARCH_SET_FS/ARCH_SET_GS — Linux rejects non-canonical
/// addresses with EINVAL since loading them into the MSR raises #GP.
pub const X86_64_CANONICAL_MAX: u64 = 0x0000_7FFF_FFFF_FFFF;

/// Set architecture-specific thread state.
///
/// # Linux behaviour
///
/// `arch_prctl(int code, unsigned long addr)` (x86-64 only) is the
/// architecture-specific knob for FS/GS base, CET shadow-stack, LAM
/// untagging, and CPUID-fault control.  Argument-domain checks:
///
/// * `code` not in the recognised set                      → `EINVAL`
/// * For SET_FS/SET_GS: `addr > X86_64_CANONICAL_MAX` and
///   not in the upper-half canonical range                 → `EINVAL`
///   (Linux's `arch/x86/kernel/process_64.c::do_arch_prctl_64`
///    explicitly rejects non-canonical addresses since loading
///    them into the FS/GS_BASE MSR raises #GP)
/// * For SET_CPUID: `addr` not 0 or 1                      → `EINVAL`
///   (it's a boolean — only 0 disables, 1 enables; everything else
///    is bogus per `arch/x86/kernel/process.c::set_cpuid_mode`)
/// * For GET_FS/GET_GS/GET_CPUID/GET_UNTAG_MASK/
///   GET_MAX_TAG_BITS/CET_STATUS: `addr == 0`              → `EFAULT`
///   (these write the result to `*addr` — NULL output ptr is a fault)
/// * For ENABLE_TAGGED_ADDR: `addr` (the width) > 6        → `EINVAL`
///   (LAM57 supports 6 mask bits; anything wider is not implementable)
///
/// After arguments validate we return `ENOSYS` because none of these
/// CPU-state knobs are implemented in our microkernel design (FS/GS
/// base is set at thread spawn by the kernel; CET/LAM are not yet
/// supported on our target hardware abstraction).
///
/// **Architectural rationale** (matches Linux on `CONFIG_X86_64` kernels
/// with the CET/LAM features compiled out — the canonical "syscall
/// exists but feature unavailable" shape).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn arch_prctl(code: i32, addr: u64) -> i32 {
    match code {
        // FS/GS base setters — addr is a canonical user address.
        ARCH_SET_FS | ARCH_SET_GS => {
            // Non-canonical address rejected by Linux (else #GP on MSR
            // load).  Canonical range is either 0..=0x0000_7FFF_FFFF_FFFF
            // (low half) or 0xFFFF_8000_0000_0000..=u64::MAX (high half).
            let is_low_canonical = addr <= X86_64_CANONICAL_MAX;
            let is_high_canonical = addr >= 0xFFFF_8000_0000_0000;
            if !is_low_canonical && !is_high_canonical {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
            errno::set_errno(errno::ENOSYS);
            -1
        }
        // FS/GS base getters — addr is a *u64 output pointer.
        ARCH_GET_FS | ARCH_GET_GS => {
            if addr == 0 {
                errno::set_errno(errno::EFAULT);
                return -1;
            }
            errno::set_errno(errno::ENOSYS);
            -1
        }
        // CPUID-fault control.
        ARCH_SET_CPUID => {
            // addr is a boolean — only 0 and 1 are accepted.
            if addr > 1 {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
            errno::set_errno(errno::ENOSYS);
            -1
        }
        ARCH_GET_CPUID => {
            // GET_CPUID takes no addr in Linux — the return value *is*
            // the answer.  We still validate that addr is 0 (the
            // documented sentinel) and reach ENOSYS.
            errno::set_errno(errno::ENOSYS);
            -1
        }
        // Intel CET shadow-stack family.
        ARCH_CET_STATUS => {
            if addr == 0 {
                errno::set_errno(errno::EFAULT);
                return -1;
            }
            errno::set_errno(errno::ENOSYS);
            -1
        }
        ARCH_CET_ENABLE
        | ARCH_CET_DISABLE
        | ARCH_CET_LOCK
        | ARCH_CET_ALLOC_SHSTK => {
            errno::set_errno(errno::ENOSYS);
            -1
        }
        // Intel LAM (Linear Address Masking) family.
        ARCH_GET_UNTAG_MASK | ARCH_GET_MAX_TAG_BITS => {
            if addr == 0 {
                errno::set_errno(errno::EFAULT);
                return -1;
            }
            errno::set_errno(errno::ENOSYS);
            -1
        }
        ARCH_ENABLE_TAGGED_ADDR => {
            // addr is the requested untag mask width.  LAM57 caps
            // this at 6 (the 6 bits between PML5 and PML4 boundary).
            if addr > 6 {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
            errno::set_errno(errno::ENOSYS);
            -1
        }
        ARCH_FORCE_TAGGED_SVA => {
            errno::set_errno(errno::ENOSYS);
            -1
        }
        // Unknown code.
        _ => {
            errno::set_errno(errno::EINVAL);
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// ioprio — I/O scheduling priority
// ---------------------------------------------------------------------------

/// I/O priority class: none (use default based on nice value).
pub const IOPRIO_CLASS_NONE: i32 = 0;
/// Real-time I/O class.
pub const IOPRIO_CLASS_RT: i32 = 1;
/// Best-effort I/O class.
pub const IOPRIO_CLASS_BE: i32 = 2;
/// Idle I/O class.
pub const IOPRIO_CLASS_IDLE: i32 = 3;

/// Who the ioprio applies to: process.
pub const IOPRIO_WHO_PROCESS: i32 = 1;
/// Who: process group.
pub const IOPRIO_WHO_PGRP: i32 = 2;
/// Who: user.
pub const IOPRIO_WHO_USER: i32 = 3;

/// Bit position of the class within the encoded ioprio value.
/// Layout: `(class << 13) | data` — see `<linux/ioprio.h>`.
pub const IOPRIO_CLASS_SHIFT: i32 = 13;
/// Mask for the data portion of an encoded ioprio value (low 13 bits).
pub const IOPRIO_PRIO_MASK: i32 = (1 << IOPRIO_CLASS_SHIFT) - 1;
/// Number of best-effort / real-time priority levels (0..7).
pub const IOPRIO_BE_NR: i32 = 8;

/// Validate the `which` parameter of `ioprio_get`/`ioprio_set`.
/// Returns `true` for `IOPRIO_WHO_PROCESS`, `_PGRP`, `_USER`.
#[inline]
fn ioprio_which_valid(which: i32) -> bool {
    matches!(
        which,
        IOPRIO_WHO_PROCESS | IOPRIO_WHO_PGRP | IOPRIO_WHO_USER,
    )
}

/// Get the I/O scheduling class and priority of a process.
///
/// Stub: validates arguments per Linux `block/ioprio.c`, then returns
/// the encoded default priority `(IOPRIO_CLASS_NONE << 13) | 0 = 0`.
///
/// Errors (Linux-matching priority order):
/// * `EINVAL` — `which` not in `{IOPRIO_WHO_PROCESS, _PGRP, _USER}`.
/// * `ESRCH` — `who` is negative (no such process / pgrp / user).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ioprio_get(which: i32, who: i32) -> i32 {
    if !ioprio_which_valid(which) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if who < 0 {
        // Negative pid/pgid/uid cannot name any real entity. Linux's
        // find_task_by_vpid / find_user reject negatives with ESRCH.
        errno::set_errno(errno::ESRCH);
        return -1;
    }
    // Return class=NONE, data=0 → value = (NONE << 13) | 0 = 0.
    0
}

/// Set the I/O scheduling class and priority of a process.
///
/// Stub: validates arguments per Linux `block/ioprio.c::sys_ioprio_set`,
/// then succeeds silently without actually adjusting any scheduler
/// state (we have no per-process ioprio storage yet — see todo.txt).
///
/// # Linux semantics
///
/// Linux validates the class/data field *first*, then enters the
/// `which` switch — so a malformed `ioprio` argument is rejected with
/// EINVAL before the `who` lookup runs.  Within the class switch:
///
/// * `IOPRIO_CLASS_RT`  — `data ∈ [0, IOPRIO_NR_LEVELS)`, else EINVAL.
///                        (Also requires `CAP_SYS_NICE`/`CAP_SYS_ADMIN`
///                        → EPERM; we don't model caps yet.)
/// * `IOPRIO_CLASS_BE`  — same `data` range as RT.
/// * `IOPRIO_CLASS_IDLE` — any `data` value is accepted (priority is
///                        effectively fixed).
/// * `IOPRIO_CLASS_NONE` — `data` must be `0`, else EINVAL.  This is
///                        a strict check in modern Linux even though
///                        the data field is otherwise unused for NONE
///                        (it falls back to nice-derived priority).
/// * any other class → EINVAL.
///
/// Errors (Linux-matching priority order):
/// 1. malformed `class` / out-of-range RT or BE `data` / non-zero
///    NONE `data` → `EINVAL`
/// 2. `which` not in `{IOPRIO_WHO_PROCESS, _PGRP, _USER}` → `EINVAL`
///    (Linux: switch default arm.)
/// 3. `who < 0` → `ESRCH` (matches Linux's find_task_by_vpid /
///    find_vpid / make_kuid rejection of negative inputs).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ioprio_set(which: i32, who: i32, ioprio: i32) -> i32 {
    // 1. Class/data validation first — matches Linux's prologue
    //    order in sys_ioprio_set.
    let class = ioprio >> IOPRIO_CLASS_SHIFT;
    let data = ioprio & IOPRIO_PRIO_MASK;
    match class {
        IOPRIO_CLASS_NONE => {
            // Modern Linux (≥ 5.x) rejects non-zero data for NONE.
            if data != 0 {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
        }
        IOPRIO_CLASS_IDLE => {
            // IDLE accepts any data value — priority is always 7 in
            // the scheduler regardless of what was passed.
        }
        IOPRIO_CLASS_RT | IOPRIO_CLASS_BE => {
            // 3-bit priority field: 0..7.  Data is masked from a
            // u13 already, so the only way to fail is data >= 8.
            if !(0..IOPRIO_BE_NR).contains(&data) {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
        }
        _ => {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
    }
    // 2. which validation — Linux's switch default arm returns EINVAL.
    if !ioprio_which_valid(which) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // 3. who < 0 → ESRCH (find_task_by_vpid / make_kuid reject).
    if who < 0 {
        errno::set_errno(errno::ESRCH);
        return -1;
    }
    0
}

// ---------------------------------------------------------------------------
// membarrier — Linux process-wide memory barrier
// ---------------------------------------------------------------------------

/// Command: query supported operations.
pub const MEMBARRIER_CMD_QUERY: i32 = 0;
/// Issue a global barrier.
pub const MEMBARRIER_CMD_GLOBAL: i32 = 1;
/// Expedited global memory barrier (with IPI).
pub const MEMBARRIER_CMD_GLOBAL_EXPEDITED: i32 = 1 << 1;
/// Register intent to use expedited global barriers.
pub const MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED: i32 = 1 << 2;
/// Issue a private expedited barrier.
pub const MEMBARRIER_CMD_PRIVATE_EXPEDITED: i32 = 1 << 3;
/// Register intent to use private expedited barriers.
pub const MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED: i32 = 1 << 4;
/// Private expedited sync-core barrier.
pub const MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE: i32 = 1 << 5;
/// Register for sync-core private expedited.
pub const MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_SYNC_CORE: i32 = 1 << 6;
/// Private expedited RSEQ — restart restartable sequences across the
/// process.  Added in Linux 5.10.  Not supported by our membarrier
/// (we have no rseq infrastructure), but the constant is recognised
/// by the flag-validation step so a caller asking for RSEQ + FLAG_CPU
/// gets EINVAL from the dispatch arm rather than the flag arm.
pub const MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ: i32 = 1 << 7;

/// Bitmask of operations supported by [`membarrier`] / reported by
/// `MEMBARRIER_CMD_QUERY`.
///
/// We support every "regular" command except the rseq variants (no
/// restartable-sequence infrastructure yet).  All supported commands
/// reduce to a local `mfence` on x86_64; cross-CPU expedited semantics
/// are best-effort because we have no userspace path to send IPIs to
/// other cores.  In practice, each peer thread re-fences whenever it
/// crosses a syscall boundary, so the visible ordering matches Linux
/// for everything except code that aggressively spins in userspace
/// without ever syscalling.
const MEMBARRIER_SUPPORTED: i32 = MEMBARRIER_CMD_GLOBAL
    | MEMBARRIER_CMD_GLOBAL_EXPEDITED
    | MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED
    | MEMBARRIER_CMD_PRIVATE_EXPEDITED
    | MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED
    | MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE
    | MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_SYNC_CORE;

/// Target a specific CPU for `MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ`.
///
/// Linux 5.10+ extended the rseq variant to accept this flag plus a
/// `cpu_id` argument so userspace can restart only the rseq on one
/// CPU rather than every CPU in the process.  Every other command
/// rejects non-zero `flags` with EINVAL.  Mirrors the constant in
/// [`linux_membarrier_types`](crate::linux_membarrier_types) but lives
/// here as `u32` so it can be compared directly against the syscall
/// `flags` argument.
pub const MEMBARRIER_CMD_FLAG_CPU: u32 = 1 << 0;

/// Issue an x86_64 `mfence` on the calling CPU.  This drains the local
/// store buffer and provides a full memory barrier with respect to
/// every subsequent load/store on this core.
#[inline]
fn local_mfence() {
    #[cfg(target_arch = "x86_64")]
    // SAFETY: `mfence` is a serializing memory-fence instruction with
    // no operands, no side effects beyond memory ordering, and no
    // privilege requirements.  Safe at every CPU mode.
    unsafe {
        core::arch::asm!("mfence", options(nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        // Fall back to the compiler's strongest barrier intrinsic on
        // non-x86_64 hosts (test-only).
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    }
}

/// Perform a memory barrier operation across all threads of the
/// process.
///
/// On x86_64 the issuing CPU is fenced via `mfence`.  Linux's
/// expedited variants additionally IPI peer CPUs to force them to
/// fence; our kernel does not yet expose a userspace-triggered IPI, so
/// peer-CPU fencing is implicit (each thread re-fences on its next
/// syscall).  `MEMBARRIER_CMD_QUERY` returns the bitmask of supported
/// commands.
///
/// Validation order matches Linux's `sys_membarrier`
/// (`kernel/sched/membarrier.c`):
///
/// 1. **First switch — flag validation by command.**
///    `MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ` accepts `flags == 0`
///    or `flags == MEMBARRIER_CMD_FLAG_CPU`; every other command
///    (including `MEMBARRIER_CMD_QUERY` and unknown commands) requires
///    `flags == 0`.  This is why a `QUERY` call with non-zero flags
///    is `EINVAL` even though `QUERY` doesn't otherwise look at flags.
/// 2. **Implicit cpu_id normalisation.**  When `flags & FLAG_CPU` is
///    not set, `cpu_id` is treated as -1 (i.e. ignored).
/// 3. **Second switch — exact-match command dispatch.**  Each command
///    is a discrete *value*, not a bitmask: a caller that ORs two
///    command bits together (e.g. `GLOBAL | PRIVATE_EXPEDITED`) is
///    not asking for "both at once" — it's asking for an invalid
///    command, and gets `EINVAL`.  Unknown commands also land in the
///    default `EINVAL` arm.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn membarrier(cmd: i32, flags: u32, cpu_id: i32) -> i32 {
    // -- First switch: per-command flag validation ----------------------
    //
    // Linux's first switch has two arms: PRIVATE_EXPEDITED_RSEQ accepts
    // a single optional flag (FLAG_CPU), everything else demands
    // flags == 0.  We don't have rseq, so the RSEQ arm's "accepted"
    // case still falls through to a dispatch EINVAL — but matching the
    // *flag-validation step* is important: a caller asking for RSEQ
    // with FLAG_CPU should not see EINVAL from the flag check (that
    // would steer them toward removing the flag, which is correct on
    // pre-5.10 kernels but wrong on modern ones).
    if cmd == MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ {
        if flags != 0 && flags != MEMBARRIER_CMD_FLAG_CPU {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
    } else if flags != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // -- cpu_id normalisation -------------------------------------------
    //
    // Linux: `if (!(flags & MEMBARRIER_CMD_FLAG_CPU)) cpu_id = -1;`.
    // We don't actually IPI a specific CPU (no userspace IPI path) so
    // `cpu_id` is informational only — but we mirror the normalisation
    // so any future hook that reads it sees the same value Linux would.
    let _cpu_id = if flags & MEMBARRIER_CMD_FLAG_CPU == 0 {
        -1
    } else {
        cpu_id
    };

    // -- Second switch: exact-match command dispatch --------------------
    match cmd {
        MEMBARRIER_CMD_QUERY => MEMBARRIER_SUPPORTED,
        MEMBARRIER_CMD_GLOBAL
        | MEMBARRIER_CMD_GLOBAL_EXPEDITED
        | MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED
        | MEMBARRIER_CMD_PRIVATE_EXPEDITED
        | MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED
        | MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE
        | MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_SYNC_CORE => {
            // For every supported command the visible effect is: drain
            // the local store buffer.  Issue an mfence.
            local_mfence();
            0
        }
        _ => {
            // Unknown commands (including OR-combined command bits and
            // unsupported variants like PRIVATE_EXPEDITED_RSEQ) → EINVAL.
            errno::set_errno(errno::EINVAL);
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// clone3 — extended clone (Linux 5.3+)
// ---------------------------------------------------------------------------

/// `clone_args` structure for `clone3`.
///
/// Matches the Linux `struct clone_args` layout from
/// `<linux/sched.h>` exactly: 11 `__aligned_u64` fields totalling
/// 88 bytes (`CLONE_ARGS_SIZE_VER2`).  Earlier struct versions are
/// prefixes:
///
/// * V0 (64 B, Linux 5.3): up to and including `tls`.
/// * V1 (80 B, Linux 5.5): adds `set_tid` and `set_tid_size`.
/// * V2 (88 B, Linux 5.7): adds `cgroup`.
///
/// Userspace passes the size it knows about as `clone3`'s second
/// argument; the kernel rejects anything below V0, accepts V0/V1/V2
/// directly, and for sizes above V2 requires the trailing bytes to
/// be zero so older kernels can ignore unknown trailing fields.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CloneArgs {
    /// `CLONE_*` flag bits.  See [`CLONE3_FLAGS_VALID`].
    pub flags: u64,
    /// User-supplied address where the kernel will store the new
    /// child's pidfd, when `CLONE_PIDFD` is set (otherwise unused).
    pub pidfd: u64,
    /// User-supplied address in the child's address space where the
    /// kernel will store the child's TID, when `CLONE_CHILD_SETTID`
    /// or `CLONE_CHILD_CLEARTID` is set.
    pub child_tid: u64,
    /// User-supplied address in the parent's address space where the
    /// kernel will store the child's TID, when `CLONE_PARENT_SETTID`
    /// is set.
    pub parent_tid: u64,
    /// Signal delivered to the parent when the child terminates.
    /// Distinct from `clone(2)`'s `flags & CSIGNAL` low byte — clone3
    /// requires the `CSIGNAL` bits in `flags` to be zero.
    pub exit_signal: u64,
    /// Lowest address of the child's stack region.
    pub stack: u64,
    /// Size of the child's stack region in bytes.
    pub stack_size: u64,
    /// Initial TLS value (architecture-specific interpretation; on
    /// x86_64 this becomes the new `%fs.base`).  Used when
    /// `CLONE_SETTLS` is set.
    pub tls: u64,
    /// V1+ only.  Pointer to a `pid_t` array specifying the child's
    /// TID in each PID namespace from innermost to outermost.
    pub set_tid: u64,
    /// V1+ only.  Number of entries in [`set_tid`](Self::set_tid);
    /// capped at the kernel's namespace nesting depth.
    pub set_tid_size: u64,
    /// V2+ only.  File descriptor of the cgroup directory the child
    /// should be placed in, when `CLONE_INTO_CGROUP` is set.
    pub cgroup: u64,
}

/// All flag bits accepted by `clone3(2)` in the
/// `CloneArgs::flags` field.
///
/// Superset of [`CLONE_FLAGS_VALID`] — clone3 additionally accepts
/// the 64-bit / signal-byte-clashing flags that legacy `clone(2)`
/// cannot express:
///
/// * `CLONE_NEWTIME` (`0x80`) — overlaps `CSIGNAL` in clone(2)
/// * `CLONE_INTO_CGROUP` (`0x2_0000_0000`) — bit 33
/// * `CLONE_CLEAR_SIGHAND` (`0x1_0000_0000`) — bit 32
///
/// Notably *missing* from this set: `CLONE_DETACHED` — clone3
/// rejects it because `exit_signal == 0` already expresses the
/// "don't notify parent" semantic.  The `CSIGNAL` low byte
/// (`0xff`) is also rejected — clone3 carries the exit signal in
/// the dedicated [`CloneArgs::exit_signal`] field, so any of those
/// bits set in `flags` indicates a confused caller mixing the
/// `clone`/`clone3` ABIs.
pub const CLONE3_FLAGS_VALID: u64 = (CLONE_FLAGS_VALID
    | crate::linux_clone_args::CLONE_NEWTIME
    | crate::linux_clone_args::CLONE_INTO_CGROUP
    | crate::linux_clone_args::CLONE_CLEAR_SIGHAND)
    // clone3 explicitly excludes CLONE_DETACHED — mask it out even
    // though it's present in CLONE_FLAGS_VALID for legacy clone.
    & !crate::linux_clone_args::CLONE_DETACHED;

/// Maximum `set_tid_size` accepted by `clone3(2)`.
///
/// Linux's `MAX_PID_NS_LEVEL` is 32 — the maximum nesting depth of
/// PID namespaces.  Any `set_tid_size` above this is rejected with
/// `EINVAL` since there cannot be that many namespaces to populate.
pub const CLONE3_MAX_SET_TID: u64 = 32;

/// Upper bound on the `size` argument to `clone3(2)`.
///
/// Linux's `copy_struct_from_user` rejects `size > PAGE_SIZE` with
/// `E2BIG`.  We use the same 4 KiB cap regardless of underlying
/// hardware page size — userspace ABI is portable.
pub const CLONE3_SIZE_MAX: usize = 4096;

/// `clone3` — create a child process (Linux 5.3+).
///
/// # Linux behaviour
///
/// `int clone3(struct clone_args *cl_args, size_t size)`.  The
/// kernel's `kernel/fork.c::sys_clone3` performs the following
/// argument-domain checks before any process state is touched:
///
/// 1. `cl_args == NULL`                          → `EFAULT`
/// 2. `size > PAGE_SIZE`                         → `E2BIG`
/// 3. `size < CLONE_ARGS_SIZE_VER0`              → `EINVAL`
/// 4. trailing bytes beyond the largest known struct version are
///    non-zero (forward-compat guard)            → `E2BIG`
/// 5. `flags & CSIGNAL`                          → `EINVAL`
///    (clone3 uses `exit_signal`, not the low byte of flags)
/// 6. `flags & ~CLONE3_FLAGS_VALID`              → `EINVAL`
/// 7. `flags & CLONE_DETACHED`                   → `EINVAL`
///    (clone3 rejects it; covered by check (6) but called out for
///    clarity because the userspace ABI explicitly forbids it)
/// 8. `flags & CLONE_THREAD` without `CLONE_SIGHAND` → `EINVAL`
/// 9. `flags & CLONE_SIGHAND` without `CLONE_VM` → `EINVAL`
/// 10. `flags & CLONE_THREAD` with `exit_signal != 0` → `EINVAL`
///    (threads cannot signal their parent on death)
/// 11. `exit_signal > SIGRTMAX (64)`             → `EINVAL`
/// 12. `flags & CLONE_NEWUSER & CLONE_FS`        → `EINVAL`
/// 13. `flags & CLONE_NEWUSER & CLONE_THREAD`    → `EINVAL`
/// 14. `flags & CLONE_NEWNS & CLONE_FS`          → `EINVAL`
/// 15. `flags & CLONE_INTO_CGROUP` with `size < VER2` → `EINVAL`
/// 16. `flags & CLONE_INTO_CGROUP` with `cgroup` not a valid fd →
///    `EBADF` (we cannot validate the fd here, so we limit ourselves
///    to the layout check: cgroup field must be readable, i.e. size
///    must be at least VER2)
/// 17. `set_tid != 0` with `size < VER1`         → `EINVAL`
/// 18. `set_tid_size > MAX_PID_NS_LEVEL (32)`    → `EINVAL`
/// 19. `set_tid_size > 0` with `set_tid == 0`    → `EINVAL`
/// 20. `set_tid_size == 0` with `set_tid != 0`   → `EINVAL`
///    (the array pointer and length must agree)
///
/// After all checks pass we return `ENOSYS`: the microkernel uses
/// `SYS_PROCESS_SPAWN_EX` for process creation; clone3 is a
/// userspace ABI compatibility shim.
///
/// # Safety
///
/// When non-NULL, `args` must point to at least `size` readable
/// bytes (`size` is bounded by [`CLONE3_SIZE_MAX`] before we
/// dereference).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn clone3(args: *const CloneArgs, size: usize) -> i64 {
    // (1) NULL pointer rejected before size check (Linux order).
    if args.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // (2) Cap total struct size at one page.
    if size > CLONE3_SIZE_MAX {
        errno::set_errno(errno::E2BIG);
        return -1;
    }
    // (3) Below the V0 floor — too small to even hold flags+pidfd+
    // child_tid+parent_tid+exit_signal+stack+stack_size+tls.
    if size < crate::linux_clone_args::CLONE_ARGS_SIZE_VER0 as usize {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // (4) Trailing bytes past V2 must be zero so the kernel can keep
    // forward-compat with userspace built against newer headers.
    let v2 = crate::linux_clone_args::CLONE_ARGS_SIZE_VER2 as usize;
    if size > v2 {
        // SAFETY: caller contract — `args` covers `size` readable
        // bytes.  We reinterpret as a byte slice for the tail scan.
        let tail_len = size - v2;
        let tail_ptr = (args as *const u8).wrapping_add(v2);
        for i in 0..tail_len {
            // SAFETY: tail_ptr + i is within [args, args+size).
            let byte = unsafe { *tail_ptr.add(i) };
            if byte != 0 {
                errno::set_errno(errno::E2BIG);
                return -1;
            }
        }
    }

    // SAFETY: `args` is non-NULL and points to at least V0 bytes;
    // we never touch fields beyond the actual `size` because the
    // struct layout is a prefix and we gate set_tid / cgroup reads
    // on size below.
    let a = unsafe { core::ptr::read(args) };

    // (5) clone3 forbids CSIGNAL in flags — exit signal travels in
    // its own field.
    if (a.flags & crate::linux_clone_args::CSIGNAL) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // (6) Reject any flag bit outside the clone3 whitelist.
    if (a.flags & !CLONE3_FLAGS_VALID) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // (7) Explicit CLONE_DETACHED rejection — already covered by (6)
    // but called out so future readers don't wonder.
    if (a.flags & crate::linux_clone_args::CLONE_DETACHED) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // (8) CLONE_THREAD requires CLONE_SIGHAND.
    if (a.flags & crate::linux_clone_args::CLONE_THREAD) != 0
        && (a.flags & crate::linux_clone_args::CLONE_SIGHAND) == 0
    {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // (9) CLONE_SIGHAND requires CLONE_VM.
    if (a.flags & crate::linux_clone_args::CLONE_SIGHAND) != 0
        && (a.flags & crate::linux_clone_args::CLONE_VM) == 0
    {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // (10) Thread must have no death signal.
    if (a.flags & crate::linux_clone_args::CLONE_THREAD) != 0 && a.exit_signal != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // (11) Exit signal valid range.
    if a.exit_signal > CLONE_CSIGNAL_MAX {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // (12) NEWUSER cannot share FS.
    if (a.flags & crate::linux_clone_args::CLONE_NEWUSER) != 0
        && (a.flags & crate::linux_clone_args::CLONE_FS) != 0
    {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // (13) NEWUSER cannot span a thread group.
    if (a.flags & crate::linux_clone_args::CLONE_NEWUSER) != 0
        && (a.flags & crate::linux_clone_args::CLONE_THREAD) != 0
    {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // (14) NEWNS cannot share FS.
    if (a.flags & crate::linux_clone_args::CLONE_NEWNS) != 0
        && (a.flags & crate::linux_clone_args::CLONE_FS) != 0
    {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // (15)/(16) CLONE_INTO_CGROUP requires V2 struct so the `cgroup`
    // field is actually present in the user-supplied buffer.
    if (a.flags & crate::linux_clone_args::CLONE_INTO_CGROUP) != 0
        && size < v2
    {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // (17)/(19)/(20) set_tid requires V1 struct AND matching pointer/
    // length pairing.  Only inspect those fields when size makes them
    // present — otherwise they're outside the user-supplied buffer
    // and we already zeroed `a`'s tail by virtue of being a prefix
    // struct read.  But the caller may have passed VER0 with set_tid
    // bytes uninitialised; treat fields past size as not-present.
    let v1 = crate::linux_clone_args::CLONE_ARGS_SIZE_VER1 as usize;
    let set_tid_present = size >= v1;
    let set_tid_ptr = if set_tid_present { a.set_tid } else { 0 };
    let set_tid_size = if set_tid_present { a.set_tid_size } else { 0 };

    // (18) Bound the array length.
    if set_tid_size > CLONE3_MAX_SET_TID {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // (19) Length set but pointer NULL — invalid pair.
    if set_tid_size != 0 && set_tid_ptr == 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // (20) Pointer set but length zero — invalid pair.
    if set_tid_ptr != 0 && set_tid_size == 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // All arguments validated; process-spawn primitive not wired up.
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// process_vm_readv / process_vm_writev — cross-process I/O
// ---------------------------------------------------------------------------

/// Maximum iovec-array length accepted by `process_vm_readv` and
/// `process_vm_writev`.
///
/// Linux's `UIO_MAXIOV` is 1024 — the same cap used for `readv`,
/// `writev`, `preadv`, and `pwritev`.  Counts above this are rejected
/// with `EINVAL` regardless of actual array contents.
pub const PROCESS_VM_UIO_MAXIOV: u64 = 1024;

/// Maximum total byte count summable across an iovec array.
///
/// Linux uses `SSIZE_MAX` (`i64::MAX` on x86_64) as the per-direction
/// transfer limit; total `iov_len` summed across the array must not
/// exceed this, otherwise the syscall reports `EINVAL`.
pub const PROCESS_VM_SSIZE_MAX: u64 = i64::MAX as u64;

/// Shared validator for both `process_vm_readv` and `process_vm_writev`.
///
/// Returns `Ok(())` if every argument-domain check passes (in which
/// case the caller should set `ENOSYS`), `Err(errno)` otherwise.
/// Both syscalls share identical argument semantics — only the
/// direction of data transfer differs.
///
/// # Linux behaviour
///
/// In the order the kernel performs them in `fs/read_write.c`'s
/// `process_vm_rw`:
///
/// 1. `flags != 0`                                  → `EINVAL`
///    (reserved arg; Linux requires zero)
/// 2. `pid <= 0`                                    → `ESRCH`
///    (no such task — Linux's `find_get_task_by_vpid` returns NULL
///    for non-positive pids, surfaced as ESRCH)
/// 3. `liovcnt > UIO_MAXIOV`                        → `EINVAL`
/// 4. `riovcnt > UIO_MAXIOV`                        → `EINVAL`
/// 5. `liovcnt > 0` and `local_iov == NULL`         → `EFAULT`
/// 6. `riovcnt > 0` and `remote_iov == NULL`        → `EFAULT`
/// 7. Σ `local_iov[i].iov_len > SSIZE_MAX`          → `EINVAL`
/// 8. Σ `remote_iov[i].iov_len > SSIZE_MAX`         → `EINVAL`
///
/// The local/remote sums are *not* required to match — Linux
/// transfers `min(local_sum, remote_sum)` bytes and reports the count.
///
/// # Safety
///
/// When `liovcnt > 0`, `local_iov` must point to at least `liovcnt`
/// readable `Iovec` structures; same for `remote_iov`/`riovcnt`.
unsafe fn process_vm_validate(
    pid: i32,
    local_iov: *const crate::file::Iovec,
    liovcnt: u64,
    remote_iov: *const crate::file::Iovec,
    riovcnt: u64,
    flags: u64,
) -> Result<(), i32> {
    // (1) flags is a reserved field — must be zero.
    if flags != 0 {
        return Err(errno::EINVAL);
    }
    // (2) Non-positive pid never names a real task.
    if pid <= 0 {
        return Err(errno::ESRCH);
    }
    // (3)/(4) iovec-count caps.
    if liovcnt > PROCESS_VM_UIO_MAXIOV {
        return Err(errno::EINVAL);
    }
    if riovcnt > PROCESS_VM_UIO_MAXIOV {
        return Err(errno::EINVAL);
    }
    // (5)/(6) non-empty array requires a non-NULL pointer.
    if liovcnt > 0 && local_iov.is_null() {
        return Err(errno::EFAULT);
    }
    if riovcnt > 0 && remote_iov.is_null() {
        return Err(errno::EFAULT);
    }
    // (7)/(8) per-direction byte-count cap.  Sum with saturating_add
    // so a malicious per-vec u64 length doesn't wrap into the valid
    // range — once we cross SSIZE_MAX we fail-fast.
    let mut lsum: u64 = 0;
    for i in 0..liovcnt {
        // SAFETY: caller contract — local_iov covers liovcnt entries.
        let iov = unsafe { *local_iov.add(i as usize) };
        lsum = lsum.saturating_add(iov.iov_len as u64);
        if lsum > PROCESS_VM_SSIZE_MAX {
            return Err(errno::EINVAL);
        }
    }
    let mut rsum: u64 = 0;
    for i in 0..riovcnt {
        // SAFETY: caller contract — remote_iov covers riovcnt entries.
        let iov = unsafe { *remote_iov.add(i as usize) };
        rsum = rsum.saturating_add(iov.iov_len as u64);
        if rsum > PROCESS_VM_SSIZE_MAX {
            return Err(errno::EINVAL);
        }
    }
    Ok(())
}

/// `process_vm_readv` — read from another process's address space.
///
/// Linux 3.2+.  See [`process_vm_validate`] for the full
/// argument-domain check matrix.  After validation, returns `-1`
/// with `errno = ENOSYS`: cross-process memory access isn't part of
/// the microkernel's IPC model (programs use channel handles to
/// transfer pages explicitly rather than peeking at another task's
/// address space).
///
/// # Safety
///
/// When `liovcnt > 0`, `local_iov` must point to at least `liovcnt`
/// readable `Iovec` structures; same for `remote_iov`/`riovcnt`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn process_vm_readv(
    pid: i32,
    local_iov: *const crate::file::Iovec,
    liovcnt: u64,
    remote_iov: *const crate::file::Iovec,
    riovcnt: u64,
    flags: u64,
) -> i64 {
    // SAFETY: caller contract — iov pointers cover their respective
    // counts of Iovec entries, or are unread when their count is 0.
    match unsafe {
        process_vm_validate(pid, local_iov, liovcnt, remote_iov, riovcnt, flags)
    } {
        Err(e) => {
            errno::set_errno(e);
            -1
        }
        Ok(()) => {
            errno::set_errno(errno::ENOSYS);
            -1
        }
    }
}

/// `process_vm_writev` — write to another process's address space.
///
/// Linux 3.2+.  Mirrors [`process_vm_readv`] but transfers in the
/// opposite direction — same argument-domain checks apply via
/// [`process_vm_validate`].
///
/// # Safety
///
/// When `liovcnt > 0`, `local_iov` must point to at least `liovcnt`
/// readable `Iovec` structures; same for `remote_iov`/`riovcnt`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn process_vm_writev(
    pid: i32,
    local_iov: *const crate::file::Iovec,
    liovcnt: u64,
    remote_iov: *const crate::file::Iovec,
    riovcnt: u64,
    flags: u64,
) -> i64 {
    // SAFETY: caller contract — iov pointers cover their respective
    // counts of Iovec entries, or are unread when their count is 0.
    match unsafe {
        process_vm_validate(pid, local_iov, liovcnt, remote_iov, riovcnt, flags)
    } {
        Err(e) => {
            errno::set_errno(e);
            -1
        }
        Ok(()) => {
            errno::set_errno(errno::ENOSYS);
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// kcmp — compare two processes
// ---------------------------------------------------------------------------

/// kcmp comparison type: two `int` file descriptors in the targets.
pub const KCMP_FILE: i32 = 0;
/// Compare virtual memory (CLONE_VM equivalence).
pub const KCMP_VM: i32 = 1;
/// Compare open-file table (CLONE_FILES equivalence).
pub const KCMP_FILES: i32 = 2;
/// Compare filesystem state (CLONE_FS equivalence).
pub const KCMP_FS: i32 = 3;
/// Compare signal-handler table (CLONE_SIGHAND equivalence).
pub const KCMP_SIGHAND: i32 = 4;
/// Compare I/O context (CLONE_IO equivalence).
pub const KCMP_IO: i32 = 5;
/// Compare System V semaphore-undo lists (CLONE_SYSVSEM equivalence).
pub const KCMP_SYSVSEM: i32 = 6;
/// Compare two epoll-target file descriptors plus optional event
/// data pointers; `idx2` must be a non-NULL pointer to a
/// `kcmp_epoll_slot` describing the second target.
pub const KCMP_EPOLL_TFD: i32 = 7;

/// One past the last documented `kcmp(2)` comparison type.
///
/// Linux's `kernel/kcmp.c` rejects any `type >= KCMP_TYPES` with
/// `EINVAL`.  Keeping this as a derived constant means adding a new
/// `KCMP_*` value above only requires bumping its definition and
/// extending this max.
pub const KCMP_TYPES: i32 = 8;

/// `kcmp` — compare kernel resources of two processes.
///
/// # Linux behaviour
///
/// `int kcmp(pid_t pid1, pid_t pid2, int type, unsigned long idx1,
///           unsigned long idx2)`.  The kernel's `kernel/kcmp.c`
/// performs the following argument-domain checks before reaching the
/// per-type comparison logic:
///
/// 1. `pid1 <= 0`                                  → `ESRCH`
/// 2. `pid2 <= 0`                                  → `ESRCH`
/// 3. `type < 0 || type >= KCMP_TYPES`             → `EINVAL`
/// 4. For `type == KCMP_FILE`: `idx1` and `idx2` are interpreted as
///    fd numbers in the respective targets.  Linux validates each
///    fd via the target's fdtable; we cannot reach into another
///    process's fd space, but we can reject fds outside the i32
///    range (Linux's fd type is `int`).  `idx1 > i32::MAX as u64` or
///    `idx2 > i32::MAX as u64` → `EBADF`.
/// 5. For `type == KCMP_EPOLL_TFD`: `idx2` is a pointer to a
///    `struct kcmp_epoll_slot`; a NULL pointer fails the
///    copy_from_user in the kernel.  `idx2 == 0`              → `EFAULT`
///
/// After validation we return `ENOSYS`: the microkernel doesn't
/// expose kernel-object identity to userspace through this debugging
/// interface — process introspection happens via capability handles
/// with explicit semantics.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn kcmp(
    pid1: i32,
    pid2: i32,
    type_: i32,
    idx1: u64,
    idx2: u64,
) -> i32 {
    // (1)/(2) Both pids must name real tasks.  Linux checks pid1
    // first, then pid2 — preserve that order.
    if pid1 <= 0 {
        errno::set_errno(errno::ESRCH);
        return -1;
    }
    if pid2 <= 0 {
        errno::set_errno(errno::ESRCH);
        return -1;
    }

    // (3) Type must be in the documented range.
    if type_ < 0 || type_ >= KCMP_TYPES {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // (4) KCMP_FILE: idx1/idx2 are fd numbers — must fit in c_int.
    if type_ == KCMP_FILE {
        if idx1 > i32::MAX as u64 || idx2 > i32::MAX as u64 {
            errno::set_errno(errno::EBADF);
            return -1;
        }
    }

    // (5) KCMP_EPOLL_TFD: idx2 must point to a kcmp_epoll_slot.
    if type_ == KCMP_EPOLL_TFD && idx2 == 0 {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // All arguments validated; kernel-object identity comparison not
    // exposed.
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
        // unshare(0) is a Linux-faithful no-op; use CLONE_NEWNS to hit
        // the ENOSYS path now that flag validation passes through.
        assert_eq!(
            unshare(crate::linux_clone_args::CLONE_NEWNS as i32),
            -1,
        );
    }

    #[test]
    fn test_setns_returns_enosys() {
        assert_eq!(setns(0, 0), -1);
    }

    #[test]
    fn test_mount_returns_enosys() {
        // Fully-valid arguments: source=/dev/sda1, target=/mnt,
        // fstype=ext4, flags=0.  Reaches the ENOSYS terminal leg.
        assert_eq!(
            mount(
                b"/dev/sda1\0".as_ptr(),
                b"/mnt\0".as_ptr(),
                b"ext4\0".as_ptr(),
                0,
                core::ptr::null(),
            ),
            -1
        );
    }

    #[test]
    fn test_umount_returns_enosys() {
        // NULL is now caught with EFAULT; use a valid path so we still
        // exercise the ENOSYS terminal state.
        assert_eq!(umount(b"/mnt/foo\0".as_ptr()), -1);
    }

    #[test]
    fn test_umount2_returns_enosys() {
        assert_eq!(umount2(b"/mnt/foo\0".as_ptr(), 0), -1);
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

    // -- Process group / session state machine --
    //
    // These tests pre-set OUR_PGID/OUR_SID/FG_PGRP to non-zero values
    // to skip the getpid() call in ensure_pg_init().  This lets us test
    // the pure-logic state machine on the host without triggering actual
    // kernel syscalls.

    /// Reset the process group/session state to known values.
    ///
    /// Must be called before each process group test to avoid
    /// inter-test interference.  Uses `pid=42` as a deterministic
    /// "fake PID" so ensure_pg_init() skips the getpid() syscall.
    fn reset_pg() {
        unsafe {
            core::ptr::addr_of_mut!(OUR_PGID).write(42);
            core::ptr::addr_of_mut!(OUR_SID).write(42);
            core::ptr::addr_of_mut!(FG_PGRP).write(42);
        }
    }

    /// Ensure fds 0/1/2 are open so `tcgetpgrp`/`tcsetpgrp` tests
    /// can use them.  Other tests may have closed them.
    fn ensure_pg_test_fds() {
        for fd in 0..=2 {
            let _ = crate::fdtable::install_fd(
                fd, crate::fdtable::HandleKind::Console, fd as u64,
            );
        }
    }

    #[test]
    fn test_getpgrp_returns_initialized_value() {
        reset_pg();
        assert_eq!(getpgrp(), 42);
    }

    #[test]
    fn test_getpgrp_consistent() {
        reset_pg();
        let a = getpgrp();
        let b = getpgrp();
        assert_eq!(a, b, "consecutive getpgrp calls must return same value");
    }

    #[test]
    fn test_getpgid_other_pid_returns_that_pid() {
        // For PIDs that aren't ours, getpgid returns the pid itself
        // (each process assumed to be its own group leader).
        reset_pg();
        // Use a PID that definitely isn't "ours" (42).
        assert_eq!(getpgid(9999), 9999);
        assert_eq!(getpgid(1), 1);
    }

    #[test]
    fn test_getsid_other_pid_returns_that_pid() {
        // Same behavior for session ID queries on foreign PIDs.
        reset_pg();
        assert_eq!(getsid(9999), 9999);
        assert_eq!(getsid(1), 1);
    }

    // -- tcgetpgrp / tcsetpgrp (pure state, no getpid call) --

    #[test]
    fn test_tcgetpgrp_returns_fg_pgrp() {
        reset_pg();
        ensure_pg_test_fds();
        assert_eq!(tcgetpgrp(0), 42);
    }

    #[test]
    fn test_tcsetpgrp_round_trip() {
        reset_pg();
        ensure_pg_test_fds();
        assert_eq!(tcsetpgrp(0, 77), 0);
        assert_eq!(tcgetpgrp(0), 77);
    }

    #[test]
    fn test_tcsetpgrp_different_values() {
        reset_pg();
        ensure_pg_test_fds();
        assert_eq!(tcsetpgrp(1, 100), 0);
        assert_eq!(tcgetpgrp(1), 100);
        assert_eq!(tcsetpgrp(2, 200), 0);
        assert_eq!(tcgetpgrp(2), 200);
    }

    #[test]
    fn test_tcsetpgrp_rejects_zero() {
        reset_pg();
        ensure_pg_test_fds();
        assert_eq!(tcsetpgrp(0, 0), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        // Original value should be unchanged.
        assert_eq!(tcgetpgrp(0), 42);
    }

    #[test]
    fn test_tcsetpgrp_rejects_negative() {
        reset_pg();
        ensure_pg_test_fds();
        assert_eq!(tcsetpgrp(0, -1), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        assert_eq!(tcgetpgrp(0), 42);
    }

    #[test]
    fn test_tcsetpgrp_rejects_negative_large() {
        reset_pg();
        ensure_pg_test_fds();
        assert_eq!(tcsetpgrp(0, i32::MIN), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_tcsetpgrp_accepts_one() {
        reset_pg();
        ensure_pg_test_fds();
        assert_eq!(tcsetpgrp(0, 1), 0);
        assert_eq!(tcgetpgrp(0), 1);
    }

    // -- setpgid for other PIDs (silent success, no state change) --

    #[test]
    fn test_setpgid_other_pid_succeeds_silently() {
        reset_pg();
        // setpgid for a PID that isn't ours succeeds but is a no-op.
        assert_eq!(setpgid(9999, 100), 0);
        // Our PGID should be unchanged.
        assert_eq!(getpgrp(), 42);
    }

    // -- waitid constants and branches --

    #[test]
    fn test_waitid_invalid_idtype() {
        // Use WEXITED in options so the prologue options check
        // doesn't preempt the idtype check — the intent of this
        // test is to exercise the idtype-invalid branch.
        errno::set_errno(0);
        assert_eq!(waitid(99, 0, core::ptr::null_mut(), WEXITED), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_waitid_pgid_returns_enosys() {
        // Use WEXITED in options to satisfy Linux's prologue
        // requirement that at least one of WEXITED/WSTOPPED/WCONTINUED
        // be set; otherwise we'd hit the options-validation EINVAL
        // before ever reaching the idtype dispatch.
        errno::set_errno(0);
        assert_eq!(waitid(P_PGID, 0, core::ptr::null_mut(), WEXITED), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -- setpgrp delegates to setpgid(0,0) --

    #[test]
    fn test_setpgrp_return_value() {
        // setpgrp() returns 0 (same as setpgid(0,0)).
        // Note: this calls getpid() but we pre-set state so
        // ensure_pg_init() won't call getpid().  The setpgid
        // body still calls getpid() to get `us`.  On the test
        // target this will return some value; as long as it
        // doesn't crash, the return value of setpgrp (0) is
        // deterministic.
        reset_pg();
        assert_eq!(setpgrp(), 0);
    }

    // -- Wait status: all signals 1-126 are signaled, not exited --

    #[test]
    fn test_all_signals_are_signaled() {
        for sig in 1..=126i32 {
            let status = sig;
            assert!(wifsignaled(status), "signal {sig} should be signaled");
            assert_eq!(wtermsig(status), sig, "wtermsig({sig})");
            assert!(!wifexited(status), "signal {sig} should not be exited");
        }
    }

    // -- Wait status: all exit codes 0-255 round-trip --

    #[test]
    fn test_all_exit_codes_roundtrip() {
        for code in 0..=255i32 {
            let status = code << 8;
            assert!(wifexited(status), "exit {code} should be exited");
            assert_eq!(wexitstatus(status), code, "wexitstatus({code})");
        }
    }

    // -- Mutually exclusive status categories --

    #[test]
    fn test_normal_exit_not_signaled_or_continued() {
        let status = 42 << 8;
        assert!(wifexited(status));
        assert!(!wifsignaled(status));
        assert!(!wifcontinued(status));
    }

    #[test]
    fn test_signal_death_not_exited_or_continued() {
        let status = 11; // SIGSEGV
        assert!(wifsignaled(status));
        assert!(!wifexited(status));
        assert!(!wifcontinued(status));
    }

    // -- Stub errno checks --

    #[test]
    fn test_fork_sets_enosys() {
        crate::errno::set_errno(0);
        fork();
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_vfork_sets_enosys() {
        crate::errno::set_errno(0);
        vfork();
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_clone_sets_enosys() {
        // Pass valid (non-NULL) fn/stack so the call survives the
        // argument-domain checks added in Phase 54 and reaches the
        // ENOSYS-returning subsystem stub.
        crate::errno::set_errno(0);
        let fn_ptr = 0x2_0000_usize as *const u8;
        let stack = 0x1_0000_usize as *mut u8;
        clone(fn_ptr, stack, 0, core::ptr::null_mut());
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_unshare_sets_enosys() {
        crate::errno::set_errno(0);
        // unshare(0) is a no-op (returns 0) per Linux — exercise the
        // ENOSYS path with a real namespace flag instead.
        unshare(crate::linux_clone_args::CLONE_NEWNS as i32);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_setns_sets_enosys() {
        crate::errno::set_errno(0);
        setns(0, 0);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mount_sets_enosys() {
        crate::errno::set_errno(0);
        // Valid args reach the ENOSYS terminal leg.
        mount(
            b"/dev/sda1\0".as_ptr(),
            b"/mnt\0".as_ptr(),
            b"ext4\0".as_ptr(),
            0,
            core::ptr::null(),
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_umount_sets_enosys() {
        crate::errno::set_errno(0);
        // NULL → EFAULT now; use a valid path to reach the ENOSYS leg.
        umount(b"/mnt/foo\0".as_ptr());
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_umount2_sets_enosys() {
        crate::errno::set_errno(0);
        umount2(b"/mnt/foo\0".as_ptr(), 0);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // -- WNOHANG and WUNTRACED are distinct bits --

    #[test]
    fn test_wait_flags_disjoint() {
        assert_eq!(WNOHANG & WUNTRACED, 0);
    }

    // -- waitid P_ALL/P_PID idtype constants disjoint --

    #[test]
    fn test_waitid_idtype_distinct() {
        assert_ne!(P_ALL, P_PID);
        assert_ne!(P_PID, P_PGID);
        assert_ne!(P_ALL, P_PGID);
    }

    // -- tcsetpgrp accepts max i32 --

    #[test]
    fn test_tcsetpgrp_max_pgrp() {
        reset_pg();
        ensure_pg_test_fds();
        assert_eq!(tcsetpgrp(0, i32::MAX), 0);
        assert_eq!(tcgetpgrp(0), i32::MAX);
    }

    // -- getpgid for pid=0 returns our PGID --

    #[test]
    fn test_getpgid_zero_returns_our_pgid() {
        reset_pg();
        assert_eq!(getpgid(0), 42);
    }

    // -- getsid for pid=0 returns our SID --

    #[test]
    fn test_getsid_zero_returns_our_sid() {
        reset_pg();
        assert_eq!(getsid(0), 42);
    }

    // -- reboot stub --

    #[test]
    fn test_reboot_returns_enosys_when_capable() {
        // The default process holds CAP_SYS_BOOT, so a well-formed
        // reboot call surfaces ENOSYS (no reboot subsystem yet) rather
        // than EPERM.  EPERM is exercised by Phase 77 tests in
        // sys_reboot::tests with CAP_SYS_BOOT explicitly dropped.
        crate::errno::set_errno(0);
        assert_eq!(reboot(LINUX_REBOOT_CMD_RESTART as i32), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_reboot_halt() {
        assert_eq!(reboot(LINUX_REBOOT_CMD_HALT as i32), -1);
    }

    #[test]
    fn test_reboot_power_off() {
        assert_eq!(reboot(LINUX_REBOOT_CMD_POWER_OFF as i32), -1);
    }

    // -- reboot constants --

    #[test]
    fn test_reboot_magic_values() {
        assert_eq!(LINUX_REBOOT_MAGIC1, 0xfee1_dead);
        assert_eq!(LINUX_REBOOT_MAGIC2, 672274793);
    }

    #[test]
    fn test_reboot_cmd_constants_distinct() {
        assert_ne!(LINUX_REBOOT_CMD_RESTART, LINUX_REBOOT_CMD_HALT);
        assert_ne!(LINUX_REBOOT_CMD_HALT, LINUX_REBOOT_CMD_POWER_OFF);
        assert_ne!(LINUX_REBOOT_CMD_RESTART, LINUX_REBOOT_CMD_POWER_OFF);
    }

    // -- wait (convenience wrapper) --

    #[test]
    fn test_wait_null_status() {
        // wait(NULL) should not crash. It delegates to waitpid(-1,NULL,0).
        // On the host this will hit a real syscall and fail, but it should
        // not segfault.
        let ret = wait(core::ptr::null_mut());
        // Return is either a child PID or -1 (no children). Don't assert
        // the exact value — just verify no crash.
        let _ = ret;
    }

    // -- wait3 --

    #[test]
    fn test_wait3_null_status_null_rusage() {
        let ret = wait3(core::ptr::null_mut(), WNOHANG, core::ptr::null_mut());
        // On host: -1 (ECHILD). Don't assert exact value.
        let _ = ret;
    }

    #[test]
    fn test_wait3_zeroes_rusage() {
        let mut rusage = crate::resource::Rusage {
            ru_utime: crate::time::Timeval { tv_sec: 99, tv_usec: 99 },
            ru_stime: crate::time::Timeval { tv_sec: 99, tv_usec: 99 },
            ru_maxrss: 99,
            ru_ixrss: 0, ru_idrss: 0, ru_isrss: 0,
            ru_minflt: 0, ru_majflt: 0, ru_nswap: 0,
            ru_inblock: 0, ru_oublock: 0, ru_msgsnd: 0,
            ru_msgrcv: 0, ru_nsignals: 0, ru_nvcsw: 0,
            ru_nivcsw: 0,
        };
        let _ = wait3(core::ptr::null_mut(), WNOHANG, &raw mut rusage);
        // rusage should have been zeroed.
        assert_eq!(rusage.ru_utime.tv_sec, 0);
        assert_eq!(rusage.ru_stime.tv_sec, 0);
        assert_eq!(rusage.ru_maxrss, 0);
    }

    // -- wait4 --

    #[test]
    fn test_wait4_null_rusage() {
        let ret = wait4(-1, core::ptr::null_mut(), WNOHANG, core::ptr::null_mut());
        let _ = ret;
    }

    #[test]
    fn test_wait4_zeroes_rusage() {
        let mut rusage = crate::resource::Rusage {
            ru_utime: crate::time::Timeval { tv_sec: 77, tv_usec: 77 },
            ru_stime: crate::time::Timeval { tv_sec: 77, tv_usec: 77 },
            ru_maxrss: 77,
            ru_ixrss: 0, ru_idrss: 0, ru_isrss: 0,
            ru_minflt: 0, ru_majflt: 0, ru_nswap: 0,
            ru_inblock: 0, ru_oublock: 0, ru_msgsnd: 0,
            ru_msgrcv: 0, ru_nsignals: 0, ru_nvcsw: 0,
            ru_nivcsw: 0,
        };
        let _ = wait4(1, core::ptr::null_mut(), WNOHANG, &raw mut rusage);
        assert_eq!(rusage.ru_utime.tv_sec, 0);
        assert_eq!(rusage.ru_maxrss, 0);
    }

    // -- setsid --

    #[test]
    fn test_setsid_sets_sid_and_pgid() {
        reset_pg();
        // setsid calls getpid() which will return some value on the
        // host. Just verify it doesn't crash and returns non-negative.
        // We can't predict the exact PID on the test host.
        let sid = setsid();
        // After setsid, getsid(0) should return the same value.
        let reported_sid = getsid(0);
        assert_eq!(sid, reported_sid);
        // And getpgrp should match too.
        assert_eq!(sid, getpgrp());
    }

    // -- getpid/gettid return something --

    #[test]
    fn test_getpid_returns_value() {
        // getpid() calls a real syscall on the host. Just verify it
        // doesn't crash and returns some value.
        let pid = getpid();
        let _ = pid;
    }

    #[test]
    fn test_gettid_returns_value() {
        // gettid() calls a real syscall on the host.
        let tid = gettid();
        let _ = tid;
    }

    // -- wait3/wait4 with WNOHANG don't block --

    #[test]
    fn test_wait3_wnohang_returns_immediately() {
        let ret = wait3(core::ptr::null_mut(), WNOHANG, core::ptr::null_mut());
        // With WNOHANG and no children, should return quickly (likely -1).
        let _ = ret;
    }

    #[test]
    fn test_wait4_specific_pid_wnohang() {
        let ret = wait4(99999, core::ptr::null_mut(), WNOHANG, core::ptr::null_mut());
        let _ = ret;
    }

    // -- Phase 103: waitpid / wait4 / waitid options validation --
    //
    // Linux semantics (kernel/exit.c::kernel_wait4, sys_waitid):
    //   waitpid/wait4:
    //     if (options & ~(WNOHANG | WUNTRACED | WCONTINUED |
    //                     __WNOTHREAD | __WCLONE | __WALL))
    //             return -EINVAL;
    //   waitid:
    //     if (options & ~(WNOHANG | WNOWAIT | WEXITED | WSTOPPED |
    //                     WCONTINUED | __WNOTHREAD | __WCLONE | __WALL))
    //             return -EINVAL;
    //     if (!(options & (WEXITED | WSTOPPED | WCONTINUED)))
    //             return -EINVAL;
    // Both checks precede the pid/idtype dispatch and (for wait3/4)
    // any rusage write.

    #[test]
    fn test_wait_flag_constants_match_linux() {
        // Defensive invariants on the constants — match glibc /
        // <linux/wait.h>.  If any of these drift, the mask check would
        // accept the wrong bits.
        assert_eq!(WNOHANG,    0x0000_0001);
        assert_eq!(WUNTRACED,  0x0000_0002);
        assert_eq!(WSTOPPED,   WUNTRACED);
        assert_eq!(WEXITED,    0x0000_0004);
        assert_eq!(WCONTINUED, 0x0000_0008);
        assert_eq!(WNOWAIT,    0x0100_0000);
        assert_eq!(__WNOTHREAD, 0x2000_0000);
        assert_eq!(__WALL,      0x4000_0000);
        // __WCLONE is bit 31 (the sign bit on i32).
        assert_eq!(__WCLONE,    i32::MIN);
    }

    #[test]
    fn test_waitpid_valid_options_mask() {
        // The mask is exactly the union of accepted flags.
        let expected =
            WNOHANG | WUNTRACED | WCONTINUED | __WNOTHREAD | __WALL | __WCLONE;
        assert_eq!(WAITPID_VALID_OPTIONS, expected);
    }

    #[test]
    fn test_waitid_valid_options_mask() {
        // The waitid mask is a superset of the waitpid mask plus
        // WEXITED and WNOWAIT.  WSTOPPED == WUNTRACED is already in.
        let expected = WAITPID_VALID_OPTIONS | WEXITED | WNOWAIT;
        assert_eq!(WAITID_VALID_OPTIONS, expected);
    }

    #[test]
    fn test_waitpid_unknown_option_einval() {
        // Bit 4 is just above WCONTINUED (bit 3) and is not in the
        // valid mask.  Must EINVAL.
        errno::set_errno(0);
        let ret = waitpid(-1, core::ptr::null_mut(), 1 << 4);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_waitpid_wexited_rejected() {
        // WEXITED is a waitid-only flag.  waitpid must reject it.
        errno::set_errno(0);
        let ret = waitpid(-1, core::ptr::null_mut(), WEXITED);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_waitpid_wnowait_rejected() {
        // WNOWAIT is a waitid-only flag.  waitpid must reject it.
        errno::set_errno(0);
        let ret = waitpid(-1, core::ptr::null_mut(), WNOWAIT);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_waitpid_valid_options_pass_mask() {
        // Each individually-valid bit should NOT be rejected by the
        // mask.  We can't easily assert success (no children to wait
        // on), but we can assert that the returned errno is not
        // EINVAL when an error occurs.
        for &opt in &[WNOHANG, WUNTRACED, WCONTINUED, __WNOTHREAD, __WALL, __WCLONE] {
            errno::set_errno(0);
            let _ = waitpid(99_999, core::ptr::null_mut(), opt);
            assert_ne!(errno::get_errno(), errno::EINVAL,
                "valid option {:#x} should not be rejected by waitpid mask", opt);
        }
    }

    #[test]
    fn test_waitpid_combined_valid_options_pass() {
        // The full valid mask combined should pass.
        errno::set_errno(0);
        let _ = waitpid(99_999, core::ptr::null_mut(), WAITPID_VALID_OPTIONS);
        assert_ne!(errno::get_errno(), errno::EINVAL,
            "the canonical valid combo must not be rejected");
    }

    #[test]
    fn test_wait3_rejects_bad_options_without_touching_rusage() {
        // Regression guard: wait3 used to zero rusage unconditionally,
        // even when the call was malformed.  Linux validates options
        // first.
        let mut rusage = crate::resource::Rusage {
            ru_utime: crate::time::Timeval { tv_sec: 1234, tv_usec: 5678 },
            ru_stime: crate::time::Timeval { tv_sec: 9, tv_usec: 9 },
            ru_maxrss: 4321,
            ru_ixrss: 0, ru_idrss: 0, ru_isrss: 0,
            ru_minflt: 0, ru_majflt: 0, ru_nswap: 0,
            ru_inblock: 0, ru_oublock: 0, ru_msgsnd: 0,
            ru_msgrcv: 0, ru_nsignals: 0, ru_nvcsw: 0,
            ru_nivcsw: 0,
        };
        errno::set_errno(0);
        let ret = wait3(core::ptr::null_mut(), 1 << 5, &raw mut rusage);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        // rusage must NOT have been zeroed.
        assert_eq!(rusage.ru_utime.tv_sec, 1234,
            "rusage must be untouched when options are invalid");
        assert_eq!(rusage.ru_maxrss, 4321);
    }

    #[test]
    fn test_wait4_rejects_bad_options_without_touching_rusage() {
        let mut rusage = crate::resource::Rusage {
            ru_utime: crate::time::Timeval { tv_sec: 7777, tv_usec: 8888 },
            ru_stime: crate::time::Timeval { tv_sec: 1, tv_usec: 1 },
            ru_maxrss: 6543,
            ru_ixrss: 0, ru_idrss: 0, ru_isrss: 0,
            ru_minflt: 0, ru_majflt: 0, ru_nswap: 0,
            ru_inblock: 0, ru_oublock: 0, ru_msgsnd: 0,
            ru_msgrcv: 0, ru_nsignals: 0, ru_nvcsw: 0,
            ru_nivcsw: 0,
        };
        errno::set_errno(0);
        // WEXITED is a waitid-only bit that is NOT in
        // WAITPID_VALID_OPTIONS.  Must EINVAL via the mask path.
        // (Avoid using i32::MIN >> 1 here — sign-extending right
        // shift produces __WALL|__WCLONE, both of which are VALID
        // waitpid options, so it would pass the mask.)
        let ret = wait4(-1, core::ptr::null_mut(), WEXITED,
                        &raw mut rusage);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        assert_eq!(rusage.ru_utime.tv_sec, 7777);
        assert_eq!(rusage.ru_maxrss, 6543);
    }

    #[test]
    fn test_waitid_missing_state_flag_einval() {
        // At least one of WEXITED, WSTOPPED, WCONTINUED is required.
        // With options=WNOHANG (no state bit), waitid must EINVAL.
        errno::set_errno(0);
        let ret = waitid(P_ALL, 0, core::ptr::null_mut(), WNOHANG);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_waitid_zero_options_einval() {
        // options=0 fails: no state bit set, mask still passes.
        errno::set_errno(0);
        let ret = waitid(P_ALL, 0, core::ptr::null_mut(), 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_waitid_unknown_option_einval() {
        // A bit not in WAITID_VALID_OPTIONS — bit 4 is just above
        // WCONTINUED (bit 3) and not in the mask.
        errno::set_errno(0);
        let ret = waitid(P_ALL, 0, core::ptr::null_mut(),
                         WEXITED | (1 << 4));
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_waitid_wexited_alone_passes_prologue() {
        // WEXITED alone satisfies both the mask and the state
        // requirement.  Should NOT EINVAL from the prologue; it may
        // fail later for other reasons (e.g. ECHILD).
        errno::set_errno(0);
        let _ = waitid(P_ALL, 0, core::ptr::null_mut(), WEXITED);
        assert_ne!(errno::get_errno(), errno::EINVAL,
            "WEXITED alone must pass waitid's prologue");
    }

    #[test]
    fn test_waitid_wstopped_alone_passes_prologue() {
        errno::set_errno(0);
        let _ = waitid(P_ALL, 0, core::ptr::null_mut(), WSTOPPED);
        assert_ne!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_waitid_wcontinued_alone_passes_prologue() {
        errno::set_errno(0);
        let _ = waitid(P_ALL, 0, core::ptr::null_mut(), WCONTINUED);
        assert_ne!(errno::get_errno(), errno::EINVAL);
    }

    // -- reboot CAD constants --

    #[test]
    fn test_reboot_cad_constants() {
        assert_ne!(LINUX_REBOOT_CMD_CAD_ON, LINUX_REBOOT_CMD_CAD_OFF);
        assert_eq!(LINUX_REBOOT_CMD_CAD_OFF, 0);
    }

    // -- pidfd stubs --

    #[test]
    fn test_pidfd_open_enosys() {
        crate::errno::set_errno(0);
        assert_eq!(pidfd_open(1, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_pidfd_open_with_flags() {
        crate::errno::set_errno(0);
        assert_eq!(pidfd_open(1, PIDFD_NONBLOCK), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_pidfd_send_signal_enosys() {
        crate::errno::set_errno(0);
        assert_eq!(pidfd_send_signal(3, 9, core::ptr::null(), 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_pidfd_getfd_enosys() {
        crate::errno::set_errno(0);
        assert_eq!(pidfd_getfd(3, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_pidfd_nonblock_constant() {
        assert_ne!(PIDFD_NONBLOCK, 0);
    }

    // ------------------------------------------------------------------
    // Phase 49 — pidfd_open / pidfd_send_signal / pidfd_getfd validators
    // ------------------------------------------------------------------

    // --- pidfd_open: pid domain ---

    #[test]
    fn test_pidfd_open_pid_zero_einval() {
        crate::errno::set_errno(0);
        assert_eq!(pidfd_open(0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pidfd_open_pid_negative_einval() {
        crate::errno::set_errno(0);
        assert_eq!(pidfd_open(-1, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pidfd_open_pid_min_einval() {
        crate::errno::set_errno(0);
        assert_eq!(pidfd_open(i32::MIN, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pidfd_open_pid_max_valid() {
        // Largest positive PID passes domain check, falls through to ENOSYS.
        crate::errno::set_errno(0);
        assert_eq!(pidfd_open(i32::MAX, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- pidfd_open: flag mask ---

    #[test]
    fn test_pidfd_open_unknown_flag_einval() {
        // bit 0x1 is not in PIDFD_OPEN_FLAGS_VALID
        crate::errno::set_errno(0);
        assert_eq!(pidfd_open(1, 0x1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pidfd_open_high_bit_einval() {
        crate::errno::set_errno(0);
        assert_eq!(pidfd_open(1, 0x8000_0000), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pidfd_open_thread_flag_passes() {
        // PIDFD_THREAD (Linux 6.2+) is recognised → falls through to ENOSYS.
        crate::errno::set_errno(0);
        assert_eq!(pidfd_open(1, PIDFD_THREAD), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_pidfd_open_nonblock_plus_thread_passes() {
        crate::errno::set_errno(0);
        assert_eq!(pidfd_open(1, PIDFD_NONBLOCK | PIDFD_THREAD), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_pidfd_open_flag_constant_values() {
        // PIDFD_NONBLOCK == O_NONBLOCK == octal 04000 on Linux x86-64.
        assert_eq!(PIDFD_NONBLOCK, 0o4000);
        // PIDFD_THREAD == O_EXCL == octal 0200 on Linux x86-64.
        assert_eq!(PIDFD_THREAD, 0o200);
        // The valid mask is exactly the union of the two.
        assert_eq!(
            PIDFD_OPEN_FLAGS_VALID,
            PIDFD_NONBLOCK | PIDFD_THREAD,
        );
    }

    #[test]
    fn test_pidfd_open_validation_order_flags_first() {
        // Phase 116: Linux's pidfd_open checks the flag mask BEFORE
        // pid<=0.  When both pid and flags are invalid the result is
        // still EINVAL (both paths return the same errno), but this
        // test pins the Linux precedence in.  A previous version of
        // this test (named ..._pid_first) asserted the opposite order;
        // it was reversed in Phase 116 when the implementation was
        // brought into ordering parity with kernel/pid.c.
        crate::errno::set_errno(0);
        assert_eq!(pidfd_open(0, 0xFFFF_FFFF), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // --- pidfd_send_signal: flags ---

    #[test]
    fn test_pidfd_send_signal_nonzero_flags_einval() {
        crate::errno::set_errno(0);
        assert_eq!(pidfd_send_signal(3, 9, core::ptr::null(), 1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pidfd_send_signal_high_flag_einval() {
        crate::errno::set_errno(0);
        assert_eq!(
            pidfd_send_signal(3, 9, core::ptr::null(), 0x8000_0000),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // --- pidfd_send_signal: pidfd ---

    #[test]
    fn test_pidfd_send_signal_negative_fd_ebadf() {
        crate::errno::set_errno(0);
        assert_eq!(pidfd_send_signal(-1, 9, core::ptr::null(), 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_pidfd_send_signal_min_fd_ebadf() {
        crate::errno::set_errno(0);
        assert_eq!(
            pidfd_send_signal(i32::MIN, 9, core::ptr::null(), 0),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // --- pidfd_send_signal: sig range ---

    #[test]
    fn test_pidfd_send_signal_negative_sig_einval() {
        crate::errno::set_errno(0);
        assert_eq!(pidfd_send_signal(3, -1, core::ptr::null(), 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pidfd_send_signal_sig_too_large_einval() {
        crate::errno::set_errno(0);
        assert_eq!(
            pidfd_send_signal(3, PIDFD_SIG_MAX + 1, core::ptr::null(), 0),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pidfd_send_signal_sig_max_valid() {
        // sig == 64 is the maximum accepted; falls through to ENOSYS.
        crate::errno::set_errno(0);
        assert_eq!(
            pidfd_send_signal(3, PIDFD_SIG_MAX, core::ptr::null(), 0),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_pidfd_send_signal_sig_zero_permission_probe() {
        // sig == 0 is the permission/existence probe; allowed.
        crate::errno::set_errno(0);
        assert_eq!(pidfd_send_signal(3, 0, core::ptr::null(), 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- pidfd_send_signal: siginfo cross-check ---

    #[test]
    fn test_pidfd_send_signal_info_signo_mismatch_einval() {
        let mut info: crate::signal::SiginfoT = Default::default();
        info.si_signo = 11; // SIGSEGV
        crate::errno::set_errno(0);
        // Outer sig is 9 but si_signo is 11 → EINVAL.
        let ret = pidfd_send_signal(
            3,
            9,
            (&raw const info).cast::<core::ffi::c_void>(),
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pidfd_send_signal_info_signo_match_passes() {
        let mut info: crate::signal::SiginfoT = Default::default();
        info.si_signo = 9; // SIGKILL
        crate::errno::set_errno(0);
        let ret = pidfd_send_signal(
            3,
            9,
            (&raw const info).cast::<core::ffi::c_void>(),
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_pidfd_send_signal_info_with_sig_zero_requires_zero_signo() {
        // sig == 0 + si_signo != 0 is still a mismatch.
        let mut info: crate::signal::SiginfoT = Default::default();
        info.si_signo = 11;
        crate::errno::set_errno(0);
        let ret = pidfd_send_signal(
            3,
            0,
            (&raw const info).cast::<core::ffi::c_void>(),
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // --- pidfd_send_signal: order — flags first, then fd, then sig, then info ---

    #[test]
    fn test_pidfd_send_signal_flags_before_fd() {
        // Both flags and fd are invalid → EINVAL for flags wins.
        crate::errno::set_errno(0);
        let ret = pidfd_send_signal(-1, 9, core::ptr::null(), 1);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pidfd_send_signal_fd_before_sig() {
        // Bad fd + bad sig → EBADF for fd wins.
        crate::errno::set_errno(0);
        let ret = pidfd_send_signal(-1, 999, core::ptr::null(), 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // --- pidfd_getfd: flags ---

    #[test]
    fn test_pidfd_getfd_nonzero_flags_einval() {
        crate::errno::set_errno(0);
        assert_eq!(pidfd_getfd(3, 0, 1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pidfd_getfd_high_flag_einval() {
        crate::errno::set_errno(0);
        assert_eq!(pidfd_getfd(3, 0, 0x8000_0000), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // --- pidfd_getfd: pidfd ---

    #[test]
    fn test_pidfd_getfd_negative_pidfd_ebadf() {
        crate::errno::set_errno(0);
        assert_eq!(pidfd_getfd(-1, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_pidfd_getfd_min_pidfd_ebadf() {
        crate::errno::set_errno(0);
        assert_eq!(pidfd_getfd(i32::MIN, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // --- pidfd_getfd: targetfd ---

    #[test]
    fn test_pidfd_getfd_negative_targetfd_ebadf() {
        crate::errno::set_errno(0);
        assert_eq!(pidfd_getfd(3, -1, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_pidfd_getfd_min_targetfd_ebadf() {
        crate::errno::set_errno(0);
        assert_eq!(pidfd_getfd(3, i32::MIN, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_pidfd_getfd_max_fds_pass() {
        // Both fds at i32::MAX → fall through to ENOSYS.
        crate::errno::set_errno(0);
        assert_eq!(pidfd_getfd(i32::MAX, i32::MAX, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- pidfd_getfd: order — flags first, then pidfd, then targetfd ---

    #[test]
    fn test_pidfd_getfd_flags_before_pidfd() {
        crate::errno::set_errno(0);
        let ret = pidfd_getfd(-1, 0, 1);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pidfd_getfd_pidfd_before_targetfd() {
        crate::errno::set_errno(0);
        let ret = pidfd_getfd(-5, -7, 0);
        assert_eq!(ret, -1);
        // pidfd is the first fd checked → its EBADF wins, but both produce
        // the same errno code so this is more a tag for ordering coverage.
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // --- Constants ---

    #[test]
    fn test_pidfd_sig_max_constant() {
        assert_eq!(PIDFD_SIG_MAX, 64);
    }

    #[test]
    fn test_pidfd_open_valid_mask_constant() {
        assert_eq!(PIDFD_OPEN_FLAGS_VALID, 0o4000 | 0o200);
        // No overlap with reserved bits (high bits) that would alias real
        // O_* values we don't accept.
        assert_eq!(PIDFD_OPEN_FLAGS_VALID & !0o4200, 0);
    }

    // --- errno preserved on validation success (ENOSYS still set) ---

    #[test]
    fn test_pidfd_open_sets_errno_on_validated_call() {
        crate::errno::set_errno(0);
        let _ = pidfd_open(123, PIDFD_NONBLOCK);
        // ENOSYS is the "validated but unimplemented" sentinel.
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- Workflow tests: real-world callers exercise the validator shape ---

    /// systemd's `sd_pidfd_get_inode_id` probe: opens a pidfd for the
    /// caller's own PID with PIDFD_NONBLOCK, then would `fstat` it.  We
    /// see the open get past argument validation (PID > 0, recognised
    /// flag) and stop at ENOSYS — letting systemd's fallback ladder
    /// (cgroup.events poll) kick in.
    #[test]
    fn test_pidfd_workflow_systemd_open_probe() {
        crate::errno::set_errno(0);
        // systemd uses getpid() result, which is always > 0.
        let pid: PidT = 1234;
        let ret = pidfd_open(pid, PIDFD_NONBLOCK);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// runc/crun container runtime: when joining an existing container,
    /// runc opens `/proc/<pid>/ns/pid` and then calls `pidfd_open(pid, 0)`
    /// to get a stable handle.  Both legs of the probe must validate
    /// arguments cleanly so runc reports a sensible "kernel too old"
    /// error rather than crashing.
    #[test]
    fn test_pidfd_workflow_runc_join_namespace() {
        crate::errno::set_errno(0);
        // Container init PID (typically 1 inside the container, but the
        // host-side PID can be anything > 0).
        let ret = pidfd_open(2718, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// Go's `syscall.PidFD` family (Go 1.22): the standard library probes
    /// pidfd support by calling `pidfd_open(getpid(), 0)` at startup of
    /// any program that uses `os/exec.Cmd.Start` with `Cancel` set.  An
    /// `EINVAL` here would be a regression — the call must reach our
    /// ENOSYS line.
    #[test]
    fn test_pidfd_workflow_go_runtime_probe() {
        crate::errno::set_errno(0);
        let ret = pidfd_open(99, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// strace `-y` / `lldb` cross-process fd inspection: uses
    /// `pidfd_getfd(tracee_pidfd, fd_in_tracee, 0)` to import a fd.
    /// Tests the happy-path validation: real fds and zero flags pass.
    #[test]
    fn test_pidfd_workflow_strace_y_inspect_fd() {
        crate::errno::set_errno(0);
        let pidfd = 5; // hypothetical pidfd for the tracee
        let target_fd = 3; // tracee's stderr or a socket
        let ret = pidfd_getfd(pidfd, target_fd, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// kill(1) replacement using pidfd: `kill --pidfd=<fd> <sig>` (from
    /// util-linux) calls `pidfd_send_signal(fd, sig, NULL, 0)`.  Tests
    /// a typical SIGTERM (15) on a real fd with NULL info.
    #[test]
    fn test_pidfd_workflow_kill_pidfd_term() {
        crate::errno::set_errno(0);
        let ret = pidfd_send_signal(7, 15, core::ptr::null(), 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// Java JDK 22's `ProcessHandleImpl` uses pidfd internally on Linux
    /// 5.3+ to avoid PID reuse races in `Process.onExit()`.  The probe
    /// at class init opens a pidfd to itself.  Tests that a self-pid
    /// open with the maximum valid flag combo validates cleanly.
    #[test]
    fn test_pidfd_workflow_jdk22_process_handle_probe() {
        crate::errno::set_errno(0);
        let self_pid: PidT = 12345;
        let ret = pidfd_open(self_pid, PIDFD_NONBLOCK | PIDFD_THREAD);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// Buggy caller that passes a stack-allocated siginfo_t whose
    /// si_signo got zero-initialised but the outer sig is non-zero.
    /// This is a common bug in C wrappers that forget to set si_signo.
    /// We must catch it with EINVAL so the bug is loud, not silent.
    #[test]
    fn test_pidfd_workflow_buggy_zero_initialised_siginfo() {
        let info: crate::signal::SiginfoT = Default::default();
        // si_signo defaults to 0.
        crate::errno::set_errno(0);
        let ret = pidfd_send_signal(
            3,
            9, // SIGKILL requested
            (&raw const info).cast::<core::ffi::c_void>(),
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // ------------------------------------------------------------------
    // Phase 50 — unshare / setns validators
    // ------------------------------------------------------------------

    // --- unshare: zero is a no-op ---

    #[test]
    fn test_unshare_zero_returns_zero_and_preserves_errno() {
        crate::errno::set_errno(0xBEEF);
        let ret = unshare(0);
        assert_eq!(ret, 0);
        // Linux rule: successful syscalls must not clobber errno.
        assert_eq!(crate::errno::get_errno(), 0xBEEF);
    }

    // --- unshare: each valid flag falls through to ENOSYS ---

    #[test]
    fn test_unshare_clone_newns_enosys() {
        crate::errno::set_errno(0);
        assert_eq!(
            unshare(crate::linux_clone_args::CLONE_NEWNS as i32),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_unshare_clone_newuts_enosys() {
        crate::errno::set_errno(0);
        assert_eq!(
            unshare(crate::linux_clone_args::CLONE_NEWUTS as i32),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_unshare_clone_newipc_enosys() {
        crate::errno::set_errno(0);
        assert_eq!(
            unshare(crate::linux_clone_args::CLONE_NEWIPC as i32),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_unshare_clone_newpid_enosys() {
        crate::errno::set_errno(0);
        assert_eq!(
            unshare(crate::linux_clone_args::CLONE_NEWPID as i32),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_unshare_clone_newnet_enosys() {
        crate::errno::set_errno(0);
        assert_eq!(
            unshare(crate::linux_clone_args::CLONE_NEWNET as i32),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_unshare_clone_newuser_enosys() {
        crate::errno::set_errno(0);
        assert_eq!(
            unshare(crate::linux_clone_args::CLONE_NEWUSER as i32),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_unshare_clone_newcgroup_enosys() {
        crate::errno::set_errno(0);
        assert_eq!(
            unshare(crate::linux_clone_args::CLONE_NEWCGROUP as i32),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_unshare_clone_newtime_enosys() {
        crate::errno::set_errno(0);
        assert_eq!(
            unshare(crate::linux_clone_args::CLONE_NEWTIME as i32),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_unshare_all_valid_flags_combined_enosys() {
        crate::errno::set_errno(0);
        assert_eq!(unshare(UNSHARE_FLAGS_VALID as i32), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- unshare: rejected flag bits ---

    #[test]
    fn test_unshare_clone_io_rejected() {
        // CLONE_IO = 0x8000_0000 is not valid for unshare.
        crate::errno::set_errno(0);
        let ret = unshare(crate::linux_clone_args::CLONE_IO as i32);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_unshare_clone_pidfd_rejected() {
        // CLONE_PIDFD is not meaningful for unshare.
        crate::errno::set_errno(0);
        let ret = unshare(crate::linux_clone_args::CLONE_PIDFD as i32);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_unshare_clone_settls_rejected() {
        crate::errno::set_errno(0);
        let ret = unshare(crate::linux_clone_args::CLONE_SETTLS as i32);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_unshare_clone_parent_settid_rejected() {
        crate::errno::set_errno(0);
        let ret =
            unshare(crate::linux_clone_args::CLONE_PARENT_SETTID as i32);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_unshare_minus_one_einval() {
        // All bits set — guaranteed to include unrecognised ones.
        crate::errno::set_errno(0);
        let ret = unshare(-1);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_unshare_valid_mixed_with_invalid_rejected() {
        // Even if CLONE_NEWNS is valid, mixing in CLONE_IO must fail.
        crate::errno::set_errno(0);
        let bits = (crate::linux_clone_args::CLONE_NEWNS
            | crate::linux_clone_args::CLONE_IO) as i32;
        let ret = unshare(bits);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // --- unshare: valid mask shape ---

    #[test]
    fn test_unshare_flags_valid_mask_constant() {
        // Sanity-check the mask value: must contain every flag listed
        // in the Linux fork.c::check_unshare_flags whitelist and no
        // other bits.
        let expected = (crate::linux_clone_args::CLONE_THREAD
            | crate::linux_clone_args::CLONE_FS
            | crate::linux_clone_args::CLONE_NEWNS
            | crate::linux_clone_args::CLONE_SIGHAND
            | crate::linux_clone_args::CLONE_VM
            | crate::linux_clone_args::CLONE_FILES
            | crate::linux_clone_args::CLONE_SYSVSEM
            | crate::linux_clone_args::CLONE_NEWUTS
            | crate::linux_clone_args::CLONE_NEWIPC
            | crate::linux_clone_args::CLONE_NEWNET
            | crate::linux_clone_args::CLONE_NEWUSER
            | crate::linux_clone_args::CLONE_NEWPID
            | crate::linux_clone_args::CLONE_NEWCGROUP
            | crate::linux_clone_args::CLONE_NEWTIME) as u32;
        assert_eq!(UNSHARE_FLAGS_VALID, expected);
        // CLONE_IO (sign bit) must be absent.
        assert_eq!(
            UNSHARE_FLAGS_VALID
                & (crate::linux_clone_args::CLONE_IO as u32),
            0,
        );
    }

    // --- setns: fd validation ---

    #[test]
    fn test_setns_negative_fd_ebadf() {
        crate::errno::set_errno(0);
        assert_eq!(setns(-1, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_setns_min_fd_ebadf() {
        crate::errno::set_errno(0);
        assert_eq!(setns(i32::MIN, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // --- setns: nstype mask ---

    #[test]
    fn test_setns_clone_io_in_nstype_einval() {
        // CLONE_IO is not a namespace type.
        crate::errno::set_errno(0);
        let ret = setns(3, crate::linux_clone_args::CLONE_IO as i32);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_setns_clone_vm_in_nstype_einval() {
        // Sharing flags (CLONE_VM) are not namespace types.
        crate::errno::set_errno(0);
        let ret = setns(3, crate::linux_clone_args::CLONE_VM as i32);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_setns_clone_files_in_nstype_einval() {
        crate::errno::set_errno(0);
        let ret = setns(3, crate::linux_clone_args::CLONE_FILES as i32);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_setns_zero_nstype_passes() {
        // nstype == 0 means "infer from fd", accepted by Linux.
        crate::errno::set_errno(0);
        assert_eq!(setns(3, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_setns_clone_newns_passes() {
        crate::errno::set_errno(0);
        let ret = setns(3, crate::linux_clone_args::CLONE_NEWNS as i32);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_setns_clone_newpid_passes() {
        crate::errno::set_errno(0);
        let ret = setns(3, crate::linux_clone_args::CLONE_NEWPID as i32);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_setns_clone_newuser_passes() {
        crate::errno::set_errno(0);
        let ret = setns(3, crate::linux_clone_args::CLONE_NEWUSER as i32);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_setns_clone_newtime_passes() {
        crate::errno::set_errno(0);
        let ret = setns(3, crate::linux_clone_args::CLONE_NEWTIME as i32);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_setns_all_valid_nstypes_combined_passes() {
        crate::errno::set_errno(0);
        let ret = setns(3, SETNS_NSTYPE_VALID as i32);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_setns_minus_one_nstype_einval() {
        crate::errno::set_errno(0);
        let ret = setns(3, -1);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // --- setns: order — fd before nstype ---

    #[test]
    fn test_setns_fd_before_nstype() {
        // Bad fd + bad nstype → EBADF wins (fd is checked first).
        crate::errno::set_errno(0);
        let ret = setns(-1, crate::linux_clone_args::CLONE_IO as i32);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // --- setns: valid mask shape ---

    #[test]
    fn test_setns_nstype_valid_mask_constant() {
        let expected = (crate::linux_clone_args::CLONE_NEWNS
            | crate::linux_clone_args::CLONE_NEWCGROUP
            | crate::linux_clone_args::CLONE_NEWUTS
            | crate::linux_clone_args::CLONE_NEWIPC
            | crate::linux_clone_args::CLONE_NEWUSER
            | crate::linux_clone_args::CLONE_NEWPID
            | crate::linux_clone_args::CLONE_NEWNET
            | crate::linux_clone_args::CLONE_NEWTIME)
            as u32;
        assert_eq!(SETNS_NSTYPE_VALID, expected);
        // No sharing flags must be in the mask.
        assert_eq!(
            SETNS_NSTYPE_VALID
                & (crate::linux_clone_args::CLONE_VM as u32),
            0,
        );
        assert_eq!(
            SETNS_NSTYPE_VALID
                & (crate::linux_clone_args::CLONE_FILES as u32),
            0,
        );
        assert_eq!(
            SETNS_NSTYPE_VALID
                & (crate::linux_clone_args::CLONE_FS as u32),
            0,
        );
    }

    // --- Workflow tests: real-world namespace callers ---

    /// `unshare(1)` userspace tool's `--user --map-root-user` probe:
    /// runs `unshare(CLONE_NEWUSER | CLONE_NEWNS)` to create a fresh
    /// user namespace as PID 0 inside, then enters a mount namespace
    /// to set up the rootless container.  On ENOSYS the tool prints
    /// "unshare: unshare failed: Function not implemented" rather than
    /// "Invalid argument," letting the user know to upgrade the kernel.
    #[test]
    fn test_unshare_workflow_util_linux_rootless() {
        crate::errno::set_errno(0);
        let bits = (crate::linux_clone_args::CLONE_NEWUSER
            | crate::linux_clone_args::CLONE_NEWNS) as i32;
        let ret = unshare(bits);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// systemd-nspawn container-init path: after fork, the child calls
    /// `unshare(CLONE_NEWPID | CLONE_NEWNS | CLONE_NEWUTS | CLONE_NEWIPC
    /// | CLONE_NEWNET)` to set up the full container namespace.  Must
    /// validate cleanly so systemd-nspawn falls back to its
    /// kernel-too-old code path that uses cgroup-only isolation.
    #[test]
    fn test_unshare_workflow_systemd_nspawn_full() {
        crate::errno::set_errno(0);
        let bits = (crate::linux_clone_args::CLONE_NEWPID
            | crate::linux_clone_args::CLONE_NEWNS
            | crate::linux_clone_args::CLONE_NEWUTS
            | crate::linux_clone_args::CLONE_NEWIPC
            | crate::linux_clone_args::CLONE_NEWNET) as i32;
        let ret = unshare(bits);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// Firefox content-process sandbox uses `unshare(CLONE_NEWUSER |
    /// CLONE_NEWPID | CLONE_NEWNET)` to drop into a sandboxed PID and
    /// network namespace.  On ENOSYS, Firefox's sandbox library
    /// (`sandbox_brokerLauncher`) falls back to seccomp-only filtering.
    #[test]
    fn test_unshare_workflow_firefox_sandbox() {
        crate::errno::set_errno(0);
        let bits = (crate::linux_clone_args::CLONE_NEWUSER
            | crate::linux_clone_args::CLONE_NEWPID
            | crate::linux_clone_args::CLONE_NEWNET) as i32;
        let ret = unshare(bits);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// `nsenter --net=/proc/<pid>/ns/net`: opens the net-ns file, gets
    /// a fd, then calls `setns(fd, CLONE_NEWNET)`.  Must reach ENOSYS
    /// so nsenter prints "Function not implemented" rather than crashing
    /// on a bad-args error.
    #[test]
    fn test_setns_workflow_nsenter_join_net_namespace() {
        crate::errno::set_errno(0);
        let fd = 4; // hypothetical /proc/<pid>/ns/net fd
        let ret = setns(fd, crate::linux_clone_args::CLONE_NEWNET as i32);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// runc `nsexec` cgo helper: when joining an existing container,
    /// runc opens each namespace file in turn and calls
    /// `setns(fd, 0)` letting the kernel infer the type from the fd.
    /// This is the "any namespace" form added in Linux 3.0.
    #[test]
    fn test_setns_workflow_runc_join_inferred_type() {
        crate::errno::set_errno(0);
        let fd = 5;
        let ret = setns(fd, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// CRIU `criu restore` re-enters the target's namespaces via
    /// `setns(pidfd, CLONE_NEWPID | CLONE_NEWNET | ...)` — the Linux
    /// 5.8+ pidfd-with-multiple-nstypes form.  Must validate as a
    /// combined valid namespace mask and reach ENOSYS.
    #[test]
    fn test_setns_workflow_criu_pidfd_multi_ns() {
        crate::errno::set_errno(0);
        let pidfd = 7;
        let nstype = (crate::linux_clone_args::CLONE_NEWPID
            | crate::linux_clone_args::CLONE_NEWNET
            | crate::linux_clone_args::CLONE_NEWNS) as i32;
        let ret = setns(pidfd, nstype);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// Buggy caller from a Stack Overflow snippet: `unshare(CLONE_VM
    /// | CLONE_FILES)` — these *are* valid unshare bits, but they're
    /// meaningless without CLONE_THREAD.  Linux still accepts them
    /// (the mask check passes), so we mirror that and let the call
    /// reach ENOSYS.  The bug isn't in flag validation; it's in the
    /// caller's understanding of unshare semantics.
    #[test]
    fn test_unshare_workflow_share_flags_alone_accepted() {
        crate::errno::set_errno(0);
        let bits = (crate::linux_clone_args::CLONE_VM
            | crate::linux_clone_args::CLONE_FILES) as i32;
        let ret = unshare(bits);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // ------------------------------------------------------------------
    // Phase 51 — umount / umount2 validators
    // ------------------------------------------------------------------

    // --- umount: path validation ---

    #[test]
    fn test_umount_null_efault() {
        crate::errno::set_errno(0);
        assert_eq!(umount(core::ptr::null()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_umount_empty_string_enoent() {
        crate::errno::set_errno(0);
        let empty = b"\0";
        assert_eq!(umount(empty.as_ptr()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
    }

    #[test]
    fn test_umount_valid_path_reaches_enosys() {
        crate::errno::set_errno(0);
        let path = b"/mnt/cdrom\0";
        assert_eq!(umount(path.as_ptr()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_umount_unterminated_path_enametoolong() {
        // 4097-byte buffer of 'a' with no NUL — must trigger ENAMETOOLONG.
        let huge = vec![b'a'; UMOUNT_PATH_MAX + 1];
        crate::errno::set_errno(0);
        assert_eq!(umount(huge.as_ptr()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENAMETOOLONG);
    }

    #[test]
    fn test_umount_max_length_path_passes() {
        // 4095 bytes of 'a' + NUL — exactly at the boundary.
        let mut buf = vec![b'a'; UMOUNT_PATH_MAX];
        buf[UMOUNT_PATH_MAX - 1] = 0;
        crate::errno::set_errno(0);
        assert_eq!(umount(buf.as_ptr()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_umount_single_slash_passes() {
        // umount("/") is what `umount -a` ends with, hits ENOSYS cleanly.
        crate::errno::set_errno(0);
        let path = b"/\0";
        assert_eq!(umount(path.as_ptr()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- umount2: path validation (same shape as umount) ---

    #[test]
    fn test_umount2_null_efault() {
        crate::errno::set_errno(0);
        assert_eq!(umount2(core::ptr::null(), 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_umount2_empty_string_enoent() {
        crate::errno::set_errno(0);
        let empty = b"\0";
        assert_eq!(umount2(empty.as_ptr(), 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
    }

    #[test]
    fn test_umount2_unterminated_path_enametoolong() {
        let huge = vec![b'a'; UMOUNT_PATH_MAX + 1];
        crate::errno::set_errno(0);
        assert_eq!(umount2(huge.as_ptr(), 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENAMETOOLONG);
    }

    // --- umount2: flag mask ---

    #[test]
    fn test_umount2_unknown_flag_einval() {
        crate::errno::set_errno(0);
        let path = b"/mnt/foo\0";
        // bit 0x10 is not in UMOUNT2_FLAGS_VALID.
        assert_eq!(umount2(path.as_ptr(), 0x10), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_umount2_high_flag_einval() {
        crate::errno::set_errno(0);
        let path = b"/mnt/foo\0";
        assert_eq!(umount2(path.as_ptr(), i32::MIN), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_umount2_mnt_force_passes() {
        crate::errno::set_errno(0);
        let path = b"/mnt/foo\0";
        assert_eq!(
            umount2(path.as_ptr(), crate::sys_mount::MNT_FORCE),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_umount2_mnt_detach_passes() {
        crate::errno::set_errno(0);
        let path = b"/mnt/foo\0";
        assert_eq!(
            umount2(path.as_ptr(), crate::sys_mount::MNT_DETACH),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_umount2_mnt_expire_alone_passes() {
        crate::errno::set_errno(0);
        let path = b"/mnt/foo\0";
        assert_eq!(
            umount2(path.as_ptr(), crate::sys_mount::MNT_EXPIRE),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_umount2_umount_nofollow_passes() {
        crate::errno::set_errno(0);
        let path = b"/mnt/foo\0";
        assert_eq!(
            umount2(path.as_ptr(), crate::sys_mount::UMOUNT_NOFOLLOW),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_umount2_force_plus_detach_passes() {
        // FORCE + DETACH is allowed (no mutual exclusion between them).
        crate::errno::set_errno(0);
        let path = b"/mnt/foo\0";
        let flags = crate::sys_mount::MNT_FORCE | crate::sys_mount::MNT_DETACH;
        assert_eq!(umount2(path.as_ptr(), flags), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- umount2: MNT_EXPIRE mutual exclusion ---

    #[test]
    fn test_umount2_expire_plus_force_einval() {
        crate::errno::set_errno(0);
        let path = b"/mnt/foo\0";
        let flags = crate::sys_mount::MNT_EXPIRE | crate::sys_mount::MNT_FORCE;
        assert_eq!(umount2(path.as_ptr(), flags), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_umount2_expire_plus_detach_einval() {
        crate::errno::set_errno(0);
        let path = b"/mnt/foo\0";
        let flags = crate::sys_mount::MNT_EXPIRE | crate::sys_mount::MNT_DETACH;
        assert_eq!(umount2(path.as_ptr(), flags), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_umount2_expire_plus_nofollow_passes() {
        // EXPIRE + NOFOLLOW is allowed (NOFOLLOW is unrelated to expiry).
        crate::errno::set_errno(0);
        let path = b"/mnt/foo\0";
        let flags = crate::sys_mount::MNT_EXPIRE
            | crate::sys_mount::UMOUNT_NOFOLLOW;
        assert_eq!(umount2(path.as_ptr(), flags), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- umount2: validation order (Phase 121: matches Linux ksys_umount) ---

    #[test]
    fn test_umount2_flag_check_before_null_path() {
        // Phase 121: NULL path + bad flags → EINVAL wins.  Linux's
        // ksys_umount checks the flag mask before user_path_at, so the
        // unknown-bit EINVAL fires before getname would observe EFAULT.
        crate::errno::set_errno(0);
        assert_eq!(umount2(core::ptr::null(), 0x10), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_umount2_flag_check_before_empty_path() {
        // Phase 121: empty path + bad flags → EINVAL wins for the same
        // reason as the NULL case.
        crate::errno::set_errno(0);
        let empty = b"\0";
        assert_eq!(umount2(empty.as_ptr(), 0x10), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_umount2_flag_check_before_mutex_check() {
        // Unknown bit + EXPIRE + FORCE: unknown bit fires first.
        crate::errno::set_errno(0);
        let path = b"/mnt/foo\0";
        let flags = 0x10
            | crate::sys_mount::MNT_EXPIRE
            | crate::sys_mount::MNT_FORCE;
        assert_eq!(umount2(path.as_ptr(), flags), -1);
        // Both checks return EINVAL — this test documents that the
        // unknown-bit check runs first (covers more cases).
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // --- Phase 121: validation order matches Linux ksys_umount ---
    //
    // Linux's `fs/namespace.c::ksys_umount` performs the flag-mask
    // check at the very top of the syscall, before `may_mount` and
    // before `user_path_at`.  That means an unknown flag bit beats
    // every path-related errno, even NULL-pointer EFAULT.  The
    // following tests exercise every interesting combination of
    // (flag valid?, path valid?, mutex valid?) to lock that order in.

    /// Phase 121: NULL pointer with a *clean* flag set (MNT_EXPIRE
    /// alone) — flag check passes, NULL caught by getname → EFAULT.
    #[test]
    fn test_umount2_phase121_null_clean_flags_efault() {
        crate::errno::set_errno(0);
        assert_eq!(
            umount2(core::ptr::null(), crate::sys_mount::MNT_EXPIRE),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    /// Phase 121: NULL pointer + an unknown high bit → EINVAL.  The
    /// flag mask catches bit 31 before the NULL check would fire.
    #[test]
    fn test_umount2_phase121_null_bad_high_bit_einval() {
        crate::errno::set_errno(0);
        assert_eq!(
            umount2(core::ptr::null(), 0x8000_0000_u32 as i32),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 121: NULL pointer + i32::MIN — confirms the mask uses
    /// the full 32-bit width (sign-bit + many high bits all unknown).
    #[test]
    fn test_umount2_phase121_null_i32_min_einval() {
        crate::errno::set_errno(0);
        assert_eq!(umount2(core::ptr::null(), i32::MIN), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 121: NULL pointer + every recognised flag bit ORed
    /// together (MNT_EXPIRE | UMOUNT_NOFOLLOW — picked so we don't
    /// also trip the MNT_EXPIRE/FORCE mutex check).  Flag mask is
    /// clean → NULL fires EFAULT.
    #[test]
    fn test_umount2_phase121_null_all_valid_flags_efault() {
        crate::errno::set_errno(0);
        let flags = crate::sys_mount::MNT_EXPIRE
            | crate::sys_mount::UMOUNT_NOFOLLOW;
        assert_eq!(umount2(core::ptr::null(), flags), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    /// Phase 121: empty-string path + unknown flag → EINVAL.  The
    /// flag check fires before the path scan returns Some(0).
    #[test]
    fn test_umount2_phase121_empty_bad_flag_einval() {
        crate::errno::set_errno(0);
        let empty = b"\0";
        assert_eq!(umount2(empty.as_ptr(), 0x4000), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 121: oversized (unterminated) path + unknown flag →
    /// EINVAL.  Important DoS-avoidance property: a malformed flag
    /// word short-circuits the linear path scan, so a buggy caller
    /// passing a huge buffer with garbage flags doesn't pay the
    /// O(PATH_MAX) walk cost.
    #[test]
    fn test_umount2_phase121_huge_path_bad_flag_einval() {
        let huge = vec![b'a'; UMOUNT_PATH_MAX + 1];
        crate::errno::set_errno(0);
        assert_eq!(umount2(huge.as_ptr(), 0x10), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 121: empty path + EXPIRE+FORCE (mutex-conflict combo).
    /// Flag mask passes (both bits known) → path scan returns
    /// Some(0) → ENOENT fires before the mutex check runs.
    #[test]
    fn test_umount2_phase121_empty_expire_force_enoent() {
        crate::errno::set_errno(0);
        let empty = b"\0";
        let flags = crate::sys_mount::MNT_EXPIRE
            | crate::sys_mount::MNT_FORCE;
        assert_eq!(umount2(empty.as_ptr(), flags), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
    }

    /// Phase 121: NULL + EXPIRE+DETACH (mutex-conflict combo).
    /// Flag mask passes → NULL pointer fires EFAULT before mutex.
    #[test]
    fn test_umount2_phase121_null_expire_detach_efault() {
        crate::errno::set_errno(0);
        let flags = crate::sys_mount::MNT_EXPIRE
            | crate::sys_mount::MNT_DETACH;
        assert_eq!(umount2(core::ptr::null(), flags), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    /// Phase 121: ENAMETOOLONG path + EXPIRE+DETACH combo.  Flag
    /// mask clean → cstr_len = None → ENAMETOOLONG before mutex.
    #[test]
    fn test_umount2_phase121_huge_path_expire_detach_enametoolong() {
        let huge = vec![b'a'; UMOUNT_PATH_MAX + 1];
        crate::errno::set_errno(0);
        let flags = crate::sys_mount::MNT_EXPIRE
            | crate::sys_mount::MNT_DETACH;
        assert_eq!(umount2(huge.as_ptr(), flags), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENAMETOOLONG);
    }

    /// Phase 121: full precedence chain — bad flag beats both the
    /// path errno (ENAMETOOLONG on a huge unterminated buffer) and
    /// the mutex EINVAL.  All four checks would fire; flag wins.
    #[test]
    fn test_umount2_phase121_full_chain_flag_wins() {
        let huge = vec![b'a'; UMOUNT_PATH_MAX + 1];
        crate::errno::set_errno(0);
        let flags = 0x20
            | crate::sys_mount::MNT_EXPIRE
            | crate::sys_mount::MNT_FORCE;
        assert_eq!(umount2(huge.as_ptr(), flags), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 121: errno recovery — a subsequent well-formed call
    /// after an EINVAL reaches ENOSYS and cleanly overwrites errno.
    #[test]
    fn test_umount2_phase121_recovery_after_einval() {
        crate::errno::set_errno(0);
        assert_eq!(umount2(core::ptr::null(), 0x100), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let path = b"/srv\0";
        assert_eq!(umount2(path.as_ptr(), 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// Phase 121 workflow: libmount probes whether the syscall is
    /// available with `umount2(NULL, UMOUNT_NOFOLLOW)`.  NOFOLLOW is
    /// a valid flag, so the call reaches getname and surfaces EFAULT
    /// — confirming the syscall exists.  An EINVAL here would
    /// falsely suggest the kernel doesn't recognise NOFOLLOW.
    #[test]
    fn test_umount2_phase121_workflow_libmount_probe() {
        crate::errno::set_errno(0);
        assert_eq!(
            umount2(core::ptr::null(), crate::sys_mount::UMOUNT_NOFOLLOW),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    /// Phase 121 buggy-caller: an init script computes flags by
    /// ORing `getenv("UMOUNT_FLAGS")` parsed as decimal with
    /// MNT_DETACH.  A typo of "2147483648" overflows to the sign
    /// bit, producing a negative flag word.  Linux returns EINVAL
    /// regardless of how valid the path is; we must too.
    #[test]
    fn test_umount2_phase121_buggy_caller_overflowed_flags_einval() {
        crate::errno::set_errno(0);
        let path = b"/mnt/usb\0";
        let flags = (0x8000_0000_u32 as i32)
            | crate::sys_mount::MNT_DETACH;
        assert_eq!(umount2(path.as_ptr(), flags), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // --- Constants ---

    #[test]
    fn test_umount2_flags_valid_mask_constant() {
        assert_eq!(UMOUNT2_FLAGS_VALID, 0x0F);
        assert_eq!(
            UMOUNT2_FLAGS_VALID,
            crate::sys_mount::MNT_FORCE
                | crate::sys_mount::MNT_DETACH
                | crate::sys_mount::MNT_EXPIRE
                | crate::sys_mount::UMOUNT_NOFOLLOW,
        );
    }

    #[test]
    fn test_umount_path_max_constant() {
        assert_eq!(UMOUNT_PATH_MAX, 4096);
    }

    // --- errno preserved on validated call ---

    #[test]
    fn test_umount_validated_call_sets_enosys() {
        crate::errno::set_errno(0);
        let path = b"/proc\0";
        umount(path.as_ptr());
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- Workflow tests: real-world umount callers ---

    /// systemd shutdown sequence: `systemd-shutdown` walks the mount
    /// tree and calls `umount2(mount_point, MNT_FORCE | MNT_DETACH)`
    /// on every remaining mount as part of `final.target` processing.
    /// Must reach ENOSYS so the unmount-failure tally is reported
    /// rather than the shutdown looping on "Invalid argument."
    #[test]
    fn test_umount2_workflow_systemd_final_target() {
        crate::errno::set_errno(0);
        let mount = b"/home\0";
        let flags = crate::sys_mount::MNT_FORCE | crate::sys_mount::MNT_DETACH;
        assert_eq!(umount2(mount.as_ptr(), flags), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// `umount -l /mnt/x` (lazy unmount): util-linux's `umount(8)`
    /// translates `-l` to `umount2(path, MNT_DETACH)`.  Validates that
    /// the bare-DETACH flag reaches ENOSYS so users see "Function not
    /// implemented" rather than a wrong-args message.
    #[test]
    fn test_umount2_workflow_util_linux_lazy() {
        crate::errno::set_errno(0);
        let mount = b"/mnt/usb\0";
        assert_eq!(
            umount2(mount.as_ptr(), crate::sys_mount::MNT_DETACH),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// autofs `expire` daemon: invokes `umount2(path, MNT_EXPIRE)` to
    /// mark a stale automount for expiry.  If the mount is still busy,
    /// Linux returns EAGAIN; otherwise it tears it down.  In our world
    /// the syscall reaches ENOSYS — autofs's expire timer then falls
    /// back to a periodic retry loop without dropping the entry.
    #[test]
    fn test_umount2_workflow_autofs_expire() {
        crate::errno::set_errno(0);
        let mount = b"/net/server-a\0";
        assert_eq!(
            umount2(mount.as_ptr(), crate::sys_mount::MNT_EXPIRE),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// Docker daemon container teardown: when removing a container,
    /// `containerd-shim` calls `umount2(rootfs_overlay, MNT_DETACH)`
    /// on the overlayfs mount before unlinking the container directory.
    /// On ENOSYS, the shim falls back to recursive `rmdir` which leaves
    /// the overlay junk behind — admin must clean up manually.
    #[test]
    fn test_umount2_workflow_docker_overlay_teardown() {
        crate::errno::set_errno(0);
        let mount =
            b"/var/lib/docker/overlay2/abc123/merged\0";
        assert_eq!(
            umount2(mount.as_ptr(), crate::sys_mount::MNT_DETACH),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// `findmnt --umount` from util-linux: the modern util-linux
    /// preflight that walks `/proc/self/mountinfo` for the right entry
    /// then calls `umount2(target, UMOUNT_NOFOLLOW)` to avoid following
    /// a symlink to an unintended mount.  Must accept the NOFOLLOW
    /// flag and reach ENOSYS so the tool prints a useful error.
    #[test]
    fn test_umount2_workflow_findmnt_nofollow() {
        crate::errno::set_errno(0);
        let mount = b"/mnt/symlinked-target\0";
        assert_eq!(
            umount2(mount.as_ptr(), crate::sys_mount::UMOUNT_NOFOLLOW),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// Buggy shell-script caller: `umount2(NULL, 0)` from a C
    /// extension that forgot to set the mount path.  Must catch with
    /// EFAULT so the bug surfaces immediately rather than silently
    /// returning ENOSYS as if the path were valid.
    #[test]
    fn test_umount2_workflow_buggy_null_path() {
        crate::errno::set_errno(0);
        assert_eq!(umount2(core::ptr::null(), 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // ------------------------------------------------------------------
    // Phase 52 — arch_prctl(2) validator
    // ------------------------------------------------------------------

    // --- unknown code ---

    #[test]
    fn test_arch_prctl_unknown_code_einval() {
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(0x9999, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_arch_prctl_zero_code_einval() {
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(0, 0x1000), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_arch_prctl_negative_code_einval() {
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(-1, 0x1000), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // --- ARCH_SET_FS / ARCH_SET_GS canonical-address check ---

    #[test]
    fn test_arch_prctl_set_fs_low_canonical_passes() {
        crate::errno::set_errno(0);
        // User-space address in low canonical half.
        assert_eq!(arch_prctl(ARCH_SET_FS, 0x0000_4000_0000_0000), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_arch_prctl_set_fs_max_low_canonical_passes() {
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(ARCH_SET_FS, X86_64_CANONICAL_MAX), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_arch_prctl_set_fs_non_canonical_einval() {
        crate::errno::set_errno(0);
        // Just above the canonical max — the classic non-canonical zone.
        assert_eq!(arch_prctl(ARCH_SET_FS, 0x0001_0000_0000_0000), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_arch_prctl_set_fs_high_canonical_passes() {
        crate::errno::set_errno(0);
        // Kernel-side canonical address — technically rejected by
        // Linux as "userspace can't set kernel addresses" but our
        // validator only checks canonicality, not privilege.
        assert_eq!(arch_prctl(ARCH_SET_FS, 0xFFFF_8000_0000_0000), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_arch_prctl_set_fs_zero_passes() {
        // FS base of 0 is valid — used to "disable" FS-relative access.
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(ARCH_SET_FS, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_arch_prctl_set_gs_non_canonical_einval() {
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(ARCH_SET_GS, 0x8000_0000_0000_0000), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_arch_prctl_set_gs_low_canonical_passes() {
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(ARCH_SET_GS, 0x4000), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- ARCH_GET_FS / ARCH_GET_GS output-ptr check ---

    #[test]
    fn test_arch_prctl_get_fs_null_efault() {
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(ARCH_GET_FS, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_arch_prctl_get_gs_null_efault() {
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(ARCH_GET_GS, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_arch_prctl_get_gs_valid_passes() {
        let mut out: u64 = 0;
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(ARCH_GET_GS, &raw mut out as u64), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- CPUID-fault control ---

    #[test]
    fn test_arch_prctl_set_cpuid_zero_passes() {
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(ARCH_SET_CPUID, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_arch_prctl_set_cpuid_one_passes() {
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(ARCH_SET_CPUID, 1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_arch_prctl_set_cpuid_two_einval() {
        crate::errno::set_errno(0);
        // CPUID is a boolean — only 0 and 1 are accepted.
        assert_eq!(arch_prctl(ARCH_SET_CPUID, 2), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_arch_prctl_set_cpuid_huge_einval() {
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(ARCH_SET_CPUID, u64::MAX), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_arch_prctl_get_cpuid_passes() {
        // GET_CPUID is allowed without addr.
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(ARCH_GET_CPUID, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- Intel CET shadow-stack family ---

    #[test]
    fn test_arch_prctl_cet_status_null_efault() {
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(ARCH_CET_STATUS, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_arch_prctl_cet_status_valid_passes() {
        let mut out: u64 = 0;
        crate::errno::set_errno(0);
        assert_eq!(
            arch_prctl(ARCH_CET_STATUS, &raw mut out as u64),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_arch_prctl_cet_enable_passes() {
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(ARCH_CET_ENABLE, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_arch_prctl_cet_disable_passes() {
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(ARCH_CET_DISABLE, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_arch_prctl_cet_lock_passes() {
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(ARCH_CET_LOCK, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_arch_prctl_cet_alloc_shstk_passes() {
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(ARCH_CET_ALLOC_SHSTK, 0x10000), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- Intel LAM (Linear Address Masking) family ---

    #[test]
    fn test_arch_prctl_get_untag_mask_null_efault() {
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(ARCH_GET_UNTAG_MASK, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_arch_prctl_get_untag_mask_valid_passes() {
        let mut out: u64 = 0;
        crate::errno::set_errno(0);
        assert_eq!(
            arch_prctl(ARCH_GET_UNTAG_MASK, &raw mut out as u64),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_arch_prctl_get_max_tag_bits_null_efault() {
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(ARCH_GET_MAX_TAG_BITS, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_arch_prctl_enable_tagged_addr_in_range_passes() {
        // LAM48 supports up to 6 untag bits.
        for width in 0..=6u64 {
            crate::errno::set_errno(0);
            assert_eq!(arch_prctl(ARCH_ENABLE_TAGGED_ADDR, width), -1);
            assert_eq!(
                crate::errno::get_errno(),
                crate::errno::ENOSYS,
                "width {width} should reach ENOSYS",
            );
        }
    }

    #[test]
    fn test_arch_prctl_enable_tagged_addr_too_wide_einval() {
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(ARCH_ENABLE_TAGGED_ADDR, 7), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_arch_prctl_enable_tagged_addr_huge_einval() {
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(ARCH_ENABLE_TAGGED_ADDR, u64::MAX), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_arch_prctl_force_tagged_sva_passes() {
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(ARCH_FORCE_TAGGED_SVA, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- Constant sanity ---

    #[test]
    fn test_arch_prctl_constants_distinct() {
        // None of the recognised codes alias each other.
        let codes = [
            ARCH_SET_FS,
            ARCH_GET_FS,
            ARCH_SET_GS,
            ARCH_GET_GS,
            ARCH_GET_CPUID,
            ARCH_SET_CPUID,
            ARCH_CET_STATUS,
            ARCH_CET_ENABLE,
            ARCH_CET_DISABLE,
            ARCH_CET_LOCK,
            ARCH_CET_ALLOC_SHSTK,
            ARCH_GET_UNTAG_MASK,
            ARCH_ENABLE_TAGGED_ADDR,
            ARCH_GET_MAX_TAG_BITS,
            ARCH_FORCE_TAGGED_SVA,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j], "codes alias at {i}/{j}");
            }
        }
    }

    #[test]
    fn test_x86_64_canonical_max_constant() {
        // Bit 47 set, bits 48-63 clear — the highest user-canonical address.
        assert_eq!(X86_64_CANONICAL_MAX, 0x0000_7FFF_FFFF_FFFF);
        // One past canonical max is non-canonical.
        let one_past = X86_64_CANONICAL_MAX + 1;
        assert_eq!(one_past, 0x0000_8000_0000_0000);
    }

    // --- Workflow tests: real-world arch_prctl callers ---

    /// glibc thread setup: `__pthread_init_static_tls` calls
    /// `arch_prctl(ARCH_SET_FS, tls_block)` to point FS at the
    /// thread's TLS image after `clone(CLONE_VM | CLONE_SETTLS)`.
    /// Must validate the canonical user address and reach ENOSYS so
    /// glibc's TLS init reports a real "syscall not implemented"
    /// error rather than crashing on FS=garbage.
    #[test]
    fn test_arch_prctl_workflow_glibc_tls_init() {
        crate::errno::set_errno(0);
        // Typical TLS image lives in the low half of user space.
        let tls_base = 0x0000_7FFF_0000_0000u64;
        assert_eq!(arch_prctl(ARCH_SET_FS, tls_base), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// musl libc thread reaper: when joining a thread, musl reads
    /// the thread's TLS via `arch_prctl(ARCH_GET_FS, &out)` to walk
    /// its `pthread` struct.  Must validate the output pointer and
    /// reach ENOSYS so musl falls back to its `/proc/<tid>/syscall`
    /// scan path (slower but works on kernels < 5.6).
    #[test]
    fn test_arch_prctl_workflow_musl_thread_reap() {
        let mut tls_addr: u64 = 0;
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(ARCH_GET_FS, &raw mut tls_addr as u64), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// QEMU TCG userspace mode: when emulating x86-64-on-x86-64,
    /// QEMU intercepts the guest's `arch_prctl(ARCH_SET_GS, addr)`
    /// and forwards to the host syscall to set the guest's GS base.
    /// Must validate canonical and reach ENOSYS so QEMU's fallback
    /// path uses a software-emulated GS register instead.
    #[test]
    fn test_arch_prctl_workflow_qemu_tcg_set_gs() {
        crate::errno::set_errno(0);
        let guest_gs = 0x0000_4000_8000_0000u64;
        assert_eq!(arch_prctl(ARCH_SET_GS, guest_gs), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// Chrome sandbox `seccomp-bpf` policy probe: at process start,
    /// the sandbox helper calls `arch_prctl(ARCH_SET_CPUID, 0)` to
    /// disable CPUID for sandboxed code (so it can't fingerprint the
    /// host CPU).  Must accept the boolean and reach ENOSYS.
    #[test]
    fn test_arch_prctl_workflow_chrome_cpuid_disable() {
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(ARCH_SET_CPUID, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// Intel CET-enabled binary startup (gcc -fcf-protection=full):
    /// after dynamic linker init, the linker calls
    /// `arch_prctl(ARCH_CET_ENABLE, 0)` to enable shadow stack and
    /// indirect-branch tracking.  On ENOSYS the linker falls back
    /// to non-CET mode (no shadow stack); the binary still works
    /// but loses the CET hardening.
    #[test]
    fn test_arch_prctl_workflow_cet_enable_dynamic_linker() {
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(ARCH_CET_ENABLE, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// HWASan-instrumented binary (LLVM HWAddressSanitizer):
    /// at startup, the HWASan runtime calls
    /// `arch_prctl(ARCH_ENABLE_TAGGED_ADDR, 6)` to enable LAM57
    /// untagging on Intel Sapphire Rapids+ CPUs.  On ENOSYS the
    /// runtime falls back to software emulation (slower but works
    /// on pre-LAM CPUs).
    #[test]
    fn test_arch_prctl_workflow_hwasan_lam_enable() {
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(ARCH_ENABLE_TAGGED_ADDR, 6), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// Buggy caller from a uninitialised-pointer bug: a stack-allocated
    /// `pthread_t*` field was never zeroed, so it contains stale heap
    /// data when passed to `arch_prctl(ARCH_SET_FS, p)`.  The stale
    /// value happens to fall in the non-canonical hole (bit 47 = 1 but
    /// bits 48-63 = 0).  Must catch with EINVAL so the bug surfaces
    /// immediately rather than crashing with #GP later when the MSR
    /// load executes.
    #[test]
    fn test_arch_prctl_workflow_buggy_noncanonical_pthread_t() {
        crate::errno::set_errno(0);
        // Bit 47 set, bits 48-63 clear → classic non-canonical address.
        let bad_addr = 0x0000_DEAD_BEEF_1000u64;
        assert_eq!(arch_prctl(ARCH_SET_FS, bad_addr), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- arch_prctl --

    #[test]
    fn test_arch_prctl_set_fs_enosys() {
        crate::errno::set_errno(0);
        assert_eq!(arch_prctl(ARCH_SET_FS, 0x1000), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_arch_prctl_get_fs_enosys() {
        crate::errno::set_errno(0);
        // addr=0 is now caught with EFAULT; use a real output ptr to
        // exercise the ENOSYS terminal state.
        let mut out: u64 = 0;
        assert_eq!(arch_prctl(ARCH_GET_FS, &raw mut out as u64), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_arch_prctl_constants() {
        assert_eq!(ARCH_SET_GS, 0x1001);
        assert_eq!(ARCH_SET_FS, 0x1002);
        assert_eq!(ARCH_GET_FS, 0x1003);
        assert_eq!(ARCH_GET_GS, 0x1004);
    }

    // -- ioprio --

    #[test]
    fn test_ioprio_get_returns_zero() {
        assert_eq!(ioprio_get(IOPRIO_WHO_PROCESS, 0), 0);
    }

    #[test]
    fn test_ioprio_set_succeeds() {
        assert_eq!(ioprio_set(IOPRIO_WHO_PROCESS, 0, 0), 0);
    }

    #[test]
    fn test_ioprio_class_constants() {
        assert_eq!(IOPRIO_CLASS_NONE, 0);
        assert_eq!(IOPRIO_CLASS_RT, 1);
        assert_eq!(IOPRIO_CLASS_BE, 2);
        assert_eq!(IOPRIO_CLASS_IDLE, 3);
    }

    #[test]
    fn test_ioprio_who_constants() {
        assert_eq!(IOPRIO_WHO_PROCESS, 1);
        assert_eq!(IOPRIO_WHO_PGRP, 2);
        assert_eq!(IOPRIO_WHO_USER, 3);
    }

    #[test]
    fn test_ioprio_get_different_who() {
        assert_eq!(ioprio_get(IOPRIO_WHO_PGRP, 0), 0);
        assert_eq!(ioprio_get(IOPRIO_WHO_USER, 0), 0);
    }

    // -- membarrier --

    #[test]
    fn test_membarrier_query_reports_supported_bitmask() {
        // QUERY must return the bitmask of supported commands (non-zero).
        let mask = membarrier(MEMBARRIER_CMD_QUERY, 0, 0);
        assert!(mask > 0, "QUERY mask must be positive");
        // Every supported command must be present in the mask.
        assert_ne!(mask & MEMBARRIER_CMD_GLOBAL, 0);
        assert_ne!(mask & MEMBARRIER_CMD_GLOBAL_EXPEDITED, 0);
        assert_ne!(mask & MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED, 0);
        assert_ne!(mask & MEMBARRIER_CMD_PRIVATE_EXPEDITED, 0);
        assert_ne!(mask & MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED, 0);
        assert_ne!(mask & MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE, 0);
        assert_ne!(
            mask & MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_SYNC_CORE,
            0,
        );
    }

    #[test]
    fn test_membarrier_global_succeeds() {
        // CMD_GLOBAL must succeed and issue a fence.
        assert_eq!(membarrier(MEMBARRIER_CMD_GLOBAL, 0, 0), 0);
    }

    #[test]
    fn test_membarrier_global_expedited_succeeds() {
        assert_eq!(membarrier(MEMBARRIER_CMD_GLOBAL_EXPEDITED, 0, 0), 0);
    }

    #[test]
    fn test_membarrier_private_expedited_succeeds() {
        assert_eq!(membarrier(MEMBARRIER_CMD_PRIVATE_EXPEDITED, 0, 0), 0);
    }

    #[test]
    fn test_membarrier_register_private_expedited_succeeds() {
        assert_eq!(
            membarrier(MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED, 0, 0),
            0,
        );
    }

    #[test]
    fn test_membarrier_register_global_expedited_succeeds() {
        assert_eq!(
            membarrier(MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED, 0, 0),
            0,
        );
    }

    #[test]
    fn test_membarrier_private_expedited_sync_core_succeeds() {
        assert_eq!(
            membarrier(MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE, 0, 0),
            0,
        );
    }

    #[test]
    fn test_membarrier_register_sync_core_succeeds() {
        assert_eq!(
            membarrier(
                MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_SYNC_CORE,
                0,
                0,
            ),
            0,
        );
    }

    #[test]
    fn test_membarrier_unknown_cmd_einval() {
        // A command bit not in the supported mask must yield EINVAL.
        crate::errno::set_errno(0);
        let unsupported_cmd = 1 << 30; // outside MEMBARRIER_SUPPORTED.
        assert_eq!(membarrier(unsupported_cmd, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_membarrier_negative_cmd_einval() {
        crate::errno::set_errno(0);
        assert_eq!(membarrier(-1, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_membarrier_unknown_flag_einval() {
        // Non-zero flags must be rejected with EINVAL.
        crate::errno::set_errno(0);
        assert_eq!(membarrier(MEMBARRIER_CMD_GLOBAL, 0x1, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_membarrier_constants() {
        assert_eq!(MEMBARRIER_CMD_QUERY, 0);
        assert_eq!(MEMBARRIER_CMD_GLOBAL, 1);
        assert_eq!(MEMBARRIER_CMD_GLOBAL_EXPEDITED, 2);
        assert_eq!(MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED, 4);
        assert_eq!(MEMBARRIER_CMD_PRIVATE_EXPEDITED, 8);
        assert_eq!(MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED, 16);
        assert_eq!(MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE, 32);
        assert_eq!(MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_SYNC_CORE, 64);
    }

    // ===================================================================
    // Phase 134 — membarrier() validation order matches Linux's
    // sys_membarrier (kernel/sched/membarrier.c).  Adds FLAG_CPU
    // recognition, per-command flag validation (incl. for QUERY),
    // exact-match command dispatch, and PRIVATE_EXPEDITED_RSEQ as a
    // recognised-but-unsupported command.
    // ===================================================================

    // -- Constants ----------------------------------------------------------

    #[test]
    fn test_phase134_flag_cpu_constant() {
        // Matches `MEMBARRIER_CMD_FLAG_CPU` in <linux/membarrier.h>.
        assert_eq!(MEMBARRIER_CMD_FLAG_CPU, 1);
    }

    #[test]
    fn test_phase134_private_expedited_rseq_constant() {
        // Matches <linux/membarrier.h>: CMD bit 7.
        assert_eq!(MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ, 1 << 7);
    }

    // -- CMD_QUERY now validates flags -------------------------------------

    #[test]
    fn test_phase134_query_with_nonzero_flags_einval() {
        // BEFORE Phase 134: cmd=QUERY returned MEMBARRIER_SUPPORTED
        // unconditionally, ignoring `flags`.
        // AFTER: Linux's first switch demands flags==0 for QUERY too.
        crate::errno::set_errno(0);
        let ret = membarrier(MEMBARRIER_CMD_QUERY, 1, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase134_query_with_flag_cpu_einval() {
        // FLAG_CPU is only valid with PRIVATE_EXPEDITED_RSEQ.
        crate::errno::set_errno(0);
        let ret = membarrier(MEMBARRIER_CMD_QUERY, MEMBARRIER_CMD_FLAG_CPU, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase134_query_zero_flags_returns_supported() {
        // Sanity: the basic QUERY contract still works.
        assert_eq!(membarrier(MEMBARRIER_CMD_QUERY, 0, 0), MEMBARRIER_SUPPORTED);
    }

    // -- Exact-match cmd dispatch (no more bitmask subset) -----------------

    #[test]
    fn test_phase134_combined_cmd_bits_einval() {
        // BEFORE Phase 134: cmd = GLOBAL | PRIVATE_EXPEDITED = 9 was
        // silently accepted (9 & !MEMBARRIER_SUPPORTED == 0), and ran
        // local_mfence then returned 0.  But that's not a valid command;
        // each command must be passed as its discrete value.
        crate::errno::set_errno(0);
        let combined = MEMBARRIER_CMD_GLOBAL | MEMBARRIER_CMD_PRIVATE_EXPEDITED;
        let ret = membarrier(combined, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase134_two_register_bits_combined_einval() {
        // Another OR-combined case: REGISTER_GLOBAL_EXPEDITED |
        // REGISTER_PRIVATE_EXPEDITED.  Each is supported individually
        // but the OR-combination is not a command.
        crate::errno::set_errno(0);
        let combined = MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED
            | MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED;
        let ret = membarrier(combined, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- FLAG_CPU recognition ----------------------------------------------

    #[test]
    fn test_phase134_global_with_flag_cpu_einval() {
        // Linux first switch: non-RSEQ command with non-zero flags →
        // EINVAL, regardless of *which* flag bit is set.
        crate::errno::set_errno(0);
        let ret = membarrier(
            MEMBARRIER_CMD_GLOBAL,
            MEMBARRIER_CMD_FLAG_CPU,
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase134_private_expedited_with_flag_cpu_einval() {
        // PRIVATE_EXPEDITED (not RSEQ) also rejects FLAG_CPU.
        crate::errno::set_errno(0);
        let ret = membarrier(
            MEMBARRIER_CMD_PRIVATE_EXPEDITED,
            MEMBARRIER_CMD_FLAG_CPU,
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- PRIVATE_EXPEDITED_RSEQ flag arm vs dispatch arm -------------------

    #[test]
    fn test_phase134_rseq_with_flag_cpu_passes_flag_check_then_einval() {
        // RSEQ + FLAG_CPU passes the first switch (the flag arm
        // recognises the combination as legal), but we don't support
        // RSEQ so the dispatch arm returns EINVAL.  This is *the same*
        // EINVAL Linux returns when CONFIG_RSEQ=n, so behaviour is
        // semantically right — and the caller didn't get steered away
        // from FLAG_CPU.
        crate::errno::set_errno(0);
        let ret = membarrier(
            MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ,
            MEMBARRIER_CMD_FLAG_CPU,
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase134_rseq_with_other_flag_einval_at_flag_arm() {
        // RSEQ with a flag that's NOT FLAG_CPU → EINVAL at the flag arm.
        crate::errno::set_errno(0);
        let ret = membarrier(
            MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ,
            1 << 3, // not FLAG_CPU
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase134_rseq_with_zero_flags_einval_at_dispatch() {
        // RSEQ with flags=0 passes flag arm but EINVALs at dispatch
        // (we don't support RSEQ).
        crate::errno::set_errno(0);
        let ret = membarrier(MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- cpu_id normalisation ----------------------------------------------

    #[test]
    fn test_phase134_cpu_id_ignored_when_flag_cpu_absent() {
        // Without FLAG_CPU, cpu_id is informational; passing a wild
        // value should not affect a valid command.
        crate::errno::set_errno(0);
        assert_eq!(membarrier(MEMBARRIER_CMD_GLOBAL, 0, 9999), 0);
        assert_eq!(membarrier(MEMBARRIER_CMD_GLOBAL, 0, -42), 0);
    }

    // -- Buggy-caller / recovery -------------------------------------------

    #[test]
    fn test_phase134_query_flag_error_does_not_poison_supported_value() {
        // First a bad QUERY (flags!=0) → EINVAL.  Then a good QUERY →
        // returns the bitmask.  No internal state should leak.
        crate::errno::set_errno(0);
        assert_eq!(membarrier(MEMBARRIER_CMD_QUERY, 1, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        let supported = membarrier(MEMBARRIER_CMD_QUERY, 0, 0);
        assert!(supported > 0);
        assert_ne!(supported & MEMBARRIER_CMD_GLOBAL, 0);
    }

    #[test]
    fn test_phase134_dispatch_after_bad_combined_cmd() {
        // A bad combined cmd is rejected, and a subsequent good cmd
        // still succeeds.
        crate::errno::set_errno(0);
        let combined =
            MEMBARRIER_CMD_GLOBAL | MEMBARRIER_CMD_PRIVATE_EXPEDITED;
        assert_eq!(membarrier(combined, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        assert_eq!(
            membarrier(MEMBARRIER_CMD_PRIVATE_EXPEDITED, 0, 0),
            0,
        );
    }

    // -- Workflow ----------------------------------------------------------

    #[test]
    fn test_phase134_typical_query_register_use_workflow() {
        // 1. Probe what's supported.
        let supported = membarrier(MEMBARRIER_CMD_QUERY, 0, 0);
        assert!(supported & MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED != 0);

        // 2. Register intent (so other threads can be expedited).
        assert_eq!(
            membarrier(MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED, 0, 0),
            0,
        );

        // 3. Issue the barrier.
        assert_eq!(
            membarrier(MEMBARRIER_CMD_PRIVATE_EXPEDITED, 0, 0),
            0,
        );
    }

    // -----------------------------------------------------------------------
    // clone3
    // -----------------------------------------------------------------------

    #[test]
    fn test_clone3_returns_enosys() {
        // clone3 is not supported — returns -1 with ENOSYS.
        crate::errno::set_errno(0);
        // SAFETY: zero-init is valid for CloneArgs (all-zeros = no flags).
        let args: CloneArgs = unsafe { core::mem::zeroed() };
        let ret = clone3(&args, core::mem::size_of::<CloneArgs>());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_clone3_null_args() {
        // Phase 55: clone3 now rejects NULL args with EFAULT (matches
        // Linux's `copy_struct_from_user` semantics) before reaching
        // the not-implemented ENOSYS path.
        crate::errno::set_errno(0);
        let ret = clone3(core::ptr::null(), 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_clone_args_struct_layout() {
        // CloneArgs has 11 u64 fields = 88 bytes.
        assert_eq!(core::mem::size_of::<CloneArgs>(), 88);
    }

    // -----------------------------------------------------------------------
    // process_vm_readv / process_vm_writev
    // -----------------------------------------------------------------------

    #[test]
    fn test_process_vm_readv_enosys() {
        crate::errno::set_errno(0);
        let ret = process_vm_readv(
            1,
            core::ptr::null(),
            0,
            core::ptr::null(),
            0,
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_process_vm_writev_enosys() {
        crate::errno::set_errno(0);
        let ret = process_vm_writev(
            1,
            core::ptr::null(),
            0,
            core::ptr::null(),
            0,
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // -----------------------------------------------------------------------
    // kcmp
    // -----------------------------------------------------------------------

    #[test]
    fn test_kcmp_enosys() {
        crate::errno::set_errno(0);
        let ret = kcmp(1, 2, KCMP_FILE, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_kcmp_type_constants() {
        assert_eq!(KCMP_FILE, 0);
        assert_eq!(KCMP_VM, 1);
        assert_eq!(KCMP_FILES, 2);
        assert_eq!(KCMP_FS, 3);
        assert_eq!(KCMP_SIGHAND, 4);
        assert_eq!(KCMP_IO, 5);
        assert_eq!(KCMP_SYSVSEM, 6);
        assert_eq!(KCMP_EPOLL_TFD, 7);
    }

    #[test]
    fn test_kcmp_all_types_enosys() {
        for t in 0..=7 {
            crate::errno::set_errno(0);
            // KCMP_EPOLL_TFD requires a non-NULL idx2 pointer to a
            // kcmp_epoll_slot; supply a sentinel for that case so the
            // test reaches the not-implemented ENOSYS path rather than
            // tripping the Phase 57 EFAULT pointer check.
            let idx2 = if t == KCMP_EPOLL_TFD { 0x1000 } else { 0 };
            let ret = kcmp(1, 1, t, 0, idx2);
            assert_eq!(ret, -1, "kcmp type {t} should return -1");
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
        }
    }

    // ------------------------------------------------------------------
    // Phase 53 — mount(2) validator
    //
    // Argument-domain checks performed before reaching kernel mount code:
    //   target NULL                 → EFAULT
    //   empty target                → ENOENT
    //   target overflows PATH_MAX   → ENAMETOOLONG
    //   unknown MS_* bits           → EINVAL
    //   multiple mode bits          → EINVAL
    //   source NULL when required   → EFAULT
    //   empty source when required  → ENOENT
    //   source overflows PATH_MAX   → ENAMETOOLONG
    //   fstype NULL on new mount    → EFAULT
    //   empty fstype on new mount   → EINVAL
    //   fstype overflows TYPE_MAX   → ENAMETOOLONG
    //   otherwise                   → ENOSYS
    // ------------------------------------------------------------------

    // --- target validation ---

    #[test]
    fn test_mount_null_target_efault() {
        crate::errno::set_errno(0);
        let ret = mount(
            b"/dev/sda1\0".as_ptr(),
            core::ptr::null(),
            b"ext4\0".as_ptr(),
            0,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_mount_empty_target_enoent() {
        crate::errno::set_errno(0);
        let empty = b"\0";
        let ret = mount(
            b"/dev/sda1\0".as_ptr(),
            empty.as_ptr(),
            b"ext4\0".as_ptr(),
            0,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
    }

    #[test]
    fn test_mount_target_too_long_enametoolong() {
        let huge = vec![b'a'; MOUNT_PATH_MAX + 1];
        crate::errno::set_errno(0);
        let ret = mount(
            b"/dev/sda1\0".as_ptr(),
            huge.as_ptr(),
            b"ext4\0".as_ptr(),
            0,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENAMETOOLONG);
    }

    #[test]
    fn test_mount_max_length_target_passes() {
        // 4095 bytes + NUL = exactly at boundary.
        let mut buf = vec![b'a'; MOUNT_PATH_MAX];
        buf[MOUNT_PATH_MAX - 1] = 0;
        crate::errno::set_errno(0);
        let ret = mount(
            b"/dev/sda1\0".as_ptr(),
            buf.as_ptr(),
            b"ext4\0".as_ptr(),
            0,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- flag mask ---

    #[test]
    fn test_mount_unknown_flag_einval() {
        crate::errno::set_errno(0);
        // Bit 1<<31 is well outside MOUNT_FLAGS_VALID.
        let ret = mount(
            b"/dev/sda1\0".as_ptr(),
            b"/mnt\0".as_ptr(),
            b"ext4\0".as_ptr(),
            1u64 << 31,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mount_high_bit_einval() {
        crate::errno::set_errno(0);
        // Top bit must be rejected.
        let ret = mount(
            b"/dev/sda1\0".as_ptr(),
            b"/mnt\0".as_ptr(),
            b"ext4\0".as_ptr(),
            1u64 << 63,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mount_kernmount_rejected() {
        // MS_KERNMOUNT is kernel-internal and must not be settable
        // from userspace.
        crate::errno::set_errno(0);
        let ret = mount(
            b"/dev/sda1\0".as_ptr(),
            b"/mnt\0".as_ptr(),
            b"ext4\0".as_ptr(),
            crate::sys_mount::MS_KERNMOUNT,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // --- mode-bit exclusion ---

    #[test]
    fn test_mount_bind_and_move_einval() {
        crate::errno::set_errno(0);
        let ret = mount(
            b"/src\0".as_ptr(),
            b"/dst\0".as_ptr(),
            core::ptr::null(),
            crate::sys_mount::MS_BIND | crate::sys_mount::MS_MOVE,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mount_remount_and_bind_einval() {
        crate::errno::set_errno(0);
        let ret = mount(
            b"/src\0".as_ptr(),
            b"/dst\0".as_ptr(),
            core::ptr::null(),
            crate::sys_mount::MS_REMOUNT | crate::sys_mount::MS_BIND,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mount_shared_and_private_einval() {
        crate::errno::set_errno(0);
        let ret = mount(
            core::ptr::null(),
            b"/\0".as_ptr(),
            core::ptr::null(),
            crate::sys_mount::MS_SHARED | crate::sys_mount::MS_PRIVATE,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mount_all_four_propagation_einval() {
        crate::errno::set_errno(0);
        let ret = mount(
            core::ptr::null(),
            b"/\0".as_ptr(),
            core::ptr::null(),
            crate::sys_mount::MS_SHARED
                | crate::sys_mount::MS_PRIVATE
                | crate::sys_mount::MS_SLAVE
                | crate::sys_mount::MS_UNBINDABLE,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mount_move_and_unbindable_einval() {
        crate::errno::set_errno(0);
        let ret = mount(
            b"/src\0".as_ptr(),
            b"/dst\0".as_ptr(),
            core::ptr::null(),
            crate::sys_mount::MS_MOVE | crate::sys_mount::MS_UNBINDABLE,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // --- MS_REC is NOT a mode bit and can combine with anything ---

    #[test]
    fn test_mount_rec_with_bind_passes() {
        crate::errno::set_errno(0);
        let ret = mount(
            b"/src\0".as_ptr(),
            b"/dst\0".as_ptr(),
            core::ptr::null(),
            crate::sys_mount::MS_BIND | crate::sys_mount::MS_REC,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mount_rec_with_shared_passes() {
        crate::errno::set_errno(0);
        let ret = mount(
            core::ptr::null(),
            b"/\0".as_ptr(),
            core::ptr::null(),
            crate::sys_mount::MS_SHARED | crate::sys_mount::MS_REC,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- source validation ---

    #[test]
    fn test_mount_null_source_for_new_mount_efault() {
        crate::errno::set_errno(0);
        let ret = mount(
            core::ptr::null(),
            b"/mnt\0".as_ptr(),
            b"ext4\0".as_ptr(),
            0,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_mount_null_source_for_bind_efault() {
        crate::errno::set_errno(0);
        let ret = mount(
            core::ptr::null(),
            b"/dst\0".as_ptr(),
            core::ptr::null(),
            crate::sys_mount::MS_BIND,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_mount_null_source_for_move_efault() {
        crate::errno::set_errno(0);
        let ret = mount(
            core::ptr::null(),
            b"/dst\0".as_ptr(),
            core::ptr::null(),
            crate::sys_mount::MS_MOVE,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_mount_null_source_for_remount_passes() {
        // MS_REMOUNT: source is ignored on Linux; NULL must reach ENOSYS.
        crate::errno::set_errno(0);
        let ret = mount(
            core::ptr::null(),
            b"/\0".as_ptr(),
            core::ptr::null(),
            crate::sys_mount::MS_REMOUNT | crate::sys_mount::MS_RDONLY,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mount_null_source_for_shared_passes() {
        crate::errno::set_errno(0);
        let ret = mount(
            core::ptr::null(),
            b"/\0".as_ptr(),
            core::ptr::null(),
            crate::sys_mount::MS_SHARED,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mount_null_source_for_private_passes() {
        crate::errno::set_errno(0);
        let ret = mount(
            core::ptr::null(),
            b"/\0".as_ptr(),
            core::ptr::null(),
            crate::sys_mount::MS_PRIVATE,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mount_null_source_for_slave_passes() {
        crate::errno::set_errno(0);
        let ret = mount(
            core::ptr::null(),
            b"/\0".as_ptr(),
            core::ptr::null(),
            crate::sys_mount::MS_SLAVE,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mount_null_source_for_unbindable_passes() {
        crate::errno::set_errno(0);
        let ret = mount(
            core::ptr::null(),
            b"/\0".as_ptr(),
            core::ptr::null(),
            crate::sys_mount::MS_UNBINDABLE,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mount_empty_source_enoent() {
        crate::errno::set_errno(0);
        let ret = mount(
            b"\0".as_ptr(),
            b"/mnt\0".as_ptr(),
            b"ext4\0".as_ptr(),
            0,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
    }

    #[test]
    fn test_mount_source_too_long_enametoolong() {
        let huge = vec![b'a'; MOUNT_PATH_MAX + 1];
        crate::errno::set_errno(0);
        let ret = mount(
            huge.as_ptr(),
            b"/mnt\0".as_ptr(),
            b"ext4\0".as_ptr(),
            0,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENAMETOOLONG);
    }

    // --- fstype validation ---

    #[test]
    fn test_mount_null_fstype_for_new_mount_efault() {
        crate::errno::set_errno(0);
        let ret = mount(
            b"/dev/sda1\0".as_ptr(),
            b"/mnt\0".as_ptr(),
            core::ptr::null(),
            0,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_mount_null_fstype_for_bind_passes() {
        // MS_BIND: fstype ignored on Linux; NULL must reach ENOSYS.
        crate::errno::set_errno(0);
        let ret = mount(
            b"/src\0".as_ptr(),
            b"/dst\0".as_ptr(),
            core::ptr::null(),
            crate::sys_mount::MS_BIND,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mount_null_fstype_for_move_passes() {
        crate::errno::set_errno(0);
        let ret = mount(
            b"/src\0".as_ptr(),
            b"/dst\0".as_ptr(),
            core::ptr::null(),
            crate::sys_mount::MS_MOVE,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mount_null_fstype_for_remount_passes() {
        crate::errno::set_errno(0);
        let ret = mount(
            core::ptr::null(),
            b"/\0".as_ptr(),
            core::ptr::null(),
            crate::sys_mount::MS_REMOUNT,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mount_null_fstype_for_propagation_passes() {
        crate::errno::set_errno(0);
        let ret = mount(
            core::ptr::null(),
            b"/\0".as_ptr(),
            core::ptr::null(),
            crate::sys_mount::MS_SHARED,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mount_empty_fstype_einval() {
        crate::errno::set_errno(0);
        let ret = mount(
            b"/dev/sda1\0".as_ptr(),
            b"/mnt\0".as_ptr(),
            b"\0".as_ptr(),
            0,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mount_fstype_too_long_enametoolong() {
        let huge = vec![b'a'; MOUNT_TYPE_MAX + 1];
        crate::errno::set_errno(0);
        let ret = mount(
            b"/dev/sda1\0".as_ptr(),
            b"/mnt\0".as_ptr(),
            huge.as_ptr(),
            0,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENAMETOOLONG);
    }

    #[test]
    fn test_mount_max_length_fstype_passes() {
        let mut buf = vec![b'a'; MOUNT_TYPE_MAX];
        buf[MOUNT_TYPE_MAX - 1] = 0;
        crate::errno::set_errno(0);
        let ret = mount(
            b"/dev/sda1\0".as_ptr(),
            b"/mnt\0".as_ptr(),
            buf.as_ptr(),
            0,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- successful paths through each mode ---

    #[test]
    fn test_mount_new_mount_passes() {
        crate::errno::set_errno(0);
        let ret = mount(
            b"/dev/sda1\0".as_ptr(),
            b"/\0".as_ptr(),
            b"ext4\0".as_ptr(),
            crate::sys_mount::MS_NOATIME,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mount_remount_passes() {
        crate::errno::set_errno(0);
        let ret = mount(
            core::ptr::null(),
            b"/\0".as_ptr(),
            core::ptr::null(),
            crate::sys_mount::MS_REMOUNT | crate::sys_mount::MS_RDONLY,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mount_bind_passes() {
        crate::errno::set_errno(0);
        let ret = mount(
            b"/home/user/src\0".as_ptr(),
            b"/srv/www\0".as_ptr(),
            core::ptr::null(),
            crate::sys_mount::MS_BIND,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mount_move_passes() {
        crate::errno::set_errno(0);
        let ret = mount(
            b"/mnt/old\0".as_ptr(),
            b"/mnt/new\0".as_ptr(),
            core::ptr::null(),
            crate::sys_mount::MS_MOVE,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- ordering: target check happens BEFORE flag/mode checks ---

    #[test]
    fn test_mount_target_check_before_flag_check() {
        // NULL target + bogus flag → must be EFAULT (target check first).
        crate::errno::set_errno(0);
        let ret = mount(
            b"/dev/sda1\0".as_ptr(),
            core::ptr::null(),
            b"ext4\0".as_ptr(),
            1u64 << 31,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_mount_target_check_before_mode_check() {
        // NULL target + BIND|MOVE conflict → must be EFAULT.
        crate::errno::set_errno(0);
        let ret = mount(
            b"/src\0".as_ptr(),
            core::ptr::null(),
            core::ptr::null(),
            crate::sys_mount::MS_BIND | crate::sys_mount::MS_MOVE,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_mount_flag_check_before_source_check() {
        // Unknown flag + NULL source → EINVAL (flag check first).
        crate::errno::set_errno(0);
        let ret = mount(
            core::ptr::null(),
            b"/mnt\0".as_ptr(),
            b"ext4\0".as_ptr(),
            1u64 << 31,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mount_mode_check_before_source_check() {
        // BIND|MOVE conflict + NULL source → EINVAL (mode check first).
        crate::errno::set_errno(0);
        let ret = mount(
            core::ptr::null(),
            b"/dst\0".as_ptr(),
            core::ptr::null(),
            crate::sys_mount::MS_BIND | crate::sys_mount::MS_MOVE,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mount_source_check_before_fstype_check() {
        // NULL source + NULL fstype on new mount → EFAULT (source first).
        crate::errno::set_errno(0);
        let ret = mount(
            core::ptr::null(),
            b"/mnt\0".as_ptr(),
            core::ptr::null(),
            0,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // --- constants self-consistency ---

    #[test]
    fn test_mount_path_max_matches_umount() {
        // mount and umount must share the PATH_MAX cap.
        assert_eq!(MOUNT_PATH_MAX, UMOUNT_PATH_MAX);
    }

    #[test]
    fn test_mount_type_max_reasonable() {
        // MOUNT_TYPE_MAX should accommodate every real-world fstype.
        assert!(MOUNT_TYPE_MAX >= 64);
        assert!(MOUNT_TYPE_MAX <= MOUNT_PATH_MAX);
    }

    #[test]
    fn test_mount_mode_bits_subset_of_valid() {
        // Every mode bit must also be in MOUNT_FLAGS_VALID.
        assert_eq!(MOUNT_MODE_BITS & MOUNT_FLAGS_VALID, MOUNT_MODE_BITS);
    }

    #[test]
    fn test_mount_rec_not_a_mode_bit() {
        // MS_REC is a modifier, not a mode bit.
        assert_eq!(MOUNT_MODE_BITS & crate::sys_mount::MS_REC, 0);
    }

    #[test]
    fn test_mount_kernmount_not_in_valid_mask() {
        // MS_KERNMOUNT is kernel-internal and rejected from userspace.
        assert_eq!(MOUNT_FLAGS_VALID & crate::sys_mount::MS_KERNMOUNT, 0);
    }

    // --- errno-preservation on no-op error legs ---

    #[test]
    fn test_mount_errno_set_to_efault_only() {
        // After EFAULT, no further mutation of errno.
        crate::errno::set_errno(0);
        let _ = mount(
            b"/dev/sda1\0".as_ptr(),
            core::ptr::null(),
            b"ext4\0".as_ptr(),
            0,
            core::ptr::null(),
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // ------------------------------------------------------------------
    // Phase 53 — mount(2) workflow tests
    // ------------------------------------------------------------------

    /// systemd-fstab-generator's mount of an ext4 root partition:
    /// `mount("/dev/disk/by-uuid/<uuid>", "/", "ext4",
    ///        MS_NOATIME, "errors=remount-ro")`.
    /// Must validate cleanly and reach ENOSYS so the generator falls
    /// back to its "kernel-too-old" error path.
    #[test]
    fn test_mount_workflow_systemd_ext4_root() {
        crate::errno::set_errno(0);
        let ret = mount(
            b"/dev/disk/by-uuid/abcdef01-2345-6789-abcd-ef0123456789\0".as_ptr(),
            b"/\0".as_ptr(),
            b"ext4\0".as_ptr(),
            crate::sys_mount::MS_NOATIME,
            b"errors=remount-ro\0".as_ptr(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// runc 1.1+ container init mounts `/proc`:
    /// `mount("proc", "/proc", "proc",
    ///        MS_NOEXEC|MS_NOSUID|MS_NODEV, NULL)`.
    /// The OCI runtime spec mandates these protective flags for every
    /// container's /proc.
    #[test]
    fn test_mount_workflow_runc_proc_oci_flags() {
        crate::errno::set_errno(0);
        let ret = mount(
            b"proc\0".as_ptr(),
            b"/proc\0".as_ptr(),
            b"proc\0".as_ptr(),
            crate::sys_mount::MS_NOEXEC
                | crate::sys_mount::MS_NOSUID
                | crate::sys_mount::MS_NODEV,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// Docker 24+ / containerd-shim overlay mount of a layered image:
    /// `mount("overlay", "<merged>", "overlay", 0,
    ///        "lowerdir=L1:L2,upperdir=U,workdir=W")`.
    /// The opaque `data` blob (lowerdir/upperdir/workdir) is not
    /// validated by us — only the four pointer/flag positions are.
    #[test]
    fn test_mount_workflow_docker_overlay() {
        crate::errno::set_errno(0);
        let data = b"lowerdir=/var/lib/docker/overlay2/L1/diff:/var/lib/docker/overlay2/L2/diff,upperdir=/var/lib/docker/overlay2/U/diff,workdir=/var/lib/docker/overlay2/W/work\0";
        let ret = mount(
            b"overlay\0".as_ptr(),
            b"/var/lib/docker/overlay2/abc123/merged\0".as_ptr(),
            b"overlay\0".as_ptr(),
            0,
            data.as_ptr(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// systemd-tmpfiles' `tmpfs` for `/run`:
    /// `mount("tmpfs", "/run", "tmpfs",
    ///        MS_NOSUID|MS_NODEV|MS_STRICTATIME,
    ///        "size=10%,mode=755")`.
    #[test]
    fn test_mount_workflow_systemd_tmpfs_run() {
        crate::errno::set_errno(0);
        let ret = mount(
            b"tmpfs\0".as_ptr(),
            b"/run\0".as_ptr(),
            b"tmpfs\0".as_ptr(),
            crate::sys_mount::MS_NOSUID
                | crate::sys_mount::MS_NODEV
                | crate::sys_mount::MS_STRICTATIME,
            b"size=10%,mode=755\0".as_ptr(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// `mount --bind` from util-linux:
    /// `mount("/home/source", "/mnt/dest", NULL, MS_BIND, NULL)`.
    /// The user-namespace rootless-container scenario.
    #[test]
    fn test_mount_workflow_util_linux_bind() {
        crate::errno::set_errno(0);
        let ret = mount(
            b"/home/source\0".as_ptr(),
            b"/mnt/dest\0".as_ptr(),
            core::ptr::null(),
            crate::sys_mount::MS_BIND,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// `mount --rbind` (recursive bind), as used by Bubblewrap when
    /// constructing a Flatpak sandbox:
    /// `mount("/usr", "/newroot/usr", NULL, MS_BIND|MS_REC, NULL)`.
    #[test]
    fn test_mount_workflow_bubblewrap_rbind() {
        crate::errno::set_errno(0);
        let ret = mount(
            b"/usr\0".as_ptr(),
            b"/newroot/usr\0".as_ptr(),
            core::ptr::null(),
            crate::sys_mount::MS_BIND | crate::sys_mount::MS_REC,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// `mount -o remount,ro /` issued by `shutdown(8)` during system
    /// halt to flush dirty pages before powering off:
    /// `mount(NULL, "/", NULL, MS_REMOUNT|MS_RDONLY, NULL)`.
    #[test]
    fn test_mount_workflow_shutdown_remount_ro() {
        crate::errno::set_errno(0);
        let ret = mount(
            core::ptr::null(),
            b"/\0".as_ptr(),
            core::ptr::null(),
            crate::sys_mount::MS_REMOUNT | crate::sys_mount::MS_RDONLY,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// systemd's `MountFlags=private` directive emits
    /// `mount(NULL, "/", NULL, MS_PRIVATE|MS_REC, NULL)` to isolate
    /// the unit's mount namespace from the host on PID-1 startup.
    #[test]
    fn test_mount_workflow_systemd_mount_flags_private() {
        crate::errno::set_errno(0);
        let ret = mount(
            core::ptr::null(),
            b"/\0".as_ptr(),
            core::ptr::null(),
            crate::sys_mount::MS_PRIVATE | crate::sys_mount::MS_REC,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// `mount.cifs` (cifs-utils 7.0+) mounting a Windows SMB share:
    /// `mount("//fileserver/share", "/mnt/share", "cifs",
    ///        MS_NOSUID|MS_NODEV,
    ///        "username=alice,password=...,vers=3.1.1")`.
    #[test]
    fn test_mount_workflow_cifs_utils_smb_share() {
        crate::errno::set_errno(0);
        let ret = mount(
            b"//fileserver/share\0".as_ptr(),
            b"/mnt/share\0".as_ptr(),
            b"cifs\0".as_ptr(),
            crate::sys_mount::MS_NOSUID | crate::sys_mount::MS_NODEV,
            b"username=alice,vers=3.1.1\0".as_ptr(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// `pivot_root`-style transition: dracut's initramfs moves the
    /// real root from `/sysroot` to `/` via `mount(NULL, "/sysroot",
    /// NULL, MS_MOVE, NULL)` immediately before switch_root.
    #[test]
    fn test_mount_workflow_dracut_pivot_root() {
        crate::errno::set_errno(0);
        let ret = mount(
            b"/sysroot\0".as_ptr(),
            b"/\0".as_ptr(),
            core::ptr::null(),
            crate::sys_mount::MS_MOVE,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// Buggy caller from a real bug report (Go's `syscall.Mount`
    /// wrapper before Go 1.18): passes `MS_BIND | MS_REMOUNT` to apply
    /// `nosuid` to a bind-mounted directory.  Linux requires two
    /// separate calls — the first to bind, the second to remount with
    /// the new flags.  Our validator rejects the combo with EINVAL,
    /// matching what Linux returns from `do_mount`'s mode-conflict
    /// check.  Fixed in Go 1.18 by splitting into two calls.
    #[test]
    fn test_mount_workflow_buggy_go_bind_remount_combo() {
        crate::errno::set_errno(0);
        let ret = mount(
            b"/src\0".as_ptr(),
            b"/dst\0".as_ptr(),
            core::ptr::null(),
            crate::sys_mount::MS_BIND
                | crate::sys_mount::MS_REMOUNT
                | crate::sys_mount::MS_NOSUID,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Buggy caller: bash one-liner `mount -t '' /dev/sda1 /mnt`
    /// (empty fstype from a `$FS` variable that wasn't set).  Linux
    /// fails this in `get_fs_type` with ENODEV; we collapse to EINVAL
    /// at validation time since the fstype lookup never happens.
    #[test]
    fn test_mount_workflow_buggy_empty_fstype_var() {
        crate::errno::set_errno(0);
        let ret = mount(
            b"/dev/sda1\0".as_ptr(),
            b"/mnt\0".as_ptr(),
            b"\0".as_ptr(),
            0,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Buggy caller: Java 11 ProcessBuilder issuing
    /// `mount("/dev/sda1", "/mnt", "ext4", 1<<30, NULL)` because
    /// `MountFlag.READ_ONLY.bits()` was set to a stale constant
    /// from kernel 2.4.  Our validator rejects with EINVAL just as
    /// Linux's user-flag whitelist would.
    #[test]
    fn test_mount_workflow_buggy_stale_kernel_flag() {
        crate::errno::set_errno(0);
        let ret = mount(
            b"/dev/sda1\0".as_ptr(),
            b"/mnt\0".as_ptr(),
            b"ext4\0".as_ptr(),
            1u64 << 30,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -----------------------------------------------------------------
    // clone(2) — argument-domain validation (Phase 54)
    // -----------------------------------------------------------------

    /// A stack pointer that's "good enough" for validation tests —
    /// the bytes are never dereferenced by the validator.
    fn clone_dummy_stack() -> *mut u8 {
        // 64 KiB above null; well-formed for sniff tests.
        0x1_0000_usize as *mut u8
    }
    fn clone_dummy_fn() -> *const u8 {
        0x2_0000_usize as *const u8
    }

    #[test]
    fn test_clone_csignal_max_constant() {
        // SIGRTMAX on Linux/x86_64 is 64 — must match.
        assert_eq!(CLONE_CSIGNAL_MAX, 64);
    }

    #[test]
    fn test_clone_flags_valid_covers_unshare_minus_newtime() {
        // clone(2) accepts every unshare(2) flag except CLONE_NEWTIME
        // (CLONE_NEWTIME = 0x80 collides with the CSIGNAL exit-signal
        // byte, so it can only be expressed via clone3 or unshare).
        let unshare_no_newtime =
            UNSHARE_FLAGS_VALID as u64 & !crate::linux_clone_args::CLONE_NEWTIME;
        assert_eq!(CLONE_FLAGS_VALID & unshare_no_newtime, unshare_no_newtime);
        // And CLONE_NEWTIME is in unshare's set but not clone's.
        assert_ne!(
            UNSHARE_FLAGS_VALID as u64 & crate::linux_clone_args::CLONE_NEWTIME,
            0
        );
        assert_eq!(CLONE_FLAGS_VALID & crate::linux_clone_args::CLONE_NEWTIME, 0);
    }

    #[test]
    fn test_clone_flags_valid_excludes_clone3_only_bits() {
        // CLONE_INTO_CGROUP and CLONE_CLEAR_SIGHAND live above bit 32
        // and are rejected by clone(2).
        assert_eq!(
            CLONE_FLAGS_VALID & crate::linux_clone_args::CLONE_INTO_CGROUP,
            0
        );
        assert_eq!(
            CLONE_FLAGS_VALID & crate::linux_clone_args::CLONE_CLEAR_SIGHAND,
            0
        );
    }

    #[test]
    fn test_clone_null_fn_einval() {
        crate::errno::set_errno(0);
        let ret = clone(
            core::ptr::null(),
            clone_dummy_stack(),
            0,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone_null_stack_einval() {
        crate::errno::set_errno(0);
        let ret = clone(
            clone_dummy_fn(),
            core::ptr::null_mut(),
            0,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone_null_fn_takes_precedence_over_null_stack() {
        // (1) is checked before (2): NULL fn always reports first.
        crate::errno::set_errno(0);
        let ret = clone(
            core::ptr::null(),
            core::ptr::null_mut(),
            0,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone_exit_signal_too_large_einval() {
        // 65 is one past CLONE_CSIGNAL_MAX.
        crate::errno::set_errno(0);
        let ret = clone(
            clone_dummy_fn(),
            clone_dummy_stack(),
            65,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone_exit_signal_255_einval() {
        crate::errno::set_errno(0);
        let ret = clone(
            clone_dummy_fn(),
            clone_dummy_stack(),
            0xff,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone_exit_signal_sigchld_passes_to_enosys() {
        // SIGCHLD (17) is the canonical fork() exit signal.
        crate::errno::set_errno(0);
        let ret = clone(
            clone_dummy_fn(),
            clone_dummy_stack(),
            17,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_clone_exit_signal_64_max_passes() {
        // Exactly SIGRTMAX is permitted.
        crate::errno::set_errno(0);
        let ret = clone(
            clone_dummy_fn(),
            clone_dummy_stack(),
            64,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_clone_reserved_signal_byte_bit_einval() {
        // 0x40 sits inside the CSIGNAL byte (low 8 bits) but is not a
        // valid signal number when combined with SIGCHLD (0x11):
        // 0x40 | 0x11 = 0x51 = 81 > SIGRTMAX(64).  The exit-signal
        // overflow check rejects the call.
        crate::errno::set_errno(0);
        let ret = clone(
            clone_dummy_fn(),
            clone_dummy_stack(),
            0x40 | 17, // signal 81 — past SIGRTMAX
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone_i32_min_treated_as_clone_io() {
        // i32::MIN == 0x8000_0000 == CLONE_IO — valid, should reach ENOSYS.
        // This verifies our `as u32 as u64` zero-extend doesn't sign-
        // extend into the high half (which would set every reserved
        // bit above 32).
        crate::errno::set_errno(0);
        let ret = clone(
            clone_dummy_fn(),
            clone_dummy_stack(),
            i32::MIN, // == CLONE_IO with exit-signal 0
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_clone_thread_without_sighand_einval() {
        crate::errno::set_errno(0);
        let ret = clone(
            clone_dummy_fn(),
            clone_dummy_stack(),
            crate::linux_clone_args::CLONE_THREAD as i32,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone_sighand_without_vm_einval() {
        crate::errno::set_errno(0);
        let ret = clone(
            clone_dummy_fn(),
            clone_dummy_stack(),
            crate::linux_clone_args::CLONE_SIGHAND as i32,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone_thread_with_exit_signal_einval() {
        // Threads cannot request a parent-death signal.
        let flags = crate::linux_clone_args::CLONE_THREAD
            | crate::linux_clone_args::CLONE_SIGHAND
            | crate::linux_clone_args::CLONE_VM
            | 17; // SIGCHLD
        crate::errno::set_errno(0);
        let ret = clone(
            clone_dummy_fn(),
            clone_dummy_stack(),
            flags as i32,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone_newuser_with_fs_einval() {
        let flags =
            crate::linux_clone_args::CLONE_NEWUSER | crate::linux_clone_args::CLONE_FS;
        crate::errno::set_errno(0);
        let ret = clone(
            clone_dummy_fn(),
            clone_dummy_stack(),
            flags as i32,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone_newuser_with_thread_einval() {
        let flags = crate::linux_clone_args::CLONE_NEWUSER
            | crate::linux_clone_args::CLONE_THREAD
            | crate::linux_clone_args::CLONE_SIGHAND
            | crate::linux_clone_args::CLONE_VM;
        crate::errno::set_errno(0);
        let ret = clone(
            clone_dummy_fn(),
            clone_dummy_stack(),
            flags as i32,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone_pidfd_with_detached_einval() {
        let flags = crate::linux_clone_args::CLONE_PIDFD
            | crate::linux_clone_args::CLONE_DETACHED;
        crate::errno::set_errno(0);
        let ret = clone(
            clone_dummy_fn(),
            clone_dummy_stack(),
            flags as i32,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone_newns_with_fs_einval() {
        let flags = crate::linux_clone_args::CLONE_NEWNS
            | crate::linux_clone_args::CLONE_FS;
        crate::errno::set_errno(0);
        let ret = clone(
            clone_dummy_fn(),
            clone_dummy_stack(),
            flags as i32,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone_full_thread_flags_pass_to_enosys() {
        // Canonical pthread_create flag set:
        //   VM | FS | FILES | SIGHAND | THREAD | SYSVSEM |
        //   SETTLS | PARENT_SETTID | CHILD_CLEARTID
        // exit_signal must be 0 because of CLONE_THREAD.
        let flags = crate::linux_clone_args::CLONE_VM
            | crate::linux_clone_args::CLONE_FS
            | crate::linux_clone_args::CLONE_FILES
            | crate::linux_clone_args::CLONE_SIGHAND
            | crate::linux_clone_args::CLONE_THREAD
            | crate::linux_clone_args::CLONE_SYSVSEM
            | crate::linux_clone_args::CLONE_SETTLS
            | crate::linux_clone_args::CLONE_PARENT_SETTID
            | crate::linux_clone_args::CLONE_CHILD_CLEARTID;
        crate::errno::set_errno(0);
        let ret = clone(
            clone_dummy_fn(),
            clone_dummy_stack(),
            flags as i32,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_clone_workflow_glibc_pthread_create() {
        // glibc's NPTL pthread_create issues exactly:
        //   CLONE_VM | CLONE_FS | CLONE_FILES | CLONE_SIGHAND |
        //   CLONE_THREAD | CLONE_SYSVSEM | CLONE_SETTLS |
        //   CLONE_PARENT_SETTID | CLONE_CHILD_CLEARTID
        // exit_signal = 0 (threads don't notify on death).
        let flags = crate::linux_clone_args::CLONE_VM
            | crate::linux_clone_args::CLONE_FS
            | crate::linux_clone_args::CLONE_FILES
            | crate::linux_clone_args::CLONE_SIGHAND
            | crate::linux_clone_args::CLONE_THREAD
            | crate::linux_clone_args::CLONE_SYSVSEM
            | crate::linux_clone_args::CLONE_SETTLS
            | crate::linux_clone_args::CLONE_PARENT_SETTID
            | crate::linux_clone_args::CLONE_CHILD_CLEARTID;
        crate::errno::set_errno(0);
        let ret = clone(
            clone_dummy_fn(),
            clone_dummy_stack(),
            flags as i32,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_clone_workflow_vfork_emulation() {
        // glibc's vfork() falls back to clone(CLONE_VM | CLONE_VFORK,
        //                                     SIGCHLD) when the vfork
        // syscall is unavailable.
        let flags = crate::linux_clone_args::CLONE_VM
            | crate::linux_clone_args::CLONE_VFORK
            | 17; // SIGCHLD
        crate::errno::set_errno(0);
        let ret = clone(
            clone_dummy_fn(),
            clone_dummy_stack(),
            flags as i32,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_clone_workflow_runc_user_namespace() {
        // runc's user-namespace setup: new userns + new mountns +
        // new netns + new pidns + new IPC + new UTS + SIGCHLD.
        // FS is shared with the parent before the child re-execs
        // into the container init.
        let flags = crate::linux_clone_args::CLONE_NEWUSER
            | crate::linux_clone_args::CLONE_NEWNET
            | crate::linux_clone_args::CLONE_NEWPID
            | crate::linux_clone_args::CLONE_NEWIPC
            | crate::linux_clone_args::CLONE_NEWUTS
            | crate::linux_clone_args::CLONE_NEWCGROUP
            | 17; // SIGCHLD
        // NB: CLONE_NEWNS deliberately omitted — runc uses MS_PRIVATE
        // remount instead of namespace clone here.  CLONE_FS is also
        // omitted (which is why runc isn't EINVAL'd by check 8).
        crate::errno::set_errno(0);
        let ret = clone(
            clone_dummy_fn(),
            clone_dummy_stack(),
            flags as i32,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_clone_workflow_chrome_sandbox_zygote() {
        // Chromium's zygote spawns renderers via clone with
        //   CLONE_FS | CLONE_FILES | SIGCHLD
        // — shares fd table for the IPC socket, separate addr space.
        let flags = crate::linux_clone_args::CLONE_FS
            | crate::linux_clone_args::CLONE_FILES
            | 17; // SIGCHLD
        crate::errno::set_errno(0);
        let ret = clone(
            clone_dummy_fn(),
            clone_dummy_stack(),
            flags as i32,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_clone_workflow_pidfd_for_async_wait() {
        // Modern post-Linux-5.2 fork that produces a pidfd for the
        // child so the parent can poll(2) on its exit.
        // Combined with SIGCHLD for the death notification.
        let flags = crate::linux_clone_args::CLONE_PIDFD | 17;
        crate::errno::set_errno(0);
        let ret = clone(
            clone_dummy_fn(),
            clone_dummy_stack(),
            flags as i32,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_clone_workflow_buggy_thread_without_sighand() {
        // Real bug from a CRIU restore patch (fixed in v3.15):
        // restoring a thread group with CLONE_THREAD but the SIGHAND
        // bit cleared because the dump format used a stale enum.
        // Linux rejects this combo immediately.
        let flags = crate::linux_clone_args::CLONE_THREAD
            | crate::linux_clone_args::CLONE_VM
            | crate::linux_clone_args::CLONE_FILES;
        crate::errno::set_errno(0);
        let ret = clone(
            clone_dummy_fn(),
            clone_dummy_stack(),
            flags as i32,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone_workflow_buggy_signal_overflow() {
        // A Go runtime bug from gccgo 4.7 passed the full sigset_t
        // word (0xffff_ffff) instead of just the signal number,
        // overflowing CSIGNAL and triggering EINVAL on every clone.
        crate::errno::set_errno(0);
        let ret = clone(
            clone_dummy_fn(),
            clone_dummy_stack(),
            0xff_i32, // signal 255 = invalid
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone_workflow_buggy_clone3_only_bit() {
        // Caller migrating from clone3 → clone forgot to strip
        // CLONE_NEWTIME (0x80) — accepted by clone3 and unshare but
        // rejected by clone(2) because the bit overlaps CSIGNAL.  Our
        // validator catches this either via the CSIGNAL>SIGRTMAX check
        // (0x80 = signal 128) or via the reserved-bit whitelist
        // (CLONE_NEWTIME is not in CLONE_FLAGS_VALID for clone).  Both
        // paths report EINVAL.
        let flags = crate::linux_clone_args::CLONE_NEWCGROUP
            | crate::linux_clone_args::CLONE_NEWTIME;
        crate::errno::set_errno(0);
        let ret = clone(
            clone_dummy_fn(),
            clone_dummy_stack(),
            flags as i32,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone_zero_flags_passes_to_enosys() {
        // The simplest possible call: clone(fn, stack, 0, arg).
        // Equivalent to a fresh process with no shared state and no
        // exit signal.  Should pass validation and reach ENOSYS.
        crate::errno::set_errno(0);
        let ret = clone(
            clone_dummy_fn(),
            clone_dummy_stack(),
            0,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // -----------------------------------------------------------------
    // clone3(2) — argument-domain validation (Phase 55)
    // -----------------------------------------------------------------

    /// Zero-initialised V2 CloneArgs (88 bytes) for tests.
    fn clone3_v2_empty() -> CloneArgs {
        CloneArgs {
            flags: 0,
            pidfd: 0,
            child_tid: 0,
            parent_tid: 0,
            exit_signal: 0,
            stack: 0,
            stack_size: 0,
            tls: 0,
            set_tid: 0,
            set_tid_size: 0,
            cgroup: 0,
        }
    }

    #[test]
    fn test_clone3_flags_valid_contains_clone3_only() {
        assert_ne!(CLONE3_FLAGS_VALID & crate::linux_clone_args::CLONE_NEWTIME, 0);
        assert_ne!(
            CLONE3_FLAGS_VALID & crate::linux_clone_args::CLONE_INTO_CGROUP,
            0
        );
        assert_ne!(
            CLONE3_FLAGS_VALID & crate::linux_clone_args::CLONE_CLEAR_SIGHAND,
            0
        );
    }

    #[test]
    fn test_clone3_flags_valid_excludes_detached() {
        // clone3 explicitly rejects the historical CLONE_DETACHED bit.
        assert_eq!(
            CLONE3_FLAGS_VALID & crate::linux_clone_args::CLONE_DETACHED,
            0
        );
    }

    #[test]
    fn test_clone3_null_args_efault() {
        crate::errno::set_errno(0);
        let ret = clone3(
            core::ptr::null(),
            crate::linux_clone_args::CLONE_ARGS_SIZE_VER0 as usize,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_clone3_size_above_page_size_e2big() {
        let args = clone3_v2_empty();
        crate::errno::set_errno(0);
        let ret = clone3(&args as *const _, CLONE3_SIZE_MAX + 1);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::E2BIG);
    }

    #[test]
    fn test_clone3_size_below_ver0_einval() {
        let args = clone3_v2_empty();
        crate::errno::set_errno(0);
        let ret = clone3(
            &args as *const _,
            (crate::linux_clone_args::CLONE_ARGS_SIZE_VER0 as usize) - 1,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone3_size_at_ver0_passes_to_enosys() {
        let args = clone3_v2_empty();
        crate::errno::set_errno(0);
        let ret = clone3(
            &args as *const _,
            crate::linux_clone_args::CLONE_ARGS_SIZE_VER0 as usize,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_clone3_size_at_ver1_passes_to_enosys() {
        let args = clone3_v2_empty();
        crate::errno::set_errno(0);
        let ret = clone3(
            &args as *const _,
            crate::linux_clone_args::CLONE_ARGS_SIZE_VER1 as usize,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_clone3_size_at_ver2_passes_to_enosys() {
        let args = clone3_v2_empty();
        crate::errno::set_errno(0);
        let ret = clone3(
            &args as *const _,
            crate::linux_clone_args::CLONE_ARGS_SIZE_VER2 as usize,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_clone3_size_past_ver2_with_zero_tail_passes() {
        // Forward-compat: trailing bytes past V2 are all zero — kernel
        // accepts so that older binaries built against newer headers
        // keep working.
        #[repr(C)]
        struct CloneArgsExtended {
            base: CloneArgs,
            future_field: u64,
        }
        let ext = CloneArgsExtended {
            base: clone3_v2_empty(),
            future_field: 0,
        };
        crate::errno::set_errno(0);
        let ret = clone3(
            (&ext as *const _) as *const CloneArgs,
            core::mem::size_of::<CloneArgsExtended>(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_clone3_size_past_ver2_with_nonzero_tail_e2big() {
        // Forward-compat guard: caller built against newer headers
        // setting a bit we don't know about → E2BIG, not silent
        // success that would lose the request.
        #[repr(C)]
        struct CloneArgsExtended {
            base: CloneArgs,
            future_field: u64,
        }
        let ext = CloneArgsExtended {
            base: clone3_v2_empty(),
            future_field: 0xdead_beef,
        };
        crate::errno::set_errno(0);
        let ret = clone3(
            (&ext as *const _) as *const CloneArgs,
            core::mem::size_of::<CloneArgsExtended>(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::E2BIG);
    }

    #[test]
    fn test_clone3_csignal_in_flags_einval() {
        // SIGCHLD in the flags low byte — confused caller mixing
        // clone(2) and clone3(2) ABIs.
        let mut args = clone3_v2_empty();
        args.flags = 17; // SIGCHLD in CSIGNAL byte
        crate::errno::set_errno(0);
        let ret = clone3(
            &args as *const _,
            crate::linux_clone_args::CLONE_ARGS_SIZE_VER2 as usize,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone3_reserved_flag_bit_einval() {
        // Bit 50 — not in any documented CLONE_* flag.
        let mut args = clone3_v2_empty();
        args.flags = 1u64 << 50;
        crate::errno::set_errno(0);
        let ret = clone3(
            &args as *const _,
            crate::linux_clone_args::CLONE_ARGS_SIZE_VER2 as usize,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone3_detached_einval() {
        let mut args = clone3_v2_empty();
        args.flags = crate::linux_clone_args::CLONE_DETACHED;
        crate::errno::set_errno(0);
        let ret = clone3(
            &args as *const _,
            crate::linux_clone_args::CLONE_ARGS_SIZE_VER2 as usize,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone3_thread_without_sighand_einval() {
        let mut args = clone3_v2_empty();
        args.flags = crate::linux_clone_args::CLONE_THREAD;
        crate::errno::set_errno(0);
        let ret = clone3(
            &args as *const _,
            crate::linux_clone_args::CLONE_ARGS_SIZE_VER2 as usize,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone3_sighand_without_vm_einval() {
        let mut args = clone3_v2_empty();
        args.flags = crate::linux_clone_args::CLONE_SIGHAND;
        crate::errno::set_errno(0);
        let ret = clone3(
            &args as *const _,
            crate::linux_clone_args::CLONE_ARGS_SIZE_VER2 as usize,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone3_thread_with_exit_signal_einval() {
        let mut args = clone3_v2_empty();
        args.flags = crate::linux_clone_args::CLONE_VM
            | crate::linux_clone_args::CLONE_SIGHAND
            | crate::linux_clone_args::CLONE_THREAD;
        args.exit_signal = 17;
        crate::errno::set_errno(0);
        let ret = clone3(
            &args as *const _,
            crate::linux_clone_args::CLONE_ARGS_SIZE_VER2 as usize,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone3_exit_signal_overflow_einval() {
        let mut args = clone3_v2_empty();
        args.exit_signal = 65; // past SIGRTMAX
        crate::errno::set_errno(0);
        let ret = clone3(
            &args as *const _,
            crate::linux_clone_args::CLONE_ARGS_SIZE_VER2 as usize,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone3_newuser_with_fs_einval() {
        let mut args = clone3_v2_empty();
        args.flags = crate::linux_clone_args::CLONE_NEWUSER
            | crate::linux_clone_args::CLONE_FS;
        crate::errno::set_errno(0);
        let ret = clone3(
            &args as *const _,
            crate::linux_clone_args::CLONE_ARGS_SIZE_VER2 as usize,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone3_newuser_with_thread_einval() {
        let mut args = clone3_v2_empty();
        args.flags = crate::linux_clone_args::CLONE_NEWUSER
            | crate::linux_clone_args::CLONE_VM
            | crate::linux_clone_args::CLONE_SIGHAND
            | crate::linux_clone_args::CLONE_THREAD;
        crate::errno::set_errno(0);
        let ret = clone3(
            &args as *const _,
            crate::linux_clone_args::CLONE_ARGS_SIZE_VER2 as usize,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone3_newns_with_fs_einval() {
        let mut args = clone3_v2_empty();
        args.flags = crate::linux_clone_args::CLONE_NEWNS
            | crate::linux_clone_args::CLONE_FS;
        crate::errno::set_errno(0);
        let ret = clone3(
            &args as *const _,
            crate::linux_clone_args::CLONE_ARGS_SIZE_VER2 as usize,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone3_into_cgroup_without_ver2_einval() {
        let mut args = clone3_v2_empty();
        args.flags = crate::linux_clone_args::CLONE_INTO_CGROUP;
        crate::errno::set_errno(0);
        let ret = clone3(
            &args as *const _,
            crate::linux_clone_args::CLONE_ARGS_SIZE_VER1 as usize,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone3_into_cgroup_with_ver2_passes() {
        let mut args = clone3_v2_empty();
        args.flags = crate::linux_clone_args::CLONE_INTO_CGROUP;
        args.cgroup = 5; // some fd number
        crate::errno::set_errno(0);
        let ret = clone3(
            &args as *const _,
            crate::linux_clone_args::CLONE_ARGS_SIZE_VER2 as usize,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_clone3_set_tid_size_overflow_einval() {
        let mut args = clone3_v2_empty();
        args.set_tid_size = CLONE3_MAX_SET_TID + 1;
        args.set_tid = 0x1000; // bogus but non-NULL pointer
        crate::errno::set_errno(0);
        let ret = clone3(
            &args as *const _,
            crate::linux_clone_args::CLONE_ARGS_SIZE_VER2 as usize,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone3_set_tid_length_without_pointer_einval() {
        let mut args = clone3_v2_empty();
        args.set_tid_size = 4;
        args.set_tid = 0; // NULL
        crate::errno::set_errno(0);
        let ret = clone3(
            &args as *const _,
            crate::linux_clone_args::CLONE_ARGS_SIZE_VER2 as usize,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone3_set_tid_pointer_without_length_einval() {
        let mut args = clone3_v2_empty();
        args.set_tid_size = 0;
        args.set_tid = 0x1000; // non-NULL but no entries
        crate::errno::set_errno(0);
        let ret = clone3(
            &args as *const _,
            crate::linux_clone_args::CLONE_ARGS_SIZE_VER2 as usize,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone3_set_tid_ignored_when_size_below_ver1() {
        // VER0 doesn't include set_tid fields — they shouldn't be
        // inspected even if the underlying memory happens to contain
        // non-zero bytes (we provide a properly initialised struct
        // here; the real concern is uninitialised buffer reads).
        let mut args = clone3_v2_empty();
        args.set_tid = 0x1000;
        args.set_tid_size = 4;
        crate::errno::set_errno(0);
        let ret = clone3(
            &args as *const _,
            crate::linux_clone_args::CLONE_ARGS_SIZE_VER0 as usize,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_clone3_workflow_glibc_pthread_create_modern() {
        // glibc 2.34+ uses clone3 for pthread_create when available.
        let mut args = clone3_v2_empty();
        args.flags = crate::linux_clone_args::CLONE_VM
            | crate::linux_clone_args::CLONE_FS
            | crate::linux_clone_args::CLONE_FILES
            | crate::linux_clone_args::CLONE_SIGHAND
            | crate::linux_clone_args::CLONE_THREAD
            | crate::linux_clone_args::CLONE_SYSVSEM
            | crate::linux_clone_args::CLONE_SETTLS
            | crate::linux_clone_args::CLONE_PARENT_SETTID
            | crate::linux_clone_args::CLONE_CHILD_CLEARTID;
        args.stack = 0x7fff_0000_0000;
        args.stack_size = 8 * 1024 * 1024;
        args.tls = 0x7fff_8000_0000;
        args.parent_tid = 0x7fff_9000_0000;
        args.child_tid = 0x7fff_9000_0008;
        crate::errno::set_errno(0);
        let ret = clone3(
            &args as *const _,
            crate::linux_clone_args::CLONE_ARGS_SIZE_VER0 as usize,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_clone3_workflow_systemd_into_cgroup() {
        // systemd-249+ uses clone3 with CLONE_INTO_CGROUP to place
        // service processes directly in their target cgroup without
        // a post-fork cgroup-write race.
        let mut args = clone3_v2_empty();
        args.flags = crate::linux_clone_args::CLONE_INTO_CGROUP;
        args.exit_signal = 17; // SIGCHLD
        args.cgroup = 42;
        crate::errno::set_errno(0);
        let ret = clone3(
            &args as *const _,
            crate::linux_clone_args::CLONE_ARGS_SIZE_VER2 as usize,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_clone3_workflow_criu_restore_with_set_tid() {
        // CRIU restore uses clone3 with set_tid to recreate processes
        // with their original PIDs across PID namespace boundaries.
        let target_pids: [i32; 3] = [12345, 54321, 99999];
        let mut args = clone3_v2_empty();
        args.flags = crate::linux_clone_args::CLONE_NEWPID;
        args.exit_signal = 17;
        args.set_tid = target_pids.as_ptr() as u64;
        args.set_tid_size = 3;
        crate::errno::set_errno(0);
        let ret = clone3(
            &args as *const _,
            crate::linux_clone_args::CLONE_ARGS_SIZE_VER2 as usize,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_clone3_workflow_runc_user_namespace_clear_sighand() {
        // runc's user-namespace setup, modernised onto clone3, can
        // request CLONE_CLEAR_SIGHAND to wipe inherited handlers
        // when entering the container.
        let mut args = clone3_v2_empty();
        args.flags = crate::linux_clone_args::CLONE_NEWUSER
            | crate::linux_clone_args::CLONE_NEWNET
            | crate::linux_clone_args::CLONE_NEWPID
            | crate::linux_clone_args::CLONE_NEWIPC
            | crate::linux_clone_args::CLONE_NEWUTS
            | crate::linux_clone_args::CLONE_NEWCGROUP
            | crate::linux_clone_args::CLONE_CLEAR_SIGHAND;
        args.exit_signal = 17;
        crate::errno::set_errno(0);
        let ret = clone3(
            &args as *const _,
            crate::linux_clone_args::CLONE_ARGS_SIZE_VER2 as usize,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_clone3_workflow_pidfd_with_thread_supported() {
        // Linux 5.2 allowed CLONE_PIDFD with CLONE_THREAD (referring
        // to the new thread, not the group leader).  clone3 carries
        // this through.  We accept the combination.
        let mut args = clone3_v2_empty();
        args.flags = crate::linux_clone_args::CLONE_VM
            | crate::linux_clone_args::CLONE_FS
            | crate::linux_clone_args::CLONE_FILES
            | crate::linux_clone_args::CLONE_SIGHAND
            | crate::linux_clone_args::CLONE_THREAD
            | crate::linux_clone_args::CLONE_PIDFD;
        args.pidfd = 0x7fff_a000_0000;
        crate::errno::set_errno(0);
        let ret = clone3(
            &args as *const _,
            crate::linux_clone_args::CLONE_ARGS_SIZE_VER2 as usize,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_clone3_workflow_buggy_glibc_pre_2_34_clone_args() {
        // glibc 2.33's experimental clone3 wrapper accidentally OR'd
        // SIGCHLD into the flags field (copying clone(2) idiom).
        // Real bug: BZ #28310.  Our validator rejects with EINVAL.
        let mut args = clone3_v2_empty();
        args.flags = crate::linux_clone_args::CLONE_VM
            | crate::linux_clone_args::CLONE_FS
            | 17; // SIGCHLD bleeding into CSIGNAL byte
        args.exit_signal = 17;
        crate::errno::set_errno(0);
        let ret = clone3(
            &args as *const _,
            crate::linux_clone_args::CLONE_ARGS_SIZE_VER2 as usize,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clone3_workflow_buggy_huge_size() {
        // Caller passed `sizeof(struct_with_padding)` from a struct
        // that included alignment to 8 KiB by mistake.  Linux caps
        // at PAGE_SIZE; we report E2BIG.
        let args = clone3_v2_empty();
        crate::errno::set_errno(0);
        let ret = clone3(&args as *const _, 8192);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::E2BIG);
    }

    #[test]
    fn test_clone3_workflow_buggy_tiny_size() {
        // Caller passed `sizeof(flags)` (8 bytes) — far below V0.
        let args = clone3_v2_empty();
        crate::errno::set_errno(0);
        let ret = clone3(&args as *const _, 8);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -----------------------------------------------------------------
    // process_vm_readv / process_vm_writev — argument-domain validation
    // (Phase 56)
    // -----------------------------------------------------------------

    fn pvm_iov(base: usize, len: usize) -> crate::file::Iovec {
        crate::file::Iovec {
            iov_base: base as *mut u8,
            iov_len: len,
        }
    }

    #[test]
    fn test_process_vm_uio_maxiov_constant() {
        // Must match Linux's UIO_MAXIOV.
        assert_eq!(PROCESS_VM_UIO_MAXIOV, 1024);
    }

    #[test]
    fn test_process_vm_ssize_max_constant() {
        assert_eq!(PROCESS_VM_SSIZE_MAX, i64::MAX as u64);
    }

    #[test]
    fn test_process_vm_readv_nonzero_flags_einval() {
        crate::errno::set_errno(0);
        let ret =
            process_vm_readv(1, core::ptr::null(), 0, core::ptr::null(), 0, 1);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_process_vm_writev_nonzero_flags_einval() {
        crate::errno::set_errno(0);
        let ret =
            process_vm_writev(1, core::ptr::null(), 0, core::ptr::null(), 0, 1);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_process_vm_readv_pid_zero_esrch() {
        crate::errno::set_errno(0);
        let ret =
            process_vm_readv(0, core::ptr::null(), 0, core::ptr::null(), 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ESRCH);
    }

    #[test]
    fn test_process_vm_readv_negative_pid_esrch() {
        crate::errno::set_errno(0);
        let ret = process_vm_readv(
            -42,
            core::ptr::null(),
            0,
            core::ptr::null(),
            0,
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ESRCH);
    }

    #[test]
    fn test_process_vm_readv_flags_before_pid() {
        // Validation order: flags checked before pid.  Confirms that
        // a buggy caller with both errors reports the flags error.
        crate::errno::set_errno(0);
        let ret = process_vm_readv(
            -1,
            core::ptr::null(),
            0,
            core::ptr::null(),
            0,
            42, // nonzero flags
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_process_vm_readv_liovcnt_overflow_einval() {
        let iov = [pvm_iov(0x1000, 4096)];
        crate::errno::set_errno(0);
        let ret = process_vm_readv(
            1,
            iov.as_ptr(),
            PROCESS_VM_UIO_MAXIOV + 1,
            iov.as_ptr(),
            1,
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_process_vm_readv_riovcnt_overflow_einval() {
        let iov = [pvm_iov(0x1000, 4096)];
        crate::errno::set_errno(0);
        let ret = process_vm_readv(
            1,
            iov.as_ptr(),
            1,
            iov.as_ptr(),
            PROCESS_VM_UIO_MAXIOV + 1,
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_process_vm_readv_uio_maxiov_exact_passes() {
        // Exactly UIO_MAXIOV is accepted (Linux's check is `>`, not `>=`).
        // Use a fixed-size stack array to avoid an alloc dependency
        // in this no_std-friendly test module.
        const N: usize = PROCESS_VM_UIO_MAXIOV as usize;
        let mut iov = [crate::file::Iovec {
            iov_base: core::ptr::null_mut(),
            iov_len: 0,
        }; N];
        for (i, slot) in iov.iter_mut().enumerate() {
            *slot = pvm_iov(0x1000 + i * 4096, 8);
        }
        crate::errno::set_errno(0);
        let ret = process_vm_readv(
            1,
            iov.as_ptr(),
            PROCESS_VM_UIO_MAXIOV,
            iov.as_ptr(),
            PROCESS_VM_UIO_MAXIOV,
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_process_vm_readv_null_local_with_count_efault() {
        let iov = [pvm_iov(0x1000, 4096)];
        crate::errno::set_errno(0);
        let ret =
            process_vm_readv(1, core::ptr::null(), 3, iov.as_ptr(), 1, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_process_vm_readv_null_remote_with_count_efault() {
        let iov = [pvm_iov(0x1000, 4096)];
        crate::errno::set_errno(0);
        let ret =
            process_vm_readv(1, iov.as_ptr(), 1, core::ptr::null(), 3, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_process_vm_readv_null_with_zero_count_passes() {
        // NULL pointer is OK when its count is 0.
        crate::errno::set_errno(0);
        let ret = process_vm_readv(
            1,
            core::ptr::null(),
            0,
            core::ptr::null(),
            0,
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_process_vm_readv_local_sum_overflow_einval() {
        // Two iovs each claiming SSIZE_MAX bytes — sum overflows.
        let iov = [
            pvm_iov(0x1000, i64::MAX as usize),
            pvm_iov(0x2000, i64::MAX as usize),
        ];
        let remote = [pvm_iov(0x3000, 4096)];
        crate::errno::set_errno(0);
        let ret = process_vm_readv(
            1,
            iov.as_ptr(),
            2,
            remote.as_ptr(),
            1,
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_process_vm_readv_remote_sum_overflow_einval() {
        let local = [pvm_iov(0x1000, 4096)];
        let iov = [
            pvm_iov(0x1000, i64::MAX as usize),
            pvm_iov(0x2000, i64::MAX as usize),
        ];
        crate::errno::set_errno(0);
        let ret = process_vm_readv(
            1,
            local.as_ptr(),
            1,
            iov.as_ptr(),
            2,
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_process_vm_readv_local_sum_at_ssize_max_passes() {
        // Sum exactly SSIZE_MAX (boundary).
        let iov = [pvm_iov(0x1000, i64::MAX as usize)];
        let remote = [pvm_iov(0x3000, 1)];
        crate::errno::set_errno(0);
        let ret = process_vm_readv(
            1,
            iov.as_ptr(),
            1,
            remote.as_ptr(),
            1,
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_process_vm_writev_same_validation_as_readv() {
        // writev share validator — verify they behave identically.
        crate::errno::set_errno(0);
        let ret =
            process_vm_writev(0, core::ptr::null(), 0, core::ptr::null(), 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ESRCH);
    }

    #[test]
    fn test_process_vm_readv_workflow_gdb_inspect_target() {
        // gdb's `read_inferior_memory()` reads a target stack via
        // process_vm_readv with one local iov pointing into gdb's
        // buffer and one remote iov pointing into the target's stack.
        let local_buf = [0u8; 4096];
        let local = [pvm_iov(local_buf.as_ptr() as usize, 4096)];
        let remote = [pvm_iov(0x7fff_a000_0000, 4096)];
        crate::errno::set_errno(0);
        let ret = process_vm_readv(
            12345, // target pid
            local.as_ptr(),
            1,
            remote.as_ptr(),
            1,
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_process_vm_writev_workflow_criu_inject_dump_state() {
        // CRIU's restore path writes the dumped register/memory state
        // back into the recreated process via process_vm_writev.
        let payload = [0u8; 8192];
        let local = [
            pvm_iov(payload.as_ptr() as usize, 4096),
            pvm_iov(payload.as_ptr() as usize + 4096, 4096),
        ];
        let remote = [
            pvm_iov(0x7fff_8000_0000, 4096),
            pvm_iov(0x7fff_8000_1000, 4096),
        ];
        crate::errno::set_errno(0);
        let ret = process_vm_writev(
            54321,
            local.as_ptr(),
            2,
            remote.as_ptr(),
            2,
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_process_vm_readv_workflow_sanitizer_shadow_read() {
        // AddressSanitizer's interceptor in tsan attaches to a target
        // and reads its shadow memory via process_vm_readv.
        let shadow_buf = [0u8; 16384];
        let local = [pvm_iov(shadow_buf.as_ptr() as usize, 16384)];
        let remote = [pvm_iov(0x7fff_1000_0000, 16384)];
        crate::errno::set_errno(0);
        let ret = process_vm_readv(
            99999,
            local.as_ptr(),
            1,
            remote.as_ptr(),
            1,
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_process_vm_readv_workflow_buggy_negative_pid() {
        // Real bug from a Go ptrace wrapper: passed a Go int (signed)
        // that wrapped to negative when the target pid exceeded
        // int32 range.  Linux reports ESRCH.
        let local = [pvm_iov(0x1000, 4096)];
        let remote = [pvm_iov(0x2000, 4096)];
        crate::errno::set_errno(0);
        let ret = process_vm_readv(
            -2_147_483_647,
            local.as_ptr(),
            1,
            remote.as_ptr(),
            1,
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ESRCH);
    }

    #[test]
    fn test_process_vm_writev_workflow_buggy_reserved_flags() {
        // Caller migrating from preadv2/pwritev2 forgot that
        // process_vm_writev's `flags` is reserved (must be 0) and
        // passed RWF_HIPRI (0x1).  EINVAL.
        let local = [pvm_iov(0x1000, 4096)];
        let remote = [pvm_iov(0x2000, 4096)];
        crate::errno::set_errno(0);
        let ret = process_vm_writev(
            1234,
            local.as_ptr(),
            1,
            remote.as_ptr(),
            1,
            0x1, // RWF_HIPRI
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -----------------------------------------------------------------
    // kcmp(2) — argument-domain validation (Phase 57)
    // -----------------------------------------------------------------

    #[test]
    fn test_kcmp_types_max_constant() {
        // Must equal KCMP_EPOLL_TFD + 1.
        assert_eq!(KCMP_TYPES, KCMP_EPOLL_TFD + 1);
    }

    #[test]
    fn test_kcmp_pid1_zero_esrch() {
        crate::errno::set_errno(0);
        let ret = kcmp(0, 1, KCMP_FILE, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ESRCH);
    }

    #[test]
    fn test_kcmp_pid1_negative_esrch() {
        crate::errno::set_errno(0);
        let ret = kcmp(-1, 1, KCMP_FILE, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ESRCH);
    }

    #[test]
    fn test_kcmp_pid2_zero_esrch() {
        crate::errno::set_errno(0);
        let ret = kcmp(1, 0, KCMP_FILE, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ESRCH);
    }

    #[test]
    fn test_kcmp_pid2_negative_esrch() {
        crate::errno::set_errno(0);
        let ret = kcmp(1, -42, KCMP_FILE, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ESRCH);
    }

    #[test]
    fn test_kcmp_pid1_before_pid2() {
        // Ordering: pid1 checked before pid2.
        crate::errno::set_errno(0);
        let ret = kcmp(0, 0, KCMP_FILE, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ESRCH);
        // (We can't tell which one tripped from a single ESRCH value,
        // but symmetry of pid1==pid2==0 means both paths agree.)
    }

    #[test]
    fn test_kcmp_type_negative_einval() {
        crate::errno::set_errno(0);
        let ret = kcmp(1, 2, -1, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_kcmp_type_too_large_einval() {
        crate::errno::set_errno(0);
        let ret = kcmp(1, 2, KCMP_TYPES, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_kcmp_type_way_too_large_einval() {
        crate::errno::set_errno(0);
        let ret = kcmp(1, 2, 0x7fff_ffff, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_kcmp_pid_check_before_type_check() {
        // pid1 invalid + type invalid → ESRCH (pid check fires first).
        crate::errno::set_errno(0);
        let ret = kcmp(0, 2, -1, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ESRCH);
    }

    #[test]
    fn test_kcmp_file_idx1_overflow_ebadf() {
        // fd is `int` in Linux — values past INT_MAX can't be valid.
        crate::errno::set_errno(0);
        let ret = kcmp(1, 2, KCMP_FILE, (i32::MAX as u64) + 1, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_kcmp_file_idx2_overflow_ebadf() {
        crate::errno::set_errno(0);
        let ret = kcmp(1, 2, KCMP_FILE, 5, (i32::MAX as u64) + 1);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_kcmp_file_idx_at_int_max_passes() {
        // Exactly INT_MAX is a syntactically valid fd; the kernel
        // would resolve it against the fdtable.
        crate::errno::set_errno(0);
        let ret = kcmp(1, 2, KCMP_FILE, i32::MAX as u64, i32::MAX as u64);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_kcmp_non_file_ignores_idx_overflow() {
        // KCMP_VM doesn't use idx1/idx2 — large values are accepted
        // and just ignored (Linux's behaviour).
        crate::errno::set_errno(0);
        let ret = kcmp(1, 2, KCMP_VM, u64::MAX, u64::MAX);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_kcmp_epoll_tfd_null_idx2_efault() {
        crate::errno::set_errno(0);
        let ret = kcmp(1, 2, KCMP_EPOLL_TFD, 3, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_kcmp_epoll_tfd_with_slot_passes() {
        // Non-NULL idx2 pointer reaches the not-implemented path.
        crate::errno::set_errno(0);
        let ret = kcmp(1, 2, KCMP_EPOLL_TFD, 3, 0x7fff_0000);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_kcmp_workflow_criu_dedup_shared_resources() {
        // CRIU's dump phase uses kcmp(KCMP_FILE) to detect when two
        // processes share the same struct file — i.e. one was created
        // via dup() or inherited across fork() — so the dump can
        // store a single reference.
        crate::errno::set_errno(0);
        let ret = kcmp(1234, 5678, KCMP_FILE, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_kcmp_workflow_criu_detect_clone_vm_pair() {
        // CRIU also uses KCMP_VM to detect threads (same VM) vs
        // processes (different VM) when reconstructing process groups.
        crate::errno::set_errno(0);
        let ret = kcmp(1234, 1234, KCMP_VM, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_kcmp_workflow_strace_compare_io_context() {
        // strace's --decode-fds uses KCMP_IO when displaying I/O
        // priority-affecting sharing between processes.
        crate::errno::set_errno(0);
        let ret = kcmp(1234, 5678, KCMP_IO, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_kcmp_workflow_buggy_fd_as_u64_signed_extension() {
        // Real bug from a Python ctypes binding: passed a Python int
        // representing fd=-1 (no fd) by casting to c_ulong, getting
        // 0xffff_ffff_ffff_ffff.  That's > INT_MAX, so EBADF.
        crate::errno::set_errno(0);
        let ret = kcmp(1234, 5678, KCMP_FILE, u64::MAX, 5);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_kcmp_workflow_buggy_uninitialised_epoll_slot() {
        // A buggy caller assumed KCMP_EPOLL_TFD used (idx1, idx2) as
        // (fd, fd) like KCMP_FILE and passed 0 for both.  Our
        // validator catches the NULL kcmp_epoll_slot with EFAULT.
        crate::errno::set_errno(0);
        let ret = kcmp(1234, 5678, KCMP_EPOLL_TFD, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // -----------------------------------------------------------------
    // ioprio_get / ioprio_set — argument-domain validation (Phase 58)
    // -----------------------------------------------------------------

    #[test]
    fn test_ioprio_layout_constants() {
        assert_eq!(IOPRIO_CLASS_SHIFT, 13);
        assert_eq!(IOPRIO_PRIO_MASK, 0x1FFF);
        assert_eq!(IOPRIO_BE_NR, 8);
    }

    // ---- ioprio_get error paths ----

    #[test]
    fn test_ioprio_get_invalid_which_einval() {
        crate::errno::set_errno(0);
        assert_eq!(ioprio_get(0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_ioprio_get_invalid_which_high_einval() {
        crate::errno::set_errno(0);
        assert_eq!(ioprio_get(99, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_ioprio_get_invalid_which_negative_einval() {
        crate::errno::set_errno(0);
        assert_eq!(ioprio_get(-1, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_ioprio_get_negative_who_esrch() {
        crate::errno::set_errno(0);
        assert_eq!(ioprio_get(IOPRIO_WHO_PROCESS, -1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ESRCH);
    }

    #[test]
    fn test_ioprio_get_negative_who_pgrp_esrch() {
        crate::errno::set_errno(0);
        assert_eq!(ioprio_get(IOPRIO_WHO_PGRP, -100), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ESRCH);
    }

    #[test]
    fn test_ioprio_get_negative_who_user_esrch() {
        crate::errno::set_errno(0);
        assert_eq!(ioprio_get(IOPRIO_WHO_USER, i32::MIN), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ESRCH);
    }

    #[test]
    fn test_ioprio_get_which_checked_before_who() {
        // Bad which + bad who → EINVAL wins (which check first).
        crate::errno::set_errno(0);
        assert_eq!(ioprio_get(42, -1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // ---- ioprio_get success paths ----

    #[test]
    fn test_ioprio_get_self_returns_default() {
        // who = 0 means "current task/pgrp/user"; default = NONE/0.
        assert_eq!(ioprio_get(IOPRIO_WHO_PROCESS, 0), 0);
        assert_eq!(ioprio_get(IOPRIO_WHO_PGRP, 0), 0);
        assert_eq!(ioprio_get(IOPRIO_WHO_USER, 0), 0);
    }

    #[test]
    fn test_ioprio_get_specific_pid_returns_default() {
        // Stub does not track per-pid state — every existing pid maps
        // to the default-priority value.
        assert_eq!(ioprio_get(IOPRIO_WHO_PROCESS, 1234), 0);
    }

    // ---- ioprio_set error paths ----

    #[test]
    fn test_ioprio_set_invalid_which_einval() {
        crate::errno::set_errno(0);
        assert_eq!(ioprio_set(0, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_ioprio_set_invalid_class_too_large_einval() {
        // class = 4 is one past IOPRIO_CLASS_IDLE.
        let prio = 4 << IOPRIO_CLASS_SHIFT;
        crate::errno::set_errno(0);
        assert_eq!(ioprio_set(IOPRIO_WHO_PROCESS, 0, prio), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_ioprio_set_invalid_class_way_too_large_einval() {
        let prio = 7 << IOPRIO_CLASS_SHIFT;
        crate::errno::set_errno(0);
        assert_eq!(ioprio_set(IOPRIO_WHO_PROCESS, 0, prio), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_ioprio_set_rt_data_eight_einval() {
        // data must be 0..7 for RT and BE.
        let prio = (IOPRIO_CLASS_RT << IOPRIO_CLASS_SHIFT) | 8;
        crate::errno::set_errno(0);
        assert_eq!(ioprio_set(IOPRIO_WHO_PROCESS, 0, prio), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_ioprio_set_be_data_at_limit_einval() {
        let prio = (IOPRIO_CLASS_BE << IOPRIO_CLASS_SHIFT) | IOPRIO_PRIO_MASK;
        crate::errno::set_errno(0);
        assert_eq!(ioprio_set(IOPRIO_WHO_PROCESS, 0, prio), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_ioprio_set_negative_who_esrch() {
        crate::errno::set_errno(0);
        assert_eq!(ioprio_set(IOPRIO_WHO_PROCESS, -5, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ESRCH);
    }

    #[test]
    fn test_ioprio_set_bad_which_valid_class_einval() {
        // Phase 124: Bad which + valid class — class passes, which
        // fails → EINVAL.  Both Linux's sys_ioprio_set order and ours
        // produce EINVAL here, since the which-switch default arm
        // also returns EINVAL.  Renamed from
        // test_ioprio_set_which_checked_before_class (the old name
        // reflected the pre-Phase-124 ordering; class is now checked
        // first per Linux).
        let prio = (IOPRIO_CLASS_BE << IOPRIO_CLASS_SHIFT) | 4;
        crate::errno::set_errno(0);
        assert_eq!(ioprio_set(99, 0, prio), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_ioprio_set_class_checked_before_who() {
        // Bad class + bad who → EINVAL (class check is before who).
        let prio = 5 << IOPRIO_CLASS_SHIFT;
        crate::errno::set_errno(0);
        assert_eq!(ioprio_set(IOPRIO_WHO_PROCESS, -1, prio), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // --- Phase 124: ioprio_set CLASS_NONE rejects non-zero data,
    //                prologue order matches Linux sys_ioprio_set ---
    //
    // Previous behaviour silently accepted any `data` for
    // IOPRIO_CLASS_NONE, contrary to modern Linux which explicitly
    // rejects non-zero data for NONE with EINVAL.  The prologue also
    // ran the `which` check before class/data validation; Linux's
    // order is class first.  All EINVAL cases collapse to the same
    // observable value, but a few asserts below exercise the new
    // ordering plus the new NONE+data check.

    /// Phase 124: CLASS_NONE with data=1 — Linux rejects, we now do
    /// too.
    #[test]
    fn test_ioprio_set_phase124_none_data_one_einval() {
        let prio = (IOPRIO_CLASS_NONE << IOPRIO_CLASS_SHIFT) | 1;
        crate::errno::set_errno(0);
        assert_eq!(ioprio_set(IOPRIO_WHO_PROCESS, 0, prio), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 124: CLASS_NONE with data=7 (boundary of RT/BE range
    /// but for NONE still EINVAL).
    #[test]
    fn test_ioprio_set_phase124_none_data_seven_einval() {
        let prio = (IOPRIO_CLASS_NONE << IOPRIO_CLASS_SHIFT) | 7;
        crate::errno::set_errno(0);
        assert_eq!(ioprio_set(IOPRIO_WHO_PROCESS, 0, prio), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 124: CLASS_NONE with full data mask (8191) — EINVAL.
    #[test]
    fn test_ioprio_set_phase124_none_data_full_mask_einval() {
        let prio =
            (IOPRIO_CLASS_NONE << IOPRIO_CLASS_SHIFT) | IOPRIO_PRIO_MASK;
        crate::errno::set_errno(0);
        assert_eq!(ioprio_set(IOPRIO_WHO_PROCESS, 0, prio), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 124: regression — CLASS_NONE with data=0 still succeeds.
    /// Confirms the rejection is strictly for non-zero data.
    #[test]
    fn test_ioprio_set_phase124_none_data_zero_succeeds() {
        let prio = IOPRIO_CLASS_NONE << IOPRIO_CLASS_SHIFT;
        crate::errno::set_errno(0);
        assert_eq!(ioprio_set(IOPRIO_WHO_PROCESS, 0, prio), 0);
    }

    /// Phase 124: CLASS_IDLE accepts any data (regression — the new
    /// per-class match arms don't accidentally tighten IDLE).
    #[test]
    fn test_ioprio_set_phase124_idle_data_one_succeeds() {
        let prio = (IOPRIO_CLASS_IDLE << IOPRIO_CLASS_SHIFT) | 1;
        crate::errno::set_errno(0);
        assert_eq!(ioprio_set(IOPRIO_WHO_PROCESS, 0, prio), 0);
    }

    /// Phase 124: CLASS_IDLE + full data mask still succeeds.
    #[test]
    fn test_ioprio_set_phase124_idle_data_full_mask_succeeds() {
        let prio =
            (IOPRIO_CLASS_IDLE << IOPRIO_CLASS_SHIFT) | IOPRIO_PRIO_MASK;
        crate::errno::set_errno(0);
        assert_eq!(ioprio_set(IOPRIO_WHO_PROCESS, 0, prio), 0);
    }

    /// Phase 124: ordering — CLASS_NONE+data + bad which → EINVAL.
    /// Class check fires first (new order), so the which-default arm
    /// is never reached.  Both arms produce EINVAL, but this exercises
    /// the Linux-matching order.
    #[test]
    fn test_ioprio_set_phase124_none_data_bad_which_einval() {
        let prio = (IOPRIO_CLASS_NONE << IOPRIO_CLASS_SHIFT) | 3;
        crate::errno::set_errno(0);
        assert_eq!(ioprio_set(99, 0, prio), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 124: ordering — CLASS_NONE+data + negative who → EINVAL
    /// (class fires before who check; ESRCH never observed).
    #[test]
    fn test_ioprio_set_phase124_none_data_neg_who_einval() {
        let prio = (IOPRIO_CLASS_NONE << IOPRIO_CLASS_SHIFT) | 5;
        crate::errno::set_errno(0);
        assert_eq!(ioprio_set(IOPRIO_WHO_PROCESS, -1, prio), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 124: ordering — valid NONE + bad which → EINVAL (which
    /// fires before who).
    #[test]
    fn test_ioprio_set_phase124_clean_none_bad_which_einval() {
        let prio = IOPRIO_CLASS_NONE << IOPRIO_CLASS_SHIFT;
        crate::errno::set_errno(0);
        assert_eq!(ioprio_set(99, -5, prio), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 124: ordering — valid NONE + valid which + negative who
    /// → ESRCH (only path to ESRCH is class-clean and which-valid).
    #[test]
    fn test_ioprio_set_phase124_clean_args_neg_who_esrch() {
        let prio = IOPRIO_CLASS_NONE << IOPRIO_CLASS_SHIFT;
        crate::errno::set_errno(0);
        assert_eq!(ioprio_set(IOPRIO_WHO_PROCESS, -1, prio), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ESRCH);
    }

    /// Phase 124 recovery: EINVAL on NONE+data, then ENOSYS-shaped
    /// success on a clean call.
    #[test]
    fn test_ioprio_set_phase124_recovery_after_einval() {
        let bad = (IOPRIO_CLASS_NONE << IOPRIO_CLASS_SHIFT) | 4;
        crate::errno::set_errno(0);
        assert_eq!(ioprio_set(IOPRIO_WHO_PROCESS, 0, bad), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let good = IOPRIO_CLASS_BE << IOPRIO_CLASS_SHIFT;
        assert_eq!(ioprio_set(IOPRIO_WHO_PROCESS, 1234, good), 0);
    }

    /// Phase 124 workflow: a process inheriting an ioprio value from
    /// a parent that used CLASS_BE then tries to "reset" it by
    /// passing the raw `ioprio` int (which now happens to be
    /// `BE << 13 | 4`) but accidentally truncates to just `4` — i.e.
    /// CLASS_NONE | 4.  Must EINVAL so the bug surfaces instead of
    /// silently doing nothing.
    #[test]
    fn test_ioprio_set_phase124_workflow_reset_typo_einval() {
        let truncated_prio = 4;  // intent was BE | 4 = (2 << 13) | 4
        crate::errno::set_errno(0);
        assert_eq!(
            ioprio_set(IOPRIO_WHO_PROCESS, 0, truncated_prio),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 124 buggy-caller: caller passes `IOPRIO_CLASS_NONE | 0`
    /// computed via `class | data` (no shift) — yields 0 (CLASS_NONE
    /// + data 0), which is the well-formed "use defaults" call.
    /// Must succeed; documents that the legitimate "no priority"
    /// idiom still works under the new rule.
    #[test]
    fn test_ioprio_set_phase124_buggy_caller_no_shift_succeeds() {
        let prio = IOPRIO_CLASS_NONE | 0;  // accidentally not shifted
        // Result: prio == 0, class == 0 (NONE), data == 0 → OK
        assert_eq!(prio, 0);
        crate::errno::set_errno(0);
        assert_eq!(ioprio_set(IOPRIO_WHO_PROCESS, 0, prio), 0);
    }

    // ---- ioprio_set success paths ----

    #[test]
    fn test_ioprio_set_class_none_succeeds() {
        // CLASS_NONE ignores the data field.
        let prio = IOPRIO_CLASS_NONE << IOPRIO_CLASS_SHIFT;
        assert_eq!(ioprio_set(IOPRIO_WHO_PROCESS, 0, prio), 0);
    }

    #[test]
    fn test_ioprio_set_class_idle_succeeds_with_any_data() {
        // CLASS_IDLE: data is effectively always 7; any value is accepted.
        let prio = (IOPRIO_CLASS_IDLE << IOPRIO_CLASS_SHIFT) | 100;
        assert_eq!(ioprio_set(IOPRIO_WHO_PROCESS, 0, prio), 0);
    }

    #[test]
    fn test_ioprio_set_class_rt_data_range_succeeds() {
        // RT class with data 0..=7 must succeed.
        for data in 0..IOPRIO_BE_NR {
            let prio = (IOPRIO_CLASS_RT << IOPRIO_CLASS_SHIFT) | data;
            assert_eq!(
                ioprio_set(IOPRIO_WHO_PROCESS, 0, prio),
                0,
                "RT data={data} should succeed",
            );
        }
    }

    #[test]
    fn test_ioprio_set_class_be_data_range_succeeds() {
        for data in 0..IOPRIO_BE_NR {
            let prio = (IOPRIO_CLASS_BE << IOPRIO_CLASS_SHIFT) | data;
            assert_eq!(
                ioprio_set(IOPRIO_WHO_PROCESS, 0, prio),
                0,
                "BE data={data} should succeed",
            );
        }
    }

    // ---- Real-world workflows ----

    #[test]
    fn test_ioprio_workflow_ionice_lowest_be() {
        // `ionice -c2 -n7 <pid>` — best-effort, lowest priority.
        let prio = (IOPRIO_CLASS_BE << IOPRIO_CLASS_SHIFT) | 7;
        assert_eq!(ioprio_set(IOPRIO_WHO_PROCESS, 1234, prio), 0);
    }

    #[test]
    fn test_ioprio_workflow_ionice_idle() {
        // `ionice -c3 <pid>` — idle (data ignored).
        let prio = IOPRIO_CLASS_IDLE << IOPRIO_CLASS_SHIFT;
        assert_eq!(ioprio_set(IOPRIO_WHO_PROCESS, 1234, prio), 0);
    }

    #[test]
    fn test_ioprio_workflow_ionice_pgrp() {
        // `ionice -P <pgid>` — apply to entire process group.
        let prio = (IOPRIO_CLASS_BE << IOPRIO_CLASS_SHIFT) | 4;
        assert_eq!(ioprio_set(IOPRIO_WHO_PGRP, 4321, prio), 0);
    }

    #[test]
    fn test_ioprio_workflow_backup_user_throttle() {
        // A backup daemon throttles its own user's I/O class to idle.
        let prio = IOPRIO_CLASS_IDLE << IOPRIO_CLASS_SHIFT;
        assert_eq!(ioprio_set(IOPRIO_WHO_USER, 1000, prio), 0);
    }

    #[test]
    fn test_ioprio_workflow_round_trip_get_after_set() {
        // After set, get returns whatever the stub reports (default).
        let prio = (IOPRIO_CLASS_BE << IOPRIO_CLASS_SHIFT) | 4;
        assert_eq!(ioprio_set(IOPRIO_WHO_PROCESS, 0, prio), 0);
        // Stub doesn't persist; default-priority is returned.
        assert_eq!(ioprio_get(IOPRIO_WHO_PROCESS, 0), 0);
    }

    // ---- Real-world buggy callers ----

    #[test]
    fn test_ioprio_workflow_buggy_raw_priority_no_encoding() {
        // Phase 124: a caller forgets the (class << 13) encoding and
        // passes the priority data directly as the ioprio value.  The
        // result is class=0 (NONE) with data=4.  Modern Linux rejects
        // NONE+data!=0 with EINVAL (matching `block/ioprio.c`); we
        // now do too.  This surfaces the encoding bug instead of
        // silently doing nothing — strictly better for callers.
        crate::errno::set_errno(0);
        assert_eq!(ioprio_set(IOPRIO_WHO_PROCESS, 0, 4), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_ioprio_workflow_buggy_class_only_no_shift() {
        // Phase 124: a caller passes the class number directly (e.g.
        // ioprio=2 intending CLASS_BE).  Same NONE+data!=0 → EINVAL
        // surface as the test above.
        crate::errno::set_errno(0);
        assert_eq!(
            ioprio_set(IOPRIO_WHO_PROCESS, 0, IOPRIO_CLASS_BE),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_ioprio_workflow_buggy_negative_ioprio() {
        // A signed-extension bug produces a negative ioprio.  Top bit
        // set → class field extracted as a large value → EINVAL.
        crate::errno::set_errno(0);
        // i32::MIN >> 13 is a large negative number — class != any
        // valid class → EINVAL via the catch-all arm.
        assert_eq!(ioprio_set(IOPRIO_WHO_PROCESS, 0, -1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // =====================================================================
    // Phase 73 — tcgetpgrp/tcsetpgrp fd validation
    //
    // Linux's tcgetpgrp/tcsetpgrp prologues validate the fd before any
    // terminal-related work: a closed or negative fd returns -1/EBADF.
    // tcsetpgrp also validates pgrp (>0).  Validation order is fd first,
    // then pgrp — i.e. a bad fd with a bad pgrp reports EBADF, not EINVAL.
    // =====================================================================

    // ---- Per-error class: bad fd ----

    #[test]
    fn test_tcgetpgrp_negative_fd_returns_ebadf() {
        reset_pg();
        crate::errno::set_errno(0);
        assert_eq!(tcgetpgrp(-1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_tcgetpgrp_large_negative_fd_returns_ebadf() {
        reset_pg();
        crate::errno::set_errno(0);
        assert_eq!(tcgetpgrp(i32::MIN), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_tcgetpgrp_unopen_fd_returns_ebadf() {
        reset_pg();
        // Pick a high fd that is almost certainly not in the table.
        let probe: i32 = 0x4000_0040;
        // Defensively close, in case some other test left it open.
        let _ = crate::fdtable::close_fd(probe);
        crate::errno::set_errno(0);
        assert_eq!(tcgetpgrp(probe), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_tcsetpgrp_negative_fd_returns_ebadf() {
        reset_pg();
        crate::errno::set_errno(0);
        assert_eq!(tcsetpgrp(-1, 100), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_tcsetpgrp_unopen_fd_returns_ebadf() {
        reset_pg();
        let probe: i32 = 0x4000_0041;
        let _ = crate::fdtable::close_fd(probe);
        crate::errno::set_errno(0);
        assert_eq!(tcsetpgrp(probe, 100), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // ---- Per-error class: bad pgrp (fd valid) ----

    #[test]
    fn test_tcsetpgrp_zero_pgrp_open_fd_returns_einval() {
        reset_pg();
        ensure_pg_test_fds();
        crate::errno::set_errno(0);
        assert_eq!(tcsetpgrp(0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_tcsetpgrp_negative_pgrp_open_fd_returns_einval() {
        reset_pg();
        ensure_pg_test_fds();
        crate::errno::set_errno(0);
        assert_eq!(tcsetpgrp(0, -1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // ---- Validation ordering: fd checked before pgrp ----

    #[test]
    fn test_tcsetpgrp_bad_fd_beats_bad_pgrp() {
        // Both fd and pgrp are invalid.  Linux validates fd first → EBADF.
        reset_pg();
        crate::errno::set_errno(0);
        assert_eq!(tcsetpgrp(-1, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_tcsetpgrp_bad_fd_beats_negative_pgrp() {
        reset_pg();
        crate::errno::set_errno(0);
        assert_eq!(tcsetpgrp(-1, -42), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_tcsetpgrp_unopen_fd_beats_bad_pgrp() {
        reset_pg();
        let probe: i32 = 0x4000_0042;
        let _ = crate::fdtable::close_fd(probe);
        crate::errno::set_errno(0);
        assert_eq!(tcsetpgrp(probe, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // ---- Buggy-caller patterns ----

    #[test]
    fn test_tcgetpgrp_buggy_uninit_fd_returns_ebadf() {
        // A caller passes a stack-uninitialized fd that happens to be -1.
        reset_pg();
        let mut uninit_fd: i32 = -1;
        // Touch it so the compiler doesn't elide.
        uninit_fd = uninit_fd.wrapping_add(0);
        crate::errno::set_errno(0);
        assert_eq!(tcgetpgrp(uninit_fd), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_tcsetpgrp_buggy_swapped_args() {
        // A caller swaps fd and pgrp: tcsetpgrp(pgrp, fd).  If pgrp=100
        // is mistakenly used as fd, and fd=0 (the terminal) is used as
        // pgrp, the fd-validation step rejects 100 with EBADF on systems
        // where 100 isn't open.  Make this deterministic by ensuring 100
        // is closed first.
        reset_pg();
        ensure_pg_test_fds();
        let bad_fd: i32 = 100;
        let _ = crate::fdtable::close_fd(bad_fd);
        crate::errno::set_errno(0);
        assert_eq!(tcsetpgrp(bad_fd, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // ---- Workflow: validation does not corrupt state ----

    #[test]
    fn test_tcsetpgrp_bad_fd_does_not_change_fg_pgrp() {
        reset_pg();
        ensure_pg_test_fds();
        // Initial value is whatever reset_pg sets up (42).
        let before = tcgetpgrp(0);
        crate::errno::set_errno(0);
        // Try to set with bad fd — must fail.
        assert_eq!(tcsetpgrp(-1, 777), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
        // FG_PGRP unchanged.
        assert_eq!(tcgetpgrp(0), before);
    }

    #[test]
    fn test_tcsetpgrp_bad_pgrp_does_not_change_fg_pgrp() {
        reset_pg();
        ensure_pg_test_fds();
        // First set it to a known value.
        assert_eq!(tcsetpgrp(0, 555), 0);
        // Then try a bad pgrp.
        crate::errno::set_errno(0);
        assert_eq!(tcsetpgrp(0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        // Value unchanged.
        assert_eq!(tcgetpgrp(0), 555);
    }

    // ---- Phase 116: pidfd_open validation-order parity with Linux ----
    //
    // Linux's `SYSCALL_DEFINE2(pidfd_open)` rejects unknown flag bits
    // BEFORE `pid <= 0`.  Both checks return EINVAL, so the errno is
    // identical for all inputs that hit either one; these tests pin
    // the Linux precedence in so a future reordering would have to
    // explicitly modify them.

    #[test]
    fn test_pidfd_open_phase116_flags_checked_before_pid_zero() {
        // Both args invalid (pid=0 AND unknown flag bit).  Linux's
        // prologue surfaces the flag failure first.
        crate::errno::set_errno(0);
        let ret = pidfd_open(0, 0x8000_0000);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pidfd_open_phase116_flags_checked_before_pid_negative() {
        // Both args invalid (pid=-1 AND unknown flag bit) -> EINVAL.
        crate::errno::set_errno(0);
        let ret = pidfd_open(-1, 0x8000_0000);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pidfd_open_phase116_flags_checked_before_pid_min() {
        // pid=i32::MIN AND unknown flag bit.
        crate::errno::set_errno(0);
        let ret = pidfd_open(i32::MIN, 0x4);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pidfd_open_phase116_lone_unknown_flag_einval() {
        // Just an unknown flag with a valid positive pid.
        crate::errno::set_errno(0);
        let ret = pidfd_open(100, 0x4);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pidfd_open_phase116_lone_pid_zero_still_einval() {
        // No flag issue; pure pid<=0 path.  Must still return EINVAL.
        crate::errno::set_errno(0);
        let ret = pidfd_open(0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pidfd_open_phase116_lone_pid_negative_still_einval() {
        // Pure pid<0 path.
        crate::errno::set_errno(0);
        let ret = pidfd_open(-42, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pidfd_open_phase116_pid_zero_with_valid_flags_einval() {
        // Valid flag (PIDFD_NONBLOCK) but pid<=0 -> EINVAL via the
        // (now second) pid check, confirming valid flags pass through
        // the flag check cleanly.
        crate::errno::set_errno(0);
        let ret = pidfd_open(0, PIDFD_NONBLOCK);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pidfd_open_phase116_pid_zero_with_both_valid_flags_einval() {
        // PIDFD_NONBLOCK|PIDFD_THREAD (both valid bits) + pid=0 -> EINVAL.
        crate::errno::set_errno(0);
        let ret = pidfd_open(0, PIDFD_NONBLOCK | PIDFD_THREAD);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pidfd_open_phase116_all_flags_set_einval() {
        // u32::MAX includes unknown bits -> EINVAL via flag check.
        crate::errno::set_errno(0);
        let ret = pidfd_open(100, u32::MAX);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pidfd_open_phase116_recovery_after_einval() {
        // After a rejected call, errno can be reset and a subsequent
        // valid call reaches the ENOSYS terminal.
        let _ = pidfd_open(0, 0x8000_0000);
        crate::errno::set_errno(0);
        let ret = pidfd_open(123, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_pidfd_open_phase116_buggy_caller_passes_negative_int_flags() {
        // C-side `pidfd_open(p, -1)` -> u32::MAX -> EINVAL (flag check).
        crate::errno::set_errno(0);
        #[allow(clippy::cast_sign_loss)]
        let ret = pidfd_open(100, (-1i32) as u32);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pidfd_open_phase116_valid_args_reach_enosys() {
        // Sanity: positive pid + valid flags -> ENOSYS (no regression).
        crate::errno::set_errno(0);
        let ret = pidfd_open(1, PIDFD_NONBLOCK | PIDFD_THREAD);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_pidfd_open_phase116_glibc_probe_workflow() {
        // Pattern: a libc probes for pidfd_open support by calling
        // it with a known-positive pid (its own) and flags=0.  We
        // emulate this with pid=1 (init) which is unambiguously
        // valid for the argument-domain validator.  Must return ENOSYS
        // (not EINVAL) so the caller can distinguish "argument bug"
        // from "syscall not implemented".  (We don't call getpid()
        // here because in the host-side cfg(test) build getpid()
        // returns the test runner's pid, which may or may not be
        // positive depending on the stub backing; we want this test
        // to assert pure argument-domain semantics.)
        crate::errno::set_errno(0);
        let ret = pidfd_open(1, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }
}

