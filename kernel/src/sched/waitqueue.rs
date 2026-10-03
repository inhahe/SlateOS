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

/// What an empty waiter slot holds. Not 0: task 0 -- the boot thread, which
/// runs the self-tests -- is a task that waits, and with 0 for "empty" its
/// registration wrote nothing anyone could see. `wake_one` skipped it, the
/// next waiter could take its slot, and the boot thread slept for ever: the
/// hang that stopped every lane A boot since 2026-09-27 at the kmutex
/// no-starvation test. Task ids count up from 0 and never reach `u64::MAX`.
const NO_WAITER: u64 = u64::MAX;

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
            waiters: Mutex::new([NO_WAITER; MAX_WAITERS]),
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
            if let Some(slot) = guard.iter_mut().find(|s| **s == NO_WAITER) {
                *slot = task_id;
                drop(guard);
                break;
            }
            // All slots full — drop lock and yield, then retry.
            drop(guard);
            super::yield_now();
        }

        // Block the current task.  We will be woken by wake_one/wake_all.
        super::block_current_on(crate::wchan::Wait::on(crate::wchan::WaitChannel::Mutex));
    }

    /// Block until a condition is true.
    ///
    /// Registers as a waiter *before* checking the condition, which
    /// prevents lost wakeups.  The sequence is:
    ///
    /// 1. Register in the waiter list.
    /// 2. Check the condition.
    /// 3. If true → unregister and return.
    /// 4. If false → `block_current()`.  Any concurrent `wake_one()`
    ///    between steps 1 and 4 is caught by the `pending_wake` flag
    ///    in the scheduler (see `block_current()`).
    /// 5. After waking, re-check the condition (handles spurious wakeups).
    ///
    /// Must NOT be called from ISR or softirq context.
    pub fn wait_until<F>(&self, condition: F)
    where
        F: Fn() -> bool,
    {
        self.wait_until_woken(|_| condition());
    }

    /// [`Self::wait_until`], telling `condition` whether this task has
    /// already slept on this queue during this wait (`true`) or not yet
    /// (`false`).
    ///
    /// A mutex needs the difference to be fair: it hands itself over only to
    /// a task that has actually waited, never to one that has just arrived --
    /// otherwise the task releasing it could take it straight back, for ever
    /// (see [`super::kmutex::KMutex`]).
    ///
    /// Must NOT be called from ISR or softirq context.
    pub fn wait_until_woken<F>(&self, condition: F)
    where
        F: Fn(bool) -> bool,
    {
        // Fast path: condition already satisfied (no registration needed).
        if condition(false) {
            return;
        }
        let mut woken = false;

        let task_id = super::current_task_id();

        loop {
            // Register as a waiter BEFORE checking the condition.
            // This ensures that if the condition becomes true between
            // our check and blocking, wake_one() will find us and
            // either wake us (if already Blocked) or set pending_wake
            // (if still Running).
            loop {
                let mut guard = self.waiters.lock();
                if let Some(slot) = guard.iter_mut().find(|s| **s == NO_WAITER) {
                    *slot = task_id;
                    drop(guard);
                    break;
                }
                // All slots full — yield and retry.
                drop(guard);
                super::yield_now();
            }

            // Re-check condition now that we're registered.
            if condition(woken) {
                // Condition met — unregister and return.
                let mut guard = self.waiters.lock();
                if let Some(slot) = guard.iter_mut().find(|s| **s == task_id) {
                    *slot = NO_WAITER;
                }
                return;
            }

            // Condition not met — block.  If wake_one() fired between
            // registration and here, pending_wake is set and
            // block_current() returns immediately.
            super::block_current_on(crate::wchan::Wait::on(crate::wchan::WaitChannel::Mutex));
            woken = true;

            // Woken up — remove from waiter list (wake_one may have
            // already cleared our slot, but clear defensively).
            {
                let mut guard = self.waiters.lock();
                if let Some(slot) = guard.iter_mut().find(|s| **s == task_id) {
                    *slot = NO_WAITER;
                }
            }

            // Re-check condition.
            if condition(woken) {
                return;
            }
            // Spurious wakeup — loop back, re-register, re-check.
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
        self.wait_timeout_woken(|_| condition(), timeout_ticks)
    }

    /// [`Self::wait_timeout`], telling `condition` whether this task has
    /// already slept during this wait, as [`Self::wait_until_woken`] does.
    pub fn wait_timeout_woken<F>(&self, condition: F, timeout_ticks: u64) -> bool
    where
        F: Fn(bool) -> bool,
    {
        if condition(false) {
            return true;
        }

        if timeout_ticks == 0 {
            return false;
        }

        let deadline = crate::apic::tick_count().saturating_add(timeout_ticks);

        let task_id = super::current_task_id();
        let mut woken = false;

        loop {
            // Register as a waiter BEFORE checking, same as wait_until.
            {
                let mut guard = self.waiters.lock();
                if let Some(slot) = guard.iter_mut().find(|s| **s == NO_WAITER) {
                    *slot = task_id;
                } else {
                    // Queue full — yield and retry.
                    drop(guard);
                    super::yield_now();
                    if crate::apic::tick_count() >= deadline {
                        return condition(woken);
                    }
                    continue;
                }
            }

            // Re-check condition now that we're registered.
            if condition(woken) {
                let mut guard = self.waiters.lock();
                if let Some(slot) = guard.iter_mut().find(|s| **s == task_id) {
                    *slot = NO_WAITER;
                }
                return true;
            }

            // Sleep with timeout.  If wake_one() fired between
            // registration and here, pending_wake ensures
            // block_current() returns immediately.
            //
            // The *interruptible* variant is required: a `wake_one()` that
            // arrives before the deadline must return control here so the
            // condition is re-checked.  `sleep_until_tick` loops on the
            // clock and would swallow that wake, degrading every
            // `wait_until_timeout` into a full-timeout wait.
            super::sleep_until_tick_interruptible(deadline);
            woken = true;

            // Remove ourselves from the waiter list (we may have been
            // woken by wake_one, or the sleep timed out).
            {
                let mut guard = self.waiters.lock();
                if let Some(slot) = guard.iter_mut().find(|s| **s == task_id) {
                    *slot = NO_WAITER;
                }
            }

            if condition(woken) {
                return true;
            }
            if crate::apic::tick_count() >= deadline {
                return false;
            }
        }
    }

    /// Block until a condition is true, with nanosecond-precision timeout.
    ///
    /// Like [`wait_timeout`] but uses the hrtimer subsystem for sub-10ms
    /// precision.  Falls back to tick-based timeout for very long durations
    /// (> 100ms).
    ///
    /// Returns `true` if the condition was met, `false` if the timeout
    /// expired.  A timeout of 0 means "check once and return immediately."
    pub fn wait_timeout_ns<F>(&self, condition: F, timeout_ns: u64) -> bool
    where
        F: Fn() -> bool,
    {
        self.wait_timeout_ns_woken(|_| condition(), timeout_ns)
    }

    /// [`Self::wait_timeout_ns`], telling `condition` whether this task has
    /// already slept during this wait, as [`Self::wait_until_woken`] does.
    pub fn wait_timeout_ns_woken<F>(&self, condition: F, timeout_ns: u64) -> bool
    where
        F: Fn(bool) -> bool,
    {
        if condition(false) {
            return true;
        }

        if timeout_ns == 0 {
            return false;
        }

        // For long timeouts (> 100ms), delegate to tick-based path.
        if timeout_ns > 100_000_000 {
            let ticks = timeout_ns
                .saturating_add(9_999_999)
                .saturating_div(10_000_000);
            return self.wait_timeout_woken(condition, ticks);
        }

        let deadline_ns = crate::hrtimer::now_ns().saturating_add(timeout_ns);
        let task_id = super::current_task_id();
        let mut woken = false;

        loop {
            // Register as waiter BEFORE checking condition.
            {
                let mut guard = self.waiters.lock();
                if let Some(slot) = guard.iter_mut().find(|s| **s == NO_WAITER) {
                    *slot = task_id;
                } else {
                    // Queue full — yield and retry.
                    drop(guard);
                    super::yield_now();
                    if crate::hrtimer::now_ns() >= deadline_ns {
                        return condition(woken);
                    }
                    continue;
                }
            }

            // Re-check condition after registration.
            if condition(woken) {
                let mut guard = self.waiters.lock();
                if let Some(slot) = guard.iter_mut().find(|s| **s == task_id) {
                    *slot = NO_WAITER;
                }
                return true;
            }

            // Compute remaining time and sleep with hrtimer precision.
            let now_ns = crate::hrtimer::now_ns();
            if now_ns >= deadline_ns {
                // Already past deadline — remove from waiters and check.
                let mut guard = self.waiters.lock();
                if let Some(slot) = guard.iter_mut().find(|s| **s == task_id) {
                    *slot = NO_WAITER;
                }
                return condition(woken);
            }

            let remaining_ns = deadline_ns.saturating_sub(now_ns);
            // Interruptible: an early `wake_one()` must bring us back here
            // to re-check the condition rather than being slept through.
            super::sleep_ns_interruptible(remaining_ns);
            woken = true;

            // Remove ourselves from the waiter list.
            {
                let mut guard = self.waiters.lock();
                if let Some(slot) = guard.iter_mut().find(|s| **s == task_id) {
                    *slot = NO_WAITER;
                }
            }

            if condition(woken) {
                return true;
            }
            if crate::hrtimer::now_ns() >= deadline_ns {
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
        if let Some(slot) = guard.iter_mut().find(|s| **s != NO_WAITER) {
            let task_id = *slot;
            *slot = NO_WAITER;
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
                if *slot != NO_WAITER {
                    if let Some(dest) = ids.get_mut(count) {
                        *dest = *slot;
                    }
                    *slot = NO_WAITER;
                    count = count.saturating_add(1);
                }
            }
        }

        // Wake all collected tasks (lock released).
        let mut woken = 0usize;
        for id in ids.iter().take(count) {
            if super::wake(*id) {
                woken = woken.saturating_add(1);
            }
        }
        woken
    }

    /// Try to wake one task using `try_lock` — safe in hard ISR context.
    ///
    /// Like [`wake_one()`](Self::wake_one) but won't block if the
    /// queue's spinlock is contended.  Returns `true` if a waiter was
    /// dequeued and its wake delivered (see [`super::try_wake`] — that
    /// includes the case where the task had not parked yet and was left a
    /// `pending_wake` token), `false` if the queue was empty or either lock
    /// was held.
    pub fn try_wake_one(&self) -> bool {
        if let Some(mut guard) = self.waiters.try_lock() {
            if let Some(slot) = guard.iter_mut().find(|s| **s != NO_WAITER) {
                let task_id = *slot;
                *slot = NO_WAITER;
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
        guard.iter().filter(|&&id| id != NO_WAITER).count()
    }

    /// Whether the queue has any waiters.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        let guard = self.waiters.lock();
        guard.iter().all(|&id| id == NO_WAITER)
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
pub fn self_test() -> crate::error::KernelResult<()> {
    serial_println!("[waitqueue] Running self-test...");

    // --- 1. Empty queue operations ---
    let wq = WaitQueue::new();
    assert!(wq.is_empty(), "New queue should be empty");
    assert_eq!(wq.waiter_count(), 0);
    assert!(
        !wq.wake_one(),
        "wake_one on empty queue should return false"
    );
    assert_eq!(wq.wake_all(), 0, "wake_all on empty queue should return 0");
    serial_println!("[waitqueue]   Empty queue operations: OK");

    // --- 2. try_wake_one on empty ---
    assert!(
        !wq.try_wake_one(),
        "try_wake_one on empty should return false"
    );
    serial_println!("[waitqueue]   try_wake_one (empty): OK");

    // --- 3. wait_until with already-true condition ---
    static TEST_FLAG: AtomicU64 = AtomicU64::new(1);
    let wq2 = WaitQueue::new();
    wq2.wait_until(|| TEST_FLAG.load(Ordering::Relaxed) != 0);
    serial_println!("[waitqueue]   wait_until (already true): OK");

    // --- 4. wait_timeout with already-true condition ---
    let result = wq2.wait_timeout(|| TEST_FLAG.load(Ordering::Relaxed) != 0, 10);
    assert!(
        result,
        "wait_timeout should return true when condition is met"
    );
    serial_println!("[waitqueue]   wait_timeout (already true): OK");

    // --- 5. wait_timeout with false condition (immediate timeout) ---
    TEST_FLAG.store(0, Ordering::Relaxed);
    let result = wq2.wait_timeout(|| TEST_FLAG.load(Ordering::Relaxed) != 0, 0);
    assert!(
        !result,
        "wait_timeout(0) with false condition should timeout"
    );
    serial_println!("[waitqueue]   wait_timeout (immediate timeout): OK");

    // --- 6. wait_timeout_ns with already-true condition ---
    TEST_FLAG.store(1, Ordering::Relaxed);
    let wq3 = WaitQueue::new();
    let result = wq3.wait_timeout_ns(
        || TEST_FLAG.load(Ordering::Relaxed) != 0,
        1_000_000, // 1ms
    );
    assert!(
        result,
        "wait_timeout_ns should return true when condition is met"
    );
    serial_println!("[waitqueue]   wait_timeout_ns (already true): OK");

    // --- 7. wait_timeout_ns with false condition (immediate timeout) ---
    TEST_FLAG.store(0, Ordering::Relaxed);
    let result = wq3.wait_timeout_ns(|| TEST_FLAG.load(Ordering::Relaxed) != 0, 0);
    assert!(
        !result,
        "wait_timeout_ns(0) with false condition should timeout"
    );
    serial_println!("[waitqueue]   wait_timeout_ns (zero timeout): OK");

    // --- 8. wait_timeout_ns with false condition (real ns timeout) ---
    let result = wq3.wait_timeout_ns(
        || TEST_FLAG.load(Ordering::Relaxed) != 0,
        500_000, // 500µs — should expire
    );
    assert!(
        !result,
        "wait_timeout_ns should return false when timeout expires"
    );
    serial_println!("[waitqueue]   wait_timeout_ns (expired): OK");

    // --- 9. wait_timeout_ns falls back for long timeouts (>100ms) ---
    // Just test the already-true fast path with a >100ms value.
    TEST_FLAG.store(1, Ordering::Relaxed);
    let result = wq3.wait_timeout_ns(
        || TEST_FLAG.load(Ordering::Relaxed) != 0,
        200_000_000, // 200ms — triggers tick-based fallback, but condition is true
    );
    assert!(
        result,
        "wait_timeout_ns long timeout should return true if condition is met"
    );
    serial_println!("[waitqueue]   wait_timeout_ns (long, already true): OK");

    // --- 10. The task running this -- task 0 at boot -- can wait. ---
    self_test_the_boot_thread_waits()?;

    serial_println!("[waitqueue] Self-test PASSED");
    Ok(())
}

/// The queue [`self_test_the_boot_thread_waits`] parks on.
static BOOT_WAIT_QUEUE: WaitQueue = WaitQueue::new();
/// Raised by the waker just before it wakes.
static BOOT_WAIT_FLAG: AtomicU64 = AtomicU64::new(0);
/// Set by the waker if it saw the waiter registered.
static BOOT_WAIT_SEEN: AtomicU64 = AtomicU64::new(0);

/// [`self_test_the_boot_thread_waits`]'s waker: wait (boundedly) until the
/// waiter is visible on the queue, then raise the flag and wake it.
extern "C" fn boot_wait_waker(_arg: u64) {
    for _ in 0..500 {
        if BOOT_WAIT_QUEUE.waiter_count() == 1 {
            BOOT_WAIT_SEEN.store(1, Ordering::Release);
            break;
        }
        super::sleep_ms(1);
    }
    BOOT_WAIT_FLAG.store(1, Ordering::Release);
    BOOT_WAIT_QUEUE.wake_one();
}

/// The task running the boot self-tests -- task 0 -- parks on a queue and a
/// second task, which waits until it can *see* the registration, wakes it.
///
/// Until 2026-10-03 an empty waiter slot was 0, so task 0's registration was
/// invisible: `waiter_count` said 0, `wake_one` passed it by, and a boot
/// thread waiting on a `KMutex` slept for ever -- the hang that stopped every
/// lane A boot since 2026-09-27 at the kmutex no-starvation test. The wait is
/// bounded, so a regression fails here instead of hanging the boot.
fn self_test_the_boot_thread_waits() -> crate::error::KernelResult<()> {
    BOOT_WAIT_FLAG.store(0, Ordering::Release);
    BOOT_WAIT_SEEN.store(0, Ordering::Release);
    let me = super::current_task_id();
    if let Err(e) = super::spawn(b"waitqueue-waker", 16, boot_wait_waker, 0, 0) {
        serial_println!("[waitqueue]   FAIL: could not start the waker: {:?}", e);
        return Err(e);
    }
    let woke = BOOT_WAIT_QUEUE.wait_timeout_ns(
        || BOOT_WAIT_FLAG.load(Ordering::Acquire) != 0,
        3_000_000_000,
    );
    let seen = BOOT_WAIT_SEEN.load(Ordering::Acquire) != 0;
    if !woke || !seen {
        serial_println!(
            "[waitqueue]   FAIL: task {} waiting on a queue: woken {}, its registration \
             seen by the waker {}",
            me,
            woke,
            seen
        );
        return Err(crate::error::KernelError::InternalError);
    }
    serial_println!(
        "[waitqueue]   a waiter that is task {} is seen on the queue and woken: OK",
        me
    );
    Ok(())
}
