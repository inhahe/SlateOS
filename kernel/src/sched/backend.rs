//! Scheduler backend enum — selectable at boot time.
//!
//! Wraps the three available scheduler implementations behind a unified
//! enum.  [`PerCpuScheduler`](super::priority_rr::PerCpuScheduler) uses
//! `SchedulerBackend` instead of a hardcoded `PriorityRoundRobin`, so
//! the scheduler algorithm can be chosen at boot time via the
//! `sched.backend` sysctl parameter.
//!
//! ## Design
//!
//! An enum with match dispatch (not `Box<dyn Scheduler>`) because:
//! - **No heap allocation per CPU** — const-constructible for statics.
//! - **No virtual dispatch on the hot path** — match arms are inlined.
//! - **No trait object overhead in the timer ISR**.
//! - **Match is exhaustive** — adding a new backend is a compile error
//!   in every dispatch site until handled.
//!
//! ## Switching Backends
//!
//! The active backend is chosen at scheduler init time and cannot be
//! changed while the system is running.  The `sched.backend` sysctl
//! parameter stores the *desired* backend; the actual backend is locked
//! in when `PerCpuScheduler::init()` runs.  Changing the sysctl requires
//! a reboot to take effect.
//!
//! ## Backend IDs
//!
//! - `0` — Priority Round-Robin (default, O(1) pick_next via bitmap)
//! - `1` — EEVDF (virtual-runtime fairness + virtual deadlines)
//! - `2` — Deadline (EDF, admission control, budget throttling)

use super::deadline::{self, DeadlineScheduler};
use super::eevdf::EevdfScheduler;
use super::priority_rr::{PriorityRoundRobin, StolenTasks, WorkloadProfile};
use super::task::TaskId;
use core::sync::atomic::{AtomicU8, Ordering};

// ---------------------------------------------------------------------------
// Backend selection
// ---------------------------------------------------------------------------

/// Backend type IDs.
pub const BACKEND_PRIORITY_RR: u8 = 0;
pub const BACKEND_EEVDF: u8 = 1;
pub const BACKEND_DEADLINE: u8 = 2;

/// The backend that will be used on next init.
///
/// Set via `sched.backend` sysctl.  Read by `PerCpuScheduler::init()`.
/// Changing this after init has no effect until reboot.
static DESIRED_BACKEND: AtomicU8 = AtomicU8::new(BACKEND_PRIORITY_RR);

/// The backend that is actually running.
///
/// Set once by `PerCpuScheduler::init()`, never changed after.
static ACTIVE_BACKEND: AtomicU8 = AtomicU8::new(BACKEND_PRIORITY_RR);

/// Get the currently active backend ID.
#[must_use]
pub fn active_backend() -> u8 {
    ACTIVE_BACKEND.load(Ordering::Relaxed)
}

/// Get the desired backend ID (for sysctl reads).
#[must_use]
pub fn desired_backend() -> u8 {
    DESIRED_BACKEND.load(Ordering::Relaxed)
}

/// Set the desired backend ID (for sysctl writes).
///
/// Takes effect on next reboot (next `PerCpuScheduler::init()` call).
/// Returns `true` if the value is a valid backend ID.
pub fn set_desired_backend(id: u8) -> bool {
    match id {
        BACKEND_PRIORITY_RR | BACKEND_EEVDF | BACKEND_DEADLINE => {
            DESIRED_BACKEND.store(id, Ordering::Relaxed);
            true
        }
        _ => false,
    }
}

/// Record which backend was actually initialized.
///
/// Called once by `PerCpuScheduler::init()`.
pub(super) fn set_active_backend(id: u8) {
    ACTIVE_BACKEND.store(id, Ordering::Release);
}

/// Human-readable name for a backend ID.
#[must_use]
pub fn backend_name(id: u8) -> &'static str {
    match id {
        BACKEND_PRIORITY_RR => "PriorityRoundRobin",
        BACKEND_EEVDF => "EEVDF",
        BACKEND_DEADLINE => "Deadline",
        _ => "Unknown",
    }
}

// ---------------------------------------------------------------------------
// SchedulerBackend enum
// ---------------------------------------------------------------------------

/// Scheduler backend — wraps the three scheduler implementations.
///
/// Each variant holds the concrete scheduler state.  All methods dispatch
/// via `match` — the compiler inlines the arms, so there's no dynamic
/// dispatch overhead.
//
// NOTE: `clippy::large_enum_variant` would have us box the larger
// variants, but this enum is stored inline in the per-CPU scheduler and
// is touched on the scheduler hot path (pick_next, enqueue).  Boxing
// would add a heap indirection per access and defeat the inline-dispatch
// design above.  There is exactly one backend per CPU, so the unused
// space is bounded and constant — an acceptable trade for hot-path speed.
#[allow(clippy::large_enum_variant)]
pub enum SchedulerBackend {
    /// Priority round-robin: O(1) bitmap-based pick_next, 32 priority
    /// levels, per-level FIFO queues.  Best for general-purpose workloads.
    PriorityRR(PriorityRoundRobin),

