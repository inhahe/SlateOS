//! POSIX signal stubs.
//!
//! Our OS uses IPC messages instead of Unix signals.  This module
//! provides the POSIX signal constants and stub functions so that
//! C programs that reference signal names and `signal()` can link.
//!
//! The stubs set errno to ENOSYS and return SIG_ERR.

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

/// Install a signal handler.
///
/// Stub: always returns SIG_ERR and sets errno to ENOSYS.
/// Our OS uses IPC messages instead of Unix signals.
#[unsafe(no_mangle)]
pub extern "C" fn signal(signum: i32, handler: SighandlerT) -> SighandlerT {
    let _ = signum;
    let _ = handler;
    errno::set_errno(errno::ENOSYS);
    SIG_ERR
}

/// Send a signal to a process.
///
/// Stub: always returns -1 and sets errno to ENOSYS.
#[unsafe(no_mangle)]
pub extern "C" fn kill(pid: i32, sig: i32) -> i32 {
    let _ = pid;
    let _ = sig;
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Send a signal to the calling process.
///
/// For SIGABRT, calls abort().  Otherwise returns -1/ENOSYS.
#[unsafe(no_mangle)]
pub extern "C" fn raise(sig: i32) -> i32 {
    if sig == SIGABRT {
        crate::unistd::abort();
    }
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Examine and change blocked signals.
///
/// Stub: sets errno to ENOSYS and returns -1.
#[unsafe(no_mangle)]
pub extern "C" fn sigprocmask(
    _how: i32,
    _set: *const u64,
    _oldset: *mut u64,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Wait for a signal.
///
/// Stub: sets errno to ENOSYS and returns -1.
#[unsafe(no_mangle)]
pub extern "C" fn sigsuspend(_mask: *const u64) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Initialize a signal set to empty.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sigemptyset(set: *mut u64) -> i32 {
    if set.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    unsafe { *set = 0; }
    0
}

/// Initialize a signal set to full.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sigfillset(set: *mut u64) -> i32 {
    if set.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    unsafe { *set = u64::MAX; }
    0
}

/// Add a signal to a signal set.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sigismember(set: *const u64, signum: i32) -> i32 {
    if set.is_null() || !(1..NSIG).contains(&signum) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // SAFETY: set is non-null; shift amount is [0, 63] per range check.
    let val = unsafe { *set };
    i32::from(val & (1u64 << (signum.wrapping_sub(1) as u32)) != 0)
}
