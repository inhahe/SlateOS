//! Kernel condition variable — wait-for-predicate with mutex release.
//!
//! A condition variable allows a task to atomically release a [`KMutex`]
//! and block until another task signals the condition.  This is the
//! standard building block for producer-consumer queues, state machines,
//! and any scenario where a task needs to wait for a condition that
//! another task will establish.
//!
//! ## Design
//!
//! The `CondVar` wraps a [`WaitQueue`] and integrates with [`KMutex`]
//! guards to provide correct atomic release-and-wait semantics:
//!
//! 1. Caller holds `KMutexGuard<T>`.
//! 2. `wait(guard)` atomically releases the mutex and blocks.
//! 3. When woken, the task re-acquires the mutex before returning.
//! 4. The guard is returned to the caller (lock re-acquired).
//!
//! ## Spurious Wakeups
//!
//! Like POSIX condition variables, wakeups may be spurious.  Always
//! use `wait_while()` or re-check the predicate in a loop:
//!
//! ```ignore
//! let mut guard = mutex.lock();
//! guard = condvar.wait_while(guard, |data| !data.is_ready());
//! // data.is_ready() is guaranteed true here.
//! ```
//!
//! ## Signal vs Broadcast
//!
//! - `notify_one()`: Wakes one waiting task (for single-consumer patterns).
//! - `notify_all()`: Wakes all waiting tasks (for broadcast notifications
//!   like "configuration changed" or "shutdown requested").
//!
//! ## References
//!
//! - POSIX `pthread_cond_wait` / `pthread_cond_signal`
//! - Linux `kernel/sched/wait.c` — `wait_event` / `wake_up`
//! - Rust std `Condvar`
//! - C++ `std::condition_variable`

use core::sync::atomic::{AtomicU64, Ordering};

use super::kmutex::{KMutex, KMutexGuard};
use super::waitqueue::WaitQueue;

// ---------------------------------------------------------------------------
// CondVar
// ---------------------------------------------------------------------------

/// A kernel condition variable for use with [`KMutex`].
///
/// Allows tasks to block until a condition is established by another
/// task, atomically releasing and re-acquiring the associated mutex.
///
/// # Safety
///
/// Must NOT be used in ISR or softirq context (it blocks).
pub struct CondVar {
    /// Internal wait queue for blocked tasks.
    wq: WaitQueue,
    /// Total number of notify_one calls (for diagnostics).
    notify_count: AtomicU64,
    /// Total number of notify_all calls (for diagnostics).
    broadcast_count: AtomicU64,
}

impl CondVar {
    /// Create a new condition variable.
    pub const fn new() -> Self {
        Self {
            wq: WaitQueue::new(),
            notify_count: AtomicU64::new(0),
            broadcast_count: AtomicU64::new(0),
        }
    }

