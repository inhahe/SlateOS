//! Futex waits and the low-level lock the pthread layer is built on.
//!
//! glibc's `lowlevellock.h` and `futex-internal.h`, over this kernel's futex
//! syscalls (`SYS_FUTEX_WAIT`, `SYS_FUTEX_WAIT_TIMEOUT`, `SYS_FUTEX_WAKE`).
//! A futex is a 32-bit word in user memory: a thread sleeps in the kernel
//! only while the word still holds the value it saw, and is woken by a
//! thread that changed it.  So the uncontended paths are pure userspace
//! atomics, and a blocked thread costs nothing until it is woken --
//! `performance-targets.md`'s "Futex wait/wake (uncontended)" row.
//!
//! Until 2026-09-26 the pthread layer did not use them: a contended mutex,
//! condition variable, rwlock or barrier slept in 1 ms steps and polled, so
//! every hand-off cost up to a millisecond and a waiter that was never going
//! to be woken kept waking up.
//!
//! ## The lock
//!
//! [`lll_lock`] is Ulrich Drepper's second mutex from "Futexes Are Tricky",
//! which is also glibc's: the word is 0 (unlocked), 1 (locked, nobody
//! waiting) or 2 (locked, somebody may be waiting).  A locker that finds it
//! held marks it 2 and sleeps; an unlocker that finds 2 wakes one sleeper.
//! A thread that wins the lock after sleeping leaves it at 2, since others
//! may still be asleep -- at the cost of one spurious wake later, never a
//! lost one.
//!
//! ## Signals
//!
//! A signal for the process ends a futex wait early.  [`futex_wait`] does
//! not tell its caller -- the thread functions and the locks re-check and
//! sleep again, as POSIX has them -- while [`futex_wait_interruptible`] says
//! whether a signal handler that ran on this thread interrupted the call it
//! waits for, by the rules [`crate::interrupt`] sets out.
//!
//! ## Host builds
//!
//! There is no kernel futex on the host, so a wait yields the thread and
//! returns: every caller already re-checks its condition in a loop, so this
//! is a correct (if busier) futex that only ever wakes spuriously.  It keeps
//! the algorithms testable with real threads.  A test can script what its
//! thread's waits meet instead ([`crate::interrupt::script`]).
//!
//! ## Futex keys
//!
//! The kernel keys a futex by (address space, virtual address)
//! (`kernel/src/ipc/futex.rs`), so a waiter in one process is never woken
//! from another.  Process-shared objects are refused with `ENOTSUP` for that
//! reason -- see `pthread::futex_supports_pshared`.

use core::sync::atomic::{AtomicI32, Ordering};

use crate::interrupt::{Mark, Restart};

/// The lock word's states.
const UNLOCKED: i32 = 0;
/// Locked, with no thread asleep on it.
const LOCKED: i32 = 1;
/// Locked, and a thread may be asleep on it.
const CONTENDED: i32 = 2;

/// Spins before sleeping: a lock held for a few hundred cycles is cheaper to
/// wait out than to sleep on, and sleeping costs two syscalls.
const SPIN: u32 = 100;

/// The futex word's value as the kernel compares it: the same 32 bits,
/// unsigned.
#[allow(clippy::cast_sign_loss)]
const fn futex_value(v: i32) -> u32 {
    v as u32
}

