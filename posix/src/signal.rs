//! POSIX signal handling layer.
//!
//! Our OS uses IPC messages instead of Unix signals for process
//! control.  This module provides the POSIX signal constants, handler
//! registration, signal sets, and `sigaction` so that C programs can
//! link and run.
//!
//! ## Design
//!
//! `signal()` and `sigaction()` store handlers in a static table.
//! `raise()` and `kill(self, sig)` dispatch through these handlers
//! via `dispatch_self_signal()`: SIG_IGN discards the signal, a
//! registered handler is invoked, and SIG_DFL applies the Linux
//! default action (terminate, ignore, stop, continue).
//!
//! Cross-process `kill()` translates terminating signals into
//! `SYS_PROCESS_KILL`, ignore signals are silently discarded, and
//! stop/continue signals return `ENOSYS` (no kernel suspend yet).
//!
//! `sigprocmask()` stores the blocked mask so get/set round-trips
//! work.  The blocked mask does not yet affect actual delivery.

use crate::errno;

// ---------------------------------------------------------------------------
// Signal numbers (Linux x86_64 compatible)
// ---------------------------------------------------------------------------

pub const SIGHUP: i32 = 1;
pub const SIGINT: i32 = 2;
pub const SIGQUIT: i32 = 3;
pub const SIGILL: i32 = 4;
pub const SIGTRAP: i32 = 5;
pub const SIGABRT: i32 = 6;
pub const SIGBUS: i32 = 7;
pub const SIGFPE: i32 = 8;
pub const SIGKILL: i32 = 9;
pub const SIGUSR1: i32 = 10;
pub const SIGSEGV: i32 = 11;
pub const SIGUSR2: i32 = 12;
pub const SIGPIPE: i32 = 13;
pub const SIGALRM: i32 = 14;
pub const SIGTERM: i32 = 15;
pub const SIGCHLD: i32 = 17;
pub const SIGCONT: i32 = 18;
pub const SIGSTOP: i32 = 19;
pub const SIGTSTP: i32 = 20;
/// Terminal input for background process.
pub const SIGTTIN: i32 = 21;
/// Terminal output for background process.
pub const SIGTTOU: i32 = 22;
/// Urgent condition on socket.
pub const SIGURG: i32 = 23;
/// CPU time limit exceeded.
pub const SIGXCPU: i32 = 24;
/// File size limit exceeded.
pub const SIGXFSZ: i32 = 25;
/// Virtual timer expired.
pub const SIGVTALRM: i32 = 26;
/// Profiling timer expired.
pub const SIGPROF: i32 = 27;
/// Window size change.
pub const SIGWINCH: i32 = 28;
/// I/O possible (same as SIGPOLL on Linux).
pub const SIGIO: i32 = 29;
/// Synonymous with SIGIO.
pub const SIGPOLL: i32 = 29;
/// Power failure.
pub const SIGPWR: i32 = 30;
/// Bad system call.
pub const SIGSYS: i32 = 31;

/// Number of signals (for sigset_t sizing).
pub const NSIG: i32 = 65;

// ---------------------------------------------------------------------------
// Signal handler types
// ---------------------------------------------------------------------------

/// Signal handler function pointer type.
pub type SighandlerT = usize; // Actually fn(i32), but usize for SIG_DFL/SIG_IGN.

/// Default signal action.
pub const SIG_DFL: SighandlerT = 0;
/// Ignore signal.
pub const SIG_IGN: SighandlerT = 1;
/// Error return from signal().
pub const SIG_ERR: SighandlerT = usize::MAX;

// ---------------------------------------------------------------------------
// sigprocmask `how` argument constants
// ---------------------------------------------------------------------------

/// Add signals to the blocked set.
pub const SIG_BLOCK: i32 = 0;
/// Remove signals from the blocked set.
pub const SIG_UNBLOCK: i32 = 1;
/// Replace the blocked set entirely.
pub const SIG_SETMASK: i32 = 2;

// ---------------------------------------------------------------------------
// Signal functions (stubs)
// ---------------------------------------------------------------------------

/// Default sigaction (SIG_DFL, no flags, empty mask).
const DEFAULT_SIGACTION: Sigaction = Sigaction {
    sa_handler: SIG_DFL,
    sa_flags: 0,
    sa_restorer: 0,
    sa_mask: SigsetT::EMPTY,
};

/// Registered signal actions.
///
/// Index 0 unused (signals are 1-based).  Initialized to SIG_DFL.
/// Stores the full `Sigaction` so that `sigaction(sig, NULL, &old)`
/// returns the correct `sa_mask`, `sa_flags`, and `sa_restorer`.
static mut ACTIONS: [Sigaction; NSIG as usize] = [DEFAULT_SIGACTION; NSIG as usize];

/// Process-wide blocked signal mask.
///
/// Updated by `sigprocmask`.  Read back by `sigprocmask` (old mask)
/// and `sigpending` (which returns the intersection of blocked and
/// pending signals — but since we have no signal delivery, pending
/// is always empty, so `sigpending` still returns empty).
///
/// Storing the mask is important for programs that do
/// `sigprocmask(SIG_BLOCK, ..., &old)` and later restore with
/// `sigprocmask(SIG_SETMASK, &old, NULL)` — the old mask must
/// round-trip correctly.
static mut BLOCKED_MASK: SigsetT = SigsetT::EMPTY;

/// Install a signal handler.
///
/// Stores the handler and returns the previous one.  Handlers are
/// never actually invoked since our OS doesn't deliver Unix signals.
///
/// POSIX: `signal()` is equivalent to `sigaction()` with
/// implementation-defined `sa_flags`.  We reset `sa_mask` and
/// `sa_flags` to zero (similar to BSD semantics without SA_RESTART).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn signal(signum: i32, handler: SighandlerT) -> SighandlerT {
    if !(1..NSIG).contains(&signum) || signum == SIGKILL || signum == SIGSTOP {
        errno::set_errno(errno::EINVAL);
        return SIG_ERR;
    }

    // SAFETY: Single-threaded access. signum range checked above.
    let idx = signum as usize;
    let actions = unsafe { core::ptr::addr_of_mut!(ACTIONS).as_mut() };
    let Some(actions) = actions else {
        errno::set_errno(errno::EINVAL);
        return SIG_ERR;
    };
    let Some(slot) = actions.get_mut(idx) else {
        errno::set_errno(errno::EINVAL);
        return SIG_ERR;
    };
    let old = slot.sa_handler;
    slot.sa_handler = handler;
    slot.sa_flags = 0;
    slot.sa_restorer = 0;
    slot.sa_mask = SigsetT::EMPTY;
    old
}

// ---------------------------------------------------------------------------
// sigset_t — signal set (128 bytes to match glibc x86_64)
// ---------------------------------------------------------------------------

/// Signal set type (matches glibc `sigset_t` = 128 bytes = 1024 bits).
///
/// Each bit represents one signal: signal N is at
/// `bits[(N-1)/64]`, bit `(N-1) % 64`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SigsetT {
    /// Bitmask storage.
    pub bits: [u64; 16],
}

impl SigsetT {
    /// Empty signal set.
    pub const EMPTY: Self = Self { bits: [0; 16] };
}

// ---------------------------------------------------------------------------
// sigaction structure (must match glibc x86_64 layout: 152 bytes)
// ---------------------------------------------------------------------------

/// `sigaction` structure for `sigaction()`.
///
/// Field order matches glibc x86_64 (`struct sigaction`):
///   sa_handler (8) + sa_flags (8) + sa_restorer (8) + sa_mask (128) = 152 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Sigaction {
    /// Signal handler (sa_handler or sa_sigaction).
    pub sa_handler: SighandlerT,
    /// Flags (SA_RESTART, SA_NOCLDSTOP, etc.).
    ///
    /// glibc uses `unsigned long` (u64 on x86_64).
    pub sa_flags: u64,
    /// Restore handler (used by the kernel's signal trampoline).
    pub sa_restorer: usize,
    /// Additional signals to block during handler execution.
    pub sa_mask: SigsetT,
}

/// Flags for sigaction (type u64 to match `unsigned long` sa_flags).
pub const SA_NOCLDSTOP: u64 = 1;
pub const SA_NOCLDWAIT: u64 = 2;
pub const SA_SIGINFO: u64 = 4;
pub const SA_ONSTACK: u64 = 0x0800_0000;
pub const SA_RESTART: u64 = 0x1000_0000;
pub const SA_NODEFER: u64 = 0x4000_0000;
pub const SA_RESETHAND: u64 = 0x8000_0000;

/// Examine and change a signal action.
///
/// Stores the new action (if provided) and returns the old action
/// (including `sa_mask`, `sa_flags`, and `sa_restorer`).
/// Handlers are never actually invoked.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn sigaction(
    signum: i32,
    act: *const Sigaction,
    oldact: *mut Sigaction,
) -> i32 {
    if !(1..NSIG).contains(&signum) || signum == SIGKILL || signum == SIGSTOP {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let idx = signum as usize;

    // Return old action via oldact.
    if !oldact.is_null() {
        // SAFETY: ACTIONS is single-threaded; idx in [1, NSIG).
        let old = unsafe {
            let actions = core::ptr::addr_of!(ACTIONS);
            (*actions).get(idx).copied().unwrap_or(DEFAULT_SIGACTION)
        };
        unsafe {
            (*oldact).sa_handler = old.sa_handler;
            (*oldact).sa_mask = old.sa_mask;
            (*oldact).sa_flags = old.sa_flags;
            (*oldact).sa_restorer = old.sa_restorer;
        }
    }

    // Store new action from act.
    if !act.is_null() {
        let new_act = unsafe { *act };
        // SAFETY: ACTIONS is single-threaded; idx in [1, NSIG).
        let actions = unsafe { core::ptr::addr_of_mut!(ACTIONS).as_mut() };
        if let Some(actions) = actions
            && let Some(slot) = actions.get_mut(idx)
        {
            *slot = new_act;
        }
    }

    0
}

// ---------------------------------------------------------------------------
// Default signal action classification
// ---------------------------------------------------------------------------

/// Linux default signal disposition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DefaultAction {
    /// Terminate the process.
    Terminate,
    /// Terminate with core dump (we treat as Terminate — no core support).
    Core,
    /// Ignore the signal (do nothing).
    Ignore,
    /// Stop the process (SIGSTOP, SIGTSTP, SIGTTIN, SIGTTOU).
    Stop,
    /// Continue the process (SIGCONT).
    Continue,
}

/// Return the Linux default action for a signal, or `None` for
/// out-of-range signal numbers.  Based on `signal(7)`.
fn default_action(sig: i32) -> Option<DefaultAction> {
    match sig {
        SIGHUP | SIGINT | SIGPIPE | SIGALRM | SIGTERM | SIGUSR1
        | SIGUSR2 | SIGVTALRM | SIGPROF | SIGIO | SIGPWR => {
            Some(DefaultAction::Terminate)
        }
        SIGQUIT | SIGILL | SIGTRAP | SIGABRT | SIGBUS | SIGFPE
        | SIGSEGV | SIGXCPU | SIGXFSZ | SIGSYS => {
            Some(DefaultAction::Core)
        }
        SIGCHLD | SIGURG | SIGWINCH => Some(DefaultAction::Ignore),
        SIGSTOP | SIGTSTP | SIGTTIN | SIGTTOU => Some(DefaultAction::Stop),
        SIGCONT => Some(DefaultAction::Continue),
        _ if (1..NSIG).contains(&sig) => {
            // RT signals (32..64) default to Terminate on Linux.
            Some(DefaultAction::Terminate)
        }
        _ => None,
    }
}

/// Dispatch a signal to the calling process.
///
/// Checks the registered `sigaction` table:
/// * `SIG_IGN` → signal discarded, returns 0.
/// * Registered handler → invokes it, returns 0.
/// * `SIG_DFL` → applies the default action (terminate, ignore, etc.).
///
/// For `SIGKILL` and `SIGSTOP`, handlers are ignored (Linux semantics:
/// they cannot be caught, blocked, or ignored).
///
/// **Returns** 0 on success, -1 on unimplemented action (errno = ENOSYS).
fn dispatch_self_signal(sig: i32) -> i32 {
    // SIGKILL / SIGSTOP: always apply default, regardless of handler.
    if sig == SIGKILL {
        // 128 + 9 = 137
        crate::process::_exit(128i32.wrapping_add(sig));
    }
    if sig == SIGSTOP {
        // We have no kernel suspend mechanism; report ENOSYS.
        errno::set_errno(errno::ENOSYS);
        return -1;
    }

    // Look up the registered action.
    let handler = if (1..NSIG).contains(&sig) {
        let idx = sig as usize;
        // SAFETY: single-threaded access, idx in [1, NSIG).
        unsafe {
            let actions = core::ptr::addr_of!(ACTIONS);
            (*actions)
                .get(idx)
                .map(|a| a.sa_handler)
                .unwrap_or(SIG_DFL)
        }
    } else {
        SIG_DFL
    };

    if handler == SIG_IGN {
        return 0;
    }

    if handler != SIG_DFL {
        // Invoke the registered handler.  POSIX: the handler receives
        // the signal number.  We cast the stored usize back to a
        // function pointer.  The handler may call longjmp or modify
        // global state — that's fine.
        //
        // SAFETY: the caller registered this via signal()/sigaction()
        // as a valid fn(i32).  We trust they provided a valid pointer.
        let func: extern "C" fn(i32) =
            unsafe { core::mem::transmute::<usize, extern "C" fn(i32)>(handler) };
        func(sig);
        return 0;
    }

    // SIG_DFL: apply default action.
    match default_action(sig) {
        Some(DefaultAction::Terminate | DefaultAction::Core) => {
            if sig == SIGABRT {
                crate::unistd::abort();
            }
            // 128 + sig (Unix convention)
            crate::process::_exit(128i32.wrapping_add(sig));
        }
        Some(DefaultAction::Ignore) => 0,
        Some(DefaultAction::Stop | DefaultAction::Continue) => {
            // No kernel suspend/resume mechanism yet.
            errno::set_errno(errno::ENOSYS);
            -1
        }
        None => {
            errno::set_errno(errno::EINVAL);
            -1
        }
    }
}

/// Send a signal to a process.
///
/// ## Signal delivery model (Phase 211)
///
/// Our OS uses IPC messages instead of Unix signals, but the POSIX
/// layer translates `kill()` into native operations:
///
/// * `sig == 0` — pure existence check via `SYS_PROCESS_IS_READY`.
/// * **Self-signals** (`pid == self`): dispatched locally via
///   `dispatch_self_signal()`, which invokes registered handlers
///   or applies the Linux default action (terminate, ignore, etc.).
/// * **Cross-process terminating signals** (`pid > 0`, default action
///   is Terminate or Core): translated to `SYS_PROCESS_KILL(pid,
///   128 + sig)`.  The target process is forcefully terminated with
///   the conventional Unix exit code.
/// * **Cross-process ignore signals**: silently discarded (return 0).
/// * **Stop/Continue signals**: not yet supported (`ENOSYS`).
/// * `pid <= 0` (process groups): not supported (`ENOSYS`).
///
/// ## Capability gate (Phase 203)
///
/// Non-self `kill()` with `pid > 0` requires `CAP_KILL` (matches
/// Linux's `check_kill_permission()` → `kill_ok_by_cred()`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn kill(pid: i32, sig: i32) -> i32 {
    // sig == 0 is a pure existence/permission check; honour it.
    if sig == 0 {
        if pid <= 0 {
            // pid == 0 → "every process in the caller's process group",
            // pid == -1 → "every process you may signal", pid < -1 →
            // "process group |pid|".  None of these are supported here.
            errno::set_errno(errno::ENOSYS);
            return -1;
        }
        // SYS_PROCESS_IS_READY: returns 1 if ready, 0 if alive but not
        // yet ready, negative error if the PID is unknown.  For an
        // existence check we collapse {0, 1} to success.
        let ret = crate::syscall::syscall1(
            crate::syscall::SYS_PROCESS_IS_READY,
            pid as u64,
        );
        if ret < 0 {
            errno::set_errno(errno::ESRCH);
            return -1;
        }
        return 0;
    }

    // Validate sig number.  Linux returns EINVAL for out-of-range.
    if !(1..NSIG).contains(&sig) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Determine if this is a self-signal.
    let self_pid = crate::syscall::syscall0(
        crate::syscall::SYS_PROCESS_ID,
    ) as i32;

    if pid == self_pid {
        return dispatch_self_signal(sig);
    }

    // --- Cross-process signal delivery ---

    if pid <= 0 {
        // Process group signaling — not supported.
        errno::set_errno(errno::ENOSYS);
        return -1;
    }

    // Phase 203: CAP_KILL gate for cross-process signals.
    if !crate::sys_capability::has_capability(
        crate::sys_capability::CAP_KILL,
    ) {
        errno::set_errno(errno::EPERM);
        return -1;
    }

    // Look up the default action for this signal.
    let action = match default_action(sig) {
        Some(a) => a,
        None => {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
    };

    match action {
        DefaultAction::Terminate | DefaultAction::Core => {
            // Translate to native process termination.
            // Exit code = 128 + signal number (Unix convention).
            let exit_code = 128i32.wrapping_add(sig);
            let ret = crate::syscall::syscall2(
                crate::syscall::SYS_PROCESS_KILL,
                pid as u64,
                exit_code as u64,
            );
            if ret < 0 {
                // Map kernel errors to POSIX errno.
                // NoSuchProcess → ESRCH, PermissionDenied → EPERM,
                // anything else → ESRCH (conservative).
                errno::set_errno(errno::ESRCH);
                return -1;
            }
            0
        }
        DefaultAction::Ignore => {
            // Cross-process ignore: silently discard (POSIX: success).
            0
        }
        DefaultAction::Stop | DefaultAction::Continue => {
            // No kernel suspend/resume support yet.
            errno::set_errno(errno::ENOSYS);
            -1
        }
    }
}

