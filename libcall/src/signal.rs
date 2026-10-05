//! Receiving signals: signal sets, handlers, the blocked mask, waiting for a
//! handler to run, and the timer whose expiry is a signal.
//!
//! GNU `timeout` is made of these, and is why they are here
//! (`userspace/coreutils/src/bin/timeout.rs`): it arms a timer, blocks the
//! signals its handlers act on, and waits in `sigsuspend` for the timer or its
//! command, whichever comes first -- nothing polled, nothing slept.
//!
//! # Any of these may be called from a signal handler
//!
//! Each function is one call into the C library, allocating nothing and taking
//! no lock of this crate's, and each C function behind it is on POSIX's list
//! of async-signal-safe functions: `sigaction`, `signal`, `sigprocmask`,
//! `sigsuspend`, `sigemptyset`, `sigaddset`, `raise`, `setitimer` and `alarm`.
//! That is a requirement rather than a courtesy: `timeout`'s handler re-arms
//! the timer from inside itself, as upstream's does.
//!
//! # Two layouts, restated
//!
//! `sigset_t` and `struct sigaction` are written out here rather than taken
//! from `posix`, for the reason the crate documentation gives for every value
//! in it. glibc, musl and SlateOS's library agree on both -- a set of 1024
//! bits, and a handler, a set, an `int` of flags and a restorer in 152 bytes --
//! and `the_layouts_are_the_librarys` checks them against `posix`'s through the
//! dev-dependency.

#[cfg(not(unix))]
use crate::ENOSYS;
#[cfg(unix)]
use crate::last_errno;

pub use crate::pty::SIGHUP;
pub use crate::{SIGCONT, SIGKILL, SIGPIPE, SIGSTOP, SIGTERM};

/// Interrupt from the keyboard: Ctrl-C.
pub const SIGINT: i32 = 2;
/// Quit from the keyboard: Ctrl-\.
pub const SIGQUIT: i32 = 3;
/// A timer set by [`set_alarm_timer`] or [`alarm`] has run out.
pub const SIGALRM: i32 = 14;
/// A child process has finished or stopped.
pub const SIGCHLD: i32 = 17;
/// A background process read from its terminal.
pub const SIGTTIN: i32 = 21;
/// A background process wrote to its terminal, where that is not allowed.
pub const SIGTTOU: i32 = 22;

/// `sigaction`'s flag for "resume a call this signal interrupted, rather than
/// failing it with `EINTR`".
#[cfg(any(unix, test))]
const SA_RESTART: i32 = 0x1000_0000;
/// `sigprocmask`: add to the blocked set.
const SIG_BLOCK: i32 = 0;
/// `sigprocmask`: take out of the blocked set.
const SIG_UNBLOCK: i32 = 1;
/// `setitimer`'s timer that counts real time and ends in `SIGALRM`.
#[cfg(any(unix, test))]
const ITIMER_REAL: i32 = 0;
/// `signal`'s "do the default thing".
#[cfg(unix)]
const SIG_DFL: usize = 0;
/// What `signal` returns when it refused.
#[cfg(unix)]
const SIG_ERR: usize = usize::MAX;

/// A signal handler: called with the number of the signal that arrived.
///
/// `extern "C"`, because the C library is what calls it. It runs between two
/// instructions of whatever the program was doing, so it may make only
/// async-signal-safe calls -- every function in this module is one -- and it
/// must not unwind.
pub type Handler = extern "C" fn(i32);

/// A set of signals: the C library's `sigset_t`.
///
/// Emptied and added to only by the library's own `sigemptyset` and
/// `sigaddset`, so which bit stands for which signal is the library's business
/// and is not restated here.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SigSet {
    bits: [u64; 16],
}

impl SigSet {
    /// The empty set: `sigemptyset`.
    #[must_use]
    pub fn empty() -> Self {
        let mut set = SigSet { bits: [0; 16] };
        empty_one(&mut set);
        set
    }

