//! Kernel counting semaphore.
//!
//! A semaphore allows up to N concurrent accessors to a resource.
//! Unlike a mutex (which allows exactly one), a semaphore with count=5
//! allows 5 tasks to proceed simultaneously; the 6th blocks until one
//! of the first 5 releases.
//!
//! ## Use Cases
//!
//! - **Bounded resources**: Limit concurrent disk I/O operations,
//!   network connections, or DMA channels.
//! - **Producer/consumer**: Count of available items in a queue.
//!   Producer signals (increments), consumer waits (decrements).
//! - **Binary semaphore** (count=1): Acts like a mutex but without
//!   ownership — any task can signal, not just the holder.
//!
//! ## Design
//!
//! An atomic counter + WaitQueue.  The fast path (no contention) is a
//! single atomic decrement with a compare-exchange loop.  Only when the
//! count reaches zero does a task block on the wait queue.
//!
//! ## References
//!
//! - Linux `kernel/locking/semaphore.c` — `down()`, `up()`
//! - Dijkstra's original P (proberen) and V (verhogen) operations

use core::sync::atomic::{AtomicI64, Ordering};

use super::waitqueue::WaitQueue;
use crate::serial_println;

// ---------------------------------------------------------------------------
// Semaphore
// ---------------------------------------------------------------------------

/// A counting semaphore.
///
/// Allows up to `count` concurrent acquisitions.  When the count
/// reaches zero, subsequent acquirers block until someone releases.
pub struct Semaphore {
    /// Current count (may go negative when tasks are waiting — negative
    /// count = number of blocked waiters).
    count: AtomicI64,
    /// Tasks waiting to acquire.
    waiters: WaitQueue,
}

impl Semaphore {
    /// Create a new semaphore with the given initial count.
    ///
    /// - `count = 0`: All acquires will block until someone signals.
    /// - `count = 1`: Binary semaphore (similar to mutex).
    /// - `count = N`: Up to N concurrent holders.
    pub const fn new(count: i64) -> Self {
        Self {
            count: AtomicI64::new(count),
            waiters: WaitQueue::new(),
        }
    }

    /// Acquire the semaphore (P / `down` / `wait`).
    ///
    /// Decrements the count.  If the result is negative, blocks until
    /// another task calls [`signal()`](Self::signal).
    ///
    /// Must NOT be called from ISR or softirq context.
    pub fn wait(&self) {
        // Try to decrement atomically.
        let old = self.count.fetch_sub(1, Ordering::AcqRel);
        if old > 0 {
            // Fast path: count was positive, we acquired without blocking.
            return;
        }

        // Count was zero or negative — we need to block.
        // The fetch_sub already decremented (count is now more negative),
        // indicating we're a waiter.
        self.waiters.wait_until(|| {
            // Try to "reclaim" a positive count.
            self.try_acquire_internal()
        });
    }

    /// Try to acquire without blocking.
    ///
    /// Returns `true` if acquired, `false` if the semaphore has no
    /// available permits (count ≤ 0).
    pub fn try_wait(&self) -> bool {
        loop {
            let current = self.count.load(Ordering::Acquire);
            if current <= 0 {
                return false;
            }
            if self
                .count
                .compare_exchange_weak(
                    current,
                    current - 1,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                return true;
            }
            // CAS failed (concurrent modification) — retry.
        }
    }

    /// Wait with nanosecond-precision timeout (P / `down` with deadline).
    ///
    /// Returns `true` if the semaphore was acquired within `timeout_ns`
    /// nanoseconds, `false` if the timeout expired.
    pub fn wait_timeout_ns(&self, timeout_ns: u64) -> bool {
        // Fast path: try to decrement immediately.
        if self.try_wait() {
            return true;
        }
        if timeout_ns == 0 {
            return false;
        }

        // Decrement count to indicate we're waiting (may go negative).
        self.count.fetch_sub(1, Ordering::AcqRel);

        // Block with timeout until count becomes positive for us.
        let acquired = self.waiters.wait_timeout_ns(
            || {
                // Try to "claim" our decrement by checking if count > our
                // reservation.  When signaled, count is incremented.
                let current = self.count.load(Ordering::Acquire);
                current >= 0
            },
            timeout_ns,
        );

        if !acquired {
            // Timeout expired — undo our decrement.
            self.count.fetch_add(1, Ordering::AcqRel);
        }
        acquired
    }

