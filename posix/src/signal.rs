// Signal-set operations in this file index a fixed-size `bits`
// array (`SigsetT`'s bit-vector representation) by word and clear/
// set bits indexed by `(signum - 1) % 64`.  `signum` is validated to
// be in `1..=NSIG` before these helpers are called.  The arithmetic
// is `signum - 1` (signum ≥ 1) and `1 << bit` where `bit < 64` — no
// overflow path exists.
#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

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
//! Cross-process `kill()` delivers via the kernel signal shim
//! (`SYS_SIGNAL_SEND`): the kernel sets the signal pending on the
//! target and delivers it asynchronously to the target's registered
//! trampoline, or applies the default action if no trampoline is
//! registered.  The sender does not classify by disposition.
//!
//! ## Asynchronous delivery
//!
//! At startup `init_signals()` registers `__signal_trampoline` with the
//! kernel (`SYS_SIGNAL_REGISTER`).  When a pending, unblocked signal is
//! delivered, the kernel redirects the interrupted thread to the
//! trampoline with a saved [`SignalContext`]; the trampoline runs
//! `dispatch_self_signal()` and then issues `SYS_SIGNAL_RETURN` to
//! resume the interrupted code.  This mirrors SEH-style exception
//! delivery and is *not* a process-control mechanism.
//!
//! ## Job-control stops
//!
//! The `Stop` default action (SIGSTOP/SIGTSTP/SIGTTIN/SIGTTOU) is the one
//! default the userspace disposition table cannot carry out on its own:
//! suspending every thread of the process is a scheduler operation.  It
//! goes through [`SYS_SIGNAL_STOP_SELF`](crate::syscall::SYS_SIGNAL_STOP_SELF),
//! which parks the caller and returns only once someone sends `SIGCONT`.
//!
//! That is a *separate* syscall from `SYS_SIGNAL_SEND` rather than
//! `kill(getpid(), SIGTSTP)`, because the kernel's send path checks for a
//! registered trampoline before it reaches the catchable stop signals — and
//! a native process always has one.  Sending ourselves a SIGTSTP would
//! therefore mark it pending for handler delivery and re-enter the very
//! dispatcher that just resolved it to `SIG_DFL`: an infinite delivery
//! loop, not a stop.  The dedicated number also lets the recorded wait
//! status name the signal that actually stopped us, so a shell's Ctrl-Z
//! reports `SIGTSTP` and not `SIGSTOP`.
//!
//! `sigprocmask()` stores the blocked mask for get/set round-trips and
//! mirrors the low 64 signals to the kernel (`SYS_SIGNAL_MASK`) so the
//! blocked set actually suppresses delivery.  `sigpending()` queries the
//! kernel (`SYS_SIGNAL_PENDING`) for the pending set.

// Calls `process::_exit` (a POSIX function whose name is literally
// underscore-prefixed in the POSIX spec — _exit(2) — not a private item).
#![allow(clippy::used_underscore_items)]

use crate::errno;
use crate::perprocess::process_global;

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

process_global! {
    /// Registered signal actions.
    ///
    /// Index 0 unused (signals are 1-based).  Initialized to SIG_DFL.
    /// Stores the full `Sigaction` so that `sigaction(sig, NULL, &old)`
    /// returns the correct `sa_mask`, `sa_flags`, and `sa_restorer`.
    fn actions_ptr() -> [Sigaction; NSIG as usize] = [DEFAULT_SIGACTION; NSIG as usize];

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
    fn blocked_mask_ptr() -> SigsetT = SigsetT::EMPTY;
}

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

    // SAFETY: `actions_ptr()` is owned solely by the caller (the process on
    // the target, this thread on host builds).  signum range checked above.
    let idx = signum as usize;
    let actions = unsafe { actions_ptr().as_mut() };
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
        // SAFETY: `actions_ptr()` is caller-owned; idx in [1, NSIG).
        let old = unsafe {
            let actions = actions_ptr();
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
        // SAFETY: `actions_ptr()` is caller-owned; idx in [1, NSIG).
        let actions = unsafe { actions_ptr().as_mut() };
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
        SIGHUP | SIGINT | SIGPIPE | SIGALRM | SIGTERM | SIGUSR1 | SIGUSR2 | SIGVTALRM | SIGPROF
        | SIGIO | SIGPWR => Some(DefaultAction::Terminate),
        SIGQUIT | SIGILL | SIGTRAP | SIGABRT | SIGBUS | SIGFPE | SIGSEGV | SIGXCPU | SIGXFSZ
        | SIGSYS => Some(DefaultAction::Core),
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

/// Bit position for signal `sig` within the low 64-signal word
/// (signal N → bit N-1).  Returns 0 for signals outside `[1, 64]`,
/// which the low word cannot represent.
fn sigmask_bit(sig: i32) -> u64 {
    if (1..=64).contains(&sig) {
        // sig-1 is in [0, 63]; the mask keeps the shift in range and
        // silences arithmetic-side-effects lints.
        1u64 << ((sig as u32).wrapping_sub(1) & 63)
    } else {
        0
    }
}

/// Compute the blocked mask to install for the duration of a handler.
///
/// Starts from `saved` (the mask active when the signal was taken),
/// unions the handler's `sa_mask` (low word), and — unless `SA_NODEFER`
/// is set — adds the delivered signal itself.  Per POSIX this is the set
/// of signals blocked while the handler runs, which prevents the handler
/// from being re-entered by its own signal.
fn handler_block_mask(saved: u64, sa_mask_low: u64, sa_flags: u64, sig: i32) -> u64 {
    let mut m = saved | sa_mask_low;
    if sa_flags & SA_NODEFER == 0 {
        m |= sigmask_bit(sig);
    }
    m
}

/// Read the current process-wide blocked mask (low 64 signals).
fn current_blocked_low() -> u64 {
    // SAFETY: `blocked_mask_ptr()` is owned solely by the caller.
    unsafe { blocked_mask_ptr().read().bits[0] }
}

/// Replace the low 64 signals of the process-wide blocked mask and mirror
/// the change to the kernel so asynchronous delivery honours it.
fn apply_blocked_low(low: u64) {
    // SAFETY: `blocked_mask_ptr()` is owned solely by the caller.
    unsafe {
        let mut m = blocked_mask_ptr().read();
        m.bits[0] = low;
        blocked_mask_ptr().write(m);
    }
    sync_kernel_blocked_mask(low);
}

/// Capture the current process-wide blocked mask for `sigsetjmp`.
///
/// Called from the `sigsetjmp` assembly when its `savemask` argument is
/// non-zero.  Returns the low 64 signals of the blocked mask so it can be
/// stored in the `sigjmp_buf` and restored by a later `siglongjmp`.  This
/// is what lets a handler escape via `siglongjmp` and still have the mask
/// that signal dispatch installed unwound correctly.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
pub extern "C" fn __posix_sigjmp_save_mask() -> u64 {
    current_blocked_low()
}

/// Restore a previously-saved blocked mask for `siglongjmp`.
///
/// Called from the `siglongjmp` assembly when the `sigjmp_buf` recorded a
/// saved mask.  Mirrors the change to the kernel so asynchronous delivery
/// honours it immediately.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
pub extern "C" fn __posix_sigjmp_restore_mask(low: u64) {
    apply_blocked_low(low);
}

/// Mark `sig` pending on the calling process.
///
/// Used when a signal is raised synchronously but is currently blocked:
/// it must be remembered and delivered once unblocked.  We route through
/// the kernel shim (`SYS_SIGNAL_SEND` to self), so that restoring the
/// blocked mask (`SYS_SIGNAL_MASK`) triggers delivery at the next
/// syscall-return boundary.  No-op on the host (no kernel to track it).
#[cfg(target_os = "none")]
fn set_pending_self(sig: i32) {
    let self_pid = crate::syscall::syscall0(crate::syscall::SYS_PROCESS_ID) as u64;
    let _ = crate::syscall::syscall2(crate::syscall::SYS_SIGNAL_SEND, self_pid, sig as u64);
}

#[cfg(not(target_os = "none"))]
fn set_pending_self(_sig: i32) {}

/// Reset a signal's disposition to the default (`SIG_DFL`) with empty
/// flags/mask.  Invoked for `SA_RESETHAND` before the handler runs, so
/// the one-shot handler is not re-installed.
fn reset_disposition(sig: i32) {
    if !(1..NSIG).contains(&sig) {
        return;
    }
    let idx = sig as usize;
    // SAFETY: `actions_ptr()` is caller-owned; idx in [1, NSIG).
    let actions = unsafe { actions_ptr().as_mut() };
    if let Some(actions) = actions
        && let Some(slot) = actions.get_mut(idx)
    {
        *slot = DEFAULT_SIGACTION;
    }
}

/// Planned outcome of dispatching a signal to the calling process.
///
/// Computed by [`plan_self_dispatch`] from the current blocked mask and
/// the registered action.  Keeping the *policy* separate from the
/// *effects* (global reads/writes and the actual handler call) makes the
/// decision logic pure and unit-testable without touching global state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelfDispatch {
    /// Signal is blocked → leave it pending, deliver nothing now.
    Pending,
    /// Disposition is `SIG_IGN` → discard the signal.
    Ignore,
    /// Run the registered handler.
    Handler {
        /// The handler function pointer (already known `!= SIG_DFL/IGN`).
        handler: usize,
        /// Blocked mask (low 64 signals) to install for the handler's
        /// duration: the saved mask plus `sa_mask`, plus the delivered
        /// signal itself unless `SA_NODEFER`.
        mask_during: u64,
        /// `SA_RESETHAND`: reset the disposition to `SIG_DFL` first.
        reset: bool,
    },
    /// Disposition is `SIG_DFL` → apply the Linux default action.
    Default,
}

/// Decide how a self-directed signal should be handled.
///
/// Pure: depends only on its arguments.  Preconditions: `sig` is not
/// `SIGKILL`/`SIGSTOP` (the caller handles those — they cannot be caught,
/// blocked, or ignored).
fn plan_self_dispatch(
    sig: i32,
    blocked_low: u64,
    handler: usize,
    sa_flags: u64,
    sa_mask_low: u64,
) -> SelfDispatch {
    // Blocked signals are not delivered now — they become pending and
    // are delivered once unblocked.
    let bit = sigmask_bit(sig);
    if bit != 0 && (blocked_low & bit) != 0 {
        return SelfDispatch::Pending;
    }
    if handler == SIG_IGN {
        return SelfDispatch::Ignore;
    }
    if handler != SIG_DFL {
        return SelfDispatch::Handler {
            handler,
            mask_during: handler_block_mask(blocked_low, sa_mask_low, sa_flags, sig),
            reset: sa_flags & SA_RESETHAND != 0,
        };
    }
    SelfDispatch::Default
}

/// Read the registered `(handler, sa_flags, sa_mask_low)` for `sig`.
///
/// Out-of-range signals report the default disposition.
fn lookup_action(sig: i32) -> (usize, u64, u64) {
    if !(1..NSIG).contains(&sig) {
        return (SIG_DFL, 0, 0);
    }
    let idx = sig as usize;
    // SAFETY: `actions_ptr()` is caller-owned; idx in [1, NSIG).
    unsafe {
        let actions = actions_ptr();
        (*actions).get(idx).map_or((SIG_DFL, 0, 0), |a| {
            (a.sa_handler, a.sa_flags, a.sa_mask.bits[0])
        })
    }
}

/// Ask the kernel to stop this process for job control, reporting `sig` as
/// the wait-status stop signal.
///
/// Returns 0 once a `SIGCONT` resumes us, or -1 with `errno` set.
///
/// This must not be expressed as `SYS_SIGNAL_SEND` to ourselves.  The
/// kernel does not hold our `sigaction` table — it only knows whether a
/// signal trampoline is registered, and for any process using this crate
/// one always is.  So re-posting a *catchable* stop signal
/// (SIGTSTP/SIGTTIN/SIGTTOU) is classified as "deliver to the handler",
/// which re-enters the very dispatcher that just resolved the disposition
/// to `SIG_DFL`: an infinite delivery loop rather than a stop.
/// `SYS_SIGNAL_STOP_SELF` reports the already-made decision instead of
/// asking the kernel to re-derive one from state it cannot see.
///
/// On host builds every raw syscall returns the `-ENOSYS` sentinel, so
/// this reports `ENOSYS` there — which is correct, as a host test process
/// has no SlateOS kernel to suspend it.
fn stop_self(sig: i32) -> i32 {
    // Only ever called for a signal `default_action` classified as `Stop`,
    // so `sig` is one of 19..=22; the fallible conversion is defensive.
    let Ok(nr) = u64::try_from(sig) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    let ret = crate::syscall::syscall1(crate::syscall::SYS_SIGNAL_STOP_SELF, nr);
    if ret < 0 {
        errno::set_errno(stop_self_errno(ret));
        return -1;
    }
    0
}

