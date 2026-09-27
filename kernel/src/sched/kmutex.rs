//! Kernel sleeping mutex — blocks instead of spinning on contention.
//!
//! Unlike `spin::Mutex`, which busy-waits when the lock is held by
//! another task, `KMutex` puts the waiting task to sleep and wakes it
//! when the lock becomes available.  This is better for locks held
//! across potentially-long operations (I/O, allocations, etc.) because
//! it doesn't waste CPU time spinning.
//!
//! ## When to Use
//!
//! - **`spin::Mutex`**: Short critical sections that complete in
//!   nanoseconds.  Required in ISR and softirq context (cannot sleep).
//!   Required during early boot (before scheduler is running).
//! - **`KMutex`**: Longer critical sections in process context.  May
//!   hold across allocations, file operations, or non-trivial
//!   computation.  Must NOT be held in ISR/softirq context.
//!
//! ## Design
//!
//! A `KMutex` combines an `AtomicBool` (for the fast-path uncontended
//! acquire via CAS) with a `WaitQueue` (for blocking when contended).
//! The uncontended path is a single atomic CAS — same cost as a
//! spinlock acquire.  Only when contention occurs does the overhead of
//! the WaitQueue come into play.
//!
//! ## Priority Inheritance
//!
//! Currently not implemented.  If a high-priority task blocks on a
//! `KMutex` held by a low-priority task, the low-priority task is not
//! boosted.  For priority-inheritance semantics, use PI futexes
//! (`futex_lock_pi`).  Adding PI to `KMutex` is planned.
//!
//! ## References
//!
//! - Linux `kernel/locking/mutex.c` — adaptive mutex (spin briefly,
//!   then sleep)
//! - Fuchsia `kernel/lib/fbl/include/fbl/mutex.h`
//! - FreeBSD `sys/kern/kern_mutex.c` — sleepable mutex

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

use super::waitqueue::WaitQueue;

// ---------------------------------------------------------------------------
// KMutex
// ---------------------------------------------------------------------------

/// A sleeping mutex for kernel process context.
///
/// Provides mutual exclusion with blocking (not spinning) on contention.
/// The API mirrors `spin::Mutex` for easy substitution.
///
/// # Safety
///
/// Must NOT be acquired in ISR or softirq context (those contexts
/// cannot sleep).  Only use in normal kernel task context.
pub struct KMutex<T> {
    /// Whether the mutex is currently locked.
    locked: AtomicBool,
    /// Waiters blocked on this mutex.
    waiters: WaitQueue,
    /// The protected data.
    data: UnsafeCell<T>,
}

// SAFETY: KMutex provides mutual exclusion via atomic ops + blocking.
// The UnsafeCell is only accessed through the lock guard.
unsafe impl<T: Send> Send for KMutex<T> {}
unsafe impl<T: Send> Sync for KMutex<T> {}

