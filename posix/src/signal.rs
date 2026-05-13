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
//! `kill()` and `sigprocmask()` remain stubs returning ENOSYS.

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
// Signal functions (stubs)
// ---------------------------------------------------------------------------

/// Registered signal handlers.
///
/// Index 0 unused (signals are 1-based).  Initialized to SIG_DFL.
static mut HANDLERS: [SighandlerT; NSIG as usize] = [SIG_DFL; NSIG as usize];

/// Install a signal handler.
///
/// Stores the handler and returns the previous one.  Handlers are
/// never actually invoked since our OS doesn't deliver Unix signals.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn signal(signum: i32, handler: SighandlerT) -> SighandlerT {
    if !(1..NSIG).contains(&signum) || signum == SIGKILL || signum == SIGSTOP {
        errno::set_errno(errno::EINVAL);
        return SIG_ERR;
    }

    // SAFETY: Single-threaded access. signum range checked above.
    let idx = signum as usize;
    let handlers = unsafe { core::ptr::addr_of_mut!(HANDLERS).as_mut() };
    let Some(handlers) = handlers else {
        errno::set_errno(errno::EINVAL);
        return SIG_ERR;
    };
    let Some(slot) = handlers.get_mut(idx) else {
        errno::set_errno(errno::EINVAL);
        return SIG_ERR;
    };
    let old = *slot;
    *slot = handler;
    old
}

/// `sigaction` structure for `sigaction()`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Sigaction {
    /// Signal handler (sa_handler or sa_sigaction).
    pub sa_handler: SighandlerT,
    /// Additional signals to block during handler.
    pub sa_mask: u64,
    /// Flags (SA_RESTART, SA_NOCLDSTOP, etc.).
    pub sa_flags: i32,
    /// Restore handler (unused).
    pub sa_restorer: usize,
}

/// Flags for sigaction.
pub const SA_NOCLDSTOP: i32 = 1;
pub const SA_NOCLDWAIT: i32 = 2;
pub const SA_SIGINFO: i32 = 4;
pub const SA_RESTART: i32 = 0x1000_0000;
pub const SA_NODEFER: i32 = 0x4000_0000;

/// Examine and change a signal action.
///
/// Stores the new action (if provided) and returns the old action.
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

    // Return old handler via oldact.
    if !oldact.is_null() {
        let idx = signum as usize;
        let handler = unsafe {
            let handlers = core::ptr::addr_of!(HANDLERS);
            (*handlers).get(idx).copied().unwrap_or(SIG_DFL)
        };
        unsafe {
            (*oldact).sa_handler = handler;
            (*oldact).sa_mask = 0;
            (*oldact).sa_flags = 0;
            (*oldact).sa_restorer = 0;
        }
    }

    // Store new handler from act.
    if !act.is_null() {
        let new_handler = unsafe { (*act).sa_handler };
        let idx = signum as usize;
        let handlers = unsafe { core::ptr::addr_of_mut!(HANDLERS).as_mut() };
        if let Some(handlers) = handlers
            && let Some(slot) = handlers.get_mut(idx)
        {
            *slot = new_handler;
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
/// Stub: succeeds silently (stores nothing).  Many programs call
/// `sigprocmask(SIG_BLOCK, &set, &oldset)` during initialization
/// and expect success.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sigprocmask(
    _how: i32,
    _set: *const u64,
    oldset: *mut u64,
) -> i32 {
    // Return empty old set if requested.
    if !oldset.is_null() {
        unsafe { *oldset = 0; }
    }
    0 // Succeed silently.
}

/// Wait for a signal.
///
/// Stub: sets errno to EINTR and returns -1 (POSIX specifies
/// sigsuspend always returns -1 with errno=EINTR).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sigsuspend(_mask: *const u64) -> i32 {
    errno::set_errno(errno::EINTR);
    -1
}

/// Examine pending signals.
///
/// Stub: returns empty set (no signals pending).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn sigpending(set: *mut u64) -> i32 {
    if set.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    unsafe { *set = 0; }
    0
}

/// Initialize a signal set to empty.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn sigemptyset(set: *mut u64) -> i32 {
    if set.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    unsafe { *set = 0; }
    0
}

/// Initialize a signal set to full.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn sigfillset(set: *mut u64) -> i32 {
    if set.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    unsafe { *set = u64::MAX; }
    0
}

/// Add a signal to a signal set.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn sigaddset(set: *mut u64, signum: i32) -> i32 {
    if set.is_null() || !(1..NSIG).contains(&signum) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // SAFETY: set is non-null (checked above). signum is in [1, NSIG),
    // so signum-1 is in [0, 63], which is a valid u64 shift amount.
    unsafe { *set |= 1u64 << (signum.wrapping_sub(1) as u32); }
    0
}

/// Remove a signal from a signal set.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn sigdelset(set: *mut u64, signum: i32) -> i32 {
    if set.is_null() || !(1..NSIG).contains(&signum) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // SAFETY: set is non-null; shift amount is [0, 63] per range check.
    unsafe { *set &= !(1u64 << (signum.wrapping_sub(1) as u32)); }
    0
}

/// Test whether a signal is in a signal set.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn sigismember(set: *const u64, signum: i32) -> i32 {
    if set.is_null() || !(1..NSIG).contains(&signum) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // SAFETY: set is non-null; shift amount is [0, 63] per range check.
    let val = unsafe { *set };
    i32::from(val & (1u64 << (signum.wrapping_sub(1) as u32)) != 0)
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
static SIGNAL_NAMES: [&[u8]; 21] = [
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
    b"Unknown signal 16\0", // 16 (unused on Linux x86_64)
    b"Child exited\0",      // 17 SIGCHLD
    b"Continued\0",         // 18 SIGCONT
    b"Stopped (signal)\0",  // 19 SIGSTOP
    b"Stopped\0",           // 20 SIGTSTP
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
/// returns `EAGAIN` (no signal delivered).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sigwait(_set: *const u64, sig: *mut i32) -> i32 {
    // Sleep briefly so callers in a loop don't spin.
    let _ = crate::syscall::syscall1(crate::syscall::SYS_SLEEP, 1_000_000_000_u64);
    if !sig.is_null() {
        // SAFETY: sig is valid if non-null (caller contract).
        unsafe { *sig = 0; }
    }
    crate::errno::EAGAIN
}

/// Wait for a signal with a timeout.
///
/// Stub: returns -1 with `EAGAIN`.  The `timeout` parameter is ignored
/// since we don't have signal delivery.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sigtimedwait(
    _set: *const u64,
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
// Realtime signal range
// ---------------------------------------------------------------------------

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
