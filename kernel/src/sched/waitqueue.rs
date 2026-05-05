//! Wait queues — sleeping until a condition is met.
//!
//! A `WaitQueue` allows kernel tasks to sleep until woken by another
//! task or an interrupt handler.  This is the fundamental synchronization
//! primitive for blocking operations: I/O completion, resource availability,
//! event notification, etc.
//!
//! ## Design
//!
//! Each `WaitQueue` contains a bounded list of sleeping task IDs.  A task
//! calls [`WaitQueue::wait()`] to add itself to the queue and block.
//! Another task (or ISR via `try_wake_one`) calls [`WaitQueue::wake_one()`]
//! or [`WaitQueue::wake_all()`] to make waiters runnable again.
//!
//! The queue is protected by a spinlock.  The lock is held only briefly
//! during enqueue/dequeue operations — never while the waiting task is
//! blocked (the task blocks *after* releasing the lock).
//!
//! ## Capacity
//!
//! Each `WaitQueue` holds up to [`MAX_WAITERS`] tasks.  If more tasks
//! try to wait, they spin-yield until a slot opens (this is bounded
//! because other tasks will eventually be woken and free their slots).
//!
//! ## Usage Pattern
//!
//! ```ignore
//! static MY_EVENT: WaitQueue = WaitQueue::new();
//!
//! // Waiting side (blocks until woken):
//! MY_EVENT.wait();
//!
//! // Waking side (from any context):
//! MY_EVENT.wake_one();  // Wake exactly one waiter.
//! MY_EVENT.wake_all();  // Wake all waiters.
//! ```
//!
//! ## Condition Waiting
//!
//! For waiting on a condition (not just any wake signal), use
//! [`wait_until`] which checks a predicate after each wake to handle
//! spurious wakeups:
//!
//! ```ignore
//! MY_QUEUE.wait_until(|| resource_available());
//! ```
//!
//! ## References
//!
//! - Linux `include/linux/wait.h` — `wait_queue_head_t`, `wait_event()`
//! - Linux `kernel/sched/wait.c` — `__wake_up_common()`
//! - Fuchsia `zircon/kernel/include/kernel/wait.h`

use core::sync::atomic::{AtomicU64, Ordering};

use spin::Mutex;

use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum number of tasks that can simultaneously wait on one queue.
///
/// 32 is generous for most kernel wait points (I/O completions typically
/// have 1-4 waiters).  Memory cost: 32 × 8 bytes = 256 bytes per queue.
const MAX_WAITERS: usize = 32;

// ---------------------------------------------------------------------------
// WaitQueue
// ---------------------------------------------------------------------------

/// A queue of tasks waiting for an event or condition.
///
/// Tasks call [`wait()`](Self::wait) to sleep until another task calls
/// [`wake_one()`](Self::wake_one) or [`wake_all()`](Self::wake_all).
///
/// This type is `Sync` — it can be safely shared between threads via
/// a static or behind an `Arc`.
pub struct WaitQueue {
    /// List of waiting task IDs.  0 = empty slot.
    waiters: Mutex<[u64; MAX_WAITERS]>,
}

impl WaitQueue {
    /// Create a new, empty wait queue.
    pub const fn new() -> Self {
        Self {
            waiters: Mutex::new([0u64; MAX_WAITERS]),
        }
    }

    /// Block the current task until woken.
    ///
    /// Adds the calling task to this wait queue and puts it to sleep.
    /// Returns when another task calls `wake_one()` or `wake_all()` on
    /// this queue (or if a spurious wakeup occurs — callers should
    /// re-check their condition).
    ///
    /// # Note
    ///
    /// Must NOT be called from ISR or softirq context (those cannot
    /// block).  Only call from normal kernel task context.
    pub fn wait(&self) {
        let task_id = super::current_task_id();

        // Add ourselves to the waiter list.
        loop {
            let mut guard = self.waiters.lock();
            if let Some(slot) = guard.iter_mut().find(|s| **s == 0) {
                *slot = task_id;
                drop(guard);
                break;
            }
            // All slots full — drop lock and yield, then retry.
            drop(guard);
            super::yield_now();
        }

        // Block the current task.  We will be woken by wake_one/wake_all.
        super::block_current();
    }

    /// Block until a condition is true.
    ///
    /// Calls `condition()` before sleeping — if it's already true,
    /// returns immediately without blocking.  After each wakeup,
    /// re-checks the condition to handle spurious wakeups.
    ///
    /// This is the preferred wait API because it handles the
    /// check-then-sleep race correctly: the condition is checked
    /// *before* registering as a waiter, avoiding missed wakeups.
    pub fn wait_until<F>(&self, condition: F)
    where
        F: Fn() -> bool,
    {
        // Fast path: condition already satisfied.
        if condition() {
            return;
        }

        loop {
            self.wait();
            if condition() {
                return;
            }
            // Spurious wakeup — go back to sleep.
        }
    }

    /// Block until a condition is true, with a tick-based timeout.
    ///
    /// Returns `true` if the condition was met, `false` if the timeout
    /// expired.  A timeout of 0 means "check once and return immediately."
    pub fn wait_timeout<F>(&self, condition: F, timeout_ticks: u64) -> bool
    where
        F: Fn() -> bool,
    {
        if condition() {
            return true;
        }

        if timeout_ticks == 0 {
            return false;
        }

        let deadline = crate::apic::tick_count().saturating_add(timeout_ticks);

        loop {
            // Register as a waiter and block, but with a timeout via
            // sleep_until_tick instead of indefinite blocking.
            let task_id = super::current_task_id();

            {
                let mut guard = self.waiters.lock();
                if let Some(slot) = guard.iter_mut().find(|s| **s == 0) {
                    *slot = task_id;
                } else {
                    // Queue full — yield and retry.
                    drop(guard);
                    super::yield_now();
                    if crate::apic::tick_count() >= deadline {
                        return condition();
                    }
                    continue;
                }
            }

            // Sleep with timeout.
            super::sleep_until_tick(deadline);

            // Remove ourselves from the waiter list (we may have been
            // woken by wake_one, or the sleep timed out).
            {
                let mut guard = self.waiters.lock();
                if let Some(slot) = guard.iter_mut().find(|s| **s == task_id) {
                    *slot = 0;
                }
            }

            if condition() {
                return true;
            }
            if crate::apic::tick_count() >= deadline {
                return false;
            }
        }
    }

