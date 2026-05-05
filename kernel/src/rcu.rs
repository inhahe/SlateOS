//! Read-Copy-Update (RCU) — lock-free read-side synchronization.
//!
//! RCU is a synchronization mechanism optimized for read-heavy workloads.
//! Readers access shared data without any locks or atomic operations.
//! Writers create a new version of the data, swap the pointer, and then
//! wait for all pre-existing readers to finish before freeing the old
//! version.
//!
//! ## Key Properties
//!
//! - **Readers are wait-free**: No locks, no atomics, no barriers.
//!   Just read a pointer and use the data.  Reading is as fast as
//!   a single pointer dereference.
//! - **Writers pay the cost**: Creating a copy, updating the pointer
//!   (one atomic swap), and waiting for a grace period.
//! - **Grace period**: The interval during which all CPUs pass through
//!   a quiescent state (context switch, idle, or user mode).  After
//!   the grace period, no reader can hold a reference to the old data.
//!
//! ## Implementation: Quiescent-State-Based RCU (QSBR)
//!
//! Each CPU maintains a per-CPU counter that is incremented at quiescent
//! points (context switches and idle entry).  A grace period completes
//! when all CPUs have passed through at least one quiescent state since
//! the grace period started.
//!
//! This is a simplified "tiny RCU" suitable for our kernel size.  It
//! does not support preemptible RCU (readers can't be preempted) or
//! tree-based scalability for >64 CPUs.
//!
//! ## Usage
//!
//! ```ignore
//! use crate::rcu;
//!
//! // Reader side (zero-cost):
//! rcu::read_lock();     // Mark beginning of RCU read-side critical section.
//! let ptr = data.load(Ordering::Acquire);
//! let value = unsafe { &*ptr };
//! // ... use value ...
//! rcu::read_unlock();   // Mark end of critical section.
//!
//! // Writer side:
//! let old = data.swap(new_ptr, Ordering::Release);
//! rcu::synchronize();   // Wait for all readers of old data to finish.
//! unsafe { drop(Box::from_raw(old)); }  // Now safe to free.
//!
//! // Deferred freeing (non-blocking writer):
//! let old = data.swap(new_ptr, Ordering::Release);
//! rcu::call(old as u64, free_callback);  // Free later, after grace period.
//! ```
//!
//! ## Grace Period Detection
//!
//! 1. Writer calls `synchronize()` or `call()`.
//! 2. Current global grace period counter is incremented.
//! 3. Each CPU's quiescent state counter is checked.
//! 4. When all CPUs have been observed in a quiescent state at least
//!    once since the grace period started, the grace period is complete.
//! 5. Deferred callbacks registered during that grace period are invoked.
//!
//! ## References
//!
//! - McKenney, "Is Parallel Programming Hard?" — Chapter on RCU
//! - Linux `kernel/rcu/` — tree-RCU, tiny-RCU
//! - Fuchsia doesn't have RCU (uses refcounting instead)
//! - FreeBSD `sys/kern/subr_epoch.c` — epoch-based reclamation

#![allow(dead_code)]

use core::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use crate::serial_println;
use crate::smp;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum pending RCU callbacks (deferred frees).
const MAX_CALLBACKS: usize = 256;

/// Maximum CPUs tracked.
const MAX_CPUS: usize = smp::MAX_CPUS;

// ---------------------------------------------------------------------------
// Per-CPU quiescent state counters
// ---------------------------------------------------------------------------

/// Per-CPU quiescent state counters.
///
/// Each CPU increments its counter whenever it passes through a
/// quiescent state (context switch, idle entry, return to userspace).
/// The grace period detector reads these counters to determine when
/// all CPUs have observed the new data.
static QS_COUNTERS: [AtomicU64; MAX_CPUS] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; MAX_CPUS]
};

/// Snapshot of per-CPU counters at the start of the current grace period.
/// When every CPU's counter exceeds its snapshot value, the grace period
/// is complete.
static QS_SNAPSHOT: [AtomicU64; MAX_CPUS] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; MAX_CPUS]
};

/// Global grace period counter.  Incremented when a new grace period
/// is requested.
static GP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Whether a grace period is currently pending (waiting for CPUs).
static GP_PENDING: AtomicBool = AtomicBool::new(false);

/// Per-CPU RCU read-side nesting depth.
/// When > 0, the CPU is in an RCU read-side critical section and
/// must not be considered quiescent.
static READ_NESTING: [AtomicU64; MAX_CPUS] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; MAX_CPUS]
};

// ---------------------------------------------------------------------------
// Deferred callbacks
// ---------------------------------------------------------------------------

/// A deferred RCU callback: function pointer + argument.
struct RcuCallback {
    /// Argument passed to the callback (typically a pointer as u64).
    arg: u64,
    /// Callback function.
    func: fn(u64),
    /// Grace period number when this callback was registered.
    gp_num: u64,
}

/// Ring buffer of pending RCU callbacks.
static CALLBACKS: spin::Mutex<CallbackQueue> = spin::Mutex::new(CallbackQueue::new());

struct CallbackQueue {
    entries: [Option<RcuCallback>; MAX_CALLBACKS],
    head: usize,
    tail: usize,
    count: usize,
}