    /// Add signal `sig`: `sigaddset`.
    ///
    /// # Errors
    ///
    /// `EINVAL` for a number that is no signal here -- glibc's also refuses 32
    /// and 33, which it keeps for its threads -- and [`ENOSYS`](crate::ENOSYS) off Unix.
    pub fn add(&mut self, sig: i32) -> Result<(), i32> {
        add_one(self, sig)
    }
}

/// C's `struct sigaction`, as glibc, musl and SlateOS's library lay it out.
#[cfg(any(unix, test))]
#[repr(C)]
struct SigAction {
    /// `sa_handler`: a function, or `SIG_DFL` (0) or `SIG_IGN` (1).
    handler: usize,
    /// `sa_mask`: what else is blocked while the handler runs.
    mask: SigSet,
    /// `sa_flags`, a C `int`; the four bytes after it are padding.
    flags: i32,
    /// `sa_restorer`. The libraries put their own in; ours is ignored.
    restorer: usize,
}

/// C's `struct itimerval`: the repeat interval, then the time to the first
/// expiry, each a `struct timeval` of seconds and microseconds.
#[cfg(any(unix, test))]
#[repr(C)]
struct Itimerval {
    interval_sec: i64,
    interval_usec: i64,
    value_sec: i64,
    value_usec: i64,
}

/// The library's symbols, declared once.
///
/// `sigprocmask` takes `u64` pointers, not [`SigSet`] ones, because `pty`
/// declares it that way too, and one symbol declared two ways in one crate is
/// what `clashing_extern_declarations` exists to refuse. The others are
/// declared nowhere else.
#[cfg(unix)]
mod sys {
    use super::{Itimerval, SigAction};

    unsafe extern "C" {
        pub fn sigemptyset(set: *mut u64) -> i32;
        pub fn sigaddset(set: *mut u64, sig: i32) -> i32;
        pub fn sigaction(sig: i32, act: *const SigAction, old: *mut SigAction) -> i32;
        pub fn sigprocmask(how: i32, set: *const u64, oldset: *mut u64) -> i32;
        pub fn sigsuspend(mask: *const u64) -> i32;
        pub fn raise(sig: i32) -> i32;
        pub fn setitimer(which: i32, new: *const Itimerval, old: *mut Itimerval) -> i32;
        pub fn alarm(seconds: u32) -> u32;
    }
}

// ---------------------------------------------------------------------------
// Sets
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn empty_one(set: &mut SigSet) {
    // SAFETY: `set.bits` is a live, writable `sigset_t`-sized array, which is
    // all `sigemptyset` writes. Given a valid pointer it cannot fail, so there
    // is no result to look at.
    unsafe { sys::sigemptyset(set.bits.as_mut_ptr()) };
}

/// Off Unix the set is already empty: it was made all zeros.
#[cfg(not(unix))]
fn empty_one(_set: &mut SigSet) {}

#[cfg(unix)]
fn add_one(set: &mut SigSet, sig: i32) -> Result<(), i32> {
    // SAFETY: as for `empty_one`: `sigaddset` writes one bit of the live array
    // it is handed, or refuses the number without writing anything.
    let rc = unsafe { sys::sigaddset(set.bits.as_mut_ptr(), sig) };
    if rc == 0 { Ok(()) } else { Err(last_errno()) }
}

#[cfg(not(unix))]
fn add_one(_set: &mut SigSet, _sig: i32) -> Result<(), i32> {
    Err(ENOSYS)
}

// ---------------------------------------------------------------------------
// What a signal does
// ---------------------------------------------------------------------------

/// Run `handler` whenever `sig` arrives: `sigaction`, with an empty `sa_mask`
/// -- so another signal may interrupt the handler -- and `SA_RESTART` when
/// `restart` is set, so that a call the signal interrupts is resumed rather
/// than failing with `EINTR`.
///
/// # Errors
///
/// `EINVAL` for a number that is no signal, and for `SIGKILL` and `SIGSTOP`,
/// which cannot be caught; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn set_handler(sig: i32, handler: Handler, restart: bool) -> Result<(), i32> {
    set_handler_one(sig, handler, restart)
}