    /// EEVDF: virtual-runtime tracking with virtual deadlines.  Better
    /// fairness guarantees and bounded latency.  O(log n) pick_next via
    /// BTreeMap.  Best for interactive desktop workloads.
    Eevdf(EevdfScheduler),

    /// Deadline: EDF with admission control and budget throttling.
    /// Guarantees timing requirements for real-time tasks.  O(log n)
    /// pick_next.  Best for audio/video/control workloads.
    Deadline(DeadlineScheduler),
}

impl SchedulerBackend {
    /// Create a new backend with const-init values (for static arrays).
    ///
    /// Defaults to PriorityRoundRobin.  The actual backend is replaced
    /// during `PerCpuScheduler::init()`.
    #[must_use]
    pub const fn new_const() -> Self {
        Self::PriorityRR(PriorityRoundRobin::new_const())
    }

    /// Create a new backend from the desired backend ID.
    #[must_use]
    pub fn from_id(id: u8) -> Self {
        match id {
            BACKEND_EEVDF => Self::Eevdf(EevdfScheduler::new()),
            BACKEND_DEADLINE => Self::Deadline(DeadlineScheduler::new()),
            _ => Self::PriorityRR(PriorityRoundRobin::new()),
        }
    }

    /// Get the backend type ID.
    #[must_use]
    pub fn id(&self) -> u8 {
        match self {
            Self::PriorityRR(_) => BACKEND_PRIORITY_RR,
            Self::Eevdf(_) => BACKEND_EEVDF,
            Self::Deadline(_) => BACKEND_DEADLINE,
        }
    }

    // -- Core scheduler operations (common to all backends) ----------------

    /// Pick the highest-priority ready task.
    ///
    /// Removes it from the run queue.  Returns `None` if empty.
    #[inline]
    pub fn pick_next(&mut self) -> Option<TaskId> {
        match self {
            Self::PriorityRR(s) => s.pick_next(),
            Self::Eevdf(s) => s.pick_next(),
            Self::Deadline(s) => s.pick_next(),
        }
    }

    /// Add a task to the run queue at the given priority.
    #[inline]
    pub fn enqueue(&mut self, id: TaskId, priority: u8) {
        match self {
            Self::PriorityRR(s) => s.enqueue(id, priority),
            Self::Eevdf(s) => s.enqueue(id, priority),
            Self::Deadline(s) => s.enqueue(id, priority),
        }
    }

    /// Remove a specific task from the run queue.
    #[inline]
    pub fn dequeue(&mut self, id: TaskId, priority: u8) -> bool {
        match self {
            Self::PriorityRR(s) => s.dequeue(id, priority),
            Self::Eevdf(s) => s.dequeue(id, priority),
            Self::Deadline(s) => s.dequeue(id, priority),
        }
    }

    /// Timer tick.  Returns `true` if the current task's time slice expired.
    #[inline]
    pub fn tick(&mut self) -> bool {
        match self {
            Self::PriorityRR(s) => s.tick(),
            Self::Eevdf(s) => s.tick(),
            Self::Deadline(s) => s.tick(),
        }
    }

    /// Check if any task is in the run queue.
    #[inline]
    pub fn has_ready(&self) -> bool {
        match self {
            Self::PriorityRR(s) => s.has_ready(),
            Self::Eevdf(s) => s.has_ready(),
            Self::Deadline(s) => s.has_ready(),
        }
    }

    /// Check if there's real work (non-idle tasks) in the queue.
    #[inline]
    pub fn has_real_work(&self) -> bool {
        match self {
            Self::PriorityRR(s) => s.has_real_work(),
            Self::Eevdf(s) => s.has_real_work(),
            Self::Deadline(s) => s.has_real_work(),
        }
    }

    /// Total number of tasks in the run queue.
    #[inline]
    pub fn total_tasks(&self) -> usize {
        match self {
            Self::PriorityRR(s) => s.total_tasks(),
            Self::Eevdf(s) => s.total_tasks(),
            Self::Deadline(s) => s.total_tasks(),
        }
    }

    /// Steal up to `count` tasks from this queue (for work stealing).
    #[inline]
    pub fn steal(&mut self, count: usize) -> StolenTasks {
        match self {
            Self::PriorityRR(s) => s.steal(count),
            Self::Eevdf(s) => s.steal(count),
            Self::Deadline(s) => s.steal(count),
        }
    }

    // -- Time slice configuration ------------------------------------------