impl<T> KMutex<T> {
    /// Create a new unlocked mutex protecting `value`.
    pub const fn new(value: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            waiters: WaitQueue::new(),
            data: UnsafeCell::new(value),
        }
    }

    /// Acquire the mutex, blocking if it's held by another task.
    ///
    /// Returns a guard that releases the lock when dropped.
    ///
    /// # Panics
    ///
    /// Does not panic.  If the lock is held, the calling task sleeps
    /// until it becomes available.
    pub fn lock(&self) -> KMutexGuard<'_, T> {
        // Fast path: try to acquire with a single CAS.
        if self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            return KMutexGuard { mutex: self };
        }

        // Slow path: the lock is held.  Block until available.
        self.lock_slow();
        KMutexGuard { mutex: self }
    }

    /// Try to acquire the mutex without blocking.
    ///
    /// Returns `Some(guard)` if the lock was acquired, `None` if it's
    /// currently held by another task.
    pub fn try_lock(&self) -> Option<KMutexGuard<'_, T>> {
        if self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            Some(KMutexGuard { mutex: self })
        } else {
            None
        }
    }

    /// Try to acquire the mutex with a nanosecond-precision timeout.
    ///
    /// Returns `Some(guard)` if the lock was acquired within `timeout_ns`
    /// nanoseconds, `None` if the timeout expired.  Uses hrtimer for
    /// sub-10ms precision.
    pub fn lock_timeout_ns(&self, timeout_ns: u64) -> Option<KMutexGuard<'_, T>> {
        // Fast path: try immediate CAS.
        if self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            return Some(KMutexGuard { mutex: self });
        }

        // Zero timeout = non-blocking try.
        if timeout_ns == 0 {
            return None;
        }

        // Brief adaptive spin (same as lock_slow).
        for _ in 0..40 {
            if self
                .locked
                .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                return Some(KMutexGuard { mutex: self });
            }
            core::hint::spin_loop();
        }

        // Block with timeout.
        let acquired = self.waiters.wait_timeout_ns(
            || {
                self.locked
                    .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                    .is_ok()
            },
            timeout_ns,
        );

        if acquired {
            Some(KMutexGuard { mutex: self })
        } else {
            None
        }
    }

    /// Whether the mutex is currently locked.
    ///
    /// This is advisory only — the state may change immediately after
    /// this call returns.
    #[must_use]
    #[allow(dead_code)]
    pub fn is_locked(&self) -> bool {
        self.locked.load(Ordering::Relaxed)
    }

    /// How many tasks are asleep waiting for this mutex.
    ///
    /// Advisory, like [`Self::is_locked`]. A task counted here took the
    /// sleeping path: a contender that only spun is never registered, which
    /// is what lets the contention self-test tell the two apart.
    #[must_use]
    pub fn waiter_count(&self) -> usize {
        self.waiters.waiter_count()
    }

    /// Slow path: spin briefly (adaptive), then block on the wait queue.
    ///
    /// The brief spin avoids the overhead of blocking for locks that are
    /// held only momentarily (the holder may release before we even
    /// schedule out).  After a short spin, we commit to sleeping.
    fn lock_slow(&self) {
        // Adaptive spin: try a few CAS attempts before blocking.
        // This helps when the lock holder is on another CPU and will
        // release quickly.  Linux's mutex does something similar.
        for _ in 0..40 {
            if self
                .locked
                .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                return;
            }
            core::hint::spin_loop();
        }

        // The lock is still held after spinning — block.
        self.waiters.wait_until(|| {
            self.locked
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
        });
    }

    /// Release the lock (called by the guard's Drop impl).
    fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
        // Wake one waiter (if any).  The woken task will retry the CAS
        // in its wait_until predicate.
        self.waiters.wake_one();
    }
}

// ---------------------------------------------------------------------------
// Guard
// ---------------------------------------------------------------------------

/// RAII guard for `KMutex`.  Releases the lock on drop.
pub struct KMutexGuard<'a, T> {
    mutex: &'a KMutex<T>,
}

impl<'a, T> KMutexGuard<'a, T> {
    /// Get a reference to the underlying mutex.
    ///
    /// Used by [`CondVar`](super::condvar::CondVar) to re-acquire the
    /// mutex after waking from a wait.
    #[must_use]
    pub fn mutex_ref(&self) -> &'a KMutex<T> {
        self.mutex
    }
}

impl<T> Deref for KMutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: We hold the lock — exclusive access guaranteed.
        unsafe { &*self.mutex.data.get() }
    }
}

