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
            self.wq
                .wait_until(|| self.generation.load(Ordering::Acquire) != my_gen);

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

/// Self-test for the barrier: construction, a one-participant barrier (never
/// blocks, always the leader, reusable), and a two-task rendezvous with a
/// spawned helper.
///
/// A failed check is returned as an error, not asserted: a self-test that
/// panics takes the whole boot down with it, and the boot's report of which
/// test failed is worth more than the stop.
///
/// The rendezvous waits for the helper by the clock ([`wait_until`]), never by
/// a count of yields. Ten yields was the old wait, and it was one only on one
/// CPU, where a yield runs the helper. On two the helper runs on the other
/// CPU and the yields return at once: the first two-CPU release boot
/// (2026-10-09) checked `HELPER_PASSED` while the helper was still on its way
/// out of the barrier, and panicked.
///
/// [`wait_until`]: crate::selftest::wait_until
pub fn self_test() -> crate::error::KernelResult<()> {
    use crate::serial_println;
    use core::sync::atomic::AtomicBool;

    /// How long the helper may take to arrive, or to pass once released:
    /// far beyond a working kernel's microseconds, and far short of a hung
    /// boot.
    const PATIENCE_MS: u64 = 5_000;

    fn fail(what: &str) -> crate::error::KernelResult<()> {
        crate::serial_println!("[barrier]   FAIL: {}", what);
        Err(crate::error::KernelError::InternalError)
    }

    serial_println!("[barrier] Running self-test...");

    // --- 1. Construction ---
    let b = Barrier::new(4);
    if b.count() != 4 || b.generation() != 0 || b.waiting_count() != 0 {
        return fail("a new barrier for four is not at generation 0 with nobody waiting");
    }
    serial_println!("[barrier]   Construction: OK");

    // --- 2. Single-participant barrier (count=1) ---
    // Never blocks: every arrival is the last, so the leader.
    let b = Barrier::new(1);
    let first = b.wait();
    if !first.is_leader() || b.generation() != 1 || b.waiting_count() != 0 {
        return fail("a one-participant barrier did not trip at once with its caller as leader");
    }
    // Reusable.
    let second = b.wait();
    if !second.is_leader() || b.generation() != 2 {
        return fail("a one-participant barrier did not trip a second time");
    }
    serial_println!("[barrier]   Single-participant: OK");

    // --- 3. Multi-task barrier (count=2) ---
    // A helper arrives first and must wait for us; our arrival releases it.
    static TEST_BARRIER: Barrier = Barrier::new(2);
    static HELPER_ARRIVED: AtomicBool = AtomicBool::new(false);
    static HELPER_PASSED: AtomicBool = AtomicBool::new(false);
    static HELPER_LED: AtomicBool = AtomicBool::new(false);

    HELPER_ARRIVED.store(false, Ordering::Relaxed);
    HELPER_PASSED.store(false, Ordering::Relaxed);
    HELPER_LED.store(false, Ordering::Relaxed);

    extern "C" fn barrier_helper(_: u64) {
        HELPER_ARRIVED.store(true, Ordering::Release);
        let r = TEST_BARRIER.wait();
        HELPER_LED.store(r.is_leader(), Ordering::Relaxed);
        HELPER_PASSED.store(true, Ordering::Release);
    }

    if crate::sched::spawn(
        b"test-barrier",
        crate::sched::task::DEFAULT_PRIORITY,
        barrier_helper,
        0,
        0,
    )
    .is_err()
    {
        return fail("could not spawn the helper task");
    }
    if !crate::selftest::wait_until(PATIENCE_MS, || HELPER_ARRIVED.load(Ordering::Acquire)) {
        return fail("the helper never reached the barrier");
    }
    // Arrived (or about to), and held there: nothing passes a barrier for two
    // until the second arrives, and that is us.
    if HELPER_PASSED.load(Ordering::Acquire) {
        return fail("the helper passed a barrier for two on its own");
    }
    let ours = TEST_BARRIER.wait();
    if !crate::selftest::wait_until(PATIENCE_MS, || HELPER_PASSED.load(Ordering::Acquire)) {
        return fail("the helper was not released when the second task arrived");
    }
    // Exactly one of the two is the leader -- which one depends on whether
    // the helper had counted itself in before we arrived.
    if ours.is_leader() == HELPER_LED.load(Ordering::Relaxed) {
        return fail("the two arrivals did not get exactly one leader between them");
    }
    serial_println!("[barrier]   Multi-task barrier (2 tasks, one leader): OK");

    serial_println!("[barrier] Self-test PASSED");
    Ok(())
}