    /// Set the time slice for a priority level.
    #[inline]
    pub fn set_time_slice(&mut self, level: usize, ticks: u32) -> bool {
        match self {
            Self::PriorityRR(s) => s.set_time_slice(level, ticks),
            Self::Eevdf(s) => s.set_time_slice(level, ticks),
            Self::Deadline(s) => s.set_time_slice(level, ticks),
        }
    }

    /// Get the time slice for a priority level.
    #[inline]
    #[must_use]
    pub fn time_slice(&self, level: usize) -> Option<u32> {
        match self {
            Self::PriorityRR(s) => s.time_slice(level),
            Self::Eevdf(s) => s.time_slice(level),
            Self::Deadline(s) => s.time_slice(level),
        }
    }

    /// Reconfigure time slices for all priority levels.
    #[inline]
    pub fn reconfigure_slices(&mut self, base: u32, increment: u32) -> bool {
        match self {
            Self::PriorityRR(s) => s.reconfigure_slices(base, increment),
            Self::Eevdf(s) => s.reconfigure_slices(base, increment),
            Self::Deadline(s) => s.reconfigure_slices(base, increment),
        }
    }

    /// Apply a workload profile (time slice presets).
    #[inline]
    pub fn apply_profile(&mut self, profile: WorkloadProfile) {
        match self {
            Self::PriorityRR(s) => s.apply_profile(profile),
            Self::Eevdf(s) => s.apply_profile(profile),
            Self::Deadline(s) => s.apply_profile(profile),
        }
    }

    /// Get the remaining ticks for the currently-running task.
    #[inline]
    #[must_use]
    pub fn current_remaining(&self) -> u32 {
        match self {
            Self::PriorityRR(s) => s.current_remaining,
            Self::Eevdf(s) => s.current_remaining,
            Self::Deadline(s) => s.current_remaining,
        }
    }

    /// Set the remaining ticks for the currently-running task.
    #[inline]
    pub fn set_current_remaining(&mut self, ticks: u32) {
        match self {
            Self::PriorityRR(s) => s.current_remaining = ticks,
            Self::Eevdf(s) => s.current_remaining = ticks,
            Self::Deadline(s) => s.current_remaining = ticks,
        }
    }

    // -- Deadline-specific operations (no-ops for other backends) ----------

    /// Register a task with deadline parameters (EDF-specific).
    ///
    /// Returns `true` if the task was accepted (passes admission control).
    /// For non-deadline backends, always returns `false`.
    #[allow(dead_code)]
    pub fn register_deadline(
        &mut self,
        id: TaskId,
        params: deadline::DeadlineParams,
    ) -> bool {
        match self {
            Self::Deadline(s) => s.register(id, params),
            _ => false,
        }
    }

    /// Unregister a task from the deadline scheduler.
    ///
    /// No-op for non-deadline backends.
    #[allow(dead_code)]
    pub fn unregister_deadline(&mut self, id: TaskId) {
        if let Self::Deadline(s) = self {
            s.unregister(id);
        }
    }

    /// Get the current tick counter (deadline-specific).
    ///
    /// Returns 0 for non-deadline backends.
    #[allow(dead_code)]
    #[must_use]
    pub fn deadline_current_tick(&self) -> u64 {
        match self {
            Self::Deadline(s) => s.current_tick(),
            _ => 0,
        }
    }

    /// Get the total utilization (deadline-specific, ×10000 scale).
    ///
    /// Returns 0 for non-deadline backends.
    #[allow(dead_code)]
    #[must_use]
    pub fn deadline_utilization(&self) -> u64 {
        match self {
            Self::Deadline(s) => s.utilization(),
            _ => 0,
        }
    }

