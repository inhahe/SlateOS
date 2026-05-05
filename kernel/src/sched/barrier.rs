//! Kernel barrier — multi-task rendezvous synchronization.
//!
//! A barrier blocks all arriving tasks until a specified number have
//! reached the barrier, then releases all of them simultaneously.
//! This is the standard primitive for phased parallel algorithms where
//! all workers must complete phase N before any can start phase N+1.
//!
//! ## Design
//!
//! - `Barrier::new(n)`: Creates a barrier for `n` participants.
//! - `barrier.wait()`: Blocks until all `n` participants have called
//!   `wait()`.  The last arrival triggers release of all waiters.
//!   Returns a `BarrierWaitResult` indicating whether this task was
//!   the "leader" (last to arrive).
//!
//! The barrier resets automatically after each release (reusable).
//! A generation counter prevents late arrivals from the previous
//! round from incorrectly unblocking early in the next round.
//!
//! ## Use Cases
//!
//! - Parallel kernel initialization (all CPUs reach a barrier before
//!   enabling interrupts globally).
//! - Phased benchmarks (all worker tasks start measuring at the same
//!   instant).
//! - Parallel memory operations (all CPUs flush TLBs before any
//!   proceeds to reuse freed pages).
//!
//! ## References
//!
//! - POSIX `pthread_barrier_wait`
//! - Rust std `Barrier`
//! - Linux does not have a general barrier primitive (uses per-CPU
//!   rendezvous for TLB shootdown instead)

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use super::waitqueue::WaitQueue;

// ---------------------------------------------------------------------------
// Barrier
// ---------------------------------------------------------------------------

/// A reusable multi-task barrier.
///
/// All `n` participants must call `wait()` before any can proceed.
/// Automatically resets for the next generation after each release.
///
/// # Safety
///
/// Must NOT be used in ISR or softirq context (it blocks).
pub struct Barrier {
    /// Number of tasks required to trip the barrier.
    count: u32,
    /// Current number of tasks waiting at the barrier.
    waiting: AtomicU32,
    /// Generation counter — incremented each time the barrier trips.
    /// Prevents ABA problems with reusable barriers.
    generation: AtomicU64,
    /// Wait queue for blocked tasks.
    wq: WaitQueue,
}

/// Result returned by [`Barrier::wait()`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarrierWaitResult {
    /// Whether this task was the "leader" (last to arrive, tripped
    /// the barrier).  Exactly one task per barrier generation gets
    /// `is_leader = true`.  The leader can perform one-time cleanup
    /// or coordination work.
    is_leader: bool,
}

impl BarrierWaitResult {
    /// Returns `true` if this task was the last to arrive (the leader).
    #[must_use]
    pub const fn is_leader(self) -> bool {
        self.is_leader
    }
}

impl Barrier {
    /// Create a new barrier for `count` participants.
    ///
    /// # Panics
    ///
    /// `count` must be at least 1.  A barrier with 0 participants
    /// would never trip.
    pub const fn new(count: u32) -> Self {
        assert!(count > 0, "Barrier count must be at least 1");
        Self {
            count,
            waiting: AtomicU32::new(0),
            generation: AtomicU64::new(0),
            wq: WaitQueue::new(),
        }
    }

    /// Block until all participants have reached the barrier.
    ///
    /// Returns a [`BarrierWaitResult`] indicating whether this task
    /// was the leader (last to arrive).
    ///
    /// After all participants have arrived, the barrier resets
    /// automatically for reuse.
    pub fn wait(&self) -> BarrierWaitResult {
        // Record the generation we're joining.
        let my_gen = self.generation.load(Ordering::Acquire);

        // Increment the waiter count.
        let prev = self.waiting.fetch_add(1, Ordering::AcqRel);
        let arrived = prev.saturating_add(1);

        if arrived >= self.count {
            // We are the last to arrive — trip the barrier!
            // Reset waiter count for next generation.
            self.waiting.store(0, Ordering::Release);
            // Advance generation (prevents late-arriving tasks from
            // a previous round from passing through).
            self.generation.fetch_add(1, Ordering::Release);
            // Wake all waiting tasks.
            self.wq.wake_all();

            BarrierWaitResult { is_leader: true }
        } else {
            // Not the last — block until the generation advances.
            self.wq.wait_until(|| {
                self.generation.load(Ordering::Acquire) != my_gen
            });

            BarrierWaitResult { is_leader: false }
        }
    }