#[cfg(unix)]
fn set_handler_one(sig: i32, handler: Handler, restart: bool) -> Result<(), i32> {
    let action = SigAction {
        handler: handler as usize,
        mask: SigSet::empty(),
        flags: if restart { SA_RESTART } else { 0 },
        restorer: 0,
    };
    // SAFETY: `action` is a complete `struct sigaction` that lives for the
    // call, which copies it; a null `old` asks for nothing back. `handler` is
    // an `extern "C" fn(i32)`, which is the type the library calls.
    let rc = unsafe { sys::sigaction(sig, &raw const action, core::ptr::null_mut()) };
    if rc == 0 { Ok(()) } else { Err(last_errno()) }
}

#[cfg(not(unix))]
fn set_handler_one(_sig: i32, _handler: Handler, _restart: bool) -> Result<(), i32> {
    Err(ENOSYS)
}

/// Give `sig` back its default action: `signal (sig, SIG_DFL)`.
///
/// # Errors
///
/// `EINVAL` for a number that is no signal; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn set_default(sig: i32) -> Result<(), i32> {
    set_default_one(sig)
}

#[cfg(unix)]
fn set_default_one(sig: i32) -> Result<(), i32> {
    // SAFETY: `SIG_DFL` installs no code of ours, and `signal` reads and
    // writes no memory we own: it returns the replaced handler, or `SIG_ERR`.
    let old = unsafe { crate::sys::signal(sig, SIG_DFL) };
    if old == SIG_ERR {
        Err(last_errno())
    } else {
        Ok(())
    }
}

#[cfg(not(unix))]
fn set_default_one(_sig: i32) -> Result<(), i32> {
    Err(ENOSYS)
}

/// Send `sig` to this process: `raise`.
///
/// A signal whose action is to end the process ends it before this returns.
///
/// # Errors
///
/// `EINVAL` for a number that is no signal; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn raise(sig: i32) -> Result<(), i32> {
    raise_one(sig)
}

#[cfg(unix)]
fn raise_one(sig: i32) -> Result<(), i32> {
    // SAFETY: `raise` takes a number and touches no memory of ours.
    let rc = unsafe { sys::raise(sig) };
    if rc == 0 { Ok(()) } else { Err(last_errno()) }
}

#[cfg(not(unix))]
fn raise_one(_sig: i32) -> Result<(), i32> {
    Err(ENOSYS)
}

// ---------------------------------------------------------------------------
// The blocked set, and waiting
// ---------------------------------------------------------------------------

/// Block every signal in `set` as well as those already blocked:
/// `sigprocmask (SIG_BLOCK, ...)`. Returns the set that was blocked before.
///
/// A blocked signal is not lost: it waits, pending, until it is unblocked --
/// which [`suspend`] does for exactly as long as it waits.
///
/// # Errors
///
/// `EINVAL` from the library, which a valid `how` and pointer cannot draw;
/// [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn block(set: &SigSet) -> Result<SigSet, i32> {
    change_mask(SIG_BLOCK, set)
}

/// Unblock every signal in `set`: `sigprocmask (SIG_UNBLOCK, ...)`. Returns
/// the set that was blocked before.
///
/// # Errors
///
/// As [`block`].
pub fn unblock(set: &SigSet) -> Result<SigSet, i32> {
    change_mask(SIG_UNBLOCK, set)
}

#[cfg(unix)]
fn change_mask(how: i32, set: &SigSet) -> Result<SigSet, i32> {
    let mut old = SigSet { bits: [0; 16] };
    // SAFETY: `set.bits` is a live `sigset_t` the call only reads, and
    // `old.bits` a live one it only writes.
    let rc = unsafe { sys::sigprocmask(how, set.bits.as_ptr(), old.bits.as_mut_ptr()) };
    if rc == 0 { Ok(old) } else { Err(last_errno()) }
}