/// Send a signal to the calling process / calling thread.
///
/// POSIX `raise(sig)`:
/// * Returns 0 on success, non-zero on error (errno set).
/// * Dispatches via `dispatch_self_signal()`, which checks the
///   registered handler table and applies the appropriate action.
///
/// Errors (Linux-matching):
/// * `EINVAL` — `sig` is not a valid signal number.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn raise(sig: i32) -> i32 {
    if !(1..NSIG).contains(&sig) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    dispatch_self_signal(sig)
}

/// Examine and change blocked signals.
///
/// Stores the signal mask in process-local state so that
/// `sigprocmask(SIG_BLOCK, ..., &old)` followed by
/// `sigprocmask(SIG_SETMASK, &old, NULL)` round-trips correctly.
///
/// Signal delivery is not implemented (our OS uses IPC), so the
/// mask only affects what `sigprocmask` returns, not actual behavior.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sigprocmask(
    how: i32,
    set: *const SigsetT,
    oldset: *mut SigsetT,
) -> i32 {
    // SAFETY: single-threaded access to BLOCKED_MASK.
    let current = unsafe { core::ptr::addr_of!(BLOCKED_MASK).read() };

    // Return old mask if requested.
    if !oldset.is_null() {
        // SAFETY: oldset verified non-null.
        unsafe { *oldset = current; }
    }

    // Apply new mask if set is non-null.
    if !set.is_null() {
        // SAFETY: set verified non-null.
        let new_set = unsafe { *set };
        let new_mask = match how {
            SIG_BLOCK => {
                // Add signals in `set` to the blocked set.
                let mut result = current;
                let mut i = 0;
                while i < 16 {
                    result.bits[i] |= new_set.bits[i];
                    i = i.wrapping_add(1);
                }
                result
            }
            SIG_UNBLOCK => {
                // Remove signals in `set` from the blocked set.
                let mut result = current;
                let mut i = 0;
                while i < 16 {
                    result.bits[i] &= !new_set.bits[i];
                    i = i.wrapping_add(1);
                }
                result
            }
            SIG_SETMASK => {
                // Replace the blocked set entirely.
                new_set
            }
            _ => {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
        };
        // SAFETY: single-threaded access.
        unsafe { core::ptr::addr_of_mut!(BLOCKED_MASK).write(new_mask); }
    }

    0
}

/// Examine and change the signal mask of the calling thread.
///
/// Identical to `sigprocmask` in our single-threaded implementation.
/// POSIX specifies that `pthread_sigmask` is the thread-safe version
/// of `sigprocmask`.
///
/// Returns 0 on success, or an error number directly (not via errno).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_sigmask(
    how: i32,
    set: *const SigsetT,
    oldset: *mut SigsetT,
) -> i32 {
    // In our single-threaded model, pthread_sigmask is identical to
    // sigprocmask.  The only difference per POSIX is that
    // pthread_sigmask returns the error code directly instead of
    // returning -1 and setting errno.
    let ret = sigprocmask(how, set, oldset);
    if ret < 0 {
        // sigprocmask sets errno — extract it as the return value.
        errno::get_errno()
    } else {
        0
    }
}

/// Wait for a signal.
///
/// Stub: sets errno to EINTR and returns -1 (POSIX specifies
/// sigsuspend always returns -1 with errno=EINTR).
///
/// Errors (Linux-matching priority):
/// * `EFAULT` — `mask` is NULL.  Linux's `sys_rt_sigsuspend` copies
///   the mask via `copy_from_user` before doing anything else, so a
///   NULL pointer faults with `EFAULT` and we return that error in
///   preference to the default `EINTR`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sigsuspend(mask: *const SigsetT) -> i32 {
    if mask.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    errno::set_errno(errno::EINTR);
    -1
}

/// Examine pending signals.
///
/// Stub: returns empty set (no signals pending).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn sigpending(set: *mut SigsetT) -> i32 {
    if set.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    unsafe { *set = SigsetT::EMPTY; }
    0
}

/// Initialize a signal set to empty.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn sigemptyset(set: *mut SigsetT) -> i32 {
    if set.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    unsafe { *set = SigsetT::EMPTY; }
    0
}

/// Initialize a signal set to full.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn sigfillset(set: *mut SigsetT) -> i32 {
    if set.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    unsafe { (*set).bits = [u64::MAX; 16]; }
    0
}

/// Add a signal to a signal set.
///
/// Errors (Linux-matching):
/// * `EFAULT` — `set` is NULL (glibc would segfault; kernel uses
///   `copy_to_user` → `-EFAULT`).
/// * `EINVAL` — `signum` is out of the valid range `[1, NSIG)`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn sigaddset(set: *mut SigsetT, signum: i32) -> i32 {
    if set.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if !(1..NSIG).contains(&signum) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let idx = (signum - 1) as usize;
    let word = idx / 64;
    let bit = idx % 64;
    // SAFETY: set is non-null, word < 1 for standard signals (< 65).
    unsafe { (*set).bits[word] |= 1u64 << bit; }
    0
}

/// Remove a signal from a signal set.
///
/// Errors (Linux-matching):
/// * `EFAULT` — `set` is NULL.
/// * `EINVAL` — `signum` is out of range.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn sigdelset(set: *mut SigsetT, signum: i32) -> i32 {
    if set.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if !(1..NSIG).contains(&signum) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let idx = (signum - 1) as usize;
    let word = idx / 64;
    let bit = idx % 64;
    // SAFETY: set is non-null.
    unsafe { (*set).bits[word] &= !(1u64 << bit); }
    0
}

/// Test whether a signal is in a signal set.
///
/// Errors (Linux-matching):
/// * `EFAULT` — `set` is NULL.
/// * `EINVAL` — `signum` is out of range.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn sigismember(set: *const SigsetT, signum: i32) -> i32 {
    if set.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if !(1..NSIG).contains(&signum) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let idx = (signum - 1) as usize;
    let word = idx / 64;
    let bit = idx % 64;
    // SAFETY: set is non-null.
    let val = unsafe { (*set).bits[word] };
    i32::from(val & (1u64 << bit) != 0)
}

// ---------------------------------------------------------------------------
// sigaltstack — alternate signal stack
// ---------------------------------------------------------------------------

/// Minimum alternate signal stack size (POSIX `MINSIGSTKSZ`).
pub const MINSIGSTKSZ: usize = 2048;
/// Default alternate signal stack size (POSIX `SIGSTKSZ`).
pub const SIGSTKSZ: usize = 8192;

/// Flags for `stack_t`.
pub const SS_ONSTACK: i32 = 1;
/// Alternate stack is disabled.
pub const SS_DISABLE: i32 = 2;
/// Auto-disarm the alternate stack on entry to a handler (Linux
/// extension, `SS_AUTODISARM` in `<bits/sigstack.h>`).  Logically OR-ed
/// with `SS_ONSTACK` or `SS_DISABLE`; the kernel masks it off before
/// classifying the mode.
pub const SS_AUTODISARM: i32 = 1 << 31;

/// Alternate signal stack descriptor.
///
/// Layout matches Linux x86_64 for binary compatibility.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct StackT {
    /// Base address of the alternate stack.
    pub ss_sp: *mut u8,
    /// Flags (`SS_ONSTACK`, `SS_DISABLE`).
    pub ss_flags: i32,
    /// Size of the alternate stack in bytes.
    pub ss_size: usize,
}

/// Set and/or get the alternate signal stack.
///
/// Stub: our OS doesn't deliver Unix signals, so there is no signal
/// stack to configure.  If `oss` is non-null, we report SS_DISABLE.
/// If `ss` is non-null, we accept the configuration silently.
///
/// Returns 0 on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sigaltstack(ss: *const StackT, oss: *mut StackT) -> i32 {
    // Return old stack state if requested.
    if !oss.is_null() {
        // SAFETY: oss is valid (caller contract).
        unsafe {
            (*oss).ss_sp = core::ptr::null_mut();
            (*oss).ss_flags = SS_DISABLE;
            (*oss).ss_size = 0;
        }
    }

    // Validate new stack if provided.
    if !ss.is_null() {
        let new_ss = unsafe { &*ss };
        // Linux `do_sigaltstack` strips the SS_AUTODISARM bit and then
        // requires the remaining mode to be exactly one of:
        // {0 (== SS_ONSTACK semantics), SS_ONSTACK, SS_DISABLE}.
        // Anything else (including bits like SS_ONSTACK|SS_DISABLE
        // together) is `EINVAL`.  We validate this *before* the size
        // check so that a caller passing nonsense flags doesn't get
        // the size-related ENOMEM by accident.
        let mode = new_ss.ss_flags & !SS_AUTODISARM;
        if mode != 0 && mode != SS_ONSTACK && mode != SS_DISABLE {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
        // POSIX: if ss_flags does not contain SS_DISABLE, and the stack
        // size is below MINSIGSTKSZ, return ENOMEM.
        if new_ss.ss_flags & SS_DISABLE == 0 && new_ss.ss_size < MINSIGSTKSZ {
            errno::set_errno(errno::ENOMEM);
            return -1;
        }
        // Accept silently — we don't actually use the alternate stack.
    }

    0
}

// ---------------------------------------------------------------------------
// siginterrupt — allow signals to interrupt system calls
// ---------------------------------------------------------------------------

/// Control whether a signal interrupts system calls.
///
/// If `flag` is nonzero, system calls interrupted by `sig` will return
/// -1 with `EINTR`.  If zero, system calls are automatically restarted.
///
/// Stub: validates `sig` against the standard signal range, then
/// returns 0.  Since our OS doesn't deliver signals, there is no
/// SA_RESTART behavior to toggle once the validation passes.
///
/// Errors (Linux-matching, via glibc's `siginterrupt` implementation
/// which internally calls `sigaction`):
/// * `EINVAL` — `sig` is not in `[1, NSIG)`, or is `SIGKILL` or
///   `SIGSTOP` (those two cannot have their action changed).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn siginterrupt(sig: i32, _flag: i32) -> i32 {
    if !(1..NSIG).contains(&sig) || sig == SIGKILL || sig == SIGSTOP {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // No signal delivery — nothing to configure.
    0
}

// ---------------------------------------------------------------------------
// strsignal / psignal
// ---------------------------------------------------------------------------

/// Signal name table.
///
/// Index by signal number.  Covers the standard Linux x86_64 signals.
static SIGNAL_NAMES: [&[u8]; 32] = [
    b"Unknown signal 0\0",  // 0
    b"Hangup\0",            // 1  SIGHUP
    b"Interrupt\0",         // 2  SIGINT
    b"Quit\0",              // 3  SIGQUIT
    b"Illegal instruction\0", // 4  SIGILL
    b"Trace/breakpoint trap\0", // 5  SIGTRAP
    b"Aborted\0",           // 6  SIGABRT
    b"Bus error\0",         // 7  SIGBUS
    b"Floating point exception\0", // 8  SIGFPE
    b"Killed\0",            // 9  SIGKILL
    b"User defined signal 1\0", // 10 SIGUSR1
    b"Segmentation fault\0", // 11 SIGSEGV
    b"User defined signal 2\0", // 12 SIGUSR2
    b"Broken pipe\0",       // 13 SIGPIPE
    b"Alarm clock\0",       // 14 SIGALRM
    b"Terminated\0",        // 15 SIGTERM
    b"Stack fault\0",       // 16 SIGSTKFLT (unused on modern Linux x86_64)
    b"Child exited\0",      // 17 SIGCHLD
    b"Continued\0",         // 18 SIGCONT
    b"Stopped (signal)\0",  // 19 SIGSTOP
    b"Stopped\0",           // 20 SIGTSTP
    b"Stopped (tty input)\0",  // 21 SIGTTIN
    b"Stopped (tty output)\0", // 22 SIGTTOU
    b"Urgent I/O condition\0", // 23 SIGURG
    b"CPU time limit exceeded\0", // 24 SIGXCPU
    b"File size limit exceeded\0", // 25 SIGXFSZ
    b"Virtual timer expired\0",   // 26 SIGVTALRM
    b"Profiling timer expired\0", // 27 SIGPROF
    b"Window changed\0",    // 28 SIGWINCH
    b"I/O possible\0",      // 29 SIGIO/SIGPOLL
    b"Power failure\0",     // 30 SIGPWR
    b"Bad system call\0",   // 31 SIGSYS
];

/// Unknown signal message buffer.
///
/// Used when the signal number is out of range.  Not reentrant but
/// matches POSIX specification.
static UNKNOWN_SIGNAL: [u8; 32] = *b"Unknown signal\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";

/// Return a string describing a signal number.
///
/// The returned pointer is valid until the next call to `strsignal`.
/// Not thread-safe (matches POSIX spec).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn strsignal(signum: i32) -> *const u8 {
    if signum >= 0
        && (signum as usize) < SIGNAL_NAMES.len()
        && let Some(name) = SIGNAL_NAMES.get(signum as usize)
    {
        return name.as_ptr();
    }
    UNKNOWN_SIGNAL.as_ptr()
}

/// Print a signal description to stderr.
///
/// If `s` is non-null and non-empty, prints "s: signal_desc\n".
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn psignal(signum: i32, s: *const u8) {
    if !s.is_null() && unsafe { *s } != 0 {
        let slen = unsafe { crate::string::strlen(s) };
        let _ = crate::file::write(2, s, slen);
        let _ = crate::file::write(2, c": ".as_ptr().cast::<u8>(), 2);
    }

    let msg = strsignal(signum);
    let mlen = unsafe { crate::string::strlen(msg) };
    let _ = crate::file::write(2, msg, mlen);

    let nl = b'\n';
    let _ = crate::file::write(2, &raw const nl, 1);
}

// ---------------------------------------------------------------------------
// sigwait / sigtimedwait / sigqueue — stubs
// ---------------------------------------------------------------------------

/// Wait for a signal from a set.
///
/// Stub: our OS doesn't deliver signals.  Sleeps for 1 second then
/// returns `EINTR` (wait interrupted, no signal delivered).
///
/// `sigwait` reports errors via its return value (positive errno),
/// **not** via `errno`.  POSIX requires the function to return zero on
/// success and a positive error number on failure.
///
/// Errors (Linux-matching priority, via glibc's `sigwait` wrapper
/// around `sigtimedwait`/`rt_sigtimedwait`):
/// * `EFAULT` — `set` is NULL (the kernel copies it via
///   `copy_from_user`, which faults).  Validated before any sleep so a
///   buggy caller doesn't silently block for a second first.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sigwait(set: *const SigsetT, sig: *mut i32) -> i32 {
    if set.is_null() {
        // sigwait returns its error code via the return value, not errno.
        return crate::errno::EFAULT;
    }
    // Sleep briefly so callers in a loop don't spin.
    let _ = crate::syscall::syscall1(crate::syscall::SYS_SLEEP, 1_000_000_000_u64);
    if !sig.is_null() {
        // SAFETY: sig is valid if non-null (caller contract).
        unsafe { *sig = 0; }
    }
    crate::errno::EINTR
}

