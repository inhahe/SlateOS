//! Futex waits and the low-level lock the pthread layer is built on.
//!
//! glibc's `lowlevellock.h` and `futex-internal.h`, over this kernel's futex
//! syscalls (`SYS_FUTEX_WAIT`, `SYS_FUTEX_WAIT_TIMEOUT`, `SYS_FUTEX_WAKE`),
//! and the priority-inheritance futexes `PTHREAD_PRIO_INHERIT` mutexes and
//! the Linux `futex(2)` PI operations sleep in (`SYS_FUTEX_LOCK_PI`,
//! `SYS_FUTEX_LOCK_PI_TIMEOUT`, `SYS_FUTEX_UNLOCK_PI`; see
//! [`futex_lock_pi`]).
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

/// Sleep until `deadline` on `clock` -- or until a signal handler has run on
/// this thread since `mark`, whatever its `SA_RESTART`: Linux never restarts
/// a sleep (signal(7)).  `Err(left)`, the time still to go, when a handler
/// ended it.  A signal that runs no handler here does not.
///
/// The sleep is a timed futex wait on a word of its own, which nothing
/// wakes, because that is the wait the kernel ends for a signal:
/// `SYS_SLEEP` sleeps its full time whatever comes, which until 2026-09-30
/// made every sleep here deaf to signals ([`crate::interrupt`]).
pub(crate) fn sleep_until(
    clock: i32,
    deadline: &crate::stat::Timespec,
    mark: Mark,
) -> Result<(), crate::stat::Timespec> {
    let word = AtomicI32::new(0);
    loop {
        let Some(left) = ns_until(&now_on(clock), deadline) else {
            return Ok(());
        };
        let waited = futex_wait_interruptible_since(&word, 0, Some(left), Restart::Never, mark);
        if waited == Waited::Interrupted {
            let left = ns_until(&now_on(clock), deadline).unwrap_or(0);
            return Err(crate::stat::Timespec {
                tv_sec: i64::try_from(left / 1_000_000_000).unwrap_or(i64::MAX),
                // Below 1e9, so it fits.
                tv_nsec: i64::try_from(left % 1_000_000_000).unwrap_or(0),
            });
        }
    }
}