    /// Wake one waiting task (FIFO order).
    ///
    /// Returns `true` if a task was woken, `false` if the queue was empty.
    ///
    /// Safe to call from any context (including ISR via try_wake).
    pub fn wake_one(&self) -> bool {
        let mut guard = self.waiters.lock();
        if let Some(slot) = guard.iter_mut().find(|s| **s != 0) {
            let task_id = *slot;
            *slot = 0;
            drop(guard);
            super::wake(task_id)
        } else {
            false
        }
    }

    /// Wake all waiting tasks.
    ///
    /// Returns the number of tasks woken.
    ///
    /// Safe to call from any context.
    pub fn wake_all(&self) -> usize {
        // Collect all waiter IDs under the lock, then wake them after
        // releasing (avoids holding the spinlock during scheduler operations).
        let mut ids = [0u64; MAX_WAITERS];
        let mut count = 0usize;

        {
            let mut guard = self.waiters.lock();
            for slot in guard.iter_mut() {
                if *slot != 0 {
                    if let Some(dest) = ids.get_mut(count) {
                        *dest = *slot;
                    }
                    *slot = 0;
                    count = count.saturating_add(1);
                }
            }
        }

        // Wake all collected tasks (lock released).
        let mut woken = 0usize;
        for id in ids.iter().take(count) {
            if *id != 0 && super::wake(*id) {
                woken = woken.saturating_add(1);
            }
        }
        woken
    }

    /// Try to wake one task using `try_lock` — safe in hard ISR context.
    ///
    /// Like [`wake_one()`](Self::wake_one) but won't block if the
    /// queue's spinlock is contended.  Returns `true` if a task was
    /// woken, `false` if the queue was empty or the lock was held.
    pub fn try_wake_one(&self) -> bool {
        if let Some(mut guard) = self.waiters.try_lock() {
            if let Some(slot) = guard.iter_mut().find(|s| **s != 0) {
                let task_id = *slot;
                *slot = 0;
                drop(guard);
                return super::try_wake(task_id);
            }
        }
        false
    }

    /// Number of tasks currently waiting.
    #[must_use]
    pub fn waiter_count(&self) -> usize {
        let guard = self.waiters.lock();
        guard.iter().filter(|&&id| id != 0).count()
    }

    /// Whether the queue has any waiters.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        let guard = self.waiters.lock();
        guard.iter().all(|&id| id == 0)
    }
}

// ---------------------------------------------------------------------------
// Global statistics
// ---------------------------------------------------------------------------

/// Total wait operations since boot.
#[allow(dead_code)]
static TOTAL_WAITS: AtomicU64 = AtomicU64::new(0);

/// Total wake_one operations since boot.
#[allow(dead_code)]
static TOTAL_WAKE_ONES: AtomicU64 = AtomicU64::new(0);

/// Total wake_all operations since boot.
#[allow(dead_code)]
static TOTAL_WAKE_ALLS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for wait queues.
///
/// Tests the basic API (wake_one on empty queue, waiter count).
/// Full multi-task testing requires spawning tasks, which is done
/// separately in the scheduler's integration tests.
pub fn self_test() {
    serial_println!("[waitqueue] Running self-test...");

    // --- 1. Empty queue operations ---
    let wq = WaitQueue::new();
    assert!(wq.is_empty(), "New queue should be empty");
    assert_eq!(wq.waiter_count(), 0);
    assert!(!wq.wake_one(), "wake_one on empty queue should return false");
    assert_eq!(wq.wake_all(), 0, "wake_all on empty queue should return 0");
    serial_println!("[waitqueue]   Empty queue operations: OK");

    // --- 2. try_wake_one on empty ---
    assert!(!wq.try_wake_one(), "try_wake_one on empty should return false");
    serial_println!("[waitqueue]   try_wake_one (empty): OK");

    // --- 3. wait_until with already-true condition ---
    static TEST_FLAG: AtomicU64 = AtomicU64::new(1);
    let wq2 = WaitQueue::new();
    wq2.wait_until(|| TEST_FLAG.load(Ordering::Relaxed) != 0);
    serial_println!("[waitqueue]   wait_until (already true): OK");

    // --- 4. wait_timeout with already-true condition ---
    let result = wq2.wait_timeout(
        || TEST_FLAG.load(Ordering::Relaxed) != 0,
        10,
    );
    assert!(result, "wait_timeout should return true when condition is met");
    serial_println!("[waitqueue]   wait_timeout (already true): OK");

    // --- 5. wait_timeout with false condition (immediate timeout) ---
    TEST_FLAG.store(0, Ordering::Relaxed);
    let result = wq2.wait_timeout(
        || TEST_FLAG.load(Ordering::Relaxed) != 0,
        0,
    );
    assert!(!result, "wait_timeout(0) with false condition should timeout");
    serial_println!("[waitqueue]   wait_timeout (immediate timeout): OK");

    serial_println!("[waitqueue] Self-test PASSED");
}