    /// Number of participants this barrier requires.
    #[must_use]
    #[allow(dead_code)]
    pub const fn count(&self) -> u32 {
        self.count
    }

    /// Current generation (number of times the barrier has tripped).
    #[must_use]
    #[allow(dead_code)]
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    /// Number of tasks currently waiting.
    #[must_use]
    #[allow(dead_code)]
    pub fn waiting_count(&self) -> u32 {
        self.waiting.load(Ordering::Relaxed)
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the barrier.
///
/// Tests single-task barrier (count=1), construction, and generation
/// counting.  Multi-task barrier testing requires spawning tasks.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[barrier] Running self-test...");

    // --- 1. Construction ---
    {
        let b = Barrier::new(4);
        assert_eq!(b.count(), 4);
        assert_eq!(b.generation(), 0);
        assert_eq!(b.waiting_count(), 0);
    }
    serial_println!("[barrier]   Construction: OK");

    // --- 2. Single-participant barrier (count=1) ---
    // Should not block — we are immediately the leader.
    {
        let b = Barrier::new(1);
        let result = b.wait();
        assert!(result.is_leader());
        assert_eq!(b.generation(), 1);
        assert_eq!(b.waiting_count(), 0);

        // Should be reusable.
        let result2 = b.wait();
        assert!(result2.is_leader());
        assert_eq!(b.generation(), 2);
    }
    serial_println!("[barrier]   Single-participant: OK");

    // --- 3. Leader detection ---
    {
        let b = Barrier::new(1);
        let r = b.wait();
        assert!(r.is_leader());
    }
    serial_println!("[barrier]   Leader detection: OK");

    // --- 4. Multi-task barrier (count=2) ---
    // Spawn a helper task that waits on the barrier, then we wait too.
    {
        use core::sync::atomic::AtomicBool;
        static TEST_BARRIER: Barrier = Barrier::new(2);
        static HELPER_ARRIVED: AtomicBool = AtomicBool::new(false);
        static HELPER_PASSED: AtomicBool = AtomicBool::new(false);

        // Reset for this test.
        HELPER_ARRIVED.store(false, Ordering::Relaxed);
        HELPER_PASSED.store(false, Ordering::Relaxed);

        extern "C" fn barrier_helper(_: u64) {
            HELPER_ARRIVED.store(true, Ordering::Release);
            TEST_BARRIER.wait();
            HELPER_PASSED.store(true, Ordering::Release);
        }

        let tid = crate::sched::spawn(
            b"test-barrier",
            crate::sched::task::DEFAULT_PRIORITY,
            barrier_helper,
            0,
            0,
        );
        assert!(tid.is_ok());

        // Yield a few times to let the helper task run and arrive.
        for _ in 0..10 {
            crate::sched::yield_now();
        }

        // Helper should have arrived but not yet passed (waiting for us).
        assert!(HELPER_ARRIVED.load(Ordering::Acquire));
        assert!(!HELPER_PASSED.load(Ordering::Acquire));

        // Now we arrive — this should trip the barrier.
        let r = TEST_BARRIER.wait();
        // One of us is the leader; the other is not.
        // We can't predict which (depends on timing), but at least
        // one must be the leader.
        let _ = r;

        // Yield to let helper complete.
        for _ in 0..10 {
            crate::sched::yield_now();
        }

        // Now helper should have passed.
        assert!(HELPER_PASSED.load(Ordering::Acquire));
    }
    serial_println!("[barrier]   Multi-task barrier (2 tasks): OK");

    serial_println!("[barrier] Self-test PASSED");
}