/// The kernel's whole answer to a futex wait on the word at `addr`: 1 when
/// woken, 0 when the word no longer held `expected`, or a native error --
/// `TimedOut` once `timeout_ns` (if any) ran out, `Interrupted` when a
/// signal ended the wait.  Every wait here comes down to this; only
/// [`crate::linux_futex`] reports the answer as it is.
///
/// On the host, where there is no kernel futex, a wait yields and answers
/// "woken" -- a spurious wake, which every caller already re-checks for --
/// unless a test has scripted it ([`crate::interrupt::script`]).
pub(crate) fn futex_wait_raw(addr: u64, expected: u32, timeout_ns: Option<u64>) -> i64 {
    #[cfg(target_os = "none")]
    {
        let expected = u64::from(expected);
        match timeout_ns {
            None => crate::syscall::syscall2(crate::syscall::SYS_FUTEX_WAIT, addr, expected),
            Some(ns) => {
                crate::syscall::syscall3(crate::syscall::SYS_FUTEX_WAIT_TIMEOUT, addr, expected, ns)
            }
        }
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (addr, expected, timeout_ns);
        #[cfg(test)]
        if let Some(interrupted) = crate::interrupt::script::next() {
            return if interrupted {
                crate::errno::native::INTERRUPTED
            } else {
                1
            };
        }
        std::thread::yield_now();
        1
    }
}

/// [`futex_wait_raw`] on `word`: whether the kernel says a signal ended it.
fn kernel_wait(word: &AtomicI32, expected: i32, timeout_ns: Option<u64>) -> bool {
    let addr = core::ptr::from_ref(word) as u64;
    futex_wait_raw(addr, futex_value(expected), timeout_ns) == crate::errno::native::INTERRUPTED
}

/// Sleep while `*word == expected`, until woken.  May return spuriously --
/// and at once if the word has already changed -- so callers re-check.
///
/// A signal does not end the wait as far as the caller can tell: the kernel
/// ends it, and the caller, re-checking, sleeps again.  For the calls POSIX
/// forbids to answer `EINTR` -- the mutexes, condition variables and the
/// rest -- and every lock; a call that may answer it waits in
/// [`futex_wait_interruptible`].
pub(crate) fn futex_wait(word: &AtomicI32, expected: i32) {
    // Woken, the word changed, or a signal: in every case the caller re-reads
    // the word, which is the only answer that matters here.
    let _ = kernel_wait(word, expected, None);
}

/// As [`futex_wait`], for at most `timeout_ns` nanoseconds.  The caller
/// decides whether the deadline passed by reading its clock, not from this.
pub(crate) fn futex_wait_timeout(word: &AtomicI32, expected: i32, timeout_ns: u64) {
    // As in `futex_wait`: the caller re-reads the word and its clock.
    let _ = kernel_wait(word, expected, Some(timeout_ns));
}

/// Why [`futex_wait_interruptible`] returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Waited {
    /// Anything but an interruption: a wake, a word that had already
    /// changed, the time running out, or a signal that ran no handler here
    /// that ends this call.  The caller re-checks what it waits for, and its
    /// clock, as after any wake.
    Woken,
    /// A signal handler ran on this thread that ends a call following the
    /// wait's [`Restart`] rule: the call answers `EINTR`, or its own form of
    /// it.
    Interrupted,
}

/// As [`futex_wait`] -- for at most `timeout_ns` nanoseconds if given -- but
/// a signal handler can end it: [`Waited::Interrupted`] when the kernel ended
/// the wait for a signal and a handler ran on this thread that ends a call
/// following `restart` ([`crate::interrupt`] says which).
pub(crate) fn futex_wait_interruptible(
    word: &AtomicI32,
    expected: i32,
    timeout_ns: Option<u64>,
    restart: Restart,
) -> Waited {
    futex_wait_interruptible_since(word, expected, timeout_ns, restart, Mark::now())
}

/// As [`futex_wait_interruptible`], counting handlers from `mark` rather than
/// from the wait: one that ran since `mark` ends it without a sleep.  For a
/// call that lets signals through before it waits -- `io_pgetevents`, which
/// sets its mask first -- and must count a handler that runs as they come
/// through as having interrupted it.
pub(crate) fn futex_wait_interruptible_since(
    word: &AtomicI32,
    expected: i32,
    timeout_ns: Option<u64>,
    restart: Restart,
    mark: Mark,
) -> Waited {
    if mark.interrupted(restart) {
        return Waited::Interrupted;
    }
    if kernel_wait(word, expected, timeout_ns) && mark.interrupted(restart) {
        Waited::Interrupted
    } else {
        Waited::Woken
    }
}