/// Map `SYS_SIGNAL_STOP_SELF`'s failure codes to `errno`.
///
/// Deliberately not `errno::translate`: that maps *native `KernelError`*
/// codes, and its catch-all is `EIO`.  The value this call site most often
/// sees on a host test build is the raw-syscall sentinel `-38`, which is
/// `-(ENOSYS)` in **Linux** numbering and means "there is no SlateOS kernel
/// here" — not an I/O error.  `-38` is not in the native code space
/// (`-1..=-6`, `-100`s, `-200`s, …), so the two cannot collide and naming
/// it here is unambiguous.
fn stop_self_errno(ret: i64) -> i32 {
    match ret {
        // The host build's raw-syscall sentinel: no kernel to suspend us.
        -38 => errno::ENOSYS,
        errno::native::INVALID_ARGUMENT => errno::EINVAL,
        errno::native::NO_SUCH_PROCESS => errno::ESRCH,
        // Any other kernel failure is genuinely unexpected here; report it
        // as an I/O error rather than inventing a more specific meaning.
        _ => errno::EIO,
    }
}

/// Apply the Linux default action (`SIG_DFL`) for `sig`.
///
/// Returns 0 for actions that complete in-process (Ignore, and Continue
/// when we are already running), terminates the process for
/// Terminate/Core, suspends it via the kernel for Stop, and reports
/// `EINVAL` for unknown signals.
fn apply_default_action(sig: i32) -> i32 {
    match default_action(sig) {
        Some(DefaultAction::Terminate | DefaultAction::Core) => {
            if sig == SIGABRT {
                crate::unistd::abort();
            }
            // 128 + sig (Unix convention)
            crate::process::_exit(128i32.wrapping_add(sig));
        }
        Some(DefaultAction::Ignore) => 0,
        Some(DefaultAction::Stop) => stop_self(sig),
        Some(DefaultAction::Continue) => {
            // Reaching the SIGCONT default action means we are already
            // running: either the kernel resumed us and then delivered the
            // signal, or we were never stopped.  Either way the default
            // action ("continue the process") is already satisfied, so
            // there is nothing to ask the kernel for.  POSIX: SIGCONT on a
            // running process has no effect beyond running a handler.
            0
        }
        None => {
            errno::set_errno(errno::EINVAL);
            -1
        }
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
/// ## Blocking semantics
///
/// If `sig` is currently blocked (per the process-wide mask), it is not
/// delivered now: it is marked pending and returns 0 (Linux: a blocked
/// signal stays pending until unblocked).  `SIGKILL`/`SIGSTOP` cannot be
/// blocked, so they bypass this check.
///
/// While a registered handler runs, the delivered signal is automatically
/// added to the blocked mask (unless `SA_NODEFER`), along with the
/// handler's `sa_mask`.  This mirrors POSIX/Linux semantics and prevents a
/// handler that re-raises its own signal from re-entering.  The previous
/// mask is restored after the handler returns.  `SA_RESETHAND` resets the
/// disposition to `SIG_DFL` before the handler is invoked.
///
/// ## Stop signals
///
/// A disposition that resolves to the `Stop` default action suspends the
/// process through the kernel (`SYS_SIGNAL_STOP_SELF`) and returns 0 only
/// once a `SIGCONT` resumes it — so a caller of `raise(SIGSTOP)` blocks
/// here for as long as the process is stopped.
///
/// **Returns** 0 on success, -1 with `errno` set on failure (`EINVAL` for
/// an unknown signal; on host builds, `ENOSYS` for a stop, since there is
/// no SlateOS kernel to suspend the test process).
fn dispatch_self_signal(sig: i32) -> i32 {
    // SIGKILL / SIGSTOP: always apply default, regardless of handler.
    // They cannot be caught, blocked, or ignored.
    if sig == SIGKILL {
        // 128 + 9 = 137
        crate::process::_exit(128i32.wrapping_add(sig));
    }
    if sig == SIGSTOP {
        // SIGSTOP cannot be caught, blocked or ignored, so its action is
        // fixed regardless of the disposition table: stop the process.
        return stop_self(sig);
    }

    let (handler, sa_flags, sa_mask_low) = lookup_action(sig);
    let blocked = current_blocked_low();

    match plan_self_dispatch(sig, blocked, handler, sa_flags, sa_mask_low) {
        SelfDispatch::Pending => {
            set_pending_self(sig);
            0
        }
        SelfDispatch::Ignore => 0,
        SelfDispatch::Handler {
            handler,
            mask_during,
            reset,
        } => {
            // SA_RESETHAND: reset to default before running the one-shot
            // handler, so it is not re-installed for the next occurrence.
            if reset {
                reset_disposition(sig);
            }

            // Auto-mask for the handler's duration, then restore.  This
            // prevents a handler that re-raises its own signal from
            // re-entering (the nested raise finds it blocked → pending).
            let changed = mask_during != blocked;
            if changed {
                apply_blocked_low(mask_during);
            }

            // Invoke the registered handler.  POSIX: the handler receives
            // the signal number.  We cast the stored usize back to a
            // function pointer.
            //
            // SAFETY: the caller registered this via signal()/sigaction()
            // as a valid fn(i32).  We trust they provided a valid pointer.
            let func: extern "C" fn(i32) =
                unsafe { core::mem::transmute::<usize, extern "C" fn(i32)>(handler) };
            func(sig);

            // Restore the mask the handler ran under.  This may unblock a
            // signal raised during the handler, which the kernel then
            // delivers at the next syscall-return boundary.
            if changed {
                apply_blocked_low(blocked);
            }
            0
        }
        SelfDispatch::Default => apply_default_action(sig),
    }
}

// ---------------------------------------------------------------------------
// Asynchronous signal delivery (Phase 211 — kernel signal shim)
// ---------------------------------------------------------------------------
//
// The kernel delivers a pending signal by rewriting the interrupted
// thread's saved frame so it resumes at a registered *trampoline*
// instead of where it was.  The kernel builds a `SignalContext` on the
// user stack capturing the interrupted register state, passes the
// signal number in RDI and a pointer to the context in RSI, then
// transfers control to the trampoline.  The trampoline runs our
// per-signal disposition (via `dispatch_self_signal`) and then issues
// `SYS_SIGNAL_RETURN` to restore the saved context, resuming the
// interrupted code exactly where it left off.
//
// This mirrors the SEH-style hardware-exception delivery already used
// for faults; it is *not* a process-control mechanism (the OS uses IPC
// for that) — it exists purely so ported POSIX programs that install
// signal handlers behave correctly.

/// Saved register context handed to the signal trampoline.
///
/// **ABI-critical**: the field order, size, and `#[repr(C)]` layout
/// must match `kernel/src/proc/signal.rs::SignalContext` exactly.  The
/// kernel writes this struct onto the user stack and passes a pointer
/// to it in RSI; `SYS_SIGNAL_RETURN` reads it back to restore state.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SignalContext {
    /// Signal number being delivered (1..=NSIG).
    pub signum: u64,
    /// Interrupted syscall's return value (restored into RAX).
    pub rax: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub r10: u64,
    pub r8: u64,
    pub r9: u64,
    pub rbx: u64,
    pub rbp: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    /// Interrupted instruction pointer.
    pub rip: u64,
    /// Interrupted stack pointer.
    pub rsp: u64,
    /// Interrupted RFLAGS.
    pub rflags: u64,
}

/// Size of [`SignalContext`] — must equal the kernel's
/// `SIGNAL_CONTEXT_SIZE` (17 × 8 = 136 bytes).
pub const SIGNAL_CONTEXT_SIZE: usize = core::mem::size_of::<SignalContext>();

/// C entry point invoked by the assembly trampoline.
///
/// Runs the registered disposition for `signum` on the current process
/// (`dispatch_self_signal` handles SIG_IGN / handler invocation /
/// default action).  If the disposition terminates the process this
/// never returns; otherwise control returns to the trampoline, which
/// issues `SYS_SIGNAL_RETURN`.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
pub extern "C" fn __signal_dispatch(signum: i32) {
    // Ignore the return value: dispatch_self_signal either terminates
    // the process (no return) or completes the handler/ignore action.
    // Any errno it sets belongs to the interrupted code's context and
    // will be clobbered when SYS_SIGNAL_RETURN restores RAX anyway.
    let _ = dispatch_self_signal(signum);
}

// The trampoline the kernel jumps to when delivering a signal.
//
// On entry (set up by the kernel's `deliver_pending_signal`):
//   RDI = signum, RSI = &SignalContext
//   RSP points at a fake (null) return slot, with RSP % 16 == 8
//   (the kernel placed the 16-aligned context just above it), so the
//   SysV ABI alignment contract for a `call` target is satisfied.
//
// We preserve the context pointer across the dispatch call, then hand
// it to SYS_SIGNAL_RETURN (arg0 = RDI) so the kernel can restore the
// interrupted state.  `push rsi` keeps RSP 16-aligned for the call.
#[cfg(target_os = "none")]
core::arch::global_asm!(
    ".globl __signal_trampoline",
    "__signal_trampoline:",
    "push rsi",               // save &SignalContext (RSP now 16-aligned)
    "call __signal_dispatch",  // dispatch_self_signal(signum); RDI = signum
    "pop rdi",                 // restore &SignalContext into arg0
    "mov rax, {sysret}",       // SYS_SIGNAL_RETURN
    "syscall",                 // kernel restores frame; does not return
    "ud2",                     // trap if the kernel ever returns here
    sysret = const crate::syscall::SYS_SIGNAL_RETURN,
);

#[cfg(target_os = "none")]
unsafe extern "C" {
    /// Assembly signal-return trampoline (see the `global_asm!` above).
    fn __signal_trampoline();
}

/// Register the signal trampoline with the kernel.
///
/// Called once during process startup (from `__libc_start_main`).  After
/// this, the kernel delivers pending catchable signals by redirecting to
/// `__signal_trampoline`.  Until a trampoline is registered the kernel
/// applies signal default actions itself (terminating signals kill the
/// process; others are dropped).
#[cfg(target_os = "none")]
pub fn init_signals() {
    let addr = __signal_trampoline as *const () as usize as u64;
    // A failure here just means async delivery stays in the kernel's
    // default-action mode; nothing else in startup depends on it.
    let _ = crate::syscall::syscall1(crate::syscall::SYS_SIGNAL_REGISTER, addr);
}

/// Host-build no-op: there is no kernel to register with, and issuing a
/// raw syscall on the host would hit the host OS's syscall table.
#[cfg(not(target_os = "none"))]
pub fn init_signals() {}

/// Push the low 64 bits of the blocked mask to the kernel so that
/// asynchronous delivery actually honours `sigprocmask`.
///
/// Our kernel signal shim supports 64 signals, stored in one word; that
/// maps exactly to `SigsetT::bits[0]` (signal N → bit N-1).  Higher
/// realtime signals are not deliverable asynchronously yet, so only the
/// low word is synchronised.  No-op on the host.
#[cfg(target_os = "none")]
fn sync_kernel_blocked_mask(low: u64) {
    let mut old: u64 = 0;
    let _ = crate::syscall::syscall2(
        crate::syscall::SYS_SIGNAL_MASK,
        low,
        core::ptr::addr_of_mut!(old) as u64,
    );
}

#[cfg(not(target_os = "none"))]
fn sync_kernel_blocked_mask(_low: u64) {}

/// Classification of a *validated* `kill(pid, sig)` request with
/// `sig != 0` (so `sig` is already known to be in `[1, NSIG)`).
///
/// Extracted as a pure function so the routing logic can be unit-tested
/// on the host without issuing real syscalls (which would otherwise hit
/// the host OS's syscall table).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KillTarget {
    /// `pid == self_pid` — dispatch the signal locally/synchronously.
    Self_,
    /// `pid <= 0` — a process *group*: `0` is the caller's own group,
    /// `< -1` is the group `-pid`, `-1` is the broadcast form. Delivered
    /// via `SYS_SIGNAL_SEND` with a sign-extended target, which fans the
    /// signal out across the group's live members.
    ProcessGroup,
    /// `pid > 0 && pid != self_pid` — deliver via `SYS_SIGNAL_SEND`.
    Other,
}