#[cfg(not(unix))]
fn change_mask(_how: i32, _set: &SigSet) -> Result<SigSet, i32> {
    Err(ENOSYS)
}

/// Wait for a signal handler to run, with `mask` as the blocked set
/// meanwhile: `sigsuspend`.
///
/// Swapping the mask and starting to wait are one step, which is the whole
/// point: a caller that blocks a signal, checks for the thing it signals,
/// and then suspends with the signal unblocked cannot miss one that arrives
/// between the check and the wait -- it was held pending, and is delivered
/// the moment the wait begins. The previous mask is back in place on return.
///
/// # Errors
///
/// None in practice: `sigsuspend` returns only once a handler has run, with
/// `EINTR`, which is success here. Anything else it says is passed on.
/// [`ENOSYS`](crate::ENOSYS) off Unix, where nothing would ever end the wait.
pub fn suspend(mask: &SigSet) -> Result<(), i32> {
    suspend_one(mask)
}

#[cfg(unix)]
fn suspend_one(mask: &SigSet) -> Result<(), i32> {
    // SAFETY: `mask.bits` is a live `sigset_t` that the call only reads.
    let rc = unsafe { sys::sigsuspend(mask.bits.as_ptr()) };
    if rc == 0 {
        // Not a return POSIX allows; read as "something woke it".
        return Ok(());
    }
    match last_errno() {
        crate::pty::EINTR => Ok(()),
        e => Err(e),
    }
}

#[cfg(not(unix))]
fn suspend_one(_mask: &SigSet) -> Result<(), i32> {
    Err(ENOSYS)
}

// ---------------------------------------------------------------------------
// The alarm timer
// ---------------------------------------------------------------------------

/// Deliver `SIGALRM` once, `seconds` and `microseconds` from now:
/// `setitimer (ITIMER_REAL, ...)` with no repeat interval. Zero for both
/// disarms a timer already set; setting one replaces it.
///
/// There is one such timer per process, shared with [`alarm`].
///
/// # Errors
///
/// `EINVAL` for a negative value or for `microseconds` of a million or more;
/// [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn set_alarm_timer(seconds: i64, microseconds: i64) -> Result<(), i32> {
    set_alarm_timer_one(seconds, microseconds)
}

#[cfg(unix)]
fn set_alarm_timer_one(seconds: i64, microseconds: i64) -> Result<(), i32> {
    let timer = Itimerval {
        interval_sec: 0,
        interval_usec: 0,
        value_sec: seconds,
        value_usec: microseconds,
    };
    // SAFETY: `timer` is a complete `struct itimerval` that lives for the
    // call, which copies it; a null `old` asks for nothing back.
    let rc = unsafe { sys::setitimer(ITIMER_REAL, &raw const timer, core::ptr::null_mut()) };
    if rc == 0 { Ok(()) } else { Err(last_errno()) }
}

#[cfg(not(unix))]
fn set_alarm_timer_one(_seconds: i64, _microseconds: i64) -> Result<(), i32> {
    Err(ENOSYS)
}

/// Deliver `SIGALRM` in `seconds` whole seconds: `alarm`, the timer's oldest
/// interface. `0` cancels one already set. Returns the seconds that were left
/// on that one, if any.
///
/// For a caller whose [`set_alarm_timer`] was refused: one-second resolution
/// is worse, and is still a timer.
#[cfg(unix)]
#[must_use = "the seconds left on a replaced alarm are its only report"]
pub fn alarm(seconds: u32) -> u32 {
    // SAFETY: `alarm` takes a number and touches no memory of ours.
    unsafe { sys::alarm(seconds) }
}