/// Maximum valid value for `timespec.tv_nsec` (one less than a second
/// in nanoseconds).  Anything outside `[0, NSEC_PER_SEC)` is `EINVAL`
/// per Linux's `kernel/time/posix-timers.c::do_sigtimedwait`.
pub const SIGTIMEDWAIT_NSEC_MAX: i64 = 999_999_999;

/// Wait for a signal with a timeout.
///
/// Stub: validates arguments per Linux `kernel/signal.c::do_sigtimedwait`,
/// then returns `-1` with `EAGAIN` (timeout expired, no signal delivered).
///
/// Errors (Linux-matching priority order):
/// * `EFAULT` — `set` is NULL (kernel copies it into a kernel sigset
///   via `copy_from_user`; NULL faults immediately).
/// * `EINVAL` — `timeout` is non-NULL and contains a negative `tv_sec`
///   or an out-of-range `tv_nsec` (must be in `[0, 999_999_999]`).
///
/// Behaviour notes:
/// * A NULL `timeout` is the "wait forever" form; we still surface
///   `EAGAIN` because no signal can ever be delivered in this stub.
/// * `info` may be NULL — POSIX explicitly allows callers that don't
///   care about siginfo to pass NULL.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sigtimedwait(
    set: *const SigsetT,
    _info: *mut core::ffi::c_void,
    timeout: *const crate::stat::Timespec,
) -> i32 {
    if set.is_null() {
        crate::errno::set_errno(crate::errno::EFAULT);
        return -1;
    }
    if !timeout.is_null() {
        // SAFETY: timeout was just confirmed non-NULL.  We read fields
        // by-value; alignment is the caller's responsibility per the
        // documented C ABI.
        let ts = unsafe { core::ptr::read_unaligned(timeout) };
        if ts.tv_sec < 0 || !(0..=SIGTIMEDWAIT_NSEC_MAX).contains(&ts.tv_nsec) {
            crate::errno::set_errno(crate::errno::EINVAL);
            return -1;
        }
    }
    crate::errno::set_errno(crate::errno::EAGAIN);
    -1
}

/// Queue a signal to a process with an attached `sigval`.
///
/// Stub: validates arguments per Linux
/// `kernel/signal.c::sys_rt_sigqueueinfo`, then returns `-1` with
/// `ENOSYS` (no signal delivery mechanism).
///
/// # Linux semantics
///
/// `sys_rt_sigqueueinfo` calls `do_rt_sigqueueinfo`, which rejects
/// `pid <= 0` with `-EINVAL` at the very top — *before* any task
/// lookup.  Unlike `kill(2)`, `sigqueue(3)` does not accept
/// process-group, "self", or "all-processes" forms; the target must
/// be a real positive PID.
///
/// # Capability gate (Phase 204)
///
/// Like `kill()`, Linux gates cross-uid signal delivery via
/// `check_kill_permission()` → `kill_ok_by_cred()`.  Since
/// `sigqueue` always targets a specific positive pid, the gate
/// runs after argument validation (EINVAL) for every well-formed
/// call.
///
/// Errors (Linux-matching priority order):
///
/// 1. `pid <= 0`                                       → `EINVAL`
///    (Linux: `do_rt_sigqueueinfo` — fires before `find_vpid`.)
/// 2. `sig < 0 || sig >= NSIG`                         → `EINVAL`
///    `sig == 0` is permitted (existence-probe form).  Linux
///    validates sig deep in `__send_signal_locked::valid_signal`,
///    after process lookup; here the stub mirrors that EINVAL value
///    without modelling process existence.
/// 3. `!CAP_KILL`                                      → `EPERM`
///    (Phase 204)
/// 4. `ENOSYS` for any otherwise-valid call.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sigqueue(pid: crate::types::PidT, sig: i32, _value: usize) -> i32 {
    // 1. pid <= 0 → EINVAL.  Linux's do_rt_sigqueueinfo rejects this
    //    before any task lookup, so it fires before sig validation.
    if pid <= 0 {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }
    // 2. sig validation: in Linux this lives inside the per-task send
    //    path and surfaces as EINVAL once the process is found.
    if !(0..NSIG).contains(&sig) {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }
    // 3. Phase 204: CAP_KILL gate — same contract as kill() Phase 203.
    //    sigqueue always targets a specific pid (no process-group
    //    forms), so the gate fires for every well-formed call.
    if !crate::sys_capability::has_capability(
        crate::sys_capability::CAP_KILL,
    ) {
        crate::errno::set_errno(crate::errno::EPERM);
        return -1;
    }
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// siginfo si_code values for SIGCHLD (used with waitid)
// ---------------------------------------------------------------------------

/// Child has exited.
pub const CLD_EXITED: i32 = 1;
/// Child was killed by a signal.
pub const CLD_KILLED: i32 = 2;
/// Child was killed by a signal and dumped core.
pub const CLD_DUMPED: i32 = 3;
/// Child was trapped (ptrace).
pub const CLD_TRAPPED: i32 = 4;
/// Child was stopped.
pub const CLD_STOPPED: i32 = 5;
/// Stopped child was continued.
pub const CLD_CONTINUED: i32 = 6;

// ---------------------------------------------------------------------------
// Realtime signal range
// ---------------------------------------------------------------------------

/// First realtime signal number (Linux x86_64).
///
/// Programs may reference `SIGRTMIN` as a constant.  On glibc this is
/// actually a function call (`__libc_current_sigrtmin()`) because glibc
/// reserves the first few RT signals for NPTL.  We expose both the
/// constant and the function.
pub const SIGRTMIN: i32 = 32;
/// Last realtime signal number (Linux x86_64).
pub const SIGRTMAX: i32 = 64;

/// glibc: return the lowest realtime signal number.
///
/// SIGRTMIN is typically 32 on Linux (signals 32-64 are realtime).
/// We don't support realtime signals, but programs that query the
/// range need valid values.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __libc_current_sigrtmin() -> i32 {
    32
}

/// glibc: return the highest realtime signal number.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __libc_current_sigrtmax() -> i32 {
    64
}

// ---------------------------------------------------------------------------
// siginfo_t — signal information structure
// ---------------------------------------------------------------------------

/// Signal information structure.
///
/// Matches the Linux x86_64 `siginfo_t` layout (128 bytes).
/// Only the common header fields are defined; the union payload
/// is represented as opaque padding.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SiginfoT {
    /// Signal number.
    pub si_signo: i32,
    /// Error number (errno value).
    pub si_errno: i32,
    /// Signal code (SI_USER, SI_KERNEL, CLD_*, etc.).
    pub si_code: i32,
    /// Padding/union payload (rest of 128 bytes).
    _pad: [u8; 116],
}