/// Route a validated `kill()` to the appropriate delivery mechanism.
///
/// Precondition: `sig` is in `[1, NSIG)` and `sig != 0`.
///
/// Note the ordering: `pid == self_pid` is checked *first*, so a
/// self-directed `kill(getpid(), sig)` still takes the synchronous
/// local-dispatch path. The group forms are all `pid <= 0` and real PIDs
/// are `>= 1`, so the two cases cannot overlap.
fn kill_target(pid: i32, self_pid: i32) -> KillTarget {
    if pid == self_pid {
        KillTarget::Self_
    } else if pid <= 0 {
        KillTarget::ProcessGroup
    } else {
        KillTarget::Other
    }
}

/// Widen a `kill(2)` target PID into the `SYS_SIGNAL_SEND` argument slot.
///
/// The kernel reads `arg0` back as a **signed** 64-bit value so it can tell
/// `kill(pid)` from `kill(-pgid)`, so a negative target has to arrive
/// *sign*-extended: `-5` must be `0xFFFF_FFFF_FFFF_FFFB`, not
/// `0x0000_0000_FFFF_FFFB` — the latter is a huge positive PID, and the send
/// would silently become an `ESRCH` against a process that does not exist.
///
/// Rust's `as` already does this correctly (a cast from a signed type to a
/// wider one sign-extends), so this is **not** repairing a defect in the
/// plain `pid as u64` it replaces. It exists because that correctness is
/// invisible at the call site and is one careless edit away from being lost:
/// an intermediate `as u32`, or a `u32` argument slot, zero-extends and
/// produces exactly the failure above with no diagnostic at all. Naming the
/// requirement and giving it a test
/// (`test_sign_extend_pid_keeps_negative_targets_negative`) turns a language
/// rule a reader has to recall into one the suite enforces.
#[allow(clippy::cast_sign_loss)]
fn sign_extend_pid(pid: i32) -> u64 {
    i64::from(pid) as u64
}