/// No timer to set off Unix, and `alarm` has no way to say so: it reports
/// "nothing was pending", which is true. Like [`crate::sync`], there is no
/// honest refusal available, so the host arm does nothing rather than invent
/// one.
#[cfg(not(unix))]
#[must_use = "the seconds left on a replaced alarm are its only report"]
pub fn alarm(_seconds: u32) -> u32 {
    0
}

// ---------------------------------------------------------------------------
// errno across a handler
// ---------------------------------------------------------------------------

/// `errno` as it was when this was made, put back when it is dropped.
///
/// For a signal handler. A handler runs between two instructions of the code
/// it interrupted, and that code may be between a failed call and its reading
/// of `errno` -- in this crate, between a C call and the `last_errno` that
/// reports it. A handler that made calls of its own would leave their `errno`
/// behind, and the interrupted code would report the handler's failure as its
/// own. Make one first thing in the handler; it is restored as the handler
/// returns.
#[must_use = "errno is put back when this is dropped; `let _ = ` drops it at once"]
pub struct ErrnoGuard {
    #[cfg(unix)]
    saved: i32,
}

impl ErrnoGuard {
    /// Remember `errno` as it is now.
    pub fn save() -> Self {
        ErrnoGuard {
            #[cfg(unix)]
            saved: last_errno(),
        }
    }
}

