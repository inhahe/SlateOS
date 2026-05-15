//! POSIX signal stubs.
//!
//! Our OS uses IPC messages instead of Unix signals.  This module
//! provides the POSIX signal constants, handler registration, signal
//! sets, and `sigaction` so that C programs can link and run.
//!
//! ## Design
//!
//! `signal()` and `sigaction()` store handlers in a static table but
//! signals are never actually delivered (our OS uses IPC messages for
//! process control).  This means `signal(SIGPIPE, SIG_IGN)` succeeds
//! (many programs do this at startup), but no handler ever fires.
//! `sigprocmask()` stores the blocked mask so get/set round-trips work.
//! `kill()` remains a stub returning ENOSYS.

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

/// Send a signal to a process.
///
/// Stub: always returns -1 and sets errno to ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn kill(pid: i32, sig: i32) -> i32 {
    let _ = pid;
    let _ = sig;
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Send a signal to the calling process.
///
/// For SIGABRT, calls abort().  Otherwise returns -1/ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn raise(sig: i32) -> i32 {
    if sig == SIGABRT {
        crate::unistd::abort();
    }
    errno::set_errno(errno::ENOSYS);
    -1
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sigsuspend(_mask: *const SigsetT) -> i32 {
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn sigaddset(set: *mut SigsetT, signum: i32) -> i32 {
    if set.is_null() || !(1..NSIG).contains(&signum) {
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn sigdelset(set: *mut SigsetT, signum: i32) -> i32 {
    if set.is_null() || !(1..NSIG).contains(&signum) {
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn sigismember(set: *const SigsetT, signum: i32) -> i32 {
    if set.is_null() || !(1..NSIG).contains(&signum) {
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
        // POSIX: if ss_flags contains anything other than SS_DISABLE,
        // and the stack size is below MINSIGSTKSZ, return ENOMEM.
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
/// Stub: always returns 0.  Since our OS doesn't deliver signals,
/// there is no SA_RESTART behavior to toggle.  Programs that call
/// `siginterrupt(SIGALRM, 1)` (common in timeout implementations)
/// will succeed.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn siginterrupt(_sig: i32, _flag: i32) -> i32 {
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sigwait(_set: *const SigsetT, sig: *mut i32) -> i32 {
    // Sleep briefly so callers in a loop don't spin.
    let _ = crate::syscall::syscall1(crate::syscall::SYS_SLEEP, 1_000_000_000_u64);
    if !sig.is_null() {
        // SAFETY: sig is valid if non-null (caller contract).
        unsafe { *sig = 0; }
    }
    crate::errno::EINTR
}

/// Wait for a signal with a timeout.
///
/// Stub: returns -1 with `EAGAIN` (timeout expired, no signal
/// delivered).  The `timeout` parameter is ignored since we don't
/// have signal delivery.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sigtimedwait(
    _set: *const SigsetT,
    _info: *mut core::ffi::c_void,
    _timeout: *const crate::stat::Timespec,
) -> i32 {
    crate::errno::set_errno(crate::errno::EAGAIN);
    -1
}

/// Queue a signal to a process.
///
/// Stub: returns -1 with `ENOSYS` (no signal delivery mechanism).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sigqueue(_pid: crate::types::PidT, _sig: i32, _value: usize) -> i32 {
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
    fn test_siginterrupt_always_succeeds() {
        assert_eq!(siginterrupt(SIGALRM, 1), 0);
        assert_eq!(siginterrupt(SIGALRM, 0), 0);
    }

    // -- kill / raise stubs --

    #[test]
    fn test_kill_returns_enosys() {
        let ret = kill(1, SIGTERM);
        assert_eq!(ret, -1);
    }

    // -- kill errno verification --

    #[test]
    fn test_kill_sets_errno() {
        crate::errno::set_errno(0);
        let ret = kill(1, SIGTERM);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
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

    // -- raise (non-SIGABRT) --

    #[test]
    fn test_raise_non_sigabrt_returns_enosys() {
        // raise(anything except SIGABRT) should return -1 with ENOSYS.
        errno::set_errno(0);
        assert_eq!(raise(SIGTERM), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_raise_sigint_returns_enosys() {
        errno::set_errno(0);
        assert_eq!(raise(SIGINT), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_raise_sighup_returns_enosys() {
        errno::set_errno(0);
        assert_eq!(raise(SIGHUP), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_raise_sigkill_returns_enosys() {
        // SIGKILL without kernel support → ENOSYS.
        errno::set_errno(0);
        assert_eq!(raise(SIGKILL), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_raise_zero_returns_enosys() {
        // Signal 0 is not SIGABRT → ENOSYS.
        errno::set_errno(0);
        assert_eq!(raise(0), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
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
}