    /// Get the number of throttled tasks (deadline-specific).
    ///
    /// Returns 0 for non-deadline backends.
    #[allow(dead_code)]
    #[must_use]
    pub fn deadline_throttled_count(&self) -> usize {
        match self {
            Self::Deadline(s) => s.throttled_count(),
            _ => 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the scheduler backend enum.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[sched::backend] Running self-test...");

    // Test 1: Default backend is PriorityRR.
    let b = SchedulerBackend::new_const();
    assert_eq!(b.id(), BACKEND_PRIORITY_RR);
    serial_println!("[sched::backend]   new_const default: OK");

    // Test 2: Create each backend from ID.
    let rr = SchedulerBackend::from_id(BACKEND_PRIORITY_RR);
    assert_eq!(rr.id(), BACKEND_PRIORITY_RR);
    let eevdf = SchedulerBackend::from_id(BACKEND_EEVDF);
    assert_eq!(eevdf.id(), BACKEND_EEVDF);
    let dl = SchedulerBackend::from_id(BACKEND_DEADLINE);
    assert_eq!(dl.id(), BACKEND_DEADLINE);
    serial_println!("[sched::backend]   from_id: OK");

    // Test 3: Unknown ID falls back to PriorityRR.
    let unknown = SchedulerBackend::from_id(255);
    assert_eq!(unknown.id(), BACKEND_PRIORITY_RR);
    serial_println!("[sched::backend]   unknown_id fallback: OK");

    // Test 4: Enqueue/pick_next cycle on each backend.
    for &backend_id in &[BACKEND_PRIORITY_RR, BACKEND_EEVDF, BACKEND_DEADLINE] {
        let mut b = SchedulerBackend::from_id(backend_id);
        assert!(!b.has_ready());
        b.enqueue(100, 10);
        assert!(b.has_ready());
        assert_eq!(b.total_tasks(), 1);
        let picked = b.pick_next();
        assert_eq!(picked, Some(100));
        assert!(!b.has_ready());
    }
    serial_println!("[sched::backend]   enqueue/pick_next all backends: OK");

    // Test 5: Tick dispatch.
    for &backend_id in &[BACKEND_PRIORITY_RR, BACKEND_EEVDF, BACKEND_DEADLINE] {
        let mut b = SchedulerBackend::from_id(backend_id);
        b.enqueue(200, 5);
        let _ = b.pick_next(); // Set current task context.
        // Tick should work without panic.
        let _ = b.tick();
    }
    serial_println!("[sched::backend]   tick all backends: OK");

    // Test 6: Desired/active backend API.
    let orig = desired_backend();
    assert!(set_desired_backend(BACKEND_EEVDF));
    assert_eq!(desired_backend(), BACKEND_EEVDF);
    assert!(set_desired_backend(BACKEND_DEADLINE));
    assert_eq!(desired_backend(), BACKEND_DEADLINE);
    assert!(!set_desired_backend(99)); // Invalid.
    assert_eq!(desired_backend(), BACKEND_DEADLINE); // Unchanged.
    // Restore original.
    set_desired_backend(orig);
    serial_println!("[sched::backend]   desired_backend API: OK");

    // Test 7: Backend names.
    assert_eq!(backend_name(BACKEND_PRIORITY_RR), "PriorityRoundRobin");
    assert_eq!(backend_name(BACKEND_EEVDF), "EEVDF");
    assert_eq!(backend_name(BACKEND_DEADLINE), "Deadline");
    assert_eq!(backend_name(99), "Unknown");
    serial_println!("[sched::backend]   backend_name: OK");

    // Test 8: Time slice dispatch.
    for &backend_id in &[BACKEND_PRIORITY_RR, BACKEND_EEVDF, BACKEND_DEADLINE] {
        let mut b = SchedulerBackend::from_id(backend_id);
        let old = b.time_slice(0);
        assert!(old.is_some());
        assert!(b.set_time_slice(0, 10));
        assert_eq!(b.time_slice(0), Some(10));
        // Restore.
        if let Some(v) = old {
            b.set_time_slice(0, v);
        }
    }
    serial_println!("[sched::backend]   time_slice dispatch: OK");

    // Test 9: Profile dispatch.
    for &backend_id in &[BACKEND_PRIORITY_RR, BACKEND_EEVDF, BACKEND_DEADLINE] {
        let mut b = SchedulerBackend::from_id(backend_id);
        b.apply_profile(WorkloadProfile::Server);
        // Server profile: base=4, so level 0 should have time_slice=4.
        assert_eq!(b.time_slice(0), Some(4));
        b.apply_profile(WorkloadProfile::Desktop);
        assert_eq!(b.time_slice(0), Some(2));
    }
    serial_println!("[sched::backend]   apply_profile dispatch: OK");

    // Test 10: Deadline-specific operations on non-deadline backends.
    let mut rr = SchedulerBackend::from_id(BACKEND_PRIORITY_RR);
    assert!(!rr.register_deadline(1, deadline::DeadlineParams {
        budget_ticks: 1,
        deadline_ticks: 5,
        period_ticks: 10,
    }));
    assert_eq!(rr.deadline_utilization(), 0);
    assert_eq!(rr.deadline_throttled_count(), 0);
    assert_eq!(rr.deadline_current_tick(), 0);
    serial_println!("[sched::backend]   deadline ops on non-deadline: OK");

    // Test 11: Deadline-specific operations on deadline backend.
    let mut dl = SchedulerBackend::from_id(BACKEND_DEADLINE);
    assert!(dl.register_deadline(1, deadline::DeadlineParams {
        budget_ticks: 1,
        deadline_ticks: 5,
        period_ticks: 10,
    }));
    assert!(dl.deadline_utilization() > 0);
    dl.unregister_deadline(1);
    assert_eq!(dl.deadline_utilization(), 0);
    serial_println!("[sched::backend]   deadline-specific ops: OK");

    serial_println!("[sched::backend] Self-test PASSED (11 tests)");
}