/// Map a `SYS_SIGNAL_SEND` failure to the POSIX errno expected by
/// `kill(2)`.
///
/// `kill` uses `EPERM` (not `EACCES`) for permission failures, so we
/// can't simply funnel through `errno::translate`.  Unknown failures
/// collapse to `ESRCH`, matching the historical conservative behaviour.
fn signal_send_errno(ret: i64) -> i32 {
    use crate::errno;
    // The explicit NO_SUCH_PROCESS arm documents its semantic mapping
    // even though the conservative fallback also yields ESRCH.
    #[allow(clippy::match_same_arms)]
    match ret {
        errno::native::NO_SUCH_PROCESS => errno::ESRCH,
        errno::native::PERMISSION_DENIED => errno::EPERM,
        errno::native::INVALID_ARGUMENT => errno::EINVAL,
        _ => errno::ESRCH,
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
/// * **Cross-process signals** (`pid > 0`, `pid != self`): delivered via
///   `SYS_SIGNAL_SEND(pid, sig)`.  The kernel sets the signal pending on
///   the target and delivers it asynchronously to the target's
///   registered trampoline (which runs the target's own disposition).
///   If the target has no trampoline, the kernel applies the default
///   action itself (terminating signals kill it, others are dropped).
///   The *sender* no longer classifies by default action — that is the
///   kernel's and the target's responsibility, which is the correct
///   POSIX semantics.
/// * **Process groups** (`pid <= 0`): delivered via `SYS_SIGNAL_SEND` with
///   the target sign-extended into the syscall argument.  The kernel
///   resolves the group's live membership from the same process-table
///   state `setpgid()` writes and fans the signal out, applying the
///   per-target authority check to each member individually.  `pid == 0`
///   is the caller's own group, `pid < -1` is the group `-pid`, and
///   `pid == -1` (broadcast to everything signalable) is not modelled and
///   reports `ESRCH`.
///
/// ## Authority — the kernel's, not libc's (§314; was a Phase-203 `CAP_KILL` gate)
///
/// Non-self sends carry **no libc-side capability test**.  Linux's rule is
/// `check_kill_permission()` → `kill_ok_by_cred()`: permitted when the sender's
/// real or effective uid matches the target's, **or** when the sender holds
/// `CAP_KILL`.  libc can evaluate neither half honestly — it cannot read the
/// *target's* credentials, and after §312 its `CAP_KILL` is a deliberately
/// conservative projection that reads false for authority the kernel would
/// grant.  Testing only the capability would therefore not be Linux's rule
/// minus a clause; it would be a strictly narrower rule that refuses the
/// ordinary parent→child send.
///
/// So the check lives where the facts are: `SYS_SIGNAL_SEND` evaluates the real
/// predicate and returns `PERMISSION_DENIED`, which surfaces here as `EPERM`.
/// The group forms are not an exception — they take the same syscall, so they
/// are not a cheaper route to anything.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn kill(pid: i32, sig: i32) -> i32 {
    // sig == 0 is a pure existence/permission check; honour it.
    if sig == 0 {
        if pid <= 0 {
            // Group existence probe: "does this group have any live
            // member?".  The kernel answers it on the same path a real
            // group send takes, so the probe cannot report a group the
            // send would then fail to find.
            let ret =
                crate::syscall::syscall2(crate::syscall::SYS_SIGNAL_SEND, sign_extend_pid(pid), 0);
            if ret < 0 {
                errno::set_errno(signal_send_errno(ret));
                return -1;
            }
            return 0;
        }
        // SYS_PROCESS_IS_READY: returns 1 if ready, 0 if alive but not
        // yet ready, negative error if the PID is unknown.  For an
        // existence check we collapse {0, 1} to success.
        let ret = crate::syscall::syscall1(crate::syscall::SYS_PROCESS_IS_READY, pid as u64);
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
    let self_pid = crate::syscall::syscall0(crate::syscall::SYS_PROCESS_ID) as i32;

    match kill_target(pid, self_pid) {
        KillTarget::Self_ => dispatch_self_signal(sig),
        KillTarget::ProcessGroup => {
            // No pre-emptive CAP_KILL test here (§314).  `SYS_SIGNAL_SEND`
            // is the authority for a group send exactly as it is for a
            // single one, and it answers `PERMISSION_DENIED` when it
            // refuses — which `signal_send_errno` turns into EPERM.  A
            // libc-side gate could only ever be a second, worse copy of
            // that decision: §312's projection is deliberately
            // conservative, so a false `CAP_KILL` does not mean the kernel
            // would refuse, and denying on it would break the group form
            // for every process the kernel would have served.
            //
            // The kernel fans the signal out across the group. Note this
            // may include *us*, if we are a member of the target group:
            // that delivery arrives asynchronously through the registered
            // trampoline rather than through `dispatch_self_signal`, which
            // is correct — POSIX does not special-case the sender out of
            // its own group.
            let ret = crate::syscall::syscall2(
                crate::syscall::SYS_SIGNAL_SEND,
                sign_extend_pid(pid),
                sig as u64,
            );
            if ret < 0 {
                errno::set_errno(signal_send_errno(ret));
                return -1;
            }
            0
        }
        KillTarget::Other => {
            // No pre-emptive CAP_KILL test here (§314).  Linux's rule is
            // "same real/effective uid as the target **or** CAP_KILL", and
            // libc cannot evaluate the first half: it has no way to learn
            // the *target's* credentials.  A capability-only test is
            // therefore not Linux's rule with a piece missing, it is a
            // different and strictly narrower rule — one that would refuse
            // the ordinary parent→child send that `services/ctest-jobctl`
            // depends on and that the kernel authorises on the parent
            // relationship alone.  `SYS_SIGNAL_SEND` evaluates the real
            // predicate and reports `PERMISSION_DENIED` when it refuses.
            //
            // Deliver via the kernel signal shim.  The kernel sets the
            // signal pending and either delivers it to the target's
            // trampoline or applies the default action.  We don't
            // classify by disposition here — that is the target's job.
            let ret =
                crate::syscall::syscall2(crate::syscall::SYS_SIGNAL_SEND, pid as u64, sig as u64);
            if ret < 0 {
                errno::set_errno(signal_send_errno(ret));
                return -1;
            }
            0
        }
    }
}

/// Send a signal to every process in a process group (POSIX `killpg`).
///
/// Defined by POSIX as exactly `kill(-pgrp, sig)`, and implemented that way so
/// there is one code path to keep correct rather than two.  `pgrp == 0` means
/// the caller's own process group.
///
/// This delivers for real: [`kill`] routes `pid <= 0` to the kernel's group
/// fanout over the process-table membership that `setpgid()` writes.  (It was
/// written while that fanout was still `ENOSYS`-only, purely so GNU bash's
/// job-control code would link; it needed no change when the kernel side
/// landed, which is the point of defining it as `kill(-pgrp, sig)`.)
///
/// Errors (Linux-matching):
/// * `EINVAL` — `sig` is not a valid signal number, or `pgrp` is negative
///   (a negative process *group* is nonsense; without this check the negation
///   below would silently turn it into a positive per-process `kill`).
/// * `ESRCH` — no such process group, or it has no live members.
/// * `EPERM` — the kernel refused the send (§314: it, not libc, decides).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn killpg(pgrp: i32, sig: i32) -> i32 {
    if pgrp < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // `0i32.checked_neg()` is trivially Some, and pgrp > 0 negates safely for
    // every i32 except i32::MIN, which pgrp < 0 already rejected.
    let Some(target) = pgrp.checked_neg() else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    kill(target, sig)
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
/// `sigprocmask(SIG_SETMASK, &old, NULL)` round-trips correctly, and
/// mirrors the low 64 bits to the kernel with `SYS_SIGNAL_MASK` so the
/// mask governs real behaviour and not just what this function reports
/// back. The kernel consults it on asynchronous delivery *and* in
/// terminal-access job control, where a blocked `SIGTTIN`/`SIGTTOU` is
/// the difference between a background access being refused and the
/// kernel raising a signal that could never be delivered.
///
/// Only `bits[0]` is synchronised: realtime signals (65+) are not
/// asynchronously deliverable yet, so the kernel has no use for them.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sigprocmask(how: i32, set: *const SigsetT, oldset: *mut SigsetT) -> i32 {
    // SAFETY: `blocked_mask_ptr()` is owned solely by the caller.
    let current = unsafe { blocked_mask_ptr().read() };

    // Return old mask if requested.
    if !oldset.is_null() {
        // SAFETY: oldset verified non-null.
        unsafe {
            *oldset = current;
        }
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
        unsafe {
            blocked_mask_ptr().write(new_mask);
        }

        // Mirror the low 64 signals to the kernel so asynchronous
        // delivery honours the blocked set.  (Realtime signals 65+
        // aren't deliverable asynchronously yet, so only bits[0] is
        // synchronised.)
        sync_kernel_blocked_mask(new_mask.bits[0]);
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
pub extern "C" fn pthread_sigmask(how: i32, set: *const SigsetT, oldset: *mut SigsetT) -> i32 {
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
/// Queries the kernel for the set of signals pending on the calling
/// process (signals raised but not yet delivered, e.g. because they are
/// blocked).  Only the low 64 signals are tracked by the kernel shim;
/// they populate `bits[0]` (signal N → bit N-1).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn sigpending(set: *mut SigsetT) -> i32 {
    if set.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    let mut out = SigsetT::EMPTY;
    out.bits[0] = pending_signals_low();
    // SAFETY: set verified non-null above.
    unsafe {
        *set = out;
    }
    0
}

/// Read the kernel's pending-signal bitmap for the calling process.
///
/// Returns the low 64-bit word (signal N → bit N-1).  No-op on the host
/// (returns 0) since there is no kernel shim to query.
#[cfg(target_os = "none")]
fn pending_signals_low() -> u64 {
    let mut out: u64 = 0;
    let ret = crate::syscall::syscall1(
        crate::syscall::SYS_SIGNAL_PENDING,
        core::ptr::addr_of_mut!(out) as u64,
    );
    if ret < 0 { 0 } else { out }
}

#[cfg(not(target_os = "none"))]
fn pending_signals_low() -> u64 {
    0
}

/// Initialize a signal set to empty.
///
/// Errors: `EINVAL` if `set` is NULL — the sigsetops never enter the
/// kernel, and glibc's `signal/sigempty.c` rejects a NULL `set` itself
/// with `EINVAL`.  See `sigaddset` for the full note.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn sigemptyset(set: *mut SigsetT) -> i32 {
    if set.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    unsafe {
        *set = SigsetT::EMPTY;
    }
    0
}

/// Initialize a signal set to full.
///
/// Errors: `EINVAL` if `set` is NULL (glibc `signal/sigfillset.c`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn sigfillset(set: *mut SigsetT) -> i32 {
    if set.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    unsafe {
        (*set).bits = [u64::MAX; 16];
    }
    0
}

/// Add a signal to a signal set.
///
/// Errors (Linux-matching):
/// * `EINVAL` — `set` is NULL, **or** `signum` is out of the valid
///   range `[1, NSIG)`.
///
/// Both are the same error because glibc makes them the same test.
/// `signal/sigaddset.c` (checked against glibc 2.39) reads:
///
/// ```c
/// if (set == NULL || signo <= 0 || signo >= NSIG
///     || is_internal_signal (signo))
///   { __set_errno (EINVAL); return -1; }
/// ```
///
/// Note what this rules out: `EFAULT` is not available here. The
/// sigsetops are pure bit-twiddling on a caller-owned `sigset_t` and
/// issue no syscall at all, so there is no `copy_to_user` to fault and
/// no kernel to report one — the only errno a Linux program can ever
/// observe from these five functions is `EINVAL`.  (An earlier sweep
/// set `EFAULT` here and justified it in a comment claiming glibc
/// would segfault; glibc does not, and the claim was never checked
/// against the source.)
///
/// We do not implement glibc's `is_internal_signal` rejection of
/// SIGCANCEL/SIGSETXID: those numbers are NPTL's private property on
/// Linux, and we have no equivalent to protect.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn sigaddset(set: *mut SigsetT, signum: i32) -> i32 {
    if set.is_null() {
        errno::set_errno(errno::EINVAL);
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
    unsafe {
        (*set).bits[word] |= 1u64 << bit;
    }
    0
}

/// Remove a signal from a signal set.
///
/// Errors (Linux-matching):
/// * `EINVAL` — `set` is NULL, or `signum` is out of range.  glibc
///   `signal/sigdelset.c` uses the same combined test as `sigaddset`;
///   see the note there for why `EFAULT` cannot arise.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn sigdelset(set: *mut SigsetT, signum: i32) -> i32 {
    if set.is_null() {
        errno::set_errno(errno::EINVAL);
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
    unsafe {
        (*set).bits[word] &= !(1u64 << bit);
    }
    0
}

/// Test whether a signal is in a signal set.
///
/// Errors (Linux-matching):
/// * `EINVAL` — `set` is NULL, or `signum` is out of range.  glibc
///   `signal/sigismem.c` combines the two; see the note on `sigaddset`
///   for why `EFAULT` cannot arise.  (Unlike `sigaddset`/`sigdelset`,
///   glibc does *not* reject internal signals here.)
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn sigismember(set: *const SigsetT, signum: i32) -> i32 {
    if set.is_null() {
        errno::set_errno(errno::EINVAL);
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
    b"Unknown signal 0\0",         // 0
    b"Hangup\0",                   // 1  SIGHUP
    b"Interrupt\0",                // 2  SIGINT
    b"Quit\0",                     // 3  SIGQUIT
    b"Illegal instruction\0",      // 4  SIGILL
    b"Trace/breakpoint trap\0",    // 5  SIGTRAP
    b"Aborted\0",                  // 6  SIGABRT
    b"Bus error\0",                // 7  SIGBUS
    b"Floating point exception\0", // 8  SIGFPE
    b"Killed\0",                   // 9  SIGKILL
    b"User defined signal 1\0",    // 10 SIGUSR1
    b"Segmentation fault\0",       // 11 SIGSEGV
    b"User defined signal 2\0",    // 12 SIGUSR2
    b"Broken pipe\0",              // 13 SIGPIPE
    b"Alarm clock\0",              // 14 SIGALRM
    b"Terminated\0",               // 15 SIGTERM
    b"Stack fault\0",              // 16 SIGSTKFLT (unused on modern Linux x86_64)
    b"Child exited\0",             // 17 SIGCHLD
    b"Continued\0",                // 18 SIGCONT
    b"Stopped (signal)\0",         // 19 SIGSTOP
    b"Stopped\0",                  // 20 SIGTSTP
    b"Stopped (tty input)\0",      // 21 SIGTTIN
    b"Stopped (tty output)\0",     // 22 SIGTTOU
    b"Urgent I/O condition\0",     // 23 SIGURG
    b"CPU time limit exceeded\0",  // 24 SIGXCPU
    b"File size limit exceeded\0", // 25 SIGXFSZ
    b"Virtual timer expired\0",    // 26 SIGVTALRM
    b"Profiling timer expired\0",  // 27 SIGPROF
    b"Window changed\0",           // 28 SIGWINCH
    b"I/O possible\0",             // 29 SIGIO/SIGPOLL
    b"Power failure\0",            // 30 SIGPWR
    b"Bad system call\0",          // 31 SIGSYS
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
        unsafe {
            *sig = 0;
        }
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

/// Wait for a signal from a set, reporting which one arrived.
///
/// Exactly `sigtimedwait(set, info, NULL)` — the "wait forever" form.  That
/// is not a convenience here but the definition: POSIX specifies
/// `sigwaitinfo(set, info)` as equivalent to `sigtimedwait` with a null
/// timeout, and glibc (`sysdeps/unix/sysv/linux/sigwaitinfo.c`) and musl
/// both implement it as that one call.  Expressing it as a forward is what
/// keeps the two from drifting: any validation — or, later, any real
/// delivery — added to [`sigtimedwait`] is inherited here for free, which a
/// hand-copied body would not be.
///
/// Returns the signal number on success, or `-1` with `errno` set.  Note the
/// `-1`/`errno` convention, unlike its near-neighbour [`sigwait`], which
/// returns the errno directly; that inconsistency is POSIX's, not ours.
///
/// While nothing can be delivered to a waiter, this reports `EAGAIN` (from
/// `sigtimedwait`) rather than blocking forever.  A caller that treats
/// `EAGAIN` as "timed out, go round again" therefore spins — which is worse
/// than a real wait and better than a hang, and is the same answer the timed
/// form already gives.
///
/// CPython reaches this from `signal.sigwaitinfo`
/// (`Modules/signalmodule.c:1178`); it was one of the thirteen symbols that
/// stopped CPython 3.12 linking against our libc.  See
/// `scripts/cpython-spike/README.md`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sigwaitinfo(set: *const SigsetT, info: *mut core::ffi::c_void) -> i32 {
    sigtimedwait(set, info, core::ptr::null())
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
/// # Authority (§314; was a Phase-204 `CAP_KILL` gate)
///
/// Linux gates cross-uid signal delivery via `check_kill_permission()` →
/// `kill_ok_by_cred()` — same-uid **or** `CAP_KILL` — exactly as for
/// [`kill`], and libc can evaluate that rule no better here than it can
/// there.  This function is additionally still a **stub**: it delivers
/// nothing.  Refusing with `EPERM` on a conservative capability projection
/// would invent an authority failure for an operation that was never going
/// to happen, and a port reading that `EPERM` would conclude it lacks a
/// privilege rather than that the call is unimplemented.  `ENOSYS` is the
/// answer that is true.
///
/// When real delivery lands here it takes the same shape as `kill`: issue
/// the send and report what the kernel says.
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
/// 3. `ENOSYS` for any otherwise-valid call.
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
    // 3. No CAP_KILL gate (§314): same reasoning as kill(), plus this is
    //    still a stub — an EPERM here would report a privilege failure for
    //    a send that does not happen either way.
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
/// Matches the Linux x86_64 `siginfo_t` layout (128 bytes): a three-`int`
/// preamble, four bytes of alignment padding (`__ARCH_SI_PREAMBLE_SIZE` is
/// `4 * sizeof(int)` on 64-bit), then the union of per-signal payloads
/// starting at offset 16.
///
/// Only the **`SIGCHLD` arm** of that union is named here, because it is the
/// only arm libc itself populates — [`crate::process::waitid`] writes it.
/// Naming the fields rather than leaving the union an opaque byte array is
/// what lets `waitid` fill them by assignment instead of by offset arithmetic
/// through a `*mut u8`, which is the kind of code that silently goes wrong
/// when a layout changes.  C callers are unaffected: in a real `<signal.h>`
/// `si_pid`, `si_uid` and `si_status` are macros onto this same union, at
/// exactly these offsets.
///
/// The remaining arms (`_kill`, `_timer`, `_sigfault`, …) stay inside `_pad`.
/// Add one only when something in this tree actually writes it; an unwritten
/// named field is a promise the code does not keep.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SiginfoT {
    /// Signal number.
    pub si_signo: i32,
    /// Error number (errno value).
    pub si_errno: i32,
    /// Signal code (SI_USER, SI_KERNEL, CLD_*, etc.).
    pub si_code: i32,
    /// Alignment padding — the union begins at offset 16 on 64-bit.
    _preamble_pad: i32,
    /// `SIGCHLD` arm: PID of the child whose state changed.
    pub si_pid: i32,
    /// `SIGCHLD` arm: real UID of that child.
    pub si_uid: u32,
    /// `SIGCHLD` arm: the child's exit code, or the signal that killed,
    /// stopped or continued it — which of the two is selected by `si_code`.
    pub si_status: i32,
    /// Alignment padding — `_utime` is a `clock_t` and so 8-byte aligned.
    _status_pad: u32,
    /// `SIGCHLD` arm: child user CPU time (`clock_t`).
    pub si_utime: i64,
    /// `SIGCHLD` arm: child system CPU time (`clock_t`).
    pub si_stime: i64,
    /// Remainder of the 128-byte union — the arms we do not write.
    _pad: [u8; 80],
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
    let signum = if info.is_null() {
        0
    } else {
        unsafe { (*info).si_signo }
    };
    unsafe {
        psignal(signum, msg);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // Tests build SiginfoT etc. by mutating defaults; clearer than functional-update for single-field tweaks.
mod tests {
    use super::*;

    // -- sigwaitinfo is sigtimedwait(set, info, NULL), verified as such --

    /// `sigwaitinfo` must *inherit* `sigtimedwait`'s validation rather
    /// than re-implement it.  The cheapest way to pin that is to demand
    /// the two agree on every input `sigwaitinfo` can produce — if
    /// someone replaces the forward with a hand-copied body and the two
    /// drift, this fails.
    #[test]
    fn test_sigwaitinfo_is_sigtimedwait_with_null_timeout() {
        let set = SigsetT::EMPTY;
        let mut info = [0u8; 128];

        for (s, i) in [
            (
                core::ptr::null::<SigsetT>(),
                core::ptr::null_mut::<core::ffi::c_void>(),
            ),
            (&raw const set, core::ptr::null_mut()),
            (&raw const set, info.as_mut_ptr().cast()),
        ] {
            crate::errno::set_errno(0);
            let want = sigtimedwait(s, i, core::ptr::null());
            let want_errno = crate::errno::get_errno();

            crate::errno::set_errno(0);
            let got = sigwaitinfo(s, i);
            assert_eq!(got, want);
            assert_eq!(crate::errno::get_errno(), want_errno);
        }
    }

    /// A NULL set is EFAULT — the kernel copies the set in before it
    /// looks at anything else — and a non-NULL set reaches the
    /// nothing-can-ever-be-delivered verdict, EAGAIN.  Stated directly
    /// as well as by equivalence, so a change that broke both functions
    /// identically would still be caught.
    #[test]
    fn test_sigwaitinfo_errno_values() {
        crate::errno::set_errno(0);
        assert_eq!(sigwaitinfo(core::ptr::null(), core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);

        let set = SigsetT::EMPTY;
        crate::errno::set_errno(0);
        assert_eq!(sigwaitinfo(&raw const set, core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EAGAIN);
    }

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
        let mut set = SigsetT {
            bits: [0xFFFF_FFFF_FFFF_FFFF; 16],
        };
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
            sigaddset(&raw mut set, SIGHUP); // bit 0
            sigaddset(&raw mut set, SIGTERM); // bit 14
            sigaddset(&raw mut set, SIGKILL); // bit 8
        }
        assert_ne!(set.bits[0] & (1u64 << 0), 0); // SIGHUP
        assert_ne!(set.bits[0] & (1u64 << 14), 0); // SIGTERM
        assert_ne!(set.bits[0] & (1u64 << 8), 0); // SIGKILL
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
        let mut set = SigsetT {
            bits: [u64::MAX; 16],
        };
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
        let mut set = SigsetT {
            bits: [u64::MAX; 16],
        };
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
        let set = SigsetT {
            bits: [u64::MAX; 16],
        };
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
        let set = SigsetT {
            bits: [u64::MAX; 16],
        };
        let ret = unsafe { sigismember(&raw const set, 0) };
        assert_eq!(ret, -1);
    }

    // -- Round-trip: add then check --

    #[test]
    fn test_sigaddset_then_sigismember() {
        let mut set = SigsetT::EMPTY;
        unsafe {
            sigemptyset(&raw mut set);
        }

        // Add SIGTERM, verify it's there
        unsafe {
            sigaddset(&raw mut set, SIGTERM);
        }
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
        unsafe {
            sigdelset(&raw mut set, SIGINT);
        }
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
        unsafe {
            sigaction(SIGTERM, &raw const old_act, core::ptr::null_mut());
        }
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
        unsafe {
            sigaction(SIGUSR1, &raw const old, core::ptr::null_mut());
        }
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
        unsafe {
            sigaction(SIGUSR2, &raw const act, core::ptr::null_mut());
        }

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
        unsafe {
            sigaction(SIGUSR2, core::ptr::null(), &raw mut check);
        }
        assert_eq!(check.sa_handler, SIG_DFL);
        assert_eq!(check.sa_mask, SigsetT::EMPTY);
        assert_eq!(check.sa_flags, 0);
        assert_eq!(check.sa_restorer, 0);
    }

    // -- sigprocmask --

    /// Reset the blocked signal mask to empty.
    ///
    /// Since the mask became per-thread on host builds (see
    /// `crate::perprocess`), each test already starts with an empty mask, so
    /// calling this first is belt-and-braces rather than load-bearing.  It
    /// still earns its keep within a test that sets a mask and then wants to
    /// assert against a clean slate.
    fn reset_blocked_mask() {
        // SAFETY: `blocked_mask_ptr()` is this thread's own storage.
        unsafe {
            blocked_mask_ptr().write(SigsetT::EMPTY);
        }
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

    // -- kill / raise --
    //
    // Note on coverage: paths that issue native syscalls
    // (SYS_PROCESS_ID, SYS_PROCESS_IS_READY, SYS_SIGNAL_SEND) aren't
    // exercisable in host-target test builds, so cross-process *routing*
    // is tested via the pure `kill_target` classifier and the
    // `signal_send_errno` mapper.  The validation paths that resolve
    // entirely in our code (pid<=0 with sig==0, out-of-range signals)
    // are tested directly through `kill()`.

    /// `kill_target` routes a self-directed pid to local dispatch.
    #[test]
    fn test_kill_target_self() {
        assert_eq!(kill_target(4242, 4242), KillTarget::Self_);
    }

    /// `kill_target` routes a distinct positive pid to cross-process
    /// delivery (regardless of the signal's default disposition — the
    /// sender no longer classifies by action).
    #[test]
    fn test_kill_target_other() {
        assert_eq!(kill_target(1, 4242), KillTarget::Other);
        assert_eq!(kill_target(99, 4242), KillTarget::Other);
    }

    /// `kill_target` routes non-positive pids to the (unsupported)
    /// process-group form.
    #[test]
    fn test_kill_target_process_group() {
        assert_eq!(kill_target(0, 4242), KillTarget::ProcessGroup);
        assert_eq!(kill_target(-1, 4242), KillTarget::ProcessGroup);
        assert_eq!(kill_target(-5, 4242), KillTarget::ProcessGroup);
    }

    /// `signal_send_errno` maps kernel error codes to the errno values
    /// `kill(2)` is expected to surface.
    #[test]
    fn test_signal_send_errno_mapping() {
        use crate::errno;
        assert_eq!(
            signal_send_errno(errno::native::NO_SUCH_PROCESS),
            errno::ESRCH
        );
        assert_eq!(
            signal_send_errno(errno::native::PERMISSION_DENIED),
            errno::EPERM
        );
        assert_eq!(
            signal_send_errno(errno::native::INVALID_ARGUMENT),
            errno::EINVAL
        );
        // Unknown failures collapse to ESRCH (conservative).
        assert_eq!(signal_send_errno(-9999), errno::ESRCH);
    }

    /// [`SignalContext`] must match the kernel ABI: 17 × 8 = 136 bytes.
    #[test]
    fn test_signal_context_abi() {
        assert_eq!(SIGNAL_CONTEXT_SIZE, 136);
        assert_eq!(core::mem::size_of::<SignalContext>(), 17 * 8);
    }

    // -- auto-masking helpers --

    /// `sigmask_bit` maps signal N to bit N-1 within the low word, and
    /// rejects signals outside the representable `[1, 64]` range.
    #[test]
    fn test_sigmask_bit() {
        assert_eq!(sigmask_bit(1), 1u64 << 0);
        assert_eq!(sigmask_bit(2), 1u64 << 1);
        assert_eq!(sigmask_bit(SIGUSR1), 1u64 << (SIGUSR1 - 1));
        assert_eq!(sigmask_bit(64), 1u64 << 63);
        // Out of range → 0 (not representable in the low 64-signal word).
        assert_eq!(sigmask_bit(0), 0);
        assert_eq!(sigmask_bit(65), 0);
        assert_eq!(sigmask_bit(-1), 0);
    }

    /// `handler_block_mask` blocks the delivered signal plus the handler's
    /// `sa_mask` on top of the saved mask.
    #[test]
    fn test_handler_block_mask_default() {
        let saved = sigmask_bit(SIGINT); // SIGINT already blocked.
        let sa_mask = sigmask_bit(SIGTERM); // handler also blocks SIGTERM.
        let m = handler_block_mask(saved, sa_mask, 0, SIGUSR1);
        // Saved + sa_mask + the delivered signal itself.
        assert_eq!(
            m,
            sigmask_bit(SIGINT) | sigmask_bit(SIGTERM) | sigmask_bit(SIGUSR1)
        );
    }

    /// `SA_NODEFER` suppresses auto-blocking of the delivered signal, but
    /// the handler's `sa_mask` is still applied.
    #[test]
    fn test_handler_block_mask_nodefer() {
        let saved = 0;
        let sa_mask = sigmask_bit(SIGTERM);
        let m = handler_block_mask(saved, sa_mask, SA_NODEFER, SIGUSR1);
        // The delivered signal is NOT added; only sa_mask.
        assert_eq!(m, sigmask_bit(SIGTERM));
        assert_eq!(m & sigmask_bit(SIGUSR1), 0);
    }

    // -- self-dispatch policy (pure; no global state, no races) --
    //
    // `plan_self_dispatch` is the decision core of `dispatch_self_signal`.
    // Testing it directly exercises the blocked-pending, ignore,
    // handler-auto-mask, SA_NODEFER, SA_RESETHAND and default-action
    // policy without touching the process-global ACTIONS/BLOCKED_MASK
    // (which would race with other tests under parallel execution).

    /// A blocked signal is left pending, not delivered.
    #[test]
    fn test_plan_blocked_is_pending() {
        let blocked = sigmask_bit(SIGUSR1);
        // Even with a registered handler, a blocked signal stays pending.
        let plan = plan_self_dispatch(SIGUSR1, blocked, 0x1234, 0, 0);
        assert_eq!(plan, SelfDispatch::Pending);
    }

    /// `SIG_IGN` discards the signal.
    #[test]
    fn test_plan_ignore() {
        let plan = plan_self_dispatch(SIGUSR1, 0, SIG_IGN, 0, 0);
        assert_eq!(plan, SelfDispatch::Ignore);
    }

    /// `SIG_DFL` selects the default action.
    #[test]
    fn test_plan_default() {
        let plan = plan_self_dispatch(SIGUSR1, 0, SIG_DFL, 0, 0);
        assert_eq!(plan, SelfDispatch::Default);
    }

    /// A registered handler runs with the delivered signal auto-masked
    /// (plus the handler's `sa_mask`), and `reset` reflects SA_RESETHAND.
    #[test]
    fn test_plan_handler_auto_masks_self() {
        let saved = sigmask_bit(SIGINT);
        let sa_mask = sigmask_bit(SIGTERM);
        let plan = plan_self_dispatch(SIGUSR1, saved, 0xABCD, 0, sa_mask);
        match plan {
            SelfDispatch::Handler {
                handler,
                mask_during,
                reset,
            } => {
                assert_eq!(handler, 0xABCD);
                assert!(!reset);
                // Saved + sa_mask + the delivered signal itself.
                assert_eq!(
                    mask_during,
                    sigmask_bit(SIGINT) | sigmask_bit(SIGTERM) | sigmask_bit(SIGUSR1)
                );
            }
            other => panic!("expected Handler, got {other:?}"),
        }
    }

    /// `SA_NODEFER` keeps the delivered signal unblocked while the handler
    /// runs (so a handler that re-raises its own signal WILL re-enter —
    /// which is exactly what the flag requests).
    #[test]
    fn test_plan_handler_nodefer() {
        let plan = plan_self_dispatch(SIGUSR1, 0, 0xABCD, SA_NODEFER, 0);
        match plan {
            SelfDispatch::Handler { mask_during, .. } => {
                assert_eq!(mask_during & sigmask_bit(SIGUSR1), 0);
            }
            other => panic!("expected Handler, got {other:?}"),
        }
    }

    /// `SA_RESETHAND` is reported so the executor resets the disposition
    /// before running the one-shot handler.
    #[test]
    fn test_plan_handler_resethand() {
        let plan = plan_self_dispatch(SIGUSR1, 0, 0xABCD, SA_RESETHAND, 0);
        match plan {
            SelfDispatch::Handler { reset, .. } => assert!(reset),
            other => panic!("expected Handler, got {other:?}"),
        }
    }

    // -- kill() group forms ------------------------------------------------
    //
    // On the OS target `kill(pid <= 0, sig)` issues SYS_SIGNAL_SEND with a
    // sign-extended target and the kernel fans the signal out across the
    // group.  Host builds have no kernel: every `syscallN` returns the
    // -ENOSYS sentinel, which `signal_send_errno` maps to its conservative
    // ESRCH fallback ("we could not name the target").  So on host these
    // assert the *routing* — that the call reaches the syscall path and
    // reports a target-resolution failure — not delivery, which only the
    // in-kernel self-tests can prove.

    #[test]
    fn test_kill_sig0_pid_zero_is_a_group_probe_not_enosys() {
        // pid == 0 means "every process in the caller's process group".
        // This used to be rejected with ENOSYS before touching a syscall;
        // it is now a real group existence probe, so the one thing that
        // must NOT come back is ENOSYS-because-unimplemented.
        crate::errno::set_errno(0);
        let ret = kill(0, 0);
        assert_eq!(ret, -1, "host has no kernel group to probe");
        assert_eq!(crate::errno::get_errno(), crate::errno::ESRCH);
    }

    #[test]
    fn test_kill_sig0_pid_negative_is_a_group_probe_not_enosys() {
        // Negative pids select process groups; same story.
        crate::errno::set_errno(0);
        let ret = kill(-5, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ESRCH);
    }

    #[test]
    fn test_sign_extend_pid_keeps_negative_targets_negative() {
        // The whole group mechanism hinges on the kernel reading arg0 back
        // as a signed value.  Zero-extending a negative pid would turn
        // kill(-7, sig) into a send to PID 4294967289 — a silent ESRCH on a
        // process that does not exist, rather than a group signal.
        //
        // Rust's `as` sign-extends from a signed type today, so these
        // assertions pass for `pid as u64` too.  That is the point: the
        // property is currently free, invisible, and one careless `as u32`
        // away from being lost with no other symptom, so it is asserted
        // here rather than left to a reader's recall of the cast rules.
        assert_eq!(sign_extend_pid(-7), 0xFFFF_FFFF_FFFF_FFF9);
        assert_eq!(sign_extend_pid(-1), u64::MAX);
        assert_eq!(sign_extend_pid(i32::MIN), 0xFFFF_FFFF_8000_0000);
        // Positive targets must be untouched — the same slot carries plain
        // per-process kills.
        assert_eq!(sign_extend_pid(0), 0);
        assert_eq!(sign_extend_pid(1), 1);
        assert_eq!(sign_extend_pid(i32::MAX), 0x7FFF_FFFF);
    }

    // -- killpg ------------------------------------------------------------
    //
    // killpg is defined by POSIX as exactly `kill(-pgrp, sig)`, so these
    // assert the delegation and the one check killpg makes on its own
    // behalf.  The ESRCH results are inherited from `kill`'s group path
    // under the host sentinel described above.

    #[test]
    fn test_killpg_negative_group_is_einval() {
        // A negative process *group* is nonsense.  killpg must reject it
        // itself: without this check the negation would turn -5 into +5 and
        // silently signal process 5 instead of failing.
        crate::errno::set_errno(0);
        let ret = killpg(-5, 15);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_killpg_i32_min_is_einval_not_a_negation_overflow() {
        // i32::MIN has no positive counterpart; it is caught by the pgrp < 0
        // gate before checked_neg can be reached, so this can never panic in
        // debug or wrap in release.
        crate::errno::set_errno(0);
        let ret = killpg(i32::MIN, 15);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_killpg_zero_means_own_group_and_delegates_to_kill() {
        // pgrp == 0 negates to 0, which `kill` treats as "the caller's own
        // process group" — the group path, not an unimplemented-feature
        // rejection.
        crate::errno::set_errno(0);
        let ret = killpg(0, 15);
        assert_eq!(ret, -1, "host has no kernel group to deliver to");
        assert_eq!(crate::errno::get_errno(), crate::errno::ESRCH);
    }

    #[test]
    fn test_killpg_positive_group_delegates_to_kill() {
        // The normal case: killpg(7, sig) becomes kill(-7, sig), which lands
        // in kill's ProcessGroup arm.
        crate::errno::set_errno(0);
        let ret = killpg(7, 15);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ESRCH);
    }

    #[test]
    fn test_killpg_invalid_signal_is_rejected_as_einval_not_esrch() {
        // Signal validation is `kill`'s job and killpg must not bypass it.
        // `kill` checks the signal number BEFORE classifying the target, so a
        // bad signal reports EINVAL even when the group would also have
        // failed to resolve. That ordering is what Linux does and it is the
        // more useful diagnostic — "your signal is wrong" rather than "that
        // group is gone" — so pin it here: if the two checks were ever
        // reordered this test would catch it.
        crate::errno::set_errno(0);
        let ret = killpg(7, 9999);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_kill_sig0_pid_minus_one_is_esrch_broadcast_not_modelled() {
        // pid == -1 means "every process you may signal".  The kernel does
        // not model broadcast (it would have to walk the whole process
        // table and decide what "may signal" means without a full
        // credential model), so it reports ESRCH — the same answer the
        // host sentinel produces here.  Documented in todo.txt.
        crate::errno::set_errno(0);
        let ret = kill(-1, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ESRCH);
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

    /// Every catchable signal — regardless of its default disposition
    /// (ignore, stop, continue, terminate) — routes cross-process to the
    /// same `Other` delivery path.  The kernel and the target process
    /// decide the disposition, not the sender.
    #[test]
    fn test_kill_cross_process_routes_all_signals_to_other() {
        for sig in [
            SIGCHLD, SIGURG, SIGWINCH, // default: ignore
            SIGSTOP, SIGTSTP, SIGTTIN, SIGTTOU, // default: stop
            SIGCONT, // default: continue
            SIGTERM, SIGINT, SIGKILL, // default: terminate
        ] {
            // sig is irrelevant to routing; pid != self_pid → Other.
            let _ = sig;
            assert_eq!(kill_target(1, 4242), KillTarget::Other);
        }
    }

    // =================================================================
    // §314 — kill() has NO libc-side CAP_KILL gate
    //
    // These were Phase-203 tests asserting that dropping CAP_KILL made a
    // cross-process kill() return EPERM.  §314 removed that gate: Linux's
    // rule is same-uid **or** CAP_KILL, libc cannot read the target's uid,
    // and after §312 its CAP_KILL is a conservative projection that reads
    // false for authority the kernel would grant.  A capability-only test
    // is therefore narrower than Linux's rule, not a subset of it, and
    // would refuse the ordinary parent→child send.
    //
    // So the tests are inverted rather than deleted: they now pin that
    // dropping CAP_KILL does *not* by itself produce EPERM.  That is the
    // property §312 step 3 will try to break, and it is worth a test that
    // fails loudly if someone reinstates the gate on the way through.
    // =================================================================

    mod phase203_cap {
        pub(super) struct CapGuard {
            lo: u32,
            hi: u32,
        }
        impl CapGuard {
            pub(super) fn snapshot() -> Self {
                let (lo, hi) = crate::sys_capability::current_caps_effective();
                Self { lo, hi }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
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
                let _ = crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        pub(super) fn drop_cap_kill() {
            let cap = crate::sys_capability::CAP_KILL;
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let new_lo = lo & !(1u32 << cap);
            let mut hdr = crate::sys_capability::CapUserHeader {
                version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
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
            let rc = crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0, "capset must succeed dropping cap");
            assert!(!crate::sys_capability::has_capability(cap));
        }
    }

    // -- routing is unaffected by the capability --------------------------
    //
    // Under the Phase-211 model the sender does not classify by disposition:
    // every cross-process `kill(pid>0, sig)` proceeds to `SYS_SIGNAL_SEND`
    // and the kernel/target decide.  The *outcome* of that syscall is not
    // host-testable (the host has no kernel shim), so these tests assert the
    // deterministic in-process invariants only.  The send-failure → errno
    // mapping is covered by `test_signal_send_errno_mapping` — including the
    // `PERMISSION_DENIED → EPERM` arm, which is now the *only* way `kill`
    // produces EPERM.

    /// Routing does not consult the capability: `kill_target` is a pure
    /// function of the two pids, with the cap held.
    #[test]
    fn test_kill_routing_ignores_capability_when_held() {
        assert!(crate::sys_capability::has_capability(
            crate::sys_capability::CAP_KILL,
        ));
        assert_eq!(kill_target(1, 4242), KillTarget::Other);
    }

    // -- §314: dropping CAP_KILL does not by itself deny ------------------
    //
    // The inverse of the old Phase-203 assertions.  What is being pinned is
    // that libc has stopped pre-empting the kernel's decision — so a
    // cross-process send with the capability dropped must reach
    // `SYS_SIGNAL_SEND` and report *its* answer.  On the host that syscall
    // is unimplemented and `signal_send_errno` maps the failure to ESRCH;
    // the load-bearing part is `!= EPERM`, since EPERM is what a
    // reinstated libc-side gate would produce.

    /// Without CAP_KILL, `kill(1, SIGHUP)` must not be denied by libc.
    #[test]
    fn test_kill_no_cap_is_not_libc_denied() {
        let _g = phase203_cap::CapGuard::snapshot();
        phase203_cap::drop_cap_kill();
        crate::errno::set_errno(0);
        let _ = kill(1, SIGHUP);
        assert_ne!(
            crate::errno::get_errno(),
            crate::errno::EPERM,
            "§314: libc must not deny a send the kernel has not been asked about"
        );
    }

    /// Same for a terminating signal to an arbitrary pid — the signal
    /// number is not what the removed gate keyed on, so it gets its own case.
    #[test]
    fn test_kill_sigterm_no_cap_is_not_libc_denied() {
        let _g = phase203_cap::CapGuard::snapshot();
        phase203_cap::drop_cap_kill();
        crate::errno::set_errno(0);
        let _ = kill(42, SIGTERM);
        assert_ne!(crate::errno::get_errno(), crate::errno::EPERM);
    }

    /// The parent→child case `services/ctest-jobctl` depends on: a plain
    /// `SIGCONT` to another pid, with no capability held.  This is the
    /// regression §314 exists to prevent — the kernel authorises it on the
    /// parent relationship, which libc cannot see and must not overrule.
    #[test]
    fn test_kill_sigcont_to_child_without_cap_is_not_libc_denied() {
        let _g = phase203_cap::CapGuard::snapshot();
        phase203_cap::drop_cap_kill();
        crate::errno::set_errno(0);
        let _ = kill(7, SIGCONT);
        assert_ne!(
            crate::errno::get_errno(),
            crate::errno::EPERM,
            "parent->child SIGCONT must not be refused by libc: the kernel \
             authorises it on the parent relationship, which libc cannot see"
        );
    }

    // -- sig==0 existence check is likewise not gated ---------------------

    /// sig==0 is a pure existence check — it never signals.  It took the
    /// early return before §314 and still does; the assertion is unchanged.
    #[test]
    fn test_kill_sig0_is_not_libc_denied() {
        let _g = phase203_cap::CapGuard::snapshot();
        phase203_cap::drop_cap_kill();
        crate::errno::set_errno(0);
        // For pid > 0, sig==0 issues SYS_PROCESS_IS_READY; whether pid 1
        // exists is not ours to assert.  Either ESRCH or 0 is acceptable.
        let _ = kill(1, 0);
        assert_ne!(
            crate::errno::get_errno(),
            crate::errno::EPERM,
            "an existence probe must never report a permission failure"
        );
    }

    // -- the group forms are not gated either -----------------------------
    //
    // These have now been through three states, which is worth recording so
    // the next reader does not "restore" an earlier one.  Originally they
    // asserted the group forms *bypassed* CAP_KILL — true only because a
    // group send could not reach anything, so the gate was dead weight.
    // Then, once the kernel fanout landed, they asserted the gate *applied*,
    // so `killpg` could not do what `kill` was denied.  §314 removes the
    // gate from both forms together, which preserves that symmetry: neither
    // is a route around the other, because neither is gated in libc at all.
    // `SYS_SIGNAL_SEND` fans out and decides, exactly as for a single send.

    /// pid == 0 (the caller's own process group) with no cap: not libc-denied.
    #[test]
    fn test_kill_pid0_no_cap_is_not_libc_denied() {
        let _g = phase203_cap::CapGuard::snapshot();
        phase203_cap::drop_cap_kill();
        crate::errno::set_errno(0);
        let _ = kill(0, SIGHUP);
        assert_ne!(
            crate::errno::get_errno(),
            crate::errno::EPERM,
            "the group form takes the same syscall as a single send, and the \
             same authority: the kernel's"
        );
    }

    /// pid == -1 (broadcast) with no cap: not libc-denied.
    #[test]
    fn test_kill_pidneg1_no_cap_is_not_libc_denied() {
        let _g = phase203_cap::CapGuard::snapshot();
        phase203_cap::drop_cap_kill();
        crate::errno::set_errno(0);
        let _ = kill(-1, SIGTERM);
        assert_ne!(crate::errno::get_errno(), crate::errno::EPERM);
    }

    // -- ordering: EINVAL still precedes the syscall ----------------------

    /// Invalid signal + no cap → EINVAL (sig check before cap).
    /// A bad signal number is still rejected before the syscall is issued.
    ///
    /// This one survives §314 unchanged and is the reason argument
    /// validation was never part of the gate discussion: `EINVAL` is libc
    /// answering a question about *its own arguments*, which it can
    /// evaluate completely, as opposed to a question about authority, which
    /// it cannot.  That is the whole line §314 draws.
    #[test]
    fn test_kill_invalid_sig_einval_precedes_the_syscall() {
        let _g = phase203_cap::CapGuard::snapshot();
        phase203_cap::drop_cap_kill();
        crate::errno::set_errno(0);
        let ret = kill(1, 0x7FFF);
        assert_eq!(ret, -1);
        assert_eq!(
            crate::errno::get_errno(),
            crate::errno::EINVAL,
            "argument validation is libc's to do and must precede the send"
        );
    }

    // -- restoration: cap drop/restore cycle ------------------------------

    /// A drop/restore cycle of `CAP_KILL` changes nothing about `kill`.
    ///
    /// Before §314 this test asserted the two halves of the gate. It now
    /// asserts the gate's absence on both sides of the cycle, which is a
    /// weaker claim about `kill` but a sharper one about `capset`: the
    /// capability machinery still works (the restore is checked), it simply
    /// no longer feeds this decision.
    ///
    /// The "succeeds" branch issues `SYS_SIGNAL_SEND` against PID 1, which
    /// only exists on the OS target — there it reaches the kernel signal
    /// shim, which dispatches to init's handler or its default (SIGCHLD's
    /// default is ignore, so the syscall returns 0).
    #[test]
    fn test_kill_cap_drop_restore_cycle_does_not_change_kill() {
        {
            let _g = phase203_cap::CapGuard::snapshot();
            phase203_cap::drop_cap_kill();
            crate::errno::set_errno(0);
            let _ = kill(1, SIGCHLD);
            assert_ne!(
                crate::errno::get_errno(),
                crate::errno::EPERM,
                "§314: dropping the capability must not make libc deny"
            );
        }
        assert!(
            crate::sys_capability::has_capability(crate::sys_capability::CAP_KILL),
            "the guard must have restored the capability"
        );
        #[cfg(target_os = "none")]
        {
            crate::errno::set_errno(0);
            // With the cap restored, an ignore-by-default signal succeeds.
            let ret = kill(1, SIGCHLD);
            assert_eq!(ret, 0);
        }
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

    /// `raise()` with a stop signal asks the kernel to suspend us.
    ///
    /// On the target that parks the process until a `SIGCONT`, so it cannot
    /// be exercised here — a host test process has no SlateOS kernel and
    /// every raw syscall returns the `-ENOSYS` sentinel, which surfaces as
    /// `ENOSYS`.  What this pins is that the call now *reaches* the syscall
    /// rather than being short-circuited: before, `apply_default_action`
    /// set `ENOSYS` itself without ever asking the kernel, so the same
    /// observable value meant something entirely different.  The
    /// distinction is covered by `every_stop_signal_takes_the_kernel_path`
    /// and `sigcont_default_action_succeeds_without_a_syscall`, which
    /// together show the `Stop` and `Continue` arms now behave differently
    /// — they shared one `ENOSYS` branch before.
    #[test]
    fn test_raise_sigtstp_enosys_on_host() {
        signal(SIGTSTP, SIG_DFL);
        errno::set_errno(0);
        assert_eq!(raise(SIGTSTP), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    /// The four stop signals all route through `stop_self`, and none of
    /// them is short-circuited before the syscall.
    ///
    /// `errno::translate` maps the host sentinel to `ENOSYS`, so a `-1` /
    /// `ENOSYS` here is evidence the syscall was issued and its return
    /// value propagated — the same value the old hard-coded branch
    /// produced, which is exactly why this asserts the *set* of signals
    /// that take the path rather than just the value.
    #[test]
    fn every_stop_signal_takes_the_kernel_path() {
        for sig in [SIGSTOP, SIGTSTP, SIGTTIN, SIGTTOU] {
            signal(sig, SIG_DFL);
            errno::set_errno(0);
            assert_eq!(raise(sig), -1, "raise({sig}) should fail on host");
            assert_eq!(
                errno::get_errno(),
                errno::ENOSYS,
                "raise({sig}) should report the host syscall sentinel"
            );
        }
    }

    /// `SIGCONT`'s default action is satisfied without a syscall.
    ///
    /// A process that reaches the `Continue` default action is by
    /// definition already running, so it must report success — not the
    /// host `ENOSYS` that a syscall would produce.  This is what
    /// distinguishes the `Continue` arm from the `Stop` arm; they shared a
    /// single `ENOSYS` branch before.
    #[test]
    fn sigcont_default_action_succeeds_without_a_syscall() {
        signal(SIGCONT, SIG_DFL);
        errno::set_errno(0);
        assert_eq!(raise(SIGCONT), 0);
        assert_eq!(errno::get_errno(), 0, "no syscall should have been made");
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
        unsafe {
            sigaddset(&raw mut block, 10);
        }
        let ret = pthread_sigmask(SIG_BLOCK, &raw const block, core::ptr::null_mut());
        assert_eq!(ret, 0);

        // Check it's blocked.
        let mut current = SigsetT::EMPTY;
        pthread_sigmask(SIG_SETMASK, core::ptr::null(), &raw mut current);
        assert_ne!(
            unsafe { sigismember(&raw const current, 10) },
            0,
            "Signal 10 should be blocked"
        );

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
        unsafe {
            sigaddset(&raw mut block, 5);
        }
        pthread_sigmask(SIG_BLOCK, &raw const block, core::ptr::null_mut());

        // Unblock signal 5.
        let ret = pthread_sigmask(SIG_UNBLOCK, &raw const block, core::ptr::null_mut());
        assert_eq!(ret, 0);

        // Verify unblocked.
        let mut current = SigsetT::EMPTY;
        pthread_sigmask(SIG_SETMASK, core::ptr::null(), &raw mut current);
        assert_eq!(
            unsafe { sigismember(&raw const current, 5) },
            0,
            "Signal 5 should be unblocked"
        );

        // Restore.
        pthread_sigmask(SIG_SETMASK, &raw const old, core::ptr::null_mut());
    }

    // -----------------------------------------------------------------------
    // psignal
    // -----------------------------------------------------------------------

    #[test]
    fn test_psignal_null_prefix() {
        // psignal with null prefix should print just the signal description.
        unsafe {
            psignal(crate::signal::SIGTERM, core::ptr::null());
        }
    }

    #[test]
    fn test_psignal_empty_prefix() {
        // Empty prefix → no "prefix: " part, just signal description.
        unsafe {
            psignal(crate::signal::SIGINT, b"\0".as_ptr());
        }
    }

    #[test]
    fn test_psignal_with_prefix() {
        unsafe {
            psignal(crate::signal::SIGKILL, b"test\0".as_ptr());
        }
    }

    #[test]
    fn test_psignal_invalid_signal() {
        // Invalid signal number should still not crash.
        unsafe {
            psignal(9999, b"bad sig\0".as_ptr());
        }
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
        assert_eq!(core::mem::align_of::<SiginfoT>(), 8);
    }

    #[test]
    fn test_siginfo_t_field_offsets_match_linux() {
        // These offsets are the ABI: a C caller passes a `siginfo_t *` that
        // its own headers laid out, and we write through it.  If a field
        // moves, `waitid` starts corrupting the caller's struct silently, so
        // pin every one.  Values are Linux x86_64 `<asm-generic/siginfo.h>`:
        // a 3-int preamble, 4 bytes of padding (`__ARCH_SI_PREAMBLE_SIZE` is
        // `4 * sizeof(int)` on 64-bit), then the `_sigchld` union arm, whose
        // `clock_t` members force another 4 bytes after `_status`.
        let si = SiginfoT::default();
        let base = core::ptr::addr_of!(si) as usize;
        let off = |p: usize| p - base;

        assert_eq!(off(core::ptr::addr_of!(si.si_signo) as usize), 0);
        assert_eq!(off(core::ptr::addr_of!(si.si_errno) as usize), 4);
        assert_eq!(off(core::ptr::addr_of!(si.si_code) as usize), 8);
        assert_eq!(off(core::ptr::addr_of!(si.si_pid) as usize), 16);
        assert_eq!(off(core::ptr::addr_of!(si.si_uid) as usize), 20);
        assert_eq!(off(core::ptr::addr_of!(si.si_status) as usize), 24);
        assert_eq!(off(core::ptr::addr_of!(si.si_utime) as usize), 32);
        assert_eq!(off(core::ptr::addr_of!(si.si_stime) as usize), 40);
    }

    #[test]
    fn test_siginfo_t_default_zeroed() {
        let si = SiginfoT::default();
        assert_eq!(si.si_signo, 0);
        assert_eq!(si.si_errno, 0);
        assert_eq!(si.si_code, 0);
        assert_eq!(si.si_pid, 0);
        assert_eq!(si.si_uid, 0);
        assert_eq!(si.si_status, 0);
        assert_eq!(si.si_utime, 0);
        assert_eq!(si.si_stime, 0);
    }

    // ------------------------------------------------------------------
    // psiginfo
    // ------------------------------------------------------------------

    #[test]
    fn test_psiginfo_null_info() {
        // psiginfo with null info → prints "Unknown signal 0".
        // Just verify no crash.
        unsafe {
            psiginfo(core::ptr::null(), b"test\0".as_ptr());
        }
    }

    #[test]
    fn test_psiginfo_with_signum() {
        // psiginfo with a valid signum → prints the signal name.
        let mut si = SiginfoT::default();
        si.si_signo = SIGTERM;
        unsafe {
            psiginfo(&si, b"test\0".as_ptr());
        }
    }

    #[test]
    fn test_psiginfo_null_msg() {
        let mut si = SiginfoT::default();
        si.si_signo = SIGINT;
        unsafe {
            psiginfo(&si, core::ptr::null());
        }
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
    // §314 — sigqueue() has NO libc-side CAP_KILL gate either
    //
    // These were the Phase-204 mirror of the Phase-203 kill() tests.  They
    // now pin the stronger property that applies to an unimplemented call:
    // its answer is ENOSYS *regardless* of the capability words, because an
    // EPERM here would report a privilege failure for a send that does not
    // happen under any capability.  A port that saw EPERM would conclude it
    // needs a privilege; the truth is that the function needs writing.
    // Reuses the kill() cap helpers.
    // =================================================================

    /// With `CAP_KILL` held, `sigqueue(1, SIGHUP, 0)` reaches ENOSYS.
    #[test]
    fn test_sigqueue_with_cap_enosys() {
        assert!(crate::sys_capability::has_capability(
            crate::sys_capability::CAP_KILL,
        ));
        crate::errno::set_errno(0);
        let ret = sigqueue(1, SIGHUP, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// …and without it, the *same* ENOSYS. This is the pair that matters:
    /// the two cases must be indistinguishable, since the capability is not
    /// an input to the answer.
    #[test]
    fn test_sigqueue_without_cap_is_still_enosys_not_eperm() {
        let _g = phase203_cap::CapGuard::snapshot();
        phase203_cap::drop_cap_kill();
        crate::errno::set_errno(0);
        let ret = sigqueue(1, SIGHUP, 0);
        assert_eq!(ret, -1);
        assert_eq!(
            crate::errno::get_errno(),
            crate::errno::ENOSYS,
            "§314: an unimplemented call reports that it is unimplemented, \
             not that the caller lacks a privilege"
        );
    }

    /// The `sig == 0` existence-probe form, likewise.  `sigqueue` has no
    /// sig-0 fast path of its own, so this exercises the main body.
    #[test]
    fn test_sigqueue_sig0_without_cap_is_enosys() {
        let _g = phase203_cap::CapGuard::snapshot();
        phase203_cap::drop_cap_kill();
        crate::errno::set_errno(0);
        let ret = sigqueue(1, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // -- ordering: argument validation still precedes everything ----------

    /// `pid <= 0` is rejected before the body — argument validation is a
    /// question libc can answer completely, unlike an authority question.
    #[test]
    fn test_sigqueue_bad_pid_einval_precedes_body() {
        let _g = phase203_cap::CapGuard::snapshot();
        phase203_cap::drop_cap_kill();
        crate::errno::set_errno(0);
        let ret = sigqueue(0, SIGHUP, 0);
        assert_eq!(ret, -1);
        assert_eq!(
            crate::errno::get_errno(),
            crate::errno::EINVAL,
            "EINVAL for bad pid must precede the ENOSYS body"
        );
    }

    /// Same for an out-of-range signal number.
    #[test]
    fn test_sigqueue_bad_sig_einval_precedes_body() {
        let _g = phase203_cap::CapGuard::snapshot();
        phase203_cap::drop_cap_kill();
        crate::errno::set_errno(0);
        let ret = sigqueue(1, NSIG, 0);
        assert_eq!(ret, -1);
        assert_eq!(
            crate::errno::get_errno(),
            crate::errno::EINVAL,
            "EINVAL for bad sig must precede the ENOSYS body"
        );
    }

    // -- restoration: cap restore cycle -----------------------------------

    /// A `CAP_KILL` drop/restore cycle leaves `sigqueue`'s answer identical
    /// on both sides, which is the same claim as the pair above stated as a
    /// round trip — it additionally proves the guard really restores.
    #[test]
    fn test_sigqueue_cap_drop_restore_cycle_answers_identically() {
        {
            let _g = phase203_cap::CapGuard::snapshot();
            phase203_cap::drop_cap_kill();
            crate::errno::set_errno(0);
            let ret = sigqueue(1, SIGHUP, 0);
            assert_eq!(ret, -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
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
        let ret = sigtimedwait(core::ptr::null(), core::ptr::null_mut(), core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_sigtimedwait_negative_tv_sec_einval() {
        let set = SigsetT { bits: [0; 16] };
        let ts = crate::stat::Timespec {
            tv_sec: -1,
            tv_nsec: 0,
        };
        crate::errno::set_errno(0);
        let ret = sigtimedwait(&raw const set, core::ptr::null_mut(), &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_sigtimedwait_nsec_at_billion_einval() {
        let set = SigsetT { bits: [0; 16] };
        let ts = crate::stat::Timespec {
            tv_sec: 1,
            tv_nsec: 1_000_000_000,
        };
        crate::errno::set_errno(0);
        let ret = sigtimedwait(&raw const set, core::ptr::null_mut(), &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_sigtimedwait_nsec_way_too_big_einval() {
        let set = SigsetT { bits: [0; 16] };
        let ts = crate::stat::Timespec {
            tv_sec: 1,
            tv_nsec: i64::MAX,
        };
        crate::errno::set_errno(0);
        let ret = sigtimedwait(&raw const set, core::ptr::null_mut(), &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_sigtimedwait_negative_nsec_einval() {
        let set = SigsetT { bits: [0; 16] };
        let ts = crate::stat::Timespec {
            tv_sec: 1,
            tv_nsec: -1,
        };
        crate::errno::set_errno(0);
        let ret = sigtimedwait(&raw const set, core::ptr::null_mut(), &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_sigtimedwait_set_check_before_timeout() {
        // NULL set + bad timeout → EFAULT (set is checked first).
        let ts = crate::stat::Timespec {
            tv_sec: -1,
            tv_nsec: -1,
        };
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
        let ts = crate::stat::Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
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
        let set = SigsetT {
            bits: [0xFFFF_FFFF_FFFF_FFFF; 16],
        };
        let ts = crate::stat::Timespec {
            tv_sec: 0,
            tv_nsec: 100_000,
        };
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
        for sig in [
            SIGHUP, SIGINT, SIGTERM, SIGUSR1, SIGUSR2, SIGCHLD, SIGALRM, SIGPIPE, SIGSEGV, SIGWINCH,
        ] {
            crate::errno::set_errno(0);
            assert_eq!(
                siginterrupt(sig, 0),
                0,
                "siginterrupt({sig}, 0) should succeed"
            );
            assert_eq!(
                siginterrupt(sig, 1),
                0,
                "siginterrupt({sig}, 1) should succeed"
            );
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
        let mask = SigsetT {
            bits: [u64::MAX; 16],
        };
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
    // Self-directed signals (kill(self,..) and raise()) are dispatched
    // synchronously via dispatch_self_signal():
    //   - SIG_IGN → return 0
    //   - handler → invoke fn(sig), return 0
    //   - SIG_DFL → default action (terminate/_exit, ignore, or ENOSYS
    //     for stop/continue which we can't apply to ourselves)
    //
    // Cross-process signals (kill(pid>0,..), pid != self) are delivered
    // via SYS_SIGNAL_SEND; the kernel sets the signal pending on the
    // target and either delivers it to the target's trampoline or
    // applies the default action.  The sender does not classify by
    // disposition — see `kill_target`.
    // =================================================================

    /// default_action classifies all standard signals correctly.
    #[test]
    fn test_phase211_default_action_classify() {
        // Terminate signals.
        for sig in [
            SIGHUP, SIGINT, SIGPIPE, SIGALRM, SIGTERM, SIGUSR1, SIGUSR2, SIGVTALRM, SIGPROF, SIGIO,
            SIGPWR,
        ] {
            assert_eq!(
                default_action(sig),
                Some(DefaultAction::Terminate),
                "signal {sig} should be Terminate"
            );
        }
        // Core-dump signals.
        for sig in [
            SIGQUIT, SIGILL, SIGTRAP, SIGABRT, SIGBUS, SIGFPE, SIGSEGV, SIGXCPU, SIGXFSZ, SIGSYS,
        ] {
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
        let old = signal(SIGUSR1, handler as *const () as SighandlerT);
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
        let old = signal(SIGUSR2, my_handler as *const () as SighandlerT);
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

    /// `raise()` with SIG_DFL for a stop signal issues the stop syscall,
    /// which on host has no kernel behind it and reports ENOSYS.
    ///
    /// On the target this suspends the process until a `SIGCONT`.
    #[test]
    fn test_phase211_raise_default_stop_enosys_on_host() {
        for sig in [SIGTSTP, SIGTTIN, SIGTTOU] {
            signal(sig, SIG_DFL);
            crate::errno::set_errno(0);
            assert_eq!(raise(sig), -1, "raise({sig}) stop should fail on host");
            assert_eq!(
                crate::errno::get_errno(),
                crate::errno::ENOSYS,
                "raise({sig}) stop should set ENOSYS"
            );
        }
    }

    /// `raise()` with SIG_DFL for `SIGCONT` succeeds.
    ///
    /// It used to share the stop signals' `ENOSYS` branch, which was wrong
    /// on both arms: a process that reaches the `Continue` default action
    /// is already running, so the action is satisfied and there is nothing
    /// to fail at.  Unlike the stop cases this is *not* host-specific — it
    /// returns 0 on the target too, because no syscall is involved.
    #[test]
    fn test_phase211_raise_default_continue_succeeds() {
        signal(SIGCONT, SIG_DFL);
        crate::errno::set_errno(0);
        assert_eq!(raise(SIGCONT), 0);
        assert_eq!(crate::errno::get_errno(), 0);
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
        signal(SIGUSR1, h as *const () as SighandlerT);
        signal(SIGUSR1, SIG_IGN);
        crate::errno::set_errno(0);
        assert_eq!(raise(SIGUSR1), 0);
        assert!(
            !CALLED.load(Ordering::Relaxed),
            "handler should NOT be called after SIG_IGN"
        );
        signal(SIGUSR1, SIG_DFL);
    }

    /// kill() with pid <= 0 and a valid signal takes the kernel group path.
    ///
    /// On host there is no kernel, so the syscall sentinel surfaces as
    /// ESRCH; the assertion that matters is that it is no longer ENOSYS —
    /// i.e. the call is routed to a real mechanism rather than refused.
    #[test]
    fn test_phase211_kill_pgroup_routes_to_kernel() {
        crate::errno::set_errno(0);
        let ret = kill(0, SIGTERM);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ESRCH);

        crate::errno::set_errno(0);
        let ret = kill(-1, SIGINT);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ESRCH);
    }

    /// Cross-process `kill()` reaches `SYS_SIGNAL_SEND` whatever the
    /// capability words say (§314), so a dropped `CAP_KILL` produces the
    /// same routed answer as a held one — here ESRCH from the host's
    /// unimplemented syscall, via `signal_send_errno`'s conservative arm.
    ///
    /// The signal chosen is `SIGCHLD` (default: ignore) because the
    /// Phase-211 model stopped classifying by disposition at the sender;
    /// this pins that the disposition is not smuggled back in as a reason
    /// to answer before the syscall.
    #[test]
    fn test_phase211_kill_cross_ignore_no_cap_still_routes() {
        let _g = phase203_cap::CapGuard::snapshot();
        phase203_cap::drop_cap_kill();
        crate::errno::set_errno(0);
        let ret = kill(1, SIGCHLD);
        assert_eq!(ret, -1);
        assert_eq!(
            crate::errno::get_errno(),
            crate::errno::ESRCH,
            "the send must be attempted and the kernel's answer reported"
        );
    }

    // =================================================================
    // Phase 212 — sigaddset/sigdelset/sigismember: EFAULT for NULL set
    //
    // Linux returns EFAULT for NULL user-space pointers (via
    // copy_from_user/copy_to_user).  Our stubs used EINVAL for both
    // NULL set and bad signum.  Phase 212 splits the check:
    //   NULL set → EFAULT, bad signum → EINVAL.
    // =================================================================

    /// sigaddset: NULL set → EINVAL (glibc `signal/sigaddset.c`).
    #[test]
    fn test_sigaddset_null_einval() {
        crate::errno::set_errno(0);
        let ret = unsafe { sigaddset(core::ptr::null_mut(), SIGINT) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// sigemptyset: NULL set → EINVAL (glibc `signal/sigempty.c`).
    #[test]
    fn test_sigemptyset_null_einval() {
        crate::errno::set_errno(0);
        let ret = unsafe { sigemptyset(core::ptr::null_mut()) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// sigfillset: NULL set → EINVAL (glibc `signal/sigfillset.c`).
    #[test]
    fn test_sigfillset_null_einval() {
        crate::errno::set_errno(0);
        let ret = unsafe { sigfillset(core::ptr::null_mut()) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// No sigsetop can ever report EFAULT: they issue no syscall, so
    /// there is no `copy_to_user` to fault.  This test exists to fail
    /// loudly if a future NULL-pointer sweep reintroduces EFAULT here.
    #[test]
    fn test_sigsetops_never_report_efault_on_null() {
        for errno_after in [
            {
                crate::errno::set_errno(0);
                let _ = unsafe { sigemptyset(core::ptr::null_mut()) };
                crate::errno::get_errno()
            },
            {
                crate::errno::set_errno(0);
                let _ = unsafe { sigfillset(core::ptr::null_mut()) };
                crate::errno::get_errno()
            },
            {
                crate::errno::set_errno(0);
                let _ = unsafe { sigaddset(core::ptr::null_mut(), SIGINT) };
                crate::errno::get_errno()
            },
            {
                crate::errno::set_errno(0);
                let _ = unsafe { sigdelset(core::ptr::null_mut(), SIGINT) };
                crate::errno::get_errno()
            },
            {
                crate::errno::set_errno(0);
                let _ = unsafe { sigismember(core::ptr::null(), SIGINT) };
                crate::errno::get_errno()
            },
        ] {
            assert_ne!(errno_after, crate::errno::EFAULT);
            assert_eq!(errno_after, crate::errno::EINVAL);
        }
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

    /// sigaddset: NULL + bad signum → EINVAL either way.  glibc tests
    /// both in one `||` chain, so there is no priority to get wrong.
    #[test]
    fn test_sigaddset_null_and_bad_signum_agree_on_einval() {
        crate::errno::set_errno(0);
        let ret = unsafe { sigaddset(core::ptr::null_mut(), 0) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// sigdelset: NULL set → EINVAL (glibc `signal/sigdelset.c`).
    #[test]
    fn test_sigdelset_null_einval() {
        crate::errno::set_errno(0);
        let ret = unsafe { sigdelset(core::ptr::null_mut(), SIGINT) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// sigdelset: bad signum → EINVAL.
    #[test]
    fn test_phase212_sigdelset_bad_signum_einval() {
        let mut set = SigsetT {
            bits: [u64::MAX; 16],
        };
        crate::errno::set_errno(0);
        let ret = unsafe { sigdelset(&raw mut set, -1) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// sigismember: NULL set → EINVAL (glibc `signal/sigismem.c`).
    #[test]
    fn test_sigismember_null_einval() {
        crate::errno::set_errno(0);
        let ret = unsafe { sigismember(core::ptr::null(), SIGINT) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// sigismember: bad signum → EINVAL.
    #[test]
    fn test_phase212_sigismember_bad_signum_einval() {
        let set = SigsetT {
            bits: [u64::MAX; 16],
        };
        crate::errno::set_errno(0);
        let ret = unsafe { sigismember(&raw const set, 0) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// sigismember: NULL + bad signum → EINVAL either way.
    #[test]
    fn test_sigismember_null_and_bad_signum_agree_on_einval() {
        crate::errno::set_errno(0);
        let ret = unsafe { sigismember(core::ptr::null(), -1) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }
}
