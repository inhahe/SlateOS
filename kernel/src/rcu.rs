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

/// Per-CPU idle flag.
///
/// Set when a CPU enters its idle loop (tickless HLT).  An idle CPU
/// is inherently quiescent — it cannot be in an RCU read-side
/// critical section.  `synchronize()` skips idle CPUs instead of
/// waiting for their QS counters to advance (which would never happen
/// since the APIC timer is stopped in tickless idle mode).
///
/// Based on Linux's `rcu_idle_enter()` / `rcu_idle_exit()`.
static CPU_IDLE: [AtomicBool; MAX_CPUS] = {
    const FALSE: AtomicBool = AtomicBool::new(false);
    [FALSE; MAX_CPUS]
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

    // The calling CPU is, by RCU invariant, NOT in an RCU read-side
    // critical section: writers (callers of synchronize) cannot also
    // be readers.  Therefore the act of calling synchronize() is a
    // quiescent point for the calling CPU itself, and we can report
    // it explicitly here.  This guarantees the calling CPU's snapshot
    // condition is satisfied without depending on a subsequent
    // yield_now() / timer_tick() to bump the counter — which is
    // important on single-CPU configurations (UP QEMU), where the
    // boot-time RCU self-test runs on the BSP before the scheduler
    // is fully driving the BSP as a regular task and yield_now() may
    // not result in a context switch.  Fixes known-issues #1 (RCU
    // self-test occasionally hangs at boot).
    //
    // Defensive check: if the caller somehow does have read_nesting > 0
    // on the current CPU (programmer error — calling synchronize from
    // inside a read-side critical section), we still bump the counter
    // (it's sound — read_nesting=0 will be re-checked below), but the
    // wait loop will refuse to break until nesting drops to 0.
    let self_cpu = smp::current_cpu_index();
    if let Some(counter) = QS_COUNTERS.get(self_cpu) {
        counter.fetch_add(1, Ordering::Release);
    }

    // Wait for all CPUs to pass through a quiescent state.
    //
    // Iteration cap: at 100 ms/iter on a busy system this caps the
    // wait at ~1000 s, but on UP QEMU each iteration is a yield_now
    // that takes microseconds — so 1_000_000 iterations bounds the
    // wait at well under a second under healthy conditions.  If we
    // hit the cap, we emit diagnostics and break: a stale RCU grace
    // period is preferable to a silent boot hang (known-issues #1
    // history).  The bound is generous; healthy callers complete in
    // 1–2 iterations.
    const MAX_WAIT_ITERS: u64 = 1_000_000;
    let mut iters: u64 = 0;
    loop {
        let mut all_quiescent = true;
        let mut not_quiescent_cpu = usize::MAX;
        for i in 0..cpu_count.min(MAX_CPUS) {
            // Skip offline CPUs.
            if !crate::cpu_hotplug::is_online(i) {
                continue;
            }

            // An idle CPU is inherently quiescent — it's in HLT/MWAIT
            // with no RCU read-side critical section active.  Skip it.
            // Without this, tickless idle CPUs (whose APIC timer is
            // stopped) would prevent grace periods from completing.
            let idle = CPU_IDLE.get(i)
                .is_some_and(|f| f.load(Ordering::Acquire));
            if idle {
                continue;
            }

            // A CPU in an RCU read-side critical section is not quiescent.
            let nesting = READ_NESTING.get(i)
                .map_or(0, |n| n.load(Ordering::Relaxed));
            if nesting > 0 {
                all_quiescent = false;
                if not_quiescent_cpu == usize::MAX { not_quiescent_cpu = i; }
                continue;
            }

            let snap = QS_SNAPSHOT.get(i)
                .map_or(0, |s| s.load(Ordering::Acquire));
            let current = QS_COUNTERS.get(i)
                .map_or(0, |c| c.load(Ordering::Acquire));

            if current <= snap {
                all_quiescent = false;
                if not_quiescent_cpu == usize::MAX { not_quiescent_cpu = i; }
            }
        }

        if all_quiescent {
            break;
        }

        iters += 1;
        if iters >= MAX_WAIT_ITERS {
            // Safety net for known-issues #1: emit a diagnostic and
            // break rather than hang the boot.  In practice we should
            // never reach this with the self-QS bump above.
            serial_println!(
                "[rcu] WARNING: synchronize() exceeded {} iterations (gp={}, stuck_cpu={}); proceeding",
                MAX_WAIT_ITERS, gp, not_quiescent_cpu
            );
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
    // IRQ-safe critical section: the same CALLBACKS lock is acquired
    // from rcu::tick() running in softirq context (which dispatches
    // with interrupts enabled), so a timer ISR that interrupts a
    // caller of call() and then runs the softirq would otherwise
    // spin-deadlock on this spinlock.  Disabling interrupts on the
    // local CPU for the brief lock-hold window prevents the re-entry.
    // Fixes known-issues #1 (RCU self-test occasionally hangs at boot)
    // — observed at 2/10 boot tests on UP QEMU, hanging between the
    // "Quiescent state" and "Callback registration" probes.
    crate::cpu::without_interrupts(|| {
        let mut queue = CALLBACKS.lock();
        let ok = queue.push(cb);
        if !ok {
            serial_println!("[rcu] WARNING: callback queue full, dropping callback");
        }
        ok
    })
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

/// Mark the current CPU as entering idle.
///
/// Call this from the idle loop before HLT/MWAIT.  An idle CPU is
/// inherently quiescent (it cannot be in an RCU read-side critical
/// section), so `synchronize()` will skip it instead of waiting for
/// its QS counter to advance.
///
/// Based on Linux `rcu_idle_enter()`.
pub fn mark_idle() {
    let cpu = smp::current_cpu_index();
    if let Some(flag) = CPU_IDLE.get(cpu) {
        flag.store(true, Ordering::Release);
    }
}

/// Mark the current CPU as leaving idle.
///
/// Call this from the idle loop after waking from HLT/MWAIT, before
/// executing any RCU-protected code.
///
/// Based on Linux `rcu_idle_exit()`.
pub fn mark_active() {
    let cpu = smp::current_cpu_index();
    if let Some(flag) = CPU_IDLE.get(cpu) {
        flag.store(false, Ordering::Release);
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
///
/// IRQ-safe wrt the CALLBACKS lock: pops one callback at a time inside
/// an interrupts-disabled critical section, then invokes the callback
/// with interrupts restored.  This way the lock is never held across
/// a window that a timer ISR could observe — preventing the deadlock
/// where a CPU holding CALLBACKS (in synchronize/tick/call) is
/// interrupted, the softirq runs on the same CPU, and tries to
/// re-acquire the lock.  See call() for the matching companion fix.
fn process_callbacks(completed_gp: u64) {
    loop {
        let next_cb = crate::cpu::without_interrupts(|| {
            let mut queue = CALLBACKS.lock();
            match queue.peek_gp() {
                Some(gp_num) if gp_num <= completed_gp => queue.pop(),
                _ => None,
            }
        });

        match next_cb {
            Some(cb) => {
                // Defense-in-depth: this dispatch runs from the BSP softirq
                // (rcu::tick).  Validate the stored callback against real
                // `.text` bounds before `call`-ing it — a corrupted queue
                // entry would otherwise jump the softirq to a wild address
                // (the B-KNULLJUMP-SIGNAL class).  A valid `fn(u64)` always
                // points into kernel code.
                let func_addr = cb.func as *const () as u64;
                if crate::idt::is_kernel_text(func_addr) {
                    // Invoke with interrupts in their natural state — the
                    // callback may itself call rcu::call(), which now does
                    // its own without_interrupts wrap.
                    (cb.func)(cb.arg);
                    CALLBACKS_INVOKED.fetch_add(1, Ordering::Relaxed);
                } else {
                    serial_println!(
                        "[rcu] CRITICAL: refusing to invoke corrupt callback func={:#x} \
                         arg={:#x} — queue corruption; skipping (see B-KNULLJUMP-SIGNAL)",
                        func_addr, cb.arg
                    );
                }
            }
            None => break,
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
    // Same IRQ-safe rationale as call() / process_callbacks(): the
    // CALLBACKS lock is also acquired in softirq context, so a stats
    // reader that gets interrupted while holding it could deadlock
    // with rcu::tick() on the same CPU.
    let pending = crate::cpu::without_interrupts(|| CALLBACKS.lock().len());
    RcuStats {
        gp_completed: GP_COMPLETED.load(Ordering::Relaxed),
        sync_calls: SYNC_CALLS.load(Ordering::Relaxed),
        callbacks_invoked: CALLBACKS_INVOKED.load(Ordering::Relaxed),
        pending_callbacks: pending,
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
    // since synchronize() now reports a self-QS for the calling CPU
    // — see the fix for known-issues #1 in `synchronize()`).
    //
    // The pre-stamp localizes any future hang: if "Synchronize: pre"
    // is the last serial output, the hang is inside synchronize();
    // if it's "Synchronize: post" without "Callback invoked", the
    // callback dispatch path is the problem.
    serial_println!("[rcu]   Synchronize: pre (gp_counter={})",
        GP_COUNTER.load(Ordering::Relaxed));
    synchronize();
    serial_println!("[rcu]   Synchronize: post (gp_completed={})",
        GP_COMPLETED.load(Ordering::Relaxed));
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