    /// Release the semaphore (V / `up` / `signal` / `post`).
    ///
    /// Increments the count.  If tasks are waiting (count was negative
    /// or zero before increment), wakes one.
    ///
    /// Safe to call from any context (including ISR via the WaitQueue's
    /// try_wake_one path).
    pub fn signal(&self) {
        let old = self.count.fetch_add(1, Ordering::AcqRel);
        if old < 0 {
            // There are blocked waiters (count was negative = |count| waiters).
            // Wake one so it can re-try acquisition.
            self.waiters.wake_one();
        }
    }

    /// Current count (may be negative if waiters are blocked).
    ///
    /// Positive: number of available permits.
    /// Zero: no permits available, no waiters.
    /// Negative: |count| tasks are waiting.
    #[must_use]
    #[allow(dead_code)]
    pub fn count(&self) -> i64 {
        self.count.load(Ordering::Relaxed)
    }

    /// Number of tasks currently waiting.
    #[must_use]
    #[allow(dead_code)]
    pub fn waiters(&self) -> usize {
        self.waiters.waiter_count()
    }

    /// Internal: try to atomically acquire (decrement from positive).
    fn try_acquire_internal(&self) -> bool {
        loop {
            let current = self.count.load(Ordering::Acquire);
            if current <= 0 {
                return false;
            }
            if self
                .count
                .compare_exchange_weak(
                    current,
                    current - 1,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                return true;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the counting semaphore.
pub fn self_test() {
    serial_println!("[semaphore] Running self-test...");

    // --- 1. Basic acquire/release ---
    let sem = Semaphore::new(3);
    assert_eq!(sem.count(), 3);

    sem.wait(); // count: 3 → 2
    assert_eq!(sem.count(), 2);
    sem.wait(); // count: 2 → 1
    assert_eq!(sem.count(), 1);
    sem.wait(); // count: 1 → 0
    assert_eq!(sem.count(), 0);

    // Release all three.
    sem.signal(); // count: 0 → 1
    assert_eq!(sem.count(), 1);
    sem.signal(); // count: 1 → 2
    sem.signal(); // count: 2 → 3
    assert_eq!(sem.count(), 3);
    serial_println!("[semaphore]   Basic acquire/release: OK");

    // --- 2. try_wait ---
    let sem2 = Semaphore::new(1);
    assert!(sem2.try_wait(), "try_wait should succeed on count=1");
    assert_eq!(sem2.count(), 0);
    assert!(!sem2.try_wait(), "try_wait should fail on count=0");
    assert_eq!(sem2.count(), 0);
    sem2.signal();
    assert_eq!(sem2.count(), 1);
    serial_println!("[semaphore]   try_wait: OK");

    // --- 3. Zero-initialized semaphore ---
    let sem3 = Semaphore::new(0);
    assert!(!sem3.try_wait(), "try_wait should fail on count=0");
    sem3.signal(); // count: 0 → 1
    assert!(sem3.try_wait(), "try_wait should succeed after signal");
    serial_println!("[semaphore]   Zero-init + signal: OK");

    // --- 4. Over-signal (count can exceed initial) ---
    let sem4 = Semaphore::new(1);
    sem4.signal(); // count: 1 → 2
    sem4.signal(); // count: 2 → 3
    assert_eq!(sem4.count(), 3);
    assert!(sem4.try_wait());
    assert!(sem4.try_wait());
    assert!(sem4.try_wait());
    assert!(!sem4.try_wait());
    serial_println!("[semaphore]   Over-signal: OK");

    serial_println!("[semaphore] Self-test PASSED");
}