impl Drop for ErrnoGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        crate::set_errno(self.saved);
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use core::mem::{offset_of, size_of};

    /// Both structures are the library's: the same size, and every field at
    /// the offset the library reads it from.
    #[test]
    fn the_layouts_are_the_librarys() {
        use posix::signal::{Sigaction, SigsetT};
        assert_eq!(size_of::<SigSet>(), size_of::<SigsetT>());
        assert_eq!(size_of::<SigAction>(), size_of::<Sigaction>());
        assert_eq!(size_of::<SigAction>(), 152, "glibc's struct sigaction");
        assert_eq!(
            offset_of!(SigAction, handler),
            offset_of!(Sigaction, sa_handler)
        );
        assert_eq!(offset_of!(SigAction, mask), offset_of!(Sigaction, sa_mask));
        assert_eq!(
            offset_of!(SigAction, flags),
            offset_of!(Sigaction, sa_flags)
        );
        assert_eq!(offset_of!(SigAction, flags), 136);
        assert_eq!(
            offset_of!(SigAction, restorer),
            offset_of!(Sigaction, sa_restorer)
        );
        assert_eq!(size_of::<Itimerval>(), size_of::<posix::time::Itimerval>());
        assert_eq!(size_of::<Itimerval>(), 32);
    }

    /// Every restated number is the library's.
    #[test]
    fn the_numbers_are_the_librarys() {
        assert_eq!(SIGINT, posix::signal::SIGINT);
        assert_eq!(SIGQUIT, posix::signal::SIGQUIT);
        assert_eq!(SIGALRM, posix::signal::SIGALRM);
        assert_eq!(SIGCHLD, posix::signal::SIGCHLD);
        assert_eq!(SIGTTIN, posix::signal::SIGTTIN);
        assert_eq!(SIGTTOU, posix::signal::SIGTTOU);
        assert_eq!(
            u32::try_from(SA_RESTART).ok(),
            Some(posix::signal::SA_RESTART)
        );
        assert_eq!(SIG_BLOCK, posix::signal::SIG_BLOCK);
        assert_eq!(SIG_UNBLOCK, posix::signal::SIG_UNBLOCK);
        assert_eq!(ITIMER_REAL, posix::time::ITIMER_REAL);
    }

    /// Off Unix every call declines, and says so with `ENOSYS`.
    #[cfg(not(unix))]
    #[test]
    fn off_unix_every_call_declines() {
        extern "C" fn nothing(_sig: i32) {}
        let mut set = SigSet::empty();
        assert_eq!(set.add(SIGINT), Err(ENOSYS));
        assert_eq!(set_handler(SIGINT, nothing, true), Err(ENOSYS));
        assert_eq!(set_default(SIGINT), Err(ENOSYS));
        assert_eq!(raise(SIGINT), Err(ENOSYS));
        assert!(block(&set).is_err());
        assert!(unblock(&set).is_err());
        assert_eq!(suspend(&set), Err(ENOSYS));
        assert_eq!(set_alarm_timer(1, 0), Err(ENOSYS));
        assert_eq!(alarm(0), 0);
        drop(ErrnoGuard::save());
    }

    /// The real library, end to end: a blocked signal waits, and `suspend`
    /// both delivers it and returns once its handler has run.
    ///
    /// One test rather than several, because handlers are the whole process's
    /// and the test harness runs tests side by side; `SIGUSR1` is used by
    /// nothing else here. `raise` sends to the calling thread, whose mask is
    /// the one `block` changed, so the sequence is this thread's alone.
    #[cfg(unix)]
    #[test]
    fn a_blocked_signal_waits_and_suspend_delivers_it() {
        use core::sync::atomic::{AtomicUsize, Ordering};
        const SIGUSR1: i32 = 10;
        static RAN: AtomicUsize = AtomicUsize::new(0);
        extern "C" fn count(_sig: i32) {
            RAN.fetch_add(1, Ordering::SeqCst);
        }

        set_handler(SIGUSR1, count, true).unwrap();
        let mut set = SigSet::empty();
        set.add(SIGUSR1).unwrap();
        let before = block(&set).unwrap();

        raise(SIGUSR1).unwrap();
        assert_eq!(RAN.load(Ordering::SeqCst), 0, "a blocked signal ran");

        // `before` does not block SIGUSR1, so the pending one is delivered as
        // the wait begins, and the wait ends because its handler ran.
        suspend(&before).unwrap();
        assert_eq!(RAN.load(Ordering::SeqCst), 1);

        unblock(&set).unwrap();
        set_default(SIGUSR1).unwrap();
        assert_eq!(set.add(0), Err(posix::errno::EINVAL));
        assert_eq!(set_handler(SIGKILL, count, true), Err(posix::errno::EINVAL));
    }

    /// The timer fires, and its signal reaches the handler.
    ///
    /// Watched by polling rather than by `suspend`: the timer's signal is the
    /// process's, not this thread's, and the harness's other threads do not
    /// block it, so any of them may be the one that runs the handler -- a
    /// `suspend` here could wait for a delivery that happened elsewhere.
    #[cfg(unix)]
    #[test]
    fn the_alarm_timer_fires() {
        extern crate std;
        use core::sync::atomic::{AtomicBool, Ordering};
        use std::time::{Duration, Instant};
        static RANG: AtomicBool = AtomicBool::new(false);
        extern "C" fn rang(_sig: i32) {
            RANG.store(true, Ordering::SeqCst);
        }

        set_handler(SIGALRM, rang, true).unwrap();
        set_alarm_timer(0, 20_000).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !RANG.load(Ordering::SeqCst) {
            assert!(Instant::now() < deadline, "SIGALRM never arrived");
            std::thread::sleep(Duration::from_millis(1));
        }
        set_default(SIGALRM).unwrap();
        assert_eq!(set_alarm_timer(-1, 0), Err(posix::errno::EINVAL));
        assert_eq!(set_alarm_timer(0, 1_000_000), Err(posix::errno::EINVAL));
        assert_eq!(alarm(0), 0, "no alarm was left set");
    }

    /// A guard puts back the `errno` it found, whatever happened since.
    #[cfg(unix)]
    #[test]
    fn errno_is_put_back() {
        crate::set_errno(posix::errno::ENOENT);
        {
            let _guard = ErrnoGuard::save();
            crate::set_errno(posix::errno::EINVAL);
        }
        assert_eq!(last_errno(), posix::errno::ENOENT);
    }
}