impl<T> DerefMut for KMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: We hold the lock — exclusive access guaranteed.
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<T> Drop for KMutexGuard<'_, T> {
    fn drop(&mut self) {
        self.mutex.unlock();
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// The mutex [`self_test_contention`]'s contender sleeps on.  Static, so the
/// contender can never outlive it, however the test ends.
static CONTENDED: KMutex<u64> = KMutex::new(0);

/// Set by the contender just before it asks for [`CONTENDED`].
static CONTENDER_ASKING: AtomicBool = AtomicBool::new(false);

/// Set by the contender once it has held [`CONTENDED`] and released it.
static CONTENDER_DONE: AtomicBool = AtomicBool::new(false);

/// Yields [`self_test_contention`] allows each of its waits: generous for a
/// kernel task at the same priority to be scheduled, park, or finish, and
/// bounded so a regression is a named failure instead of a hung boot.
const CONTENTION_YIELDS: u32 = 4000;

/// [`self_test_contention`]'s second task: take the mutex once, count it, let
/// it go.
extern "C" fn contender(_arg: u64) {
    CONTENDER_ASKING.store(true, Ordering::Release);
    let mut guard = CONTENDED.lock();
    *guard = guard.wrapping_add(1);
    drop(guard);
    CONTENDER_DONE.store(true, Ordering::Release);
}

/// Let the contender finish, however the test went: release is the caller's
/// guard drop; this waits (bounded) for the contender to have run to the end.
fn wait_contender_done() -> bool {
    for _ in 0..CONTENTION_YIELDS {
        if CONTENDER_DONE.load(Ordering::Acquire) {
            return true;
        }
        super::yield_now();
    }
    CONTENDER_DONE.load(Ordering::Acquire)
}

/// Holding a `KMutex` leaves the preempt count where it was.
///
/// Read on one CPU either side of `lock()`; a migration between the reads
/// would compare two CPUs' counts, so the pair is retaken until both reads
/// land on one CPU.
fn preempt_count_unchanged_by_lock() -> Option<(u64, u64)> {
    for _ in 0..8 {
        let cpu = super::current_cpu_id();
        let before = super::preempt_count(cpu);
        let guard = CONTENDED.lock();
        let (cpu_held, held) = {
            let c = super::current_cpu_id();
            (c, super::preempt_count(c))
        };
        drop(guard);
        if cpu_held == cpu {
            return Some((before, held));
        }
    }
    None
}

/// A contended `KMutex` puts its contender to sleep, and unlocking wakes it.
///
/// What `net::socket` relies on since its per-socket locks became `KMutex`es
/// (known-issues `A-SOCKET-LOCKS-WERE-SPINLOCKS-HELD-ACROSS-DAEMON-ROUND-TRIPS`):
/// the holder of a socket may block on the network daemon, and a second task
/// that wants the same socket meanwhile must sleep, not spin.
///
/// 1. Taking the mutex leaves this CPU's preempt count unchanged. A spinlock
///    raises it, and blocking with it raised is the bug the socket locks were
///    moved here to escape.
/// 2. With the mutex held, a second task that asks for it is registered as a
///    waiter and parks (`Blocked`): it went down the sleeping path.
/// 3. It does not get the mutex while this test holds it.
/// 4. Unlocking wakes it, and it takes the mutex.
///
/// Until 2026-09-27 nothing tested any of this; the self-test above is
/// single-task, and its old doc deferred contention to "integration tests"
/// that did not exist.
fn self_test_contention() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;
    use crate::serial_println;

    CONTENDER_ASKING.store(false, Ordering::Release);
    CONTENDER_DONE.store(false, Ordering::Release);
    *CONTENDED.lock() = 0;

    // 1.
    match preempt_count_unchanged_by_lock() {
        Some((before, held)) if before == held => {}
        Some((before, held)) => {
            serial_println!(
                "[kmutex]   FAIL: taking a KMutex moved this CPU's preempt count {} -> {}; \
                 a holder could no longer block safely",
                before,
                held
            );
            return Err(KernelError::InternalError);
        }
        None => {
            serial_println!(
                "[kmutex]   FAIL: the task migrated across every one of 8 lock() calls, so the \
                 preempt count could not be compared on one CPU"
            );
            return Err(KernelError::InternalError);
        }
    }

    // 2. Hold it, then start a task that wants it.
    let guard = CONTENDED.lock();
    let tid = match super::spawn(b"kmutex-contender", 16, contender, 0, 0) {
        Ok(t) => t,
        Err(e) => {
            drop(guard);
            serial_println!("[kmutex]   FAIL: could not start the contender: {:?}", e);
            return Err(e);
        }
    };
    let mut parked = false;
    for _ in 0..CONTENTION_YIELDS {
        if CONTENDER_ASKING.load(Ordering::Acquire)
            && CONTENDED.waiter_count() == 1
            && super::task_state(tid) == Some(super::task::TaskState::Blocked)
        {
            parked = true;
            break;
        }
        super::yield_now();
    }
    // 3. Read before releasing: the contender must not have had it.
    let had_it = CONTENDER_DONE.load(Ordering::Acquire) || *guard != 0;
    let waiters = CONTENDED.waiter_count();
    let state = super::task_state(tid);
    // 4. Release, and it must wake and take the mutex.
    drop(guard);
    let finished = wait_contender_done();
    let count = *CONTENDED.lock();

    if !parked || had_it {
        serial_println!(
            "[kmutex]   FAIL: a contender for a held KMutex should sleep on it without getting \
             it; it asked: {}, waiters: {}, its state: {:?}, it got the mutex anyway: {}",
            CONTENDER_ASKING.load(Ordering::Acquire),
            waiters,
            state,
            had_it
        );
        return Err(KernelError::InternalError);
    }
    if !finished || count != 1 {
        serial_println!(
            "[kmutex]   FAIL: unlocking did not hand the mutex to its sleeping contender \
             (finished: {}, times taken: {})",
            finished,
            count
        );
        return Err(KernelError::InternalError);
    }
    serial_println!(
        "[kmutex]   contention: a holder's preempt count is unchanged, the contender slept \
         (Blocked, 1 waiter) without the mutex, and unlocking woke it to take it: OK"
    );
    Ok(())
}