impl Default for SiginfoT {
    fn default() -> Self {
        // SAFETY: SiginfoT is a C struct, zero-init is valid.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// psiginfo — print signal info to stderr
// ---------------------------------------------------------------------------

/// `psiginfo` — print signal information to stderr.
///
/// Like `psignal`, but takes a `siginfo_t *` instead of a signal number.
/// Prints: `"<msg>: <signal-name>\n"` to stderr.
///
/// # Safety
///
/// `info` must point to a valid `SiginfoT` struct (or be null, in which
/// case "Unknown signal" is printed).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn psiginfo(info: *const SiginfoT, msg: *const u8) {
    let signum = if info.is_null() { 0 } else { unsafe { (*info).si_signo } };
    unsafe { psignal(signum, msg); }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Signal number constants match Linux x86_64 --

    #[test]
    fn test_signal_number_values() {
        assert_eq!(SIGHUP, 1);
        assert_eq!(SIGINT, 2);
        assert_eq!(SIGQUIT, 3);
        assert_eq!(SIGILL, 4);
        assert_eq!(SIGTRAP, 5);
        assert_eq!(SIGABRT, 6);
        assert_eq!(SIGBUS, 7);
        assert_eq!(SIGFPE, 8);
        assert_eq!(SIGKILL, 9);
        assert_eq!(SIGUSR1, 10);
        assert_eq!(SIGSEGV, 11);
        assert_eq!(SIGUSR2, 12);
        assert_eq!(SIGPIPE, 13);
        assert_eq!(SIGALRM, 14);
        assert_eq!(SIGTERM, 15);
        assert_eq!(SIGCHLD, 17);
        assert_eq!(SIGCONT, 18);
        assert_eq!(SIGSTOP, 19);
        assert_eq!(SIGTSTP, 20);
    }

    #[test]
    fn test_signal_number_values_extended() {
        assert_eq!(SIGTTIN, 21);
        assert_eq!(SIGTTOU, 22);
        assert_eq!(SIGURG, 23);
        assert_eq!(SIGXCPU, 24);
        assert_eq!(SIGXFSZ, 25);
        assert_eq!(SIGVTALRM, 26);
        assert_eq!(SIGPROF, 27);
        assert_eq!(SIGWINCH, 28);
        assert_eq!(SIGIO, 29);
        assert_eq!(SIGPOLL, 29); // synonym for SIGIO
        assert_eq!(SIGPWR, 30);
        assert_eq!(SIGSYS, 31);
    }

    #[test]
    fn test_nsig() {
        assert_eq!(NSIG, 65);
    }

    // -- Struct layout tests (binary compatibility with glibc x86_64) --

    #[test]
    fn test_sigset_t_layout() {
        assert_eq!(core::mem::size_of::<SigsetT>(), 128);
        assert_eq!(core::mem::align_of::<SigsetT>(), 8);
    }

    #[test]
    fn test_sigaction_layout() {
        // glibc x86_64: sa_handler(8) + sa_flags(8) + sa_restorer(8) + sa_mask(128) = 152
        assert_eq!(core::mem::size_of::<Sigaction>(), 152);
        assert_eq!(core::mem::offset_of!(Sigaction, sa_handler), 0);
        assert_eq!(core::mem::offset_of!(Sigaction, sa_flags), 8);
        assert_eq!(core::mem::offset_of!(Sigaction, sa_restorer), 16);
        assert_eq!(core::mem::offset_of!(Sigaction, sa_mask), 24);
    }

    #[test]
    fn test_stack_t_layout() {
        // Linux x86_64: ss_sp(8) + ss_flags(4) + padding(4) + ss_size(8) = 24
        assert_eq!(core::mem::size_of::<StackT>(), 24);
    }

    // -- Signal handler constants --

    #[test]
    fn test_sig_dfl_ign_err() {
        assert_eq!(SIG_DFL, 0);
        assert_eq!(SIG_IGN, 1);
        assert_eq!(SIG_ERR, usize::MAX);
    }

    // -- sigaction flags match Linux --

    #[test]
    fn test_sa_flag_values() {
        assert_eq!(SA_NOCLDSTOP, 1);
        assert_eq!(SA_NOCLDWAIT, 2);
        assert_eq!(SA_SIGINFO, 4);
        assert_eq!(SA_ONSTACK, 0x0800_0000);
        assert_eq!(SA_RESTART, 0x1000_0000);
        assert_eq!(SA_NODEFER, 0x4000_0000);
        assert_eq!(SA_RESETHAND, 0x8000_0000);
    }

    // -- sigaltstack constants --

    #[test]
    fn test_sigaltstack_constants() {
        assert_eq!(MINSIGSTKSZ, 2048);
        assert_eq!(SIGSTKSZ, 8192);
        assert_eq!(SS_ONSTACK, 1);
        assert_eq!(SS_DISABLE, 2);
    }

    // -- sigemptyset --

    #[test]
    fn test_sigemptyset_basic() {
        let mut set = SigsetT { bits: [0xFFFF_FFFF_FFFF_FFFF; 16] };
        let ret = unsafe { sigemptyset(&raw mut set) };
        assert_eq!(ret, 0);
        assert!(set.bits.iter().all(|&w| w == 0));
    }

    #[test]
    fn test_sigemptyset_null() {
        let ret = unsafe { sigemptyset(core::ptr::null_mut()) };
        assert_eq!(ret, -1);
    }

    // -- sigfillset --

    #[test]
    fn test_sigfillset_basic() {
        let mut set = SigsetT::EMPTY;
        let ret = unsafe { sigfillset(&raw mut set) };
        assert_eq!(ret, 0);
        assert!(set.bits.iter().all(|&w| w == u64::MAX));
    }

    #[test]
    fn test_sigfillset_null() {
        let ret = unsafe { sigfillset(core::ptr::null_mut()) };
        assert_eq!(ret, -1);
    }

    // -- sigaddset --

    #[test]
    fn test_sigaddset_basic() {
        let mut set = SigsetT::EMPTY;
        let ret = unsafe { sigaddset(&raw mut set, SIGINT) };
        assert_eq!(ret, 0);
        // SIGINT = 2 → bit 1 in word 0
        assert_eq!(set.bits[0], 1u64 << 1);
        assert!(set.bits[1..].iter().all(|&w| w == 0));
    }

    #[test]
    fn test_sigaddset_multiple() {
        let mut set = SigsetT::EMPTY;
        unsafe {
            sigaddset(&raw mut set, SIGHUP);   // bit 0
            sigaddset(&raw mut set, SIGTERM);   // bit 14
            sigaddset(&raw mut set, SIGKILL);   // bit 8
        }
        assert_ne!(set.bits[0] & (1u64 << 0), 0);   // SIGHUP
        assert_ne!(set.bits[0] & (1u64 << 14), 0);  // SIGTERM
        assert_ne!(set.bits[0] & (1u64 << 8), 0);   // SIGKILL
        // Only those three bits set in word 0
        assert_eq!(set.bits[0] & !(1u64 << 0 | 1u64 << 14 | 1u64 << 8), 0);
        assert!(set.bits[1..].iter().all(|&w| w == 0));
    }

    #[test]
    fn test_sigaddset_null() {
        let ret = unsafe { sigaddset(core::ptr::null_mut(), SIGINT) };
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_sigaddset_invalid_signum_zero() {
        let mut set = SigsetT::EMPTY;
        let ret = unsafe { sigaddset(&raw mut set, 0) };
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_sigaddset_invalid_signum_too_large() {
        let mut set = SigsetT::EMPTY;
        let ret = unsafe { sigaddset(&raw mut set, NSIG) };
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_sigaddset_invalid_signum_negative() {
        let mut set = SigsetT::EMPTY;
        let ret = unsafe { sigaddset(&raw mut set, -1) };
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_sigaddset_boundary_signal_1() {
        let mut set = SigsetT::EMPTY;
        let ret = unsafe { sigaddset(&raw mut set, 1) };
        assert_eq!(ret, 0);
        assert_eq!(set.bits[0], 1u64 << 0); // signal 1 → bit 0
    }

    #[test]
    fn test_sigaddset_boundary_signal_64() {
        let mut set = SigsetT::EMPTY;
        let ret = unsafe { sigaddset(&raw mut set, 64) };
        assert_eq!(ret, 0);
        // signal 64 → idx 63 → word 0, bit 63
        assert_eq!(set.bits[0], 1u64 << 63);
    }

    // -- sigdelset --

    #[test]
    fn test_sigdelset_basic() {
        let mut set = SigsetT { bits: [u64::MAX; 16] };
        let ret = unsafe { sigdelset(&raw mut set, SIGINT) };
        assert_eq!(ret, 0);
        assert_eq!(set.bits[0] & (1u64 << 1), 0); // SIGINT bit cleared
    }

    #[test]
    fn test_sigdelset_from_empty() {
        let mut set = SigsetT::EMPTY;
        let ret = unsafe { sigdelset(&raw mut set, SIGINT) };
        assert_eq!(ret, 0);
        assert!(set.bits.iter().all(|&w| w == 0));
    }

    #[test]
    fn test_sigdelset_null() {
        let ret = unsafe { sigdelset(core::ptr::null_mut(), SIGINT) };
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_sigdelset_invalid() {
        let mut set = SigsetT { bits: [u64::MAX; 16] };
        let ret = unsafe { sigdelset(&raw mut set, 0) };
        assert_eq!(ret, -1);
    }

    // -- sigismember --

    #[test]
    fn test_sigismember_present() {
        let mut set = SigsetT::EMPTY;
        set.bits[0] = 1u64 << 1; // SIGINT (signal 2 → bit 1)
        let ret = unsafe { sigismember(&raw const set, SIGINT) };
        assert_eq!(ret, 1);
    }

    #[test]
    fn test_sigismember_absent() {
        let set = SigsetT::EMPTY;
        let ret = unsafe { sigismember(&raw const set, SIGINT) };
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_sigismember_full_set() {
        let set = SigsetT { bits: [u64::MAX; 16] };
        for sig in 1..NSIG {
            let ret = unsafe { sigismember(&raw const set, sig) };
            assert_eq!(ret, 1, "signal {sig} should be in full set");
        }
    }

    #[test]
    fn test_sigismember_null() {
        let ret = unsafe { sigismember(core::ptr::null(), SIGINT) };
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_sigismember_invalid() {
        let set = SigsetT { bits: [u64::MAX; 16] };
        let ret = unsafe { sigismember(&raw const set, 0) };
        assert_eq!(ret, -1);
    }

    // -- Round-trip: add then check --

    #[test]
    fn test_sigaddset_then_sigismember() {
        let mut set = SigsetT::EMPTY;
        unsafe { sigemptyset(&raw mut set); }

        // Add SIGTERM, verify it's there
        unsafe { sigaddset(&raw mut set, SIGTERM); }
        assert_eq!(unsafe { sigismember(&raw const set, SIGTERM) }, 1);

        // SIGINT should still be absent
        assert_eq!(unsafe { sigismember(&raw const set, SIGINT) }, 0);
    }

    #[test]
    fn test_sigaddset_sigdelset_round_trip() {
        let mut set = SigsetT::EMPTY;
        unsafe {
            sigemptyset(&raw mut set);
            sigaddset(&raw mut set, SIGINT);
            sigaddset(&raw mut set, SIGTERM);
        }
        assert_eq!(unsafe { sigismember(&raw const set, SIGINT) }, 1);
        assert_eq!(unsafe { sigismember(&raw const set, SIGTERM) }, 1);

        // Remove SIGINT
        unsafe { sigdelset(&raw mut set, SIGINT); }
        assert_eq!(unsafe { sigismember(&raw const set, SIGINT) }, 0);
        assert_eq!(unsafe { sigismember(&raw const set, SIGTERM) }, 1);
    }

    #[test]
    fn test_sigfillset_then_delset() {
        let mut set = SigsetT::EMPTY;
        unsafe {
            sigfillset(&raw mut set);
            sigdelset(&raw mut set, SIGKILL);
        }
        assert_eq!(unsafe { sigismember(&raw const set, SIGKILL) }, 0);
        assert_eq!(unsafe { sigismember(&raw const set, SIGTERM) }, 1);
    }

    // -- signal() function --

    #[test]
    fn test_signal_set_handler() {
        // Reset to known state.
        let old = signal(SIGTERM, SIG_IGN);
        // old should be whatever was there before (SIG_DFL unless another test changed it)
        assert_ne!(old, SIG_ERR);

        // Now set it back
        let prev = signal(SIGTERM, SIG_DFL);
        assert_eq!(prev, SIG_IGN);
    }

    #[test]
    fn test_signal_rejects_sigkill() {
        let ret = signal(SIGKILL, SIG_IGN);
        assert_eq!(ret, SIG_ERR);
    }

    #[test]
    fn test_signal_rejects_sigstop() {
        let ret = signal(SIGSTOP, SIG_IGN);
        assert_eq!(ret, SIG_ERR);
    }

    #[test]
    fn test_signal_rejects_invalid_signum() {
        assert_eq!(signal(0, SIG_IGN), SIG_ERR);
        assert_eq!(signal(-1, SIG_IGN), SIG_ERR);
        assert_eq!(signal(NSIG, SIG_IGN), SIG_ERR);
    }

    #[test]
    fn test_signal_boundary_valid() {
        // Signal 1 (SIGHUP) should work
        let old = signal(SIGHUP, SIG_IGN);
        assert_ne!(old, SIG_ERR);
        signal(SIGHUP, old); // Restore
    }

    // -- sigaction --

    #[test]
    fn test_sigaction_set_and_get() {
        let mut mask = SigsetT::EMPTY;
        mask.bits[0] = 1u64 << (SIGINT - 1) | 1u64 << (SIGQUIT - 1);
        let new_act = Sigaction {
            sa_handler: SIG_IGN,
            sa_mask: mask,
            sa_flags: SA_RESTART,
            sa_restorer: 0,
        };
        let mut old_act = Sigaction {
            sa_handler: 0,
            sa_mask: SigsetT::EMPTY,
            sa_flags: 0,
            sa_restorer: 0,
        };

        let ret = unsafe { sigaction(SIGTERM, &raw const new_act, &raw mut old_act) };
        assert_eq!(ret, 0);

        // Now get it back — all fields must round-trip.
        let mut check_act = Sigaction {
            sa_handler: 0,
            sa_mask: SigsetT::EMPTY,
            sa_flags: 0,
            sa_restorer: 0,
        };
        let ret = unsafe { sigaction(SIGTERM, core::ptr::null(), &raw mut check_act) };
        assert_eq!(ret, 0);
        assert_eq!(check_act.sa_handler, SIG_IGN);
        assert_eq!(check_act.sa_flags, SA_RESTART);
        assert_eq!(check_act.sa_mask, mask);

        // Restore original
        unsafe { sigaction(SIGTERM, &raw const old_act, core::ptr::null_mut()); }
    }

    #[test]
    fn test_sigaction_rejects_sigkill() {
        let act = Sigaction {
            sa_handler: SIG_IGN,
            sa_mask: SigsetT::EMPTY,
            sa_flags: 0,
            sa_restorer: 0,
        };
        let ret = unsafe { sigaction(SIGKILL, &raw const act, core::ptr::null_mut()) };
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_sigaction_rejects_sigstop() {
        let act = Sigaction {
            sa_handler: SIG_IGN,
            sa_mask: SigsetT::EMPTY,
            sa_flags: 0,
            sa_restorer: 0,
        };
        let ret = unsafe { sigaction(SIGSTOP, &raw const act, core::ptr::null_mut()) };
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_sigaction_null_both() {
        // Both act and oldact null — should succeed (query nothing)
        let ret = unsafe { sigaction(SIGTERM, core::ptr::null(), core::ptr::null_mut()) };
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_sigaction_preserves_all_fields() {
        // Regression: previously only sa_handler was stored; sa_mask,
        // sa_flags, sa_restorer were always returned as zero.
        let mut mask = SigsetT::EMPTY;
        mask.bits[0] = 1u64 << (SIGPIPE - 1) | 1u64 << (SIGCHLD - 1);
        let act = Sigaction {
            sa_handler: SIG_IGN,
            sa_mask: mask,
            sa_flags: SA_RESTART | SA_NOCLDSTOP,
            sa_restorer: 0x1234_5678,
        };
        // Use SIGUSR1 to avoid interfering with other tests.
        let mut old = Sigaction {
            sa_handler: 0,
            sa_mask: SigsetT::EMPTY,
            sa_flags: 0,
            sa_restorer: 0,
        };
        let ret = unsafe { sigaction(SIGUSR1, &raw const act, &raw mut old) };
        assert_eq!(ret, 0);

        // Query back.
        let mut check = Sigaction {
            sa_handler: 0,
            sa_mask: SigsetT::EMPTY,
            sa_flags: 0,
            sa_restorer: 0,
        };
        let ret = unsafe { sigaction(SIGUSR1, core::ptr::null(), &raw mut check) };
        assert_eq!(ret, 0);
        assert_eq!(check.sa_handler, SIG_IGN);
        assert_eq!(check.sa_mask, mask);
        assert_eq!(check.sa_flags, SA_RESTART | SA_NOCLDSTOP);
        assert_eq!(check.sa_restorer, 0x1234_5678);

        // Restore.
        unsafe { sigaction(SIGUSR1, &raw const old, core::ptr::null_mut()); }
    }

    #[test]
    fn test_signal_resets_sigaction_fields() {
        // After signal(), querying via sigaction should show sa_flags=0.
        let mut mask = SigsetT::EMPTY;
        mask.bits[0] = 0xFFFF;
        let act = Sigaction {
            sa_handler: SIG_IGN,
            sa_mask: mask,
            sa_flags: SA_RESTART,
            sa_restorer: 42,
        };
        unsafe { sigaction(SIGUSR2, &raw const act, core::ptr::null_mut()); }

        // Now use signal() to change the handler — it should reset
        // sa_mask/sa_flags/sa_restorer.
        let prev = signal(SIGUSR2, SIG_DFL);
        assert_eq!(prev, SIG_IGN);

        let mut check = Sigaction {
            sa_handler: 0,
            sa_mask: SigsetT::EMPTY,
            sa_flags: 0,
            sa_restorer: 0,
        };
        unsafe { sigaction(SIGUSR2, core::ptr::null(), &raw mut check); }
        assert_eq!(check.sa_handler, SIG_DFL);
        assert_eq!(check.sa_mask, SigsetT::EMPTY);
        assert_eq!(check.sa_flags, 0);
        assert_eq!(check.sa_restorer, 0);
    }

    // -- sigprocmask --

    /// Reset the blocked signal mask to empty.
    ///
    /// Must be called at the start of sigprocmask tests because the
    /// global BLOCKED_MASK persists between tests.
    fn reset_blocked_mask() {
        // SAFETY: single-threaded, tests run with --test-threads=1.
        unsafe { core::ptr::addr_of_mut!(BLOCKED_MASK).write(SigsetT::EMPTY); }
    }

    #[test]
    fn test_sigprocmask_returns_empty_old_set() {
        reset_blocked_mask();
        let mut oldset = SigsetT { bits: [0xDEAD; 16] };
        let ret = sigprocmask(SIG_SETMASK, core::ptr::null(), &raw mut oldset);
        assert_eq!(ret, 0);
        assert_eq!(oldset, SigsetT::EMPTY);
    }

    #[test]
    fn test_sigprocmask_null_oldset() {
        reset_blocked_mask();
        let ret = sigprocmask(SIG_SETMASK, core::ptr::null(), core::ptr::null_mut());
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_sigprocmask_set_mask_round_trip() {
        reset_blocked_mask();
        // Set a mask.
        let mut set = SigsetT::EMPTY;
        set.bits[0] = 0x0000_0000_0000_2002; // SIGINT(2) + SIGPIPE(13)
        let ret = sigprocmask(SIG_SETMASK, &raw const set, core::ptr::null_mut());
        assert_eq!(ret, 0);

        // Read it back.
        let mut oldset = SigsetT::EMPTY;
        let ret = sigprocmask(SIG_SETMASK, core::ptr::null(), &raw mut oldset);
        assert_eq!(ret, 0);
        assert_eq!(oldset.bits[0], 0x0000_0000_0000_2002);
    }

    #[test]
    fn test_sigprocmask_block_adds_signals() {
        reset_blocked_mask();
        // Start with SIGINT blocked.
        let mut set = SigsetT::EMPTY;
        set.bits[0] = 1 << 1; // SIGINT = signal 2, bit index 1
        sigprocmask(SIG_SETMASK, &raw const set, core::ptr::null_mut());

        // Block SIGPIPE additionally.
        let mut add = SigsetT::EMPTY;
        add.bits[0] = 1 << 12; // SIGPIPE = signal 13, bit index 12
        let mut old = SigsetT::EMPTY;
        let ret = sigprocmask(SIG_BLOCK, &raw const add, &raw mut old);
        assert_eq!(ret, 0);
        // Old mask should have only SIGINT.
        assert_eq!(old.bits[0], 1 << 1);

        // New mask should have both.
        let mut current = SigsetT::EMPTY;
        sigprocmask(SIG_SETMASK, core::ptr::null(), &raw mut current);
        assert_eq!(current.bits[0], (1 << 1) | (1 << 12));
    }

    #[test]
    fn test_sigprocmask_unblock_removes_signals() {
        reset_blocked_mask();
        // Block SIGINT and SIGPIPE.
        let mut set = SigsetT::EMPTY;
        set.bits[0] = (1 << 1) | (1 << 12);
        sigprocmask(SIG_SETMASK, &raw const set, core::ptr::null_mut());

        // Unblock SIGINT.
        let mut remove = SigsetT::EMPTY;
        remove.bits[0] = 1 << 1;
        let ret = sigprocmask(SIG_UNBLOCK, &raw const remove, core::ptr::null_mut());
        assert_eq!(ret, 0);

        // Only SIGPIPE should remain.
        let mut current = SigsetT::EMPTY;
        sigprocmask(SIG_SETMASK, core::ptr::null(), &raw mut current);
        assert_eq!(current.bits[0], 1 << 12);
    }

    #[test]
    fn test_sigprocmask_setmask_replaces() {
        reset_blocked_mask();
        // Block SIGINT.
        let mut set = SigsetT::EMPTY;
        set.bits[0] = 1 << 1;
        sigprocmask(SIG_SETMASK, &raw const set, core::ptr::null_mut());

        // Replace with SIGTERM.
        let mut new = SigsetT::EMPTY;
        new.bits[0] = 1 << 14; // SIGTERM = 15, bit index 14
        let mut old = SigsetT::EMPTY;
        let ret = sigprocmask(SIG_SETMASK, &raw const new, &raw mut old);
        assert_eq!(ret, 0);
        // Old should have SIGINT.
        assert_eq!(old.bits[0], 1 << 1);

        // Current should have only SIGTERM.
        let mut current = SigsetT::EMPTY;
        sigprocmask(SIG_SETMASK, core::ptr::null(), &raw mut current);
        assert_eq!(current.bits[0], 1 << 14);
    }

    #[test]
    fn test_sigprocmask_invalid_how() {
        reset_blocked_mask();
        let set = SigsetT::EMPTY;
        let ret = sigprocmask(999, &raw const set, core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_sigprocmask_null_set_no_change() {
        reset_blocked_mask();
        // Set an initial mask.
        let mut set = SigsetT::EMPTY;
        set.bits[0] = 0xFF;
        sigprocmask(SIG_SETMASK, &raw const set, core::ptr::null_mut());

        // Pass null set — should not change the mask.
        let mut old = SigsetT::EMPTY;
        sigprocmask(SIG_SETMASK, core::ptr::null(), &raw mut old);
        assert_eq!(old.bits[0], 0xFF);

        // Verify it's still unchanged.
        let mut check = SigsetT::EMPTY;
        sigprocmask(SIG_SETMASK, core::ptr::null(), &raw mut check);
        assert_eq!(check.bits[0], 0xFF);
    }

    // -- sigsuspend --

    #[test]
    fn test_sigsuspend_returns_eintr() {
        let mask = SigsetT::EMPTY;
        let ret = sigsuspend(&raw const mask);
        assert_eq!(ret, -1);
        // POSIX: sigsuspend always returns -1 with EINTR
    }

    // -- sigpending --

    #[test]
    fn test_sigpending_returns_empty() {
        let mut set = SigsetT { bits: [0xFFFF; 16] };
        let ret = unsafe { sigpending(&raw mut set) };
        assert_eq!(ret, 0);
        assert_eq!(set, SigsetT::EMPTY); // No signals pending
    }

    #[test]
    fn test_sigpending_null() {
        let ret = unsafe { sigpending(core::ptr::null_mut()) };
        assert_eq!(ret, -1);
    }

    // -- strsignal --

    #[test]
    fn test_strsignal_known_signals() {
        let ptr = strsignal(SIGHUP);
        assert_eq!(unsafe { *ptr }, b'H'); // "Hangup"

        let ptr = strsignal(SIGINT);
        assert_eq!(unsafe { *ptr }, b'I'); // "Interrupt"

        let ptr = strsignal(SIGKILL);
        assert_eq!(unsafe { *ptr }, b'K'); // "Killed"

        let ptr = strsignal(SIGSEGV);
        assert_eq!(unsafe { *ptr }, b'S'); // "Segmentation fault"

        let ptr = strsignal(SIGTERM);
        assert_eq!(unsafe { *ptr }, b'T'); // "Terminated"
    }

    #[test]
    fn test_strsignal_extended_signals() {
        // Verify the newly-added signal names (21-31).
        let ptr = strsignal(SIGTTIN);
        assert_eq!(unsafe { *ptr }, b'S'); // "Stopped (tty input)"

        let ptr = strsignal(SIGXCPU);
        assert_eq!(unsafe { *ptr }, b'C'); // "CPU time limit exceeded"

        let ptr = strsignal(SIGWINCH);
        assert_eq!(unsafe { *ptr }, b'W'); // "Window changed"

        let ptr = strsignal(SIGIO);
        assert_eq!(unsafe { *ptr }, b'I'); // "I/O possible"

        let ptr = strsignal(SIGSYS);
        assert_eq!(unsafe { *ptr }, b'B'); // "Bad system call"
    }

    #[test]
    fn test_strsignal_unknown() {
        let ptr = strsignal(99);
        assert!(!ptr.is_null());
        assert_eq!(unsafe { *ptr }, b'U'); // "Unknown signal"
    }

    #[test]
    fn test_strsignal_zero() {
        let ptr = strsignal(0);
        assert!(!ptr.is_null());
        assert_eq!(unsafe { *ptr }, b'U'); // "Unknown signal 0"
    }

    #[test]
    fn test_strsignal_negative() {
        let ptr = strsignal(-1);
        assert!(!ptr.is_null());
        // Should return unknown signal message
    }

    // -- sigaltstack --

    #[test]
    fn test_sigaltstack_get_returns_disabled() {
        let mut oss = StackT {
            ss_sp: core::ptr::null_mut(),
            ss_flags: 0,
            ss_size: 0,
        };
        let ret = sigaltstack(core::ptr::null(), &raw mut oss);
        assert_eq!(ret, 0);
        assert_eq!(oss.ss_flags, SS_DISABLE);
        assert!(oss.ss_sp.is_null());
        assert_eq!(oss.ss_size, 0);
    }

    #[test]
    fn test_sigaltstack_set_valid() {
        let mut stack_buf = [0u8; SIGSTKSZ];
        let ss = StackT {
            ss_sp: stack_buf.as_mut_ptr(),
            ss_flags: 0,
            ss_size: SIGSTKSZ,
        };
        let ret = sigaltstack(&raw const ss, core::ptr::null_mut());
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_sigaltstack_too_small() {
        let mut stack_buf = [0u8; 1024]; // Less than MINSIGSTKSZ
        let ss = StackT {
            ss_sp: stack_buf.as_mut_ptr(),
            ss_flags: 0,
            ss_size: 1024,
        };
        let ret = sigaltstack(&raw const ss, core::ptr::null_mut());
        assert_eq!(ret, -1); // Should fail: stack too small
    }

    #[test]
    fn test_sigaltstack_disable() {
        let ss = StackT {
            ss_sp: core::ptr::null_mut(),
            ss_flags: SS_DISABLE,
            ss_size: 0,
        };
        let ret = sigaltstack(&raw const ss, core::ptr::null_mut());
        assert_eq!(ret, 0); // SS_DISABLE → no size check
    }

    // -- siginterrupt --

    #[test]
    fn test_siginterrupt_valid_signals_succeed() {
        // After Phase 75 validation, valid signal numbers (excluding
        // SIGKILL/SIGSTOP) still return 0 for either flag value.
        crate::errno::set_errno(0);
        assert_eq!(siginterrupt(SIGALRM, 1), 0);
        assert_eq!(siginterrupt(SIGALRM, 0), 0);
        // errno should be untouched on success.
        assert_eq!(crate::errno::get_errno(), 0);
    }

    // -- kill / raise stubs --
    //
    // Note on coverage: the `kill(pid > 0, 0)` and SIGABRT-to-self paths
    // dispatch into native syscalls (SYS_PROCESS_IS_READY / SYS_PROCESS_ID)
    // that aren't available in host-target test builds, so we don't
    // exercise those here.  We do test the validation paths that resolve
    // entirely in our code: pid<=0 with sig==0, out-of-range signals,
    // and the unchanged ENOSYS behaviour for arbitrary (pid, sig).

    /// Cross-process ignore signal → success (discarded).
    #[test]
    fn test_kill_ignore_signal_discarded() {
        crate::errno::set_errno(0);
        let ret = kill(1, SIGCHLD);
        assert_eq!(ret, 0, "ignore-default signal should be silently discarded");
    }

    /// Cross-process stop signal → ENOSYS (not supported).
    #[test]
    fn test_kill_stop_signal_enosys() {
        crate::errno::set_errno(0);
        let ret = kill(1, SIGSTOP);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// Cross-process continue signal → ENOSYS (not supported).
    #[test]
    fn test_kill_continue_signal_enosys() {
        crate::errno::set_errno(0);
        let ret = kill(1, SIGCONT);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_kill_sig0_pid_zero_enosys() {
        // pid == 0 means "every process in the caller's process group"
        // on Linux.  We don't implement process groups, so reject with
        // ENOSYS before touching any syscall.
        crate::errno::set_errno(0);
        let ret = kill(0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_kill_sig0_pid_negative_enosys() {
        // Negative pids select process groups; same story.
        crate::errno::set_errno(0);
        let ret = kill(-5, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_kill_sig0_pid_minus_one_enosys() {
        // pid == -1 means "every process you may signal".  ENOSYS.
        crate::errno::set_errno(0);
        let ret = kill(-1, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_kill_invalid_signal_einval() {
        // Out-of-range signal numbers must produce EINVAL, distinct
        // from ENOSYS.  This is the diagnostic POSIX programs expect.
        crate::errno::set_errno(0);
        let ret = kill(1, NSIG);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        crate::errno::set_errno(0);
        let ret = kill(1, -1);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        crate::errno::set_errno(0);
        let ret = kill(1, 1000);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Cross-process ignore signals (SIGCHLD, SIGURG, SIGWINCH) are
    /// silently discarded — returns 0, no syscall issued.
    #[test]
    fn test_kill_cross_process_ignore_signals() {
        for sig in [SIGCHLD, SIGURG, SIGWINCH] {
            crate::errno::set_errno(0);
            let ret = kill(1, sig);
            assert_eq!(ret, 0, "kill(1, {sig}) should succeed (ignore)");
        }
    }

    /// Cross-process stop/continue signals return ENOSYS.
    #[test]
    fn test_kill_cross_process_stop_continue_enosys() {
        for sig in [SIGSTOP, SIGTSTP, SIGTTIN, SIGTTOU, SIGCONT] {
            crate::errno::set_errno(0);
            let ret = kill(1, sig);
            assert_eq!(ret, -1, "kill(1, {sig}) should fail");
            assert_eq!(
                crate::errno::get_errno(),
                crate::errno::ENOSYS,
                "kill(1, {sig}) should set ENOSYS (not supported)"
            );
        }
    }

    // =================================================================
    // Phase 203 — CAP_KILL gate on kill() for cross-process signals
    //
    // Linux's check_kill_permission() → kill_ok_by_cred() gates
    // cross-uid signal delivery on CAP_KILL.  The gate runs after
    // the EINVAL signal check and the self-SIGABRT fast path, and
    // only for pid > 0 (process-group forms fall through to ENOSYS).
    // =================================================================

    mod phase203_cap {
        pub(super) struct CapGuard {
            lo: u32,
            hi: u32,
        }
        impl CapGuard {
            pub(super) fn snapshot() -> Self {
                let (lo, hi) =
                    crate::sys_capability::current_caps_effective();
                Self { lo, hi }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version:
                        crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                    pid: 0,
                };
                let data = [
                    crate::sys_capability::CapUserData {
                        effective: self.lo,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                    crate::sys_capability::CapUserData {
                        effective: self.hi,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                ];
                let _ =
                    crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        pub(super) fn drop_cap_kill() {
            let cap = crate::sys_capability::CAP_KILL;
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let new_lo = lo & !(1u32 << cap);
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let rc =
                crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0, "capset must succeed dropping cap");
            assert!(!crate::sys_capability::has_capability(cap));
        }
    }

    // -- cap held: ignore signals succeed, stop returns ENOSYS ----------

    /// With CAP_KILL (default), cross-process ignore signals succeed.
    #[test]
    fn test_phase203_kill_with_cap_ignore_ok() {
        assert!(crate::sys_capability::has_capability(
            crate::sys_capability::CAP_KILL,
        ));
        crate::errno::set_errno(0);
        let ret = kill(1, SIGCHLD);
        assert_eq!(ret, 0);
    }

    /// With CAP_KILL (default), stop signals return ENOSYS.
    #[test]
    fn test_phase203_kill_with_cap_stop_enosys() {
        assert!(crate::sys_capability::has_capability(
            crate::sys_capability::CAP_KILL,
        ));
        crate::errno::set_errno(0);
        let ret = kill(1, SIGSTOP);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // -- cap dropped: cross-process kill → EPERM --------------------------

    /// Without CAP_KILL, kill(1, SIGHUP) → EPERM.
    #[test]
    fn test_phase203_kill_no_cap_eperm() {
        let _g = phase203_cap::CapGuard::snapshot();
        phase203_cap::drop_cap_kill();
        crate::errno::set_errno(0);
        let ret = kill(1, SIGHUP);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EPERM);
    }

    /// Without CAP_KILL, kill(42, SIGTERM) → EPERM.
    #[test]
    fn test_phase203_kill_sigterm_no_cap_eperm() {
        let _g = phase203_cap::CapGuard::snapshot();
        phase203_cap::drop_cap_kill();
        crate::errno::set_errno(0);
        let ret = kill(42, SIGTERM);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EPERM);
    }

    // -- sig==0 existence check bypasses the cap gate ---------------------

    /// sig==0 is a pure existence check — it never signals, so
    /// CAP_KILL is not required.  The sig==0 branch returns before
    /// the cap gate.
    #[test]
    fn test_phase203_kill_sig0_bypasses_cap_gate() {
        let _g = phase203_cap::CapGuard::snapshot();
        phase203_cap::drop_cap_kill();
        crate::errno::set_errno(0);
        // pid <= 0 is ENOSYS for sig==0 (process group); that's
        // tested elsewhere.  For pid > 0, sig==0 does a
        // SYS_PROCESS_IS_READY syscall — the result depends on
        // whether pid 1 actually exists.  Either ESRCH or 0 is
        // acceptable; what matters is it's NOT EPERM.
        let ret = kill(1, 0);
        // We don't assert ret because pid 1 may or may not exist.
        // The key invariant is that errno != EPERM.
        assert_ne!(
            crate::errno::get_errno(),
            crate::errno::EPERM,
            "sig==0 must bypass CAP_KILL gate"
        );
        let _ = ret; // suppress unused warning
    }

    // -- pid <= 0 bypasses the cap gate (falls to ENOSYS) -----------------

    /// pid == 0 (process group) with no cap → ENOSYS, not EPERM.
    /// The cap gate only fires for pid > 0.
    #[test]
    fn test_phase203_kill_pid0_no_cap_enosys() {
        let _g = phase203_cap::CapGuard::snapshot();
        phase203_cap::drop_cap_kill();
        crate::errno::set_errno(0);
        let ret = kill(0, SIGHUP);
        assert_eq!(ret, -1);
        assert_eq!(
            crate::errno::get_errno(),
            crate::errno::ENOSYS,
            "pid <= 0 must bypass CAP_KILL gate"
        );
    }

    /// pid == -1 (all processes) with no cap → ENOSYS.
    #[test]
    fn test_phase203_kill_pidneg1_no_cap_enosys() {
        let _g = phase203_cap::CapGuard::snapshot();
        phase203_cap::drop_cap_kill();
        crate::errno::set_errno(0);
        let ret = kill(-1, SIGTERM);
        assert_eq!(ret, -1);
        assert_eq!(
            crate::errno::get_errno(),
            crate::errno::ENOSYS,
            "pid == -1 must bypass CAP_KILL gate"
        );
    }

    // -- ordering: EINVAL beats EPERM -------------------------------------

    /// Invalid signal + no cap → EINVAL (sig check before cap).
    #[test]
    fn test_phase203_kill_invalid_sig_einval_before_eperm() {
        let _g = phase203_cap::CapGuard::snapshot();
        phase203_cap::drop_cap_kill();
        crate::errno::set_errno(0);
        let ret = kill(1, 0x7FFF);
        assert_eq!(ret, -1);
        assert_eq!(
            crate::errno::get_errno(),
            crate::errno::EINVAL,
            "EINVAL for bad signal must precede EPERM"
        );
    }

    // -- restoration: cap drop/restore cycle ------------------------------

    /// After restoring CAP_KILL, ignore signals succeed again.
    #[test]
    fn test_phase203_kill_cap_restore() {
        {
            let _g = phase203_cap::CapGuard::snapshot();
            phase203_cap::drop_cap_kill();
            crate::errno::set_errno(0);
            // Without CAP_KILL, cross-process signal → EPERM.
            let ret = kill(1, SIGCHLD);
            assert_eq!(ret, -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EPERM);
        }
        assert!(crate::sys_capability::has_capability(
            crate::sys_capability::CAP_KILL,
        ));
        crate::errno::set_errno(0);
        // With cap restored, ignore signals succeed.
        let ret = kill(1, SIGCHLD);
        assert_eq!(ret, 0);
    }

    // -- sigtimedwait / sigqueue stubs --

    #[test]
    fn test_sigtimedwait_returns_eagain() {
        crate::errno::set_errno(0);
        let set = SigsetT { bits: [0; 16] };
        let ret = sigtimedwait(&raw const set, core::ptr::null_mut(), core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EAGAIN);
    }

    #[test]
    fn test_sigqueue_returns_enosys() {
        crate::errno::set_errno(0);
        let ret = sigqueue(1, SIGTERM, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // -- realtime signal range --

    #[test]
    fn test_sigrtmin_sigrtmax() {
        assert_eq!(__libc_current_sigrtmin(), 32);
        assert_eq!(__libc_current_sigrtmax(), 64);
        // rtmax > rtmin
        assert!(__libc_current_sigrtmax() > __libc_current_sigrtmin());
    }

    // -- SIGRTMIN / SIGRTMAX constants --

    #[test]
    fn test_sigrtmin_sigrtmax_constants() {
        assert_eq!(SIGRTMIN, 32);
        assert_eq!(SIGRTMAX, 64);
        // Constants must agree with the functions.
        assert_eq!(SIGRTMIN, __libc_current_sigrtmin());
        assert_eq!(SIGRTMAX, __libc_current_sigrtmax());
    }

    #[test]
    fn test_sigrtmin_above_standard_signals() {
        // All standard signals (1-31) must be below SIGRTMIN.
        assert!(SIGSYS < SIGRTMIN);
    }

    #[test]
    fn test_sigrtmax_within_nsig() {
        // SIGRTMAX must be < NSIG (so sigset_t can hold all signals).
        assert!(SIGRTMAX < NSIG);
    }

    // -- SIG_BLOCK / SIG_UNBLOCK / SIG_SETMASK constants --

    #[test]
    fn test_sig_block_constants() {
        // Values match Linux x86_64.
        assert_eq!(SIG_BLOCK, 0);
        assert_eq!(SIG_UNBLOCK, 1);
        assert_eq!(SIG_SETMASK, 2);
    }

    #[test]
    fn test_sig_block_constants_distinct() {
        assert_ne!(SIG_BLOCK, SIG_UNBLOCK);
        assert_ne!(SIG_BLOCK, SIG_SETMASK);
        assert_ne!(SIG_UNBLOCK, SIG_SETMASK);
    }

    // -- CLD_* siginfo si_code constants --

    #[test]
    fn test_cld_constants_values() {
        assert_eq!(CLD_EXITED, 1);
        assert_eq!(CLD_KILLED, 2);
        assert_eq!(CLD_DUMPED, 3);
        assert_eq!(CLD_TRAPPED, 4);
        assert_eq!(CLD_STOPPED, 5);
        assert_eq!(CLD_CONTINUED, 6);
    }

    #[test]
    fn test_cld_constants_sequential() {
        // All CLD_* constants are sequential starting from 1.
        assert_eq!(CLD_KILLED, CLD_EXITED + 1);
        assert_eq!(CLD_DUMPED, CLD_KILLED + 1);
        assert_eq!(CLD_TRAPPED, CLD_DUMPED + 1);
        assert_eq!(CLD_STOPPED, CLD_TRAPPED + 1);
        assert_eq!(CLD_CONTINUED, CLD_STOPPED + 1);
    }

    // -- raise (Phase 211: handler dispatch) --

    /// raise() with SIG_IGN registered returns 0 (signal ignored).
    #[test]
    fn test_raise_sig_ign_returns_zero() {
        let old = signal(SIGTERM, SIG_IGN);
        errno::set_errno(0);
        assert_eq!(raise(SIGTERM), 0);
        // Restore.
        signal(SIGTERM, old);
    }

    /// raise() with an ignore-default signal (SIGCHLD) returns 0 via
    /// SIG_DFL → Ignore default action.
    #[test]
    fn test_raise_ignore_default_returns_zero() {
        // Ensure SIG_DFL is set.
        signal(SIGCHLD, SIG_DFL);
        errno::set_errno(0);
        assert_eq!(raise(SIGCHLD), 0);
    }

    /// raise() with an ignore-default signal (SIGWINCH) returns 0.
    #[test]
    fn test_raise_sigwinch_ignore_default_returns_zero() {
        signal(SIGWINCH, SIG_DFL);
        errno::set_errno(0);
        assert_eq!(raise(SIGWINCH), 0);
    }

    /// raise() with SIG_IGN for SIGHUP returns 0.
    #[test]
    fn test_raise_sighup_sig_ign() {
        let old = signal(SIGHUP, SIG_IGN);
        errno::set_errno(0);
        assert_eq!(raise(SIGHUP), 0);
        signal(SIGHUP, old);
    }

    /// raise() with SIG_IGN for SIGINT returns 0.
    #[test]
    fn test_raise_sigint_sig_ign() {
        let old = signal(SIGINT, SIG_IGN);
        errno::set_errno(0);
        assert_eq!(raise(SIGINT), 0);
        signal(SIGINT, old);
    }

    /// raise() with a stop signal returns ENOSYS (no kernel suspend).
    #[test]
    fn test_raise_sigtstp_enosys() {
        signal(SIGTSTP, SIG_DFL);
        errno::set_errno(0);
        assert_eq!(raise(SIGTSTP), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_raise_zero_returns_einval() {
        // sig == 0 is out of the valid signal range (1..NSIG).
        errno::set_errno(0);
        assert_eq!(raise(0), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -- pthread_sigmask --

    #[test]
    fn test_pthread_sigmask_get_current() {
        let mut oldset = SigsetT::EMPTY;
        let ret = pthread_sigmask(SIG_SETMASK, core::ptr::null(), &raw mut oldset);
        assert_eq!(ret, 0, "pthread_sigmask should succeed with null set");
    }

    #[test]
    fn test_pthread_sigmask_block() {
        // Save current mask.
        let mut old = SigsetT::EMPTY;
        pthread_sigmask(SIG_SETMASK, core::ptr::null(), &raw mut old);

        // Block signal 10.
        let mut block = SigsetT::EMPTY;
        unsafe { sigaddset(&raw mut block, 10); }
        let ret = pthread_sigmask(SIG_BLOCK, &raw const block, core::ptr::null_mut());
        assert_eq!(ret, 0);

        // Check it's blocked.
        let mut current = SigsetT::EMPTY;
        pthread_sigmask(SIG_SETMASK, core::ptr::null(), &raw mut current);
        assert_ne!(unsafe { sigismember(&raw const current, 10) }, 0, "Signal 10 should be blocked");

        // Restore.
        pthread_sigmask(SIG_SETMASK, &raw const old, core::ptr::null_mut());
    }

    #[test]
    fn test_pthread_sigmask_invalid_how() {
        let set = SigsetT::EMPTY;
        let ret = pthread_sigmask(999, &raw const set, core::ptr::null_mut());
        assert_eq!(ret, errno::EINVAL, "Invalid how should return EINVAL");
    }

    #[test]
    fn test_pthread_sigmask_unblock() {
        // Save current mask.
        let mut old = SigsetT::EMPTY;
        pthread_sigmask(SIG_SETMASK, core::ptr::null(), &raw mut old);

        // Block signal 5.
        let mut block = SigsetT::EMPTY;
        unsafe { sigaddset(&raw mut block, 5); }
        pthread_sigmask(SIG_BLOCK, &raw const block, core::ptr::null_mut());

        // Unblock signal 5.
        let ret = pthread_sigmask(SIG_UNBLOCK, &raw const block, core::ptr::null_mut());
        assert_eq!(ret, 0);

        // Verify unblocked.
        let mut current = SigsetT::EMPTY;
        pthread_sigmask(SIG_SETMASK, core::ptr::null(), &raw mut current);
        assert_eq!(unsafe { sigismember(&raw const current, 5) }, 0, "Signal 5 should be unblocked");

        // Restore.
        pthread_sigmask(SIG_SETMASK, &raw const old, core::ptr::null_mut());
    }

    // -----------------------------------------------------------------------
    // psignal
    // -----------------------------------------------------------------------

    #[test]
    fn test_psignal_null_prefix() {
        // psignal with null prefix should print just the signal description.
        unsafe { psignal(crate::signal::SIGTERM, core::ptr::null()); }
    }

    #[test]
    fn test_psignal_empty_prefix() {
        // Empty prefix → no "prefix: " part, just signal description.
        unsafe { psignal(crate::signal::SIGINT, b"\0".as_ptr()); }
    }

    #[test]
    fn test_psignal_with_prefix() {
        unsafe { psignal(crate::signal::SIGKILL, b"test\0".as_ptr()); }
    }

    #[test]
    fn test_psignal_invalid_signal() {
        // Invalid signal number should still not crash.
        unsafe { psignal(9999, b"bad sig\0".as_ptr()); }
    }

    // -----------------------------------------------------------------------
    // sigwait
    // -----------------------------------------------------------------------

    // Note: sigwait sleeps for 1 second so we don't test it by default.
    // Just verify the function signature compiles and the return type is correct.

    // -----------------------------------------------------------------------
    // __libc_current_sigrtmin / __libc_current_sigrtmax
    // -----------------------------------------------------------------------

    #[test]
    fn test_sigrtmin_function() {
        let val = __libc_current_sigrtmin();
        assert_eq!(val, 32);
        assert_eq!(val, SIGRTMIN);
    }

    #[test]
    fn test_sigrtmax_function() {
        let val = __libc_current_sigrtmax();
        assert_eq!(val, 64);
        assert_eq!(val, SIGRTMAX);
    }

    #[test]
    fn test_sigrtmin_less_than_sigrtmax() {
        assert!(
            __libc_current_sigrtmin() < __libc_current_sigrtmax(),
            "SIGRTMIN must be less than SIGRTMAX"
        );
    }

    #[test]
    fn test_sigrt_range_is_nonempty() {
        let range = __libc_current_sigrtmax() - __libc_current_sigrtmin();
        assert!(range > 0, "realtime signal range must be nonempty");
    }

    // ------------------------------------------------------------------
    // SiginfoT struct
    // ------------------------------------------------------------------

    #[test]
    fn test_siginfo_t_layout() {
        // siginfo_t is 128 bytes on Linux x86_64.
        assert_eq!(core::mem::size_of::<SiginfoT>(), 128);
    }

    #[test]
    fn test_siginfo_t_default_zeroed() {
        let si = SiginfoT::default();
        assert_eq!(si.si_signo, 0);
        assert_eq!(si.si_errno, 0);
        assert_eq!(si.si_code, 0);
    }

    // ------------------------------------------------------------------
    // psiginfo
    // ------------------------------------------------------------------

    #[test]
    fn test_psiginfo_null_info() {
        // psiginfo with null info → prints "Unknown signal 0".
        // Just verify no crash.
        unsafe { psiginfo(core::ptr::null(), b"test\0".as_ptr()); }
    }

    #[test]
    fn test_psiginfo_with_signum() {
        // psiginfo with a valid signum → prints the signal name.
        let mut si = SiginfoT::default();
        si.si_signo = SIGTERM;
        unsafe { psiginfo(&si, b"test\0".as_ptr()); }
    }

    #[test]
    fn test_psiginfo_null_msg() {
        let mut si = SiginfoT::default();
        si.si_signo = SIGINT;
        unsafe { psiginfo(&si, core::ptr::null()); }
    }

    // -----------------------------------------------------------------
    // raise / sigqueue / sigtimedwait — argument-domain validation (Phase 59)
    // -----------------------------------------------------------------

    // ---- raise() ----

    #[test]
    fn test_raise_zero_einval() {
        crate::errno::set_errno(0);
        assert_eq!(raise(0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_raise_negative_einval() {
        crate::errno::set_errno(0);
        assert_eq!(raise(-1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_raise_nsig_einval() {
        // sig == NSIG is out of range (valid range is 1..NSIG).
        crate::errno::set_errno(0);
        assert_eq!(raise(NSIG), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_raise_way_above_nsig_einval() {
        crate::errno::set_errno(0);
        assert_eq!(raise(1000), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_raise_min_signal_with_sig_ign() {
        // sig == 1 (SIGHUP) is valid. With SIG_IGN, returns 0.
        let old = signal(1, SIG_IGN);
        crate::errno::set_errno(0);
        assert_eq!(raise(1), 0);
        signal(1, old);
    }

    #[test]
    fn test_raise_max_signal_with_sig_ign() {
        // sig == NSIG - 1 (top of the valid range). With SIG_IGN, returns 0.
        let old = signal(NSIG - 1, SIG_IGN);
        crate::errno::set_errno(0);
        assert_eq!(raise(NSIG - 1), 0);
        signal(NSIG - 1, old);
    }

    #[test]
    fn test_raise_rt_signal_with_sig_ign() {
        // Realtime signals (SIGRTMIN..=SIGRTMAX) pass validation.
        // With SIG_IGN, returns 0.
        let old = signal(SIGRTMIN, SIG_IGN);
        crate::errno::set_errno(0);
        assert_eq!(raise(SIGRTMIN), 0);
        signal(SIGRTMIN, old);
    }

    // ---- sigqueue() ----

    #[test]
    fn test_sigqueue_negative_sig_einval() {
        crate::errno::set_errno(0);
        assert_eq!(sigqueue(1, -1, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_sigqueue_sig_at_nsig_einval() {
        crate::errno::set_errno(0);
        assert_eq!(sigqueue(1, NSIG, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_sigqueue_sig_way_above_nsig_einval() {
        crate::errno::set_errno(0);
        assert_eq!(sigqueue(1, 1000, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_sigqueue_sig_zero_existence_probe_passes() {
        // sig == 0 is the existence/permission-probe form; should not
        // trip EINVAL but instead reach the pid/ENOSYS leg.
        crate::errno::set_errno(0);
        assert_eq!(sigqueue(1, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_sigqueue_zero_pid_einval() {
        // Phase 123: unlike kill(), sigqueue does not accept pid == 0
        // (process-group "self"); Linux's do_rt_sigqueueinfo rejects
        // pid <= 0 with EINVAL before any task lookup.
        crate::errno::set_errno(0);
        assert_eq!(sigqueue(0, SIGTERM, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_sigqueue_negative_pid_einval() {
        crate::errno::set_errno(0);
        assert_eq!(sigqueue(-1, SIGTERM, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_sigqueue_pgrp_form_einval() {
        // pid < -1 in kill() means "process group |pid|"; sigqueue
        // rejects it as EINVAL per do_rt_sigqueueinfo.
        crate::errno::set_errno(0);
        assert_eq!(sigqueue(-100, SIGTERM, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_sigqueue_pid_checked_before_sig() {
        // Phase 123: Bad sig + bad pid → EINVAL.  Both checks return
        // EINVAL, so the observable errno is the same, but the
        // implementation order now matches Linux (pid <= 0 fires in
        // do_rt_sigqueueinfo before send_signal validates sig).
        crate::errno::set_errno(0);
        assert_eq!(sigqueue(-1, -1, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_sigqueue_valid_args_reach_enosys() {
        crate::errno::set_errno(0);
        assert_eq!(sigqueue(1234, SIGUSR1, 42), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- Phase 123: pid<=0 errno corrected, prologue order matches
    //                Linux do_rt_sigqueueinfo ---
    //
    // Previous behaviour returned ESRCH for pid <= 0 citing
    // find_task_by_vpid; that lookup is never reached because
    // do_rt_sigqueueinfo intercepts the case with EINVAL before
    // anything else.

    /// Phase 123: i32::MIN as pid → EINVAL.  Confirms the `pid <= 0`
    /// check uses signed comparison, not a `pid == 0 || pid < 0` pair
    /// that might trip on signed-overflow corner cases.
    #[test]
    fn test_sigqueue_phase123_i32_min_pid_einval() {
        crate::errno::set_errno(0);
        assert_eq!(sigqueue(i32::MIN, SIGTERM, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 123: smallest positive pid (1) with a valid sig reaches
    /// ENOSYS — confirms `pid > 0` opens the gate.
    #[test]
    fn test_sigqueue_phase123_pid_one_reaches_enosys() {
        crate::errno::set_errno(0);
        assert_eq!(sigqueue(1, SIGTERM, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// Phase 123: i32::MAX as pid with a valid sig reaches ENOSYS —
    /// no upper bound on the pid check.
    #[test]
    fn test_sigqueue_phase123_i32_max_pid_reaches_enosys() {
        crate::errno::set_errno(0);
        assert_eq!(sigqueue(i32::MAX, SIGUSR1, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// Phase 123: pid <= 0 with sig == 0 (existence probe).  pid
    /// check fires first → EINVAL, even though sig is benign.
    #[test]
    fn test_sigqueue_phase123_zero_pid_existence_probe_einval() {
        crate::errno::set_errno(0);
        assert_eq!(sigqueue(0, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 123: negative pid with sig == NSIG (above range).  Both
    /// pid and sig would fault; pid check (Linux do_rt_sigqueueinfo)
    /// fires first.  Same EINVAL value, but order now matches Linux.
    #[test]
    fn test_sigqueue_phase123_neg_pid_sig_at_nsig_einval() {
        crate::errno::set_errno(0);
        assert_eq!(sigqueue(-5, NSIG, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 123: pid <= 0 with i32::MAX sig.  Pid check fires before
    /// sig range check.
    #[test]
    fn test_sigqueue_phase123_neg_pid_sig_i32_max_einval() {
        crate::errno::set_errno(0);
        assert_eq!(sigqueue(-1, i32::MAX, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 123: errno recovery — EINVAL followed by ENOSYS cleanly
    /// overwrites.
    #[test]
    fn test_sigqueue_phase123_recovery_einval_then_enosys() {
        crate::errno::set_errno(0);
        assert_eq!(sigqueue(0, SIGTERM, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        assert_eq!(sigqueue(100, SIGUSR1, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// Phase 123: errno recovery — ENOSYS followed by EINVAL.
    #[test]
    fn test_sigqueue_phase123_recovery_enosys_then_einval() {
        crate::errno::set_errno(0);
        assert_eq!(sigqueue(100, SIGUSR1, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
        assert_eq!(sigqueue(-1, SIGTERM, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 123 workflow: glibc's `sigqueue(3)` from a service
    /// daemon that misuses `getpid()` returning 0 for an
    /// unregistered thread state.  pid==0 must surface EINVAL so the
    /// daemon's error path triggers, not a "no such process" misread.
    #[test]
    fn test_sigqueue_phase123_workflow_daemon_self_pid_zero() {
        crate::errno::set_errno(0);
        // sig is valid; pid is 0 (programmer mistake — used pid_t
        // zero-init field without populating it).
        assert_eq!(sigqueue(0, SIGUSR1, 0xDEAD_BEEF_usize), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 123 workflow: realtime-signal queue from a media
    /// pipeline — `sigqueue(buddy_pid, SIGRTMIN+3, frame_id)`.  Must
    /// reach ENOSYS (stub) rather than be misread as ESRCH for a
    /// supposedly-vanished buddy.
    #[test]
    fn test_sigqueue_phase123_workflow_realtime_queue_reaches_enosys() {
        crate::errno::set_errno(0);
        let sigrtmin_plus_3 = SIGRTMIN + 3;
        assert_eq!(sigqueue(4242, sigrtmin_plus_3, 7), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// Phase 123 buggy-caller: caller computes `pid = strtol(arg, ..)`
    /// on a malformed arg, getting `0`.  Subsequent `sigqueue(0,
    /// SIGTERM, ...)` must EINVAL — not silently target some random
    /// process group as `kill(2)` would.
    #[test]
    fn test_sigqueue_phase123_buggy_caller_strtol_zero_einval() {
        crate::errno::set_errno(0);
        assert_eq!(sigqueue(0, SIGTERM, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 123 buggy-caller: pid arithmetic underflow.  Subtracting
    /// from a small pid in a buggy script produces a negative pid;
    /// must EINVAL.
    #[test]
    fn test_sigqueue_phase123_buggy_caller_underflow_pid_einval() {
        crate::errno::set_errno(0);
        let bogus_pid: crate::types::PidT = 3_i32.wrapping_sub(10);
        assert_eq!(sigqueue(bogus_pid, SIGTERM, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 123: pid <= 0 with sig == 0 + arbitrary value param —
    /// confirms `_value` is ignored by the validation chain.
    #[test]
    fn test_sigqueue_phase123_value_ignored_on_einval() {
        crate::errno::set_errno(0);
        assert_eq!(sigqueue(-1, 0, usize::MAX), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // =================================================================
    // Phase 204 — CAP_KILL gate on sigqueue()
    //
    // sigqueue always targets a specific positive pid; the gate runs
    // after argument validation (EINVAL for pid/sig).  Reuses the
    // Phase 203 cap helpers.
    // =================================================================

    // -- cap held: sigqueue reaches ENOSYS (unchanged) --------------------

    /// With CAP_KILL (default), sigqueue(1, SIGHUP, 0) reaches ENOSYS.
    #[test]
    fn test_phase204_sigqueue_with_cap_enosys() {
        assert!(crate::sys_capability::has_capability(
            crate::sys_capability::CAP_KILL,
        ));
        crate::errno::set_errno(0);
        let ret = sigqueue(1, SIGHUP, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // -- cap dropped: sigqueue → EPERM ------------------------------------

    /// Without CAP_KILL, sigqueue(1, SIGHUP, 0) → EPERM.
    #[test]
    fn test_phase204_sigqueue_no_cap_eperm() {
        let _g = phase203_cap::CapGuard::snapshot();
        phase203_cap::drop_cap_kill();
        crate::errno::set_errno(0);
        let ret = sigqueue(1, SIGHUP, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EPERM);
    }

    /// Without CAP_KILL, sig==0 (existence probe) → EPERM too.
    /// Unlike kill(pid, 0), sigqueue has no special sig-0 fast path
    /// — it goes through the same cap gate.
    #[test]
    fn test_phase204_sigqueue_sig0_no_cap_eperm() {
        let _g = phase203_cap::CapGuard::snapshot();
        phase203_cap::drop_cap_kill();
        crate::errno::set_errno(0);
        let ret = sigqueue(1, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EPERM);
    }

    // -- ordering: EINVAL beats EPERM -------------------------------------

    /// pid <= 0 + no cap → EINVAL (pid check before cap).
    #[test]
    fn test_phase204_sigqueue_bad_pid_einval_before_eperm() {
        let _g = phase203_cap::CapGuard::snapshot();
        phase203_cap::drop_cap_kill();
        crate::errno::set_errno(0);
        let ret = sigqueue(0, SIGHUP, 0);
        assert_eq!(ret, -1);
        assert_eq!(
            crate::errno::get_errno(),
            crate::errno::EINVAL,
            "EINVAL for bad pid must precede EPERM"
        );
    }

    /// Invalid sig + no cap → EINVAL (sig check before cap).
    #[test]
    fn test_phase204_sigqueue_bad_sig_einval_before_eperm() {
        let _g = phase203_cap::CapGuard::snapshot();
        phase203_cap::drop_cap_kill();
        crate::errno::set_errno(0);
        let ret = sigqueue(1, NSIG, 0);
        assert_eq!(ret, -1);
        assert_eq!(
            crate::errno::get_errno(),
            crate::errno::EINVAL,
            "EINVAL for bad sig must precede EPERM"
        );
    }

    // -- restoration: cap restore cycle -----------------------------------

    /// After restoring CAP_KILL, sigqueue reaches ENOSYS again.
    #[test]
    fn test_phase204_sigqueue_cap_restore() {
        {
            let _g = phase203_cap::CapGuard::snapshot();
            phase203_cap::drop_cap_kill();
            crate::errno::set_errno(0);
            let ret = sigqueue(1, SIGHUP, 0);
            assert_eq!(ret, -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EPERM);
        }
        assert!(crate::sys_capability::has_capability(
            crate::sys_capability::CAP_KILL,
        ));
        crate::errno::set_errno(0);
        let ret = sigqueue(1, SIGHUP, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // ---- sigtimedwait() ----

    #[test]
    fn test_sigtimedwait_null_set_efault() {
        crate::errno::set_errno(0);
        let ret = sigtimedwait(
            core::ptr::null(),
            core::ptr::null_mut(),
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_sigtimedwait_negative_tv_sec_einval() {
        let set = SigsetT { bits: [0; 16] };
        let ts = crate::stat::Timespec { tv_sec: -1, tv_nsec: 0 };
        crate::errno::set_errno(0);
        let ret = sigtimedwait(&raw const set, core::ptr::null_mut(), &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_sigtimedwait_nsec_at_billion_einval() {
        let set = SigsetT { bits: [0; 16] };
        let ts = crate::stat::Timespec { tv_sec: 1, tv_nsec: 1_000_000_000 };
        crate::errno::set_errno(0);
        let ret = sigtimedwait(&raw const set, core::ptr::null_mut(), &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_sigtimedwait_nsec_way_too_big_einval() {
        let set = SigsetT { bits: [0; 16] };
        let ts = crate::stat::Timespec { tv_sec: 1, tv_nsec: i64::MAX };
        crate::errno::set_errno(0);
        let ret = sigtimedwait(&raw const set, core::ptr::null_mut(), &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_sigtimedwait_negative_nsec_einval() {
        let set = SigsetT { bits: [0; 16] };
        let ts = crate::stat::Timespec { tv_sec: 1, tv_nsec: -1 };
        crate::errno::set_errno(0);
        let ret = sigtimedwait(&raw const set, core::ptr::null_mut(), &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_sigtimedwait_set_check_before_timeout() {
        // NULL set + bad timeout → EFAULT (set is checked first).
        let ts = crate::stat::Timespec { tv_sec: -1, tv_nsec: -1 };
        crate::errno::set_errno(0);
        let ret = sigtimedwait(core::ptr::null(), core::ptr::null_mut(), &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_sigtimedwait_max_valid_nsec_reaches_eagain() {
        let set = SigsetT { bits: [0; 16] };
        let ts = crate::stat::Timespec {
            tv_sec: 0,
            tv_nsec: SIGTIMEDWAIT_NSEC_MAX,
        };
        crate::errno::set_errno(0);
        let ret = sigtimedwait(&raw const set, core::ptr::null_mut(), &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EAGAIN);
    }

    #[test]
    fn test_sigtimedwait_zero_timeout_reaches_eagain() {
        // POSIX poll-form: timeout = {0, 0} → "return immediately if no
        // signal is pending"; our stub reports EAGAIN.
        let set = SigsetT { bits: [0; 16] };
        let ts = crate::stat::Timespec { tv_sec: 0, tv_nsec: 0 };
        crate::errno::set_errno(0);
        let ret = sigtimedwait(&raw const set, core::ptr::null_mut(), &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EAGAIN);
    }

    #[test]
    fn test_sigtimedwait_null_timeout_reaches_eagain() {
        // NULL timeout = "wait forever"; stub still reports EAGAIN.
        let set = SigsetT { bits: [0; 16] };
        crate::errno::set_errno(0);
        let ret = sigtimedwait(&raw const set, core::ptr::null_mut(), core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EAGAIN);
    }

    #[test]
    fn test_sigtimedwait_nsec_max_constant() {
        assert_eq!(SIGTIMEDWAIT_NSEC_MAX, 999_999_999);
    }

    // ---- Real-world workflows ----

    #[test]
    fn test_workflow_raise_sigusr1_libev() {
        // libev uses raise(SIGUSR1) to wake the event loop from a
        // signal handler.  When a handler is registered, raise()
        // invokes it and returns 0.  When no handler is registered
        // (SIG_DFL), SIGUSR1's default is Terminate — so libev
        // would register a handler first.
        //
        // Test with SIG_IGN to verify the dispatch path.
        let old = signal(SIGUSR1, SIG_IGN);
        crate::errno::set_errno(0);
        assert_eq!(raise(SIGUSR1), 0);
        signal(SIGUSR1, old);
    }

    #[test]
    fn test_workflow_sigqueue_realtime_with_data() {
        // Modern threading libraries use sigqueue() with an RT signal
        // and a sival_int payload to wake worker threads with a
        // cookie.  Validates and falls through to ENOSYS.
        crate::errno::set_errno(0);
        let ret = sigqueue(4321, SIGRTMIN + 1, 0xDEAD_BEEF);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_workflow_sigtimedwait_dbus_poll() {
        // dbus poll: sigtimedwait with a small timeout while servicing
        // incoming messages.  No signals delivered → EAGAIN.
        let set = SigsetT { bits: [0xFFFF_FFFF_FFFF_FFFF; 16] };
        let ts = crate::stat::Timespec { tv_sec: 0, tv_nsec: 100_000 };
        crate::errno::set_errno(0);
        let ret = sigtimedwait(&raw const set, core::ptr::null_mut(), &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EAGAIN);
    }

    // ---- Real-world buggy callers ----

    // Note: raise(SIGKILL) with SIG_DFL now calls _exit(137) and never
    // returns (correct POSIX behavior: SIGKILL cannot be caught).
    // This can't be tested without killing the test process.

    #[test]
    fn test_workflow_buggy_sigqueue_oversigned_sig() {
        // A caller passes (sig + 100) to a signal-relay function that
        // forgets to subtract 100 before sigqueue.  Out of range →
        // EINVAL is the correct diagnostic.
        crate::errno::set_errno(0);
        assert_eq!(sigqueue(1234, SIGUSR1 + 100, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_workflow_buggy_sigtimedwait_microseconds() {
        // Caller passes microseconds in tv_nsec (a classic units bug).
        // 500_000 μs in tv_nsec is fine (= 500 μs of nanoseconds), but
        // 1_500_000_000 μs (= 25 minutes) blows past NSEC_MAX → EINVAL.
        let set = SigsetT { bits: [0; 16] };
        let ts = crate::stat::Timespec {
            tv_sec: 0,
            tv_nsec: 1_500_000_000,
        };
        crate::errno::set_errno(0);
        let ret = sigtimedwait(&raw const set, core::ptr::null_mut(), &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -----------------------------------------------------------------
    // Phase 75 — argument-domain validation for signal stubs
    // -----------------------------------------------------------------

    // -- siginterrupt: signal-range validation --

    #[test]
    fn test_phase75_siginterrupt_zero_signal() {
        // sig == 0 is invalid (the existence-probe form is kill-specific,
        // not siginterrupt-specific) → EINVAL.
        crate::errno::set_errno(0);
        assert_eq!(siginterrupt(0, 1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase75_siginterrupt_negative_signal() {
        crate::errno::set_errno(0);
        assert_eq!(siginterrupt(-1, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase75_siginterrupt_signal_at_nsig() {
        // NSIG is one past the highest valid signal → EINVAL.
        crate::errno::set_errno(0);
        assert_eq!(siginterrupt(NSIG, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase75_siginterrupt_signal_above_nsig() {
        crate::errno::set_errno(0);
        assert_eq!(siginterrupt(NSIG + 100, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase75_siginterrupt_signal_int_max() {
        crate::errno::set_errno(0);
        assert_eq!(siginterrupt(i32::MAX, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase75_siginterrupt_signal_int_min() {
        crate::errno::set_errno(0);
        assert_eq!(siginterrupt(i32::MIN, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase75_siginterrupt_rejects_sigkill() {
        // SIGKILL action cannot be changed → EINVAL (matches sigaction).
        crate::errno::set_errno(0);
        assert_eq!(siginterrupt(SIGKILL, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        crate::errno::set_errno(0);
        assert_eq!(siginterrupt(SIGKILL, 1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase75_siginterrupt_rejects_sigstop() {
        crate::errno::set_errno(0);
        assert_eq!(siginterrupt(SIGSTOP, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        crate::errno::set_errno(0);
        assert_eq!(siginterrupt(SIGSTOP, 1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase75_siginterrupt_accepts_real_signals() {
        // A representative spread of well-known signals all succeed.
        for sig in [SIGHUP, SIGINT, SIGTERM, SIGUSR1, SIGUSR2, SIGCHLD,
                    SIGALRM, SIGPIPE, SIGSEGV, SIGWINCH] {
            crate::errno::set_errno(0);
            assert_eq!(siginterrupt(sig, 0), 0, "siginterrupt({sig}, 0) should succeed");
            assert_eq!(siginterrupt(sig, 1), 0, "siginterrupt({sig}, 1) should succeed");
        }
    }

    #[test]
    fn test_phase75_siginterrupt_flag_value_irrelevant_on_error() {
        // Even with flag == 1 (typical "make interruptible" call), an
        // invalid sig still wins → EINVAL.
        crate::errno::set_errno(0);
        assert_eq!(siginterrupt(999, 1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- sigaltstack: ss_flags validation --

    #[test]
    fn test_phase75_sigaltstack_unknown_flag_bits() {
        // A flag bit outside the recognised set → EINVAL.
        let mut stack_buf = [0u8; SIGSTKSZ];
        let ss = StackT {
            ss_sp: stack_buf.as_mut_ptr(),
            ss_flags: 0x10, // unrecognised bit
            ss_size: SIGSTKSZ,
        };
        crate::errno::set_errno(0);
        let ret = sigaltstack(&raw const ss, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase75_sigaltstack_onstack_plus_disable_rejected() {
        // SS_ONSTACK | SS_DISABLE together is meaningless → EINVAL.
        let mut stack_buf = [0u8; SIGSTKSZ];
        let ss = StackT {
            ss_sp: stack_buf.as_mut_ptr(),
            ss_flags: SS_ONSTACK | SS_DISABLE,
            ss_size: SIGSTKSZ,
        };
        crate::errno::set_errno(0);
        let ret = sigaltstack(&raw const ss, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase75_sigaltstack_autodisarm_with_onstack_ok() {
        // SS_AUTODISARM is a modifier bit and is allowed in combination
        // with SS_ONSTACK; the mode after masking is SS_ONSTACK.
        let mut stack_buf = [0u8; SIGSTKSZ];
        let ss = StackT {
            ss_sp: stack_buf.as_mut_ptr(),
            ss_flags: SS_AUTODISARM | SS_ONSTACK,
            ss_size: SIGSTKSZ,
        };
        crate::errno::set_errno(0);
        let ret = sigaltstack(&raw const ss, core::ptr::null_mut());
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_phase75_sigaltstack_autodisarm_alone_ok() {
        // SS_AUTODISARM alone leaves mode == 0, which is also valid.
        let mut stack_buf = [0u8; SIGSTKSZ];
        let ss = StackT {
            ss_sp: stack_buf.as_mut_ptr(),
            ss_flags: SS_AUTODISARM,
            ss_size: SIGSTKSZ,
        };
        crate::errno::set_errno(0);
        let ret = sigaltstack(&raw const ss, core::ptr::null_mut());
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_phase75_sigaltstack_autodisarm_with_disable_ok() {
        // SS_AUTODISARM | SS_DISABLE is valid (mode after masking is
        // SS_DISABLE).  Size is irrelevant because SS_DISABLE is set.
        let ss = StackT {
            ss_sp: core::ptr::null_mut(),
            ss_flags: SS_AUTODISARM | SS_DISABLE,
            ss_size: 0,
        };
        crate::errno::set_errno(0);
        let ret = sigaltstack(&raw const ss, core::ptr::null_mut());
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_phase75_sigaltstack_high_garbage_bits_rejected() {
        // High bits other than SS_AUTODISARM (bit 31) are not
        // recognised by Linux → EINVAL.
        let mut stack_buf = [0u8; SIGSTKSZ];
        let ss = StackT {
            ss_sp: stack_buf.as_mut_ptr(),
            ss_flags: 0x4000_0000, // bit 30 — unrecognised
            ss_size: SIGSTKSZ,
        };
        crate::errno::set_errno(0);
        let ret = sigaltstack(&raw const ss, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase75_sigaltstack_negative_flags_rejected() {
        // i32::MIN sets SS_AUTODISARM AND many garbage bits → EINVAL.
        let mut stack_buf = [0u8; SIGSTKSZ];
        let ss = StackT {
            ss_sp: stack_buf.as_mut_ptr(),
            ss_flags: i32::MIN | 0x4, // SS_AUTODISARM | bit2
            ss_size: SIGSTKSZ,
        };
        crate::errno::set_errno(0);
        let ret = sigaltstack(&raw const ss, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase75_sigaltstack_bad_flags_beat_bad_size() {
        // Tiny stack AND unknown flag bit: EINVAL (flags) wins over
        // ENOMEM (size).  Linux validates flags first.
        let mut stack_buf = [0u8; 64];
        let ss = StackT {
            ss_sp: stack_buf.as_mut_ptr(),
            ss_flags: 0x40, // unrecognised
            ss_size: 64,    // way below MINSIGSTKSZ
        };
        crate::errno::set_errno(0);
        let ret = sigaltstack(&raw const ss, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase75_sigaltstack_invalid_new_does_not_corrupt_old() {
        // When the new ss is invalid, oss is *still* populated first
        // (Linux behaviour) — caller should be able to read the old
        // state even if its set side fails.  We capture oss before
        // calling and verify it gets overwritten.
        let mut oss = StackT {
            ss_sp: 0xDEAD_BEEF as *mut u8,
            ss_flags: 0xCAFE,
            ss_size: 0xBAD,
        };
        let mut stack_buf = [0u8; SIGSTKSZ];
        let ss = StackT {
            ss_sp: stack_buf.as_mut_ptr(),
            ss_flags: 0x20, // garbage
            ss_size: SIGSTKSZ,
        };
        crate::errno::set_errno(0);
        let ret = sigaltstack(&raw const ss, &raw mut oss);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        // oss was populated before the validation failure.
        assert!(oss.ss_sp.is_null());
        assert_eq!(oss.ss_flags, SS_DISABLE);
        assert_eq!(oss.ss_size, 0);
    }

    // -- sigsuspend: NULL mask validation --

    #[test]
    fn test_phase75_sigsuspend_null_mask_efault() {
        crate::errno::set_errno(0);
        assert_eq!(sigsuspend(core::ptr::null()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_phase75_sigsuspend_valid_mask_returns_eintr() {
        // Empty mask is still a valid pointer → fall through to EINTR.
        let mask = SigsetT::EMPTY;
        crate::errno::set_errno(0);
        assert_eq!(sigsuspend(&raw const mask), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINTR);
    }

    #[test]
    fn test_phase75_sigsuspend_full_mask_returns_eintr() {
        let mask = SigsetT { bits: [u64::MAX; 16] };
        crate::errno::set_errno(0);
        assert_eq!(sigsuspend(&raw const mask), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINTR);
    }

    // -- sigwait: NULL set validation --

    #[test]
    fn test_phase75_sigwait_null_set_efault() {
        let mut sig: i32 = -42;
        // Save errno to make sure sigwait does NOT touch it
        // (it reports via the return value).
        crate::errno::set_errno(0);
        let ret = sigwait(core::ptr::null(), &raw mut sig);
        assert_eq!(ret, crate::errno::EFAULT);
        // errno itself must be unchanged.
        assert_eq!(crate::errno::get_errno(), 0);
        // The output sig slot must not have been written when set was
        // NULL — the validation runs before any store.
        assert_eq!(sig, -42);
    }

    #[test]
    fn test_phase75_sigwait_null_set_null_sig_efault() {
        // Buggy caller passes NULL for both — set NULL still wins (we
        // validate set first, never reaching the sig store).
        crate::errno::set_errno(0);
        let ret = sigwait(core::ptr::null(), core::ptr::null_mut());
        assert_eq!(ret, crate::errno::EFAULT);
    }

    // -- Ordering & buggy-caller scenarios --

    #[test]
    fn test_phase75_sigsuspend_null_beats_other_state() {
        // A buggy caller calls sigsuspend(NULL) in a loop — every call
        // must report EFAULT, never EINTR, never 0.  We do three calls
        // to be sure.
        for _ in 0..3 {
            crate::errno::set_errno(0);
            assert_eq!(sigsuspend(core::ptr::null()), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
        }
    }

    #[test]
    fn test_phase75_sigaltstack_size_check_still_runs_after_flag_fix() {
        // Regression: when we tightened flag validation, the existing
        // size-too-small check must still fire for legitimate flag
        // values (0 or SS_ONSTACK).
        let mut stack_buf = [0u8; 64];
        let ss = StackT {
            ss_sp: stack_buf.as_mut_ptr(),
            ss_flags: 0,
            ss_size: 64,
        };
        crate::errno::set_errno(0);
        let ret = sigaltstack(&raw const ss, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOMEM);
    }

    #[test]
    fn test_phase75_sigaltstack_size_check_runs_with_onstack_flag() {
        let mut stack_buf = [0u8; 64];
        let ss = StackT {
            ss_sp: stack_buf.as_mut_ptr(),
            ss_flags: SS_ONSTACK,
            ss_size: 64,
        };
        crate::errno::set_errno(0);
        let ret = sigaltstack(&raw const ss, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOMEM);
    }

    #[test]
    fn test_phase75_sigaltstack_autodisarm_with_too_small_stack_enomem() {
        // SS_AUTODISARM alone leaves mode == 0 (not SS_DISABLE), so the
        // size check should fire when the stack is too small.
        let mut stack_buf = [0u8; 64];
        let ss = StackT {
            ss_sp: stack_buf.as_mut_ptr(),
            ss_flags: SS_AUTODISARM,
            ss_size: 64,
        };
        crate::errno::set_errno(0);
        let ret = sigaltstack(&raw const ss, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOMEM);
    }

    #[test]
    fn test_phase75_ss_autodisarm_constant() {
        // SS_AUTODISARM is Linux's bit-31 modifier.
        assert_eq!(SS_AUTODISARM, 1 << 31);
        // It must not collide with SS_ONSTACK / SS_DISABLE.
        assert_eq!(SS_AUTODISARM & SS_ONSTACK, 0);
        assert_eq!(SS_AUTODISARM & SS_DISABLE, 0);
    }

    #[test]
    fn test_phase75_siginterrupt_ordering_with_invalid_flag_bit() {
        // POSIX defines flag as "0 or non-zero"; we accept anything for
        // flag.  Even garbage flag values should succeed on a valid
        // signal, and conversely an invalid signal beats any flag.
        assert_eq!(siginterrupt(SIGUSR1, i32::MAX), 0);
        assert_eq!(siginterrupt(SIGUSR1, i32::MIN), 0);
        crate::errno::set_errno(0);
        assert_eq!(siginterrupt(0, i32::MAX), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase75_workflow_signal_then_siginterrupt() {
        // Typical pattern: install handler with signal(), then mark
        // interruptible with siginterrupt().  Both should agree on
        // which signals are settable.
        let h: SighandlerT = SIG_DFL;
        // signal() accepts SIGUSR1; siginterrupt() should too.
        assert_ne!(signal(SIGUSR1, h), SIG_ERR);
        assert_eq!(siginterrupt(SIGUSR1, 1), 0);
        // Both reject SIGKILL.
        crate::errno::set_errno(0);
        assert_eq!(signal(SIGKILL, h), SIG_ERR);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        crate::errno::set_errno(0);
        assert_eq!(siginterrupt(SIGKILL, 1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase75_workflow_sigwait_buggy_uninit_set() {
        // A caller forgets to allocate the sigset, leaves a NULL.
        // sigwait must return EFAULT promptly without sleeping or
        // writing through sig.
        let mut sig: i32 = 12345;
        let start_errno = crate::errno::get_errno();
        let ret = sigwait(core::ptr::null(), &raw mut sig);
        assert_eq!(ret, crate::errno::EFAULT);
        // errno must not have moved.
        assert_eq!(crate::errno::get_errno(), start_errno);
        // sig must be untouched.
        assert_eq!(sig, 12345);
    }

    // =================================================================
    // Phase 211 — kill()/raise() signal delivery
    //
    // kill() now translates signals to native operations:
    //   - Terminate/Core signals → SYS_PROCESS_KILL (cross-process)
    //     or _exit(128+sig) (self, SIG_DFL)
    //   - Ignore signals → silently discarded (return 0)
    //   - Stop/Continue → ENOSYS (no kernel suspend)
    //   - Self-signals → dispatch via handler table
    //
    // raise() dispatches via dispatch_self_signal():
    //   - SIG_IGN → return 0
    //   - handler → invoke fn(sig), return 0
    //   - SIG_DFL → default action
    // =================================================================

    /// default_action classifies all standard signals correctly.
    #[test]
    fn test_phase211_default_action_classify() {
        // Terminate signals.
        for sig in [SIGHUP, SIGINT, SIGPIPE, SIGALRM, SIGTERM,
                    SIGUSR1, SIGUSR2, SIGVTALRM, SIGPROF, SIGIO, SIGPWR]
        {
            assert_eq!(
                default_action(sig),
                Some(DefaultAction::Terminate),
                "signal {sig} should be Terminate"
            );
        }
        // Core-dump signals.
        for sig in [SIGQUIT, SIGILL, SIGTRAP, SIGABRT, SIGBUS,
                    SIGFPE, SIGSEGV, SIGXCPU, SIGXFSZ, SIGSYS]
        {
            assert_eq!(
                default_action(sig),
                Some(DefaultAction::Core),
                "signal {sig} should be Core"
            );
        }
        // Ignore signals.
        for sig in [SIGCHLD, SIGURG, SIGWINCH] {
            assert_eq!(
                default_action(sig),
                Some(DefaultAction::Ignore),
                "signal {sig} should be Ignore"
            );
        }
        // Stop signals.
        for sig in [SIGSTOP, SIGTSTP, SIGTTIN, SIGTTOU] {
            assert_eq!(
                default_action(sig),
                Some(DefaultAction::Stop),
                "signal {sig} should be Stop"
            );
        }
        // Continue signal.
        assert_eq!(default_action(SIGCONT), Some(DefaultAction::Continue));
        // RT signals default to Terminate.
        assert_eq!(default_action(SIGRTMIN), Some(DefaultAction::Terminate));
        assert_eq!(default_action(SIGRTMAX), Some(DefaultAction::Terminate));
        // Out of range.
        assert_eq!(default_action(0), None);
        assert_eq!(default_action(-1), None);
        assert_eq!(default_action(NSIG), None);
    }

    /// raise() invokes a custom handler registered via signal().
    #[test]
    fn test_phase211_raise_invokes_handler() {
        use core::sync::atomic::{AtomicI32, Ordering};
        static RECEIVED: AtomicI32 = AtomicI32::new(0);

        extern "C" fn handler(sig: i32) {
            RECEIVED.store(sig, Ordering::Relaxed);
        }

        RECEIVED.store(0, Ordering::Relaxed);
        let old = signal(SIGUSR1, handler as SighandlerT);
        crate::errno::set_errno(0);
        let ret = raise(SIGUSR1);
        assert_eq!(ret, 0, "raise with handler should return 0");
        assert_eq!(
            RECEIVED.load(Ordering::Relaxed),
            SIGUSR1,
            "handler should receive the signal number"
        );
        signal(SIGUSR1, old);
    }

    /// raise() with SIG_IGN for various terminating signals.
    #[test]
    fn test_phase211_raise_sig_ign_terminators() {
        for sig in [SIGHUP, SIGINT, SIGTERM, SIGUSR1, SIGUSR2, SIGPIPE] {
            let old = signal(sig, SIG_IGN);
            crate::errno::set_errno(0);
            assert_eq!(raise(sig), 0, "raise({sig}) with SIG_IGN should return 0");
            signal(sig, old);
        }
    }

    /// kill() self-signal dispatches the registered handler.
    #[test]
    fn test_phase211_kill_self_invokes_handler() {
        use core::sync::atomic::{AtomicI32, Ordering};
        static GOT: AtomicI32 = AtomicI32::new(0);

        extern "C" fn my_handler(sig: i32) {
            GOT.store(sig, Ordering::Relaxed);
        }

        GOT.store(0, Ordering::Relaxed);
        let old = signal(SIGUSR2, my_handler as SighandlerT);
        // kill(self, SIGUSR2) → dispatch_self_signal → handler.
        // We use pid = SYS_PROCESS_ID result.  In test builds this is
        // inline asm so we use SIGUSR2 directly via raise() as proxy.
        crate::errno::set_errno(0);
        let ret = raise(SIGUSR2);
        assert_eq!(ret, 0);
        assert_eq!(GOT.load(Ordering::Relaxed), SIGUSR2);
        signal(SIGUSR2, old);
    }

    /// raise() with SIG_DFL for ignore signals returns 0.
    #[test]
    fn test_phase211_raise_default_ignore_signals() {
        for sig in [SIGCHLD, SIGURG, SIGWINCH] {
            signal(sig, SIG_DFL);
            crate::errno::set_errno(0);
            assert_eq!(
                raise(sig),
                0,
                "raise({sig}) with SIG_DFL should return 0 (default=Ignore)"
            );
        }
    }

    /// raise() with SIG_DFL for stop signals returns ENOSYS.
    #[test]
    fn test_phase211_raise_default_stop_enosys() {
        for sig in [SIGTSTP, SIGTTIN, SIGTTOU] {
            signal(sig, SIG_DFL);
            crate::errno::set_errno(0);
            assert_eq!(raise(sig), -1, "raise({sig}) stop should fail");
            assert_eq!(
                crate::errno::get_errno(),
                crate::errno::ENOSYS,
                "raise({sig}) stop should set ENOSYS"
            );
        }
    }

    /// raise() with SIG_DFL for SIGCONT returns ENOSYS.
    #[test]
    fn test_phase211_raise_default_continue_enosys() {
        signal(SIGCONT, SIG_DFL);
        crate::errno::set_errno(0);
        assert_eq!(raise(SIGCONT), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// Handler set via signal() then ignored via SIG_IGN: verify
    /// the handler is no longer called.
    #[test]
    fn test_phase211_signal_then_ignore_suppresses_handler() {
        use core::sync::atomic::{AtomicBool, Ordering};
        static CALLED: AtomicBool = AtomicBool::new(false);

        extern "C" fn h(_sig: i32) {
            CALLED.store(true, Ordering::Relaxed);
        }

        CALLED.store(false, Ordering::Relaxed);
        signal(SIGUSR1, h as SighandlerT);
        signal(SIGUSR1, SIG_IGN);
        crate::errno::set_errno(0);
        assert_eq!(raise(SIGUSR1), 0);
        assert!(
            !CALLED.load(Ordering::Relaxed),
            "handler should NOT be called after SIG_IGN"
        );
        signal(SIGUSR1, SIG_DFL);
    }

    /// kill() with pid <= 0 and valid signal still returns ENOSYS
    /// (no process group support).
    #[test]
    fn test_phase211_kill_pgroup_enosys() {
        crate::errno::set_errno(0);
        let ret = kill(0, SIGTERM);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);

        crate::errno::set_errno(0);
        let ret = kill(-1, SIGINT);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// Cross-process SIGCHLD (ignore-default) is discarded even
    /// without CAP_KILL — wait, no: CAP_KILL is checked before the
    /// default-action dispatch for pid > 0.  Without the cap, EPERM.
    #[test]
    fn test_phase211_kill_cross_ignore_no_cap_eperm() {
        let _g = phase203_cap::CapGuard::snapshot();
        phase203_cap::drop_cap_kill();
        crate::errno::set_errno(0);
        let ret = kill(1, SIGCHLD);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EPERM);
    }

    // =================================================================
    // Phase 212 — sigaddset/sigdelset/sigismember: EFAULT for NULL set
    //
    // Linux returns EFAULT for NULL user-space pointers (via
    // copy_from_user/copy_to_user).  Our stubs used EINVAL for both
    // NULL set and bad signum.  Phase 212 splits the check:
    //   NULL set → EFAULT, bad signum → EINVAL.
    // =================================================================

    /// sigaddset: NULL set → EFAULT.
    #[test]
    fn test_phase212_sigaddset_null_efault() {
        crate::errno::set_errno(0);
        let ret = unsafe { sigaddset(core::ptr::null_mut(), SIGINT) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    /// sigaddset: bad signum → EINVAL.
    #[test]
    fn test_phase212_sigaddset_bad_signum_einval() {
        let mut set = SigsetT::EMPTY;
        crate::errno::set_errno(0);
        let ret = unsafe { sigaddset(&raw mut set, 0) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// sigaddset: NULL + bad signum → EFAULT (NULL wins).
    #[test]
    fn test_phase212_sigaddset_null_beats_bad_signum() {
        crate::errno::set_errno(0);
        let ret = unsafe { sigaddset(core::ptr::null_mut(), 0) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    /// sigdelset: NULL set → EFAULT.
    #[test]
    fn test_phase212_sigdelset_null_efault() {
        crate::errno::set_errno(0);
        let ret = unsafe { sigdelset(core::ptr::null_mut(), SIGINT) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    /// sigdelset: bad signum → EINVAL.
    #[test]
    fn test_phase212_sigdelset_bad_signum_einval() {
        let mut set = SigsetT { bits: [u64::MAX; 16] };
        crate::errno::set_errno(0);
        let ret = unsafe { sigdelset(&raw mut set, -1) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// sigismember: NULL set → EFAULT.
    #[test]
    fn test_phase212_sigismember_null_efault() {
        crate::errno::set_errno(0);
        let ret = unsafe { sigismember(core::ptr::null(), SIGINT) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    /// sigismember: bad signum → EINVAL.
    #[test]
    fn test_phase212_sigismember_bad_signum_einval() {
        let set = SigsetT { bits: [u64::MAX; 16] };
        crate::errno::set_errno(0);
        let ret = unsafe { sigismember(&raw const set, 0) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// sigismember: NULL + bad signum → EFAULT (NULL wins).
    #[test]
    fn test_phase212_sigismember_null_beats_bad_signum() {
        crate::errno::set_errno(0);
        let ret = unsafe { sigismember(core::ptr::null(), -1) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }
}