/// One sleep of a polling loop: `ns` at most, in a futex wait on a word of
/// its own, which the kernel ends for a signal -- where `SYS_SLEEP` would
/// sleep it out.  It returns at once, too, when a handler that ends a call
/// following `restart` has already run here since `mark`.  Why it ended is
/// the loop's to find: it looks again, and then asks `mark`.
pub(crate) fn nap(ns: u64, restart: Restart, mark: Mark) {
    let word = AtomicI32::new(0);
    let _ = futex_wait_interruptible_since(&word, 0, Some(ns), restart, mark);
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

// ---------------------------------------------------------------------------
// Priority-inheritance futexes
// ---------------------------------------------------------------------------
//
// The kernel's PI futexes (`kernel/src/ipc/futex.rs`) use Linux's word: the
// owner's task id, 0 when free, and two flags the kernel sets.  The owner's
// id is the one `SYS_TASK_ID` answers, which is `pthread::current_tid`'s.  A
// locker that finds the word held sleeps in the kernel, which lends the
// holder the sleeper's priority; the holder's unlock hands the word to the
// highest-priority sleeper and drops what it was lent.
//
// The kernel documents a fast path besides -- take a free word with one
// compare-and-swap of 0 for one's id, and give an uncontended one back by
// swapping the id for 0, with no syscall -- but it records an owner only when
// the word was taken inside the kernel.  A holder it has no record of keeps
// the priority it was lent after its lender gave up waiting, is passed over
// when a chain of lenders is walked, and loses what it was lent for one mutex
// when it unlocks another
// (`requests/d-a-pi-futex-owners-taken-in-userspace-are-invisible-to-the-kernel.md`).
// So `pthread`'s PI mutexes take and give back every word through the kernel
// until it records such owners; `linux_futex` passes on what its caller asks.

/// The owner's task id in a PI futex word: Linux's `FUTEX_TID_MASK`.
pub(crate) const FUTEX_TID_MASK: u32 = 0x3FFF_FFFF;

// The kernel's two flags, and the conversion back into a stored word, are
// named only where the kernel is played rather than called -- the host's
// stand-in below and the tests: on the target the library sets neither flag
// and reads only the owner's id.

/// Set in a PI futex word by the kernel while a thread sleeps on it, so that
/// the owner's unlock goes through the kernel and the word is handed on.
#[cfg(not(target_os = "none"))]
pub(crate) const FUTEX_WAITERS: u32 = 0x8000_0000;
/// Set in a PI futex word by the kernel when its owner died holding it --
/// the word is then handed to a sleeper, or left free with only this bit.
#[cfg(not(target_os = "none"))]
pub(crate) const FUTEX_OWNER_DIED: u32 = 0x4000_0000;

/// A PI futex word as the kernel reads it: the same 32 bits, unsigned.
#[allow(clippy::cast_sign_loss)]
pub(crate) const fn pi_bits(v: i32) -> u32 {
    v as u32
}

/// A PI futex word as the `AtomicI32` it is stored in holds it.
#[cfg(not(target_os = "none"))]
#[allow(clippy::cast_possible_wrap)]
pub(crate) const fn pi_word(v: u32) -> i32 {
    v as i32
}

/// The PI futex word that says `tid` holds it, nobody waiting.  Task ids are
/// below 2^30 -- the kernel's own invariant, which its half of the protocol
/// (`(task_id as u32) & FUTEX_TID_MASK`) assumes as well -- so the mask
/// changes nothing for a real one.
#[allow(clippy::cast_sign_loss)]
pub(crate) const fn pi_owner(tid: i32) -> u32 {
    (tid as u32) & FUTEX_TID_MASK
}

/// Take the PI futex `word` for the calling thread, sleeping while another
/// thread holds it -- at most `timeout_ns` if given, where 0 tries once and
/// does not sleep -- and lending that thread this one's priority meanwhile.
/// A word a dead owner left (`FUTEX_OWNER_DIED` and no owner) is taken,
/// the flag kept.  0, or the kernel's negative error: `TIMED_OUT`,
/// `DEADLOCK` when this thread holds the word already.
pub(crate) fn futex_lock_pi(word: &AtomicI32, timeout_ns: Option<u64>) -> i64 {
    #[cfg(target_os = "none")]
    {
        let addr = core::ptr::from_ref(word) as u64;
        match timeout_ns {
            None => crate::syscall::syscall1(crate::syscall::SYS_FUTEX_LOCK_PI, addr),
            Some(ns) => {
                crate::syscall::syscall2(crate::syscall::SYS_FUTEX_LOCK_PI_TIMEOUT, addr, ns)
            }
        }
    }
    #[cfg(not(target_os = "none"))]
    {
        host_pi::lock(word, timeout_ns)
    }
}

/// [`futex_lock_pi`] until `until` -- an instant on a clock -- passes, or
/// with no limit for `None`: 0, or the kernel's negative error, `TIMED_OUT`
/// once the instant has passed.
///
/// The kernel's timeout is relative, so it is measured afresh from `until`'s
/// clock for each call; when the kernel's time runs out and the clock says
/// some is left -- a realtime clock set back meanwhile -- the wait goes on,
/// and once the instant has passed the last call tries the word once more
/// without sleeping, as Linux's last `try_to_take_rt_mutex` does.  A signal
/// does not end it: a PI lock is restarted, as Linux restarts one
/// (`ERESTARTNOINTR`).
pub(crate) fn futex_lock_pi_until(
    word: &AtomicI32,
    until: Option<(i32, &crate::stat::Timespec)>,
) -> i64 {
    loop {
        let timeout = until.map(|(clock, at)| ns_until(&now_on(clock), at).unwrap_or(0));
        match futex_lock_pi(word, timeout) {
            crate::errno::native::INTERRUPTED => {}
            crate::errno::native::TIMED_OUT if timeout != Some(0) => {}
            r => return r,
        }
    }
}

/// Release the PI futex `word`, which the calling thread holds, handing it
/// to the highest-priority thread asleep on it if there is one, and dropping
/// any priority this thread was lent for it.  0, or the kernel's negative
/// error: `INVALID_ARGUMENT` when the caller does not hold the word.
pub(crate) fn futex_unlock_pi(word: &AtomicI32) -> i64 {
    #[cfg(target_os = "none")]
    {
        let addr = core::ptr::from_ref(word) as u64;
        crate::syscall::syscall1(crate::syscall::SYS_FUTEX_UNLOCK_PI, addr)
    }
    #[cfg(not(target_os = "none"))]
    {
        host_pi::unlock(word)
    }
}

/// The kernel's PI futexes, for the host, which has none: the same claims on
/// the word, made the same way, but waited out by yielding rather than
/// sleeping, and with no priorities to lend.  An unlock frees the word rather
/// than handing it to a waiter, so a waiter may lose it to a newcomer -- which
/// a PI mutex's caller cannot tell from the kernel choosing another sleeper.
/// Nor is a word marked `FUTEX_WAITERS` while a thread waits: the mark
/// tells an owner that gives a word back by itself to ask the kernel instead,
/// and every PI unlock here asks.
#[cfg(not(target_os = "none"))]
mod host_pi {
    use super::{FUTEX_OWNER_DIED, FUTEX_TID_MASK, FUTEX_WAITERS, pi_bits, pi_owner, pi_word};
    use crate::errno::native;
    use core::sync::atomic::{AtomicI32, Ordering};

    /// The calling thread's id, as the kernel records an owner.
    fn me() -> u32 {
        pi_owner(crate::pthread::current_tid())
    }

    /// `kernel/src/ipc/futex.rs::lock_pi_inner`.
    pub(super) fn lock(word: &AtomicI32, timeout_ns: Option<u64>) -> i64 {
        let tid = me();
        let start = std::time::Instant::now();
        loop {
            let w = pi_bits(word.load(Ordering::Relaxed));
            let owner = w & FUTEX_TID_MASK;
            if owner == 0 {
                // Free, or a dead owner's: claim it, keeping the flags.
                let new = tid | (w & (FUTEX_OWNER_DIED | FUTEX_WAITERS));
                if word
                    .compare_exchange(
                        pi_word(w),
                        pi_word(new),
                        Ordering::Acquire,
                        Ordering::Relaxed,
                    )
                    .is_ok()
                {
                    return 0;
                }
                continue;
            }
            if owner == tid {
                return native::DEADLOCK;
            }
            if let Some(ns) = timeout_ns {
                let waited = u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX);
                if ns == 0 || waited >= ns {
                    return native::TIMED_OUT;
                }
            }
            std::thread::yield_now();
        }
    }

    /// `kernel/src/ipc/futex.rs::futex_unlock_pi`.
    pub(super) fn unlock(word: &AtomicI32) -> i64 {
        if pi_bits(word.load(Ordering::Relaxed)) & FUTEX_TID_MASK != me() {
            return native::INVALID_ARGUMENT;
        }
        word.store(0, Ordering::Release);
        0
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