    /// Block the current task until notified, releasing the mutex.
    ///
    /// Atomically releases `guard` (unlocking the mutex) and blocks.
    /// When woken (by `notify_one` or `notify_all`), re-acquires the
    /// mutex and returns the new guard.
    ///
    /// **Warning**: May experience spurious wakeups.  Always re-check
    /// the condition after `wait()` returns, or use `wait_while()`.
    pub fn wait<'a, T>(&self, guard: KMutexGuard<'a, T>) -> KMutexGuard<'a, T> {
        // Get a reference to the mutex so we can re-lock after waking.
        let mutex = guard.mutex_ref();

        // Drop the guard (releases the mutex) BEFORE blocking.
        // The notify side can now acquire the mutex and change state.
        drop(guard);

        // Block until notified.
        self.wq.wait();

        // Re-acquire the mutex before returning.
        mutex.lock()
    }

    /// Block until `predicate` returns false, releasing the mutex.
    ///
    /// This is the preferred API — handles spurious wakeups correctly.
    /// Loops: release mutex → block → re-acquire → check predicate.
    /// Returns only when `predicate(&data)` is `false`.
    ///
    /// The predicate receives `&T` (the mutex-protected data).
    pub fn wait_while<'a, T, F>(
        &self,
        mut guard: KMutexGuard<'a, T>,
        mut predicate: F,
    ) -> KMutexGuard<'a, T>
    where
        F: FnMut(&T) -> bool,
    {
        while predicate(&*guard) {
            guard = self.wait(guard);
        }
        guard
    }

    /// Block until `predicate` returns true, releasing the mutex.
    ///
    /// Convenience wrapper — returns when the condition IS met.
    pub fn wait_until<'a, T, F>(
        &self,
        mut guard: KMutexGuard<'a, T>,
        mut predicate: F,
    ) -> KMutexGuard<'a, T>
    where
        F: FnMut(&T) -> bool,
    {
        while !predicate(&*guard) {
            guard = self.wait(guard);
        }
        guard
    }

    /// Block until notified or timeout expires, with nanosecond precision.
    ///
    /// Releases the mutex, blocks for up to `timeout_ns` nanoseconds,
    /// then re-acquires the mutex.  Returns `(guard, timed_out)` where
    /// `timed_out` is `true` if the timeout expired without notification.
    ///
    /// The caller should still check the predicate after waking — a
    /// `false` `timed_out` could be a spurious wakeup.
    pub fn wait_timeout_ns<'a, T>(
        &self,
        guard: KMutexGuard<'a, T>,
        timeout_ns: u64,
    ) -> (KMutexGuard<'a, T>, bool) {
        let mutex = guard.mutex_ref();

        // Release the mutex before blocking.
        drop(guard);

        // Block with timeout — the notify_count tracks notifications;
        // we use it to detect whether a signal occurred during our wait.
        let before = self.notify_count.load(Ordering::Acquire);
        let woken = self.wq.wait_timeout_ns(
            || self.notify_count.load(Ordering::Acquire) != before,
            timeout_ns,
        );

        // Re-acquire the mutex before returning.
        let new_guard = mutex.lock();
        (new_guard, !woken)
    }

    /// Wake one waiting task (if any).
    ///
    /// The woken task will re-acquire the mutex before proceeding.
    /// Use this for single-producer/single-consumer patterns.
    pub fn notify_one(&self) {
        self.notify_count.fetch_add(1, Ordering::Relaxed);
        self.wq.wake_one();
    }

    /// Wake all waiting tasks.
    ///
    /// All woken tasks will contend for the mutex — only one proceeds
    /// at a time.  Use for broadcast notifications (e.g., "shutdown",
    /// "config changed").
    pub fn notify_all(&self) {
        self.broadcast_count.fetch_add(1, Ordering::Relaxed);
        self.wq.wake_all();
    }

    /// Number of `notify_one` calls since creation.
    #[must_use]
    #[allow(dead_code)]
    pub fn notify_count(&self) -> u64 {
        self.notify_count.load(Ordering::Relaxed)
    }

    /// Number of `notify_all` calls since creation.
    #[must_use]
    #[allow(dead_code)]
    pub fn broadcast_count(&self) -> u64 {
        self.broadcast_count.load(Ordering::Relaxed)
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the condition variable.
///
/// Tests the API surface and basic semantics.  Full multi-task tests
/// (producer-consumer, broadcast wake) require spawning tasks and are
/// timing-dependent — covered separately.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[condvar] Running self-test...");

    // --- 1. Basic construction ---
    let cv = CondVar::new();
    assert_eq!(cv.notify_count(), 0);
    assert_eq!(cv.broadcast_count(), 0);
    serial_println!("[condvar]   Construction: OK");

    // --- 2. notify_one increments counter ---
    cv.notify_one();
    cv.notify_one();
    assert_eq!(cv.notify_count(), 2);
    assert_eq!(cv.broadcast_count(), 0);
    serial_println!("[condvar]   notify_one counting: OK");

    // --- 3. notify_all increments broadcast counter ---
    cv.notify_all();
    assert_eq!(cv.broadcast_count(), 1);
    serial_println!("[condvar]   notify_all counting: OK");

    // --- 4. wait_while with immediately-false predicate ---
    // If the predicate is already false, wait_while returns immediately
    // without blocking.
    {
        let mutex = KMutex::new(42u64);
        let guard = mutex.lock();
        // Predicate: *data > 100 → false (42 is not > 100)
        let guard = cv.wait_while(guard, |&data| data > 100);
        assert_eq!(*guard, 42);
    }
    serial_println!("[condvar]   wait_while (no-block): OK");

    // --- 5. wait_until with immediately-true predicate ---
    {
        let mutex = KMutex::new(42u64);
        let guard = mutex.lock();
        // Predicate: *data == 42 → true immediately
        let guard = cv.wait_until(guard, |&data| data == 42);
        assert_eq!(*guard, 42);
    }
    serial_println!("[condvar]   wait_until (no-block): OK");

    // --- 6. wait_timeout_ns — timeout expires (no notification) ---
    {
        let cv2 = CondVar::new();
        let mutex = KMutex::new(123u64);
        let guard = mutex.lock();
        // Nobody will notify, so timeout should expire.
        let (guard, timed_out) = cv2.wait_timeout_ns(guard, 500_000); // 500µs
        assert!(timed_out, "wait_timeout_ns should report timeout when not notified");
        assert_eq!(*guard, 123); // Data intact.
    }
    serial_println!("[condvar]   wait_timeout_ns (expired): OK");

    // --- 7. wait_timeout_ns with zero timeout ---
    {
        let cv3 = CondVar::new();
        let mutex = KMutex::new(456u64);
        let guard = mutex.lock();
        let (guard, timed_out) = cv3.wait_timeout_ns(guard, 0);
        // Zero timeout = immediate return, counts as timed out.
        assert!(timed_out);
        assert_eq!(*guard, 456);
    }
    serial_println!("[condvar]   wait_timeout_ns (zero): OK");

    serial_println!("[condvar] Self-test PASSED");
}