impl CallbackQueue {
    const fn new() -> Self {
        const NONE: Option<RcuCallback> = None;
        Self {
            entries: [NONE; MAX_CALLBACKS],
            head: 0,
            tail: 0,
            count: 0,
        }
    }

    fn push(&mut self, cb: RcuCallback) -> bool {
        if self.count >= MAX_CALLBACKS {
            return false;
        }
        self.entries[self.tail] = Some(cb);
        self.tail = (self.tail + 1) % MAX_CALLBACKS;
        self.count += 1;
        true
    }

    fn peek_gp(&self) -> Option<u64> {
        if self.count == 0 {
            return None;
        }
        self.entries[self.head].as_ref().map(|cb| cb.gp_num)
    }

    fn pop(&mut self) -> Option<RcuCallback> {
        if self.count == 0 {
            return None;
        }
        let cb = self.entries[self.head].take();
        self.head = (self.head + 1) % MAX_CALLBACKS;
        self.count -= 1;
        cb
    }

    fn len(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Total grace periods completed.
static GP_COMPLETED: AtomicU64 = AtomicU64::new(0);
/// Total callbacks invoked.
static CALLBACKS_INVOKED: AtomicU64 = AtomicU64::new(0);
/// Total synchronize() calls.
static SYNC_CALLS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API — Reader Side
// ---------------------------------------------------------------------------

/// Enter an RCU read-side critical section.
///
/// Between `read_lock()` and `read_unlock()`, the CPU will not be
/// considered quiescent, preventing grace periods from completing.
///
/// This is extremely lightweight — a single atomic increment on
/// a per-CPU counter (no inter-CPU contention).
///
/// Read-side critical sections may be nested.
#[inline]
pub fn read_lock() {
    let cpu = smp::current_cpu_index();
    if let Some(nesting) = READ_NESTING.get(cpu) {
        nesting.fetch_add(1, Ordering::Relaxed);
    }
}

/// Exit an RCU read-side critical section.
///
/// After this call, the data accessed during the critical section
/// may be freed by a writer (once a grace period completes).
#[inline]
pub fn read_unlock() {
    let cpu = smp::current_cpu_index();
    if let Some(nesting) = READ_NESTING.get(cpu) {
        let prev = nesting.fetch_sub(1, Ordering::Relaxed);
        if prev == 0 {
            // Underflow — programming error, but don't panic in production.
            nesting.store(0, Ordering::Relaxed);
        }
    }
}

// ---------------------------------------------------------------------------
// Public API — Writer Side
// ---------------------------------------------------------------------------

/// Wait for a full RCU grace period to elapse.
///
/// Returns only when all CPUs have passed through at least one
/// quiescent state since this function was called.  After this
/// returns, it is safe to free any data that was replaced before
/// the call.
///
/// This blocks the caller (yields the CPU between checks).
/// For non-blocking deferred freeing, use [`call()`].
pub fn synchronize() {
    SYNC_CALLS.fetch_add(1, Ordering::Relaxed);

    let gp = GP_COUNTER.fetch_add(1, Ordering::SeqCst);

    // Snapshot all CPUs' current quiescent state counters.
    let cpu_count = smp::cpu_count();
    for i in 0..cpu_count.min(MAX_CPUS) {
        let current = QS_COUNTERS.get(i)
            .map_or(0, |c| c.load(Ordering::Acquire));
        if let Some(snap) = QS_SNAPSHOT.get(i) {
            snap.store(current, Ordering::Release);
        }
    }

    // Wait for all CPUs to pass through a quiescent state.
    loop {
        let mut all_quiescent = true;
        for i in 0..cpu_count.min(MAX_CPUS) {
            // Skip offline CPUs.
            if !crate::cpu_hotplug::is_online(i) {
                continue;
            }

            // A CPU in an RCU read-side critical section is not quiescent.
            let nesting = READ_NESTING.get(i)
                .map_or(0, |n| n.load(Ordering::Relaxed));
            if nesting > 0 {
                all_quiescent = false;
                continue;
            }

            let snap = QS_SNAPSHOT.get(i)
                .map_or(0, |s| s.load(Ordering::Acquire));
            let current = QS_COUNTERS.get(i)
                .map_or(0, |c| c.load(Ordering::Acquire));

            if current <= snap {
                all_quiescent = false;
            }
        }

        if all_quiescent {
            break;
        }

        // Yield the CPU to allow other tasks (and thus context switches
        // / quiescent states) to occur.
        crate::sched::yield_now();
    }

    GP_COMPLETED.fetch_add(1, Ordering::Relaxed);

    // Process any pending callbacks whose grace period has elapsed.
    process_callbacks(gp.wrapping_add(1));
}

/// Register a deferred callback to be invoked after a grace period.
///
/// The callback `func(arg)` will be called after all CPUs have
/// observed at least one quiescent state.  This is non-blocking
/// for the caller — the callback runs later from the periodic
/// tick handler.
///
/// Returns `true` if the callback was queued, `false` if the
/// callback queue is full.
pub fn call(arg: u64, func: fn(u64)) -> bool {
    let gp = GP_COUNTER.load(Ordering::SeqCst);
    let cb = RcuCallback { arg, func, gp_num: gp };
    let mut queue = CALLBACKS.lock();
    let ok = queue.push(cb);
    if !ok {
        serial_println!("[rcu] WARNING: callback queue full, dropping callback");
    }
    ok
}

// ---------------------------------------------------------------------------
// Quiescent State Reporting (called from scheduler)
// ---------------------------------------------------------------------------

/// Report a quiescent state for the current CPU.
///
/// Called from:
/// - Context switch (scheduler's `switch_to`)
/// - Idle loop entry
/// - Return to userspace
///
/// This is the core mechanism: each call advances the CPU's counter,
/// eventually allowing grace periods to complete.
#[inline]
pub fn quiescent_state() {
    let cpu = smp::current_cpu_index();
    if let Some(counter) = QS_COUNTERS.get(cpu) {
        counter.fetch_add(1, Ordering::Release);
    }
}

/// Periodic tick processing (called from BSP softirq).
///
/// Checks if any pending grace periods have completed and invokes
/// their callbacks.
pub fn tick() {
    let gp = GP_COMPLETED.load(Ordering::Relaxed);
    process_callbacks(gp);
}

// ---------------------------------------------------------------------------
// Internal
// ---------------------------------------------------------------------------

/// Process callbacks whose grace period has elapsed.
fn process_callbacks(completed_gp: u64) {
    let mut queue = CALLBACKS.lock();

    // Process callbacks in order, stopping at the first one whose
    // grace period hasn't completed yet.
    loop {
        match queue.peek_gp() {
            Some(gp_num) if gp_num <= completed_gp => {
                if let Some(cb) = queue.pop() {
                    // Drop the lock before invoking the callback
                    // (callback might register more callbacks).
                    drop(queue);
                    (cb.func)(cb.arg);
                    CALLBACKS_INVOKED.fetch_add(1, Ordering::Relaxed);
                    queue = CALLBACKS.lock();
                }
            }
            _ => break,
        }
    }
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// RCU statistics snapshot.
#[derive(Debug, Clone, Copy)]
pub struct RcuStats {
    /// Total grace periods completed.
    pub gp_completed: u64,
    /// Total synchronize() calls.
    pub sync_calls: u64,
    /// Total deferred callbacks invoked.
    pub callbacks_invoked: u64,
    /// Current pending callbacks.
    pub pending_callbacks: usize,
    /// Current grace period number.
    pub gp_counter: u64,
}

/// Get RCU statistics.
#[must_use]
pub fn stats() -> RcuStats {
    RcuStats {
        gp_completed: GP_COMPLETED.load(Ordering::Relaxed),
        sync_calls: SYNC_CALLS.load(Ordering::Relaxed),
        callbacks_invoked: CALLBACKS_INVOKED.load(Ordering::Relaxed),
        pending_callbacks: CALLBACKS.lock().len(),
        gp_counter: GP_COUNTER.load(Ordering::Relaxed),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the RCU subsystem.
pub fn self_test() {
    serial_println!("[rcu] Running self-test...");

    // Test 1: Read lock/unlock doesn't panic.
    read_lock();
    read_unlock();
    serial_println!("[rcu]   Read lock/unlock: OK");

    // Test 2: Nested read locks.
    read_lock();
    read_lock();
    read_unlock();
    read_unlock();
    serial_println!("[rcu]   Nested read locks: OK");

    // Test 3: Quiescent state reporting.
    let cpu = smp::current_cpu_index();
    let before = QS_COUNTERS.get(cpu)
        .map_or(0, |c| c.load(Ordering::Relaxed));
    quiescent_state();
    let after = QS_COUNTERS.get(cpu)
        .map_or(0, |c| c.load(Ordering::Relaxed));
    assert!(after > before, "quiescent_state should increment counter");
    serial_println!("[rcu]   Quiescent state: OK (counter {} → {})", before, after);

    // Test 4: Deferred callback.
    static TEST_FLAG: AtomicU64 = AtomicU64::new(0);
    fn test_callback(arg: u64) {
        TEST_FLAG.store(arg, Ordering::Relaxed);
    }

    assert!(call(42, test_callback), "callback registration should succeed");
    serial_println!("[rcu]   Callback registration: OK");

    // Test 5: Synchronize (on single-CPU, should complete immediately
    // since we report quiescent state on yield).
    synchronize();
    serial_println!("[rcu]   Synchronize: OK");

    // The callback should have been invoked during synchronize().
    let flag_val = TEST_FLAG.load(Ordering::Relaxed);
    assert_eq!(flag_val, 42, "callback should have been invoked with arg=42");
    serial_println!("[rcu]   Callback invoked: OK (flag={})", flag_val);

    // Test 6: Stats.
    let st = stats();
    assert!(st.gp_completed >= 1, "at least one GP should have completed");
    assert!(st.callbacks_invoked >= 1);
    serial_println!("[rcu]   Stats: OK (gp={}, sync={}, callbacks={})",
        st.gp_completed, st.sync_calls, st.callbacks_invoked);

    serial_println!("[rcu] Self-test PASSED");
}