/// Self-test for the sleeping mutex.
///
/// Single-task acquire/release, `try_lock` and timeout semantics, then
/// [`self_test_contention`] with a second task.
pub fn self_test() -> crate::error::KernelResult<()> {
    use crate::serial_println;

    serial_println!("[kmutex] Running self-test...");

    // --- 1. Basic lock/unlock ---
    let m: KMutex<u64> = KMutex::new(42);
    {
        let mut guard = m.lock();
        assert_eq!(*guard, 42);
        *guard = 100;
    }
    // Lock released — re-acquire should work.
    {
        let guard = m.lock();
        assert_eq!(*guard, 100);
    }
    serial_println!("[kmutex]   Basic lock/unlock: OK");

    // --- 2. try_lock ---
    let m2: KMutex<u32> = KMutex::new(0);
    {
        let _guard = m2.lock();
        // Lock is held — try_lock should fail.
        assert!(m2.try_lock().is_none(), "try_lock should fail when locked");
    }
    // Lock released — try_lock should succeed.
    assert!(
        m2.try_lock().is_some(),
        "try_lock should succeed when unlocked"
    );
    serial_println!("[kmutex]   try_lock: OK");

    // --- 3. is_locked ---
    let m3: KMutex<()> = KMutex::new(());
    assert!(!m3.is_locked());
    {
        let _guard = m3.lock();
        assert!(m3.is_locked());
    }
    assert!(!m3.is_locked());
    serial_println!("[kmutex]   is_locked: OK");

    // --- 4. lock_timeout_ns succeeds when unlocked ---
    {
        let m4: KMutex<u64> = KMutex::new(77);
        let guard = m4.lock_timeout_ns(1_000_000); // 1ms
        assert!(
            guard.is_some(),
            "lock_timeout_ns should succeed on unlocked mutex"
        );
        assert_eq!(*guard.unwrap(), 77);
    }
    serial_println!("[kmutex]   lock_timeout_ns (unlocked): OK");

    // --- 5. lock_timeout_ns with zero timeout (non-blocking try) ---
    {
        let m5: KMutex<u64> = KMutex::new(88);
        // Acquire normally first.
        let _guard = m5.lock();
        // Zero timeout while held → should fail.
        let result = m5.lock_timeout_ns(0);
        assert!(
            result.is_none(),
            "lock_timeout_ns(0) should fail when locked"
        );
    }
    serial_println!("[kmutex]   lock_timeout_ns (zero, locked): OK");

    // --- 6. lock_timeout_ns succeeds immediately after release ---
    {
        let m6: KMutex<u64> = KMutex::new(99);
        {
            let _guard = m6.lock();
            // Lock held here.
        }
        // Lock released — timeout acquire should succeed instantly.
        let guard = m6.lock_timeout_ns(5_000_000); // 5ms
        assert!(guard.is_some());
        assert_eq!(*guard.unwrap(), 99);
    }
    serial_println!("[kmutex]   lock_timeout_ns (after release): OK");

    // --- 7. Contention, with a second task ---
    self_test_contention()?;

    serial_println!("[kmutex] Self-test PASSED");
    Ok(())
}
