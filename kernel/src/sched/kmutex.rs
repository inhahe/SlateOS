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

/// Self-test for the sleeping mutex.
///
/// Tests single-threaded acquire/release and try_lock semantics.
/// Multi-task contention testing requires spawning tasks (done
/// separately in integration tests).
pub fn self_test() {
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
    assert!(m2.try_lock().is_some(), "try_lock should succeed when unlocked");
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
        assert!(guard.is_some(), "lock_timeout_ns should succeed on unlocked mutex");
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
        assert!(result.is_none(), "lock_timeout_ns(0) should fail when locked");
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

    serial_println!("[kmutex] Self-test PASSED");
}