/// Wake at most `count` threads asleep on `word`.
pub(crate) fn futex_wake(word: &AtomicI32, count: i32) {
    #[cfg(target_os = "none")]
    {
        let addr = core::ptr::from_ref(word) as u64;
        #[allow(clippy::cast_sign_loss)]
        let n = count.max(0) as u64;
        // A failed wake has no one to report to: the waiters it would have
        // woken re-check on their own timeouts or the next wake.
        let _ = crate::syscall::syscall2(crate::syscall::SYS_FUTEX_WAKE, addr, n);
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (word, count);
    }
}

/// Wake every thread asleep on `word`.
pub(crate) fn futex_wake_all(word: &AtomicI32) {
    futex_wake(word, i32::MAX);
}

/// Take the lock if it is free.
pub(crate) fn lll_trylock(word: &AtomicI32) -> bool {
    word.compare_exchange(UNLOCKED, LOCKED, Ordering::Acquire, Ordering::Relaxed)
        .is_ok()
}

/// Take the lock, sleeping while another thread holds it.
pub(crate) fn lll_lock(word: &AtomicI32) {
    if !lll_trylock(word) {
        lll_lock_wait(word);
    }
}

/// The contended half of [`lll_lock`].
fn lll_lock_wait(word: &AtomicI32) {
    let mut i: u32 = 0;
    while i < SPIN {
        if word.load(Ordering::Relaxed) == UNLOCKED && lll_trylock(word) {
            return;
        }
        core::hint::spin_loop();
        i = i.wrapping_add(1);
    }
    // Mark the lock contended and sleep until the swap finds it free: the
    // unlocker of a contended lock wakes one sleeper.
    while word.swap(CONTENDED, Ordering::Acquire) != UNLOCKED {
        futex_wait(word, CONTENDED);
    }
}

/// Take the lock unless `remaining` says the deadline has passed first.
///
/// `remaining` is asked before each sleep and answers the nanoseconds left
/// before the deadline, or `None` once it has passed; so the caller owns the
/// clock, and a spurious wake costs one clock read.  Returns whether the lock
/// was taken.  Leaving the word at [`CONTENDED`] after a timeout costs at most
/// one wake nobody waits for.
pub(crate) fn lll_timedlock(word: &AtomicI32, mut remaining: impl FnMut() -> Option<u64>) -> bool {
    if lll_trylock(word) {
        return true;
    }
    loop {
        if word.swap(CONTENDED, Ordering::Acquire) == UNLOCKED {
            return true;
        }
        let Some(ns) = remaining() else {
            return false;
        };
        futex_wait_timeout(word, CONTENDED, ns);
    }
}

/// Release the lock, waking one sleeper if there may be one.
pub(crate) fn lll_unlock(word: &AtomicI32) {
    if word.swap(UNLOCKED, Ordering::Release) == CONTENDED {
        futex_wake(word, 1);
    }
}

/// Nanoseconds from `now` until `deadline`, or `None` if it has passed --
/// the answer [`lll_timedlock`]'s `remaining` and the timed waits give.
pub(crate) fn ns_until(
    now: &crate::stat::Timespec,
    deadline: &crate::stat::Timespec,
) -> Option<u64> {
    let to_ns = |t: &crate::stat::Timespec| -> i128 {
        i128::from(t.tv_sec)
            .saturating_mul(1_000_000_000)
            .saturating_add(i128::from(t.tv_nsec))
    };
    let left = to_ns(deadline).saturating_sub(to_ns(now));
    if left <= 0 {
        None
    } else {
        Some(u64::try_from(left).unwrap_or(u64::MAX))
    }
}

/// The instant `ns` nanoseconds after `t`, saturating: a relative timeout as
/// the deadline it comes to.
pub(crate) fn after(t: &crate::stat::Timespec, ns: u64) -> crate::stat::Timespec {
    let total = i128::from(t.tv_sec)
        .saturating_mul(1_000_000_000)
        .saturating_add(i128::from(t.tv_nsec))
        .saturating_add(i128::from(ns));
    crate::stat::Timespec {
        tv_sec: i64::try_from(total.div_euclid(1_000_000_000)).unwrap_or(i64::MAX),
        // In 0..1e9, so it fits.
        tv_nsec: i64::try_from(total.rem_euclid(1_000_000_000)).unwrap_or(0),
    }
}

/// The time on `clock` (`CLOCK_REALTIME` or `CLOCK_MONOTONIC`), for a
/// deadline measured against it.
pub(crate) fn now_on(clock: i32) -> crate::stat::Timespec {
    let mut now = crate::stat::Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // A clock this module is handed has already been validated, so the read
    // cannot fail; were it to, `now` stays at the epoch and every deadline
    // reads as far off, which errs toward waiting, not toward returning early.
    let _ = crate::time::clock_gettime(clock, &raw mut now);
    now
}

/// The clocks a pthread deadline may be measured against: glibc's
/// `futex_abstimed_supported_clockid`.
pub(crate) const fn supported_clock(clock: i32) -> bool {
    clock == crate::time::CLOCK_REALTIME || clock == crate::time::CLOCK_MONOTONIC
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn test_lock_states() {
        let w = AtomicI32::new(UNLOCKED);
        assert!(lll_trylock(&w));
        assert_eq!(w.load(Ordering::Relaxed), LOCKED);
        assert!(!lll_trylock(&w));
        lll_unlock(&w);
        assert_eq!(w.load(Ordering::Relaxed), UNLOCKED);
        // A contended lock is released to unlocked too.
        w.store(CONTENDED, Ordering::Relaxed);
        lll_unlock(&w);
        assert_eq!(w.load(Ordering::Relaxed), UNLOCKED);
    }

    /// Mutual exclusion under real contention: eight threads, each adding
    /// to a counter a thousand times inside the lock, lose no update.
    #[test]
    fn test_lock_excludes_under_contention() {
        let w = Arc::new(AtomicI32::new(UNLOCKED));
        let counter = Arc::new(AtomicUsize::new(0));
        let inside = Arc::new(AtomicUsize::new(0));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let (w, counter, inside) = (w.clone(), counter.clone(), inside.clone());
                std::thread::spawn(move || {
                    for _ in 0..1000 {
                        lll_lock(&w);
                        assert_eq!(inside.fetch_add(1, Ordering::SeqCst), 0, "two holders");
                        let c = counter.load(Ordering::Relaxed);
                        counter.store(c + 1, Ordering::Relaxed);
                        inside.fetch_sub(1, Ordering::SeqCst);
                        lll_unlock(&w);
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(counter.load(Ordering::Relaxed), 8000);
        assert_eq!(w.load(Ordering::Relaxed), UNLOCKED);
    }

    #[test]
    fn test_timedlock_times_out_on_a_held_lock() {
        let w = AtomicI32::new(UNLOCKED);
        assert!(lll_trylock(&w));
        let mut asked = 0;
        let taken = lll_timedlock(&w, || {
            asked += 1;
            if asked > 3 { None } else { Some(1_000) }
        });
        assert!(!taken);
        assert_eq!(asked, 4, "asked before each sleep, until the deadline");
        lll_unlock(&w);
        assert!(lll_timedlock(&w, || None), "a free lock is taken at once");
    }

    #[test]
    fn test_ns_until() {
        let t = |s, n| crate::stat::Timespec {
            tv_sec: s,
            tv_nsec: n,
        };
        assert_eq!(ns_until(&t(1, 0), &t(2, 500)), Some(1_000_000_500));
        assert_eq!(ns_until(&t(2, 0), &t(2, 0)), None, "exactly now has passed");
        assert_eq!(ns_until(&t(3, 0), &t(2, 0)), None);
        assert_eq!(
            ns_until(&t(0, 0), &t(i64::MAX, 0)),
            Some(u64::MAX),
            "saturates"
        );
    }
}
