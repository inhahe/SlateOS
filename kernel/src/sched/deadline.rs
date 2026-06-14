//! Deadline scheduler — Earliest Deadline First (EDF) for real-time tasks.
//!
//! Provides hard scheduling guarantees for real-time workloads like
//! audio processing, video playback, and control systems.  Each task
//! specifies a **runtime budget** and a **deadline period**.  The
//! scheduler always picks the task with the nearest absolute deadline,
//! guaranteeing that tasks with tighter timing constraints run first.
//!
//! ## Algorithm: Earliest Deadline First (EDF)
//!
//! EDF is optimal for single-processor preemptive scheduling of periodic
//! and sporadic real-time tasks: if any scheduler can meet all deadlines,
//! EDF can too (Liu & Layland, 1973).
//!
//! Each task has three parameters:
//! - **runtime** (`budget_ticks`): maximum CPU time per period
//! - **deadline** (`deadline_ticks`): relative deadline from period start
//! - **period** (`period_ticks`): repetition interval (deadline ≤ period)
//!
//! When a task is enqueued, its absolute deadline is computed as
//! `current_time + deadline_ticks`.  The scheduler always picks the
//! task with the earliest absolute deadline.
//!
//! ## Admission Control
//!
//! Before accepting a new deadline task, the scheduler checks the total
//! utilization: `Σ(runtime_i / period_i) ≤ utilization_limit`.  This
//! prevents overloading the CPU — if admission would push utilization
//! above the limit (default 95%), the task is rejected.
//!
//! ## Throttling
//!
//! When a task exhausts its runtime budget before the period ends, it is
//! **throttled** — removed from the run queue until the next period
//! begins.  This prevents runaway real-time tasks from starving other
//! work.
//!
//! ## Integration with Priority Scheduler
//!
//! In a complete system, the deadline scheduler runs at highest priority:
//! deadline tasks always preempt regular (priority round-robin or EEVDF)
//! tasks.  This module implements the Scheduler trait so it can be used
//! as a standalone backend for testing, but the intended production use
//! is as a "top tier" that defers to another scheduler when no deadline
//! tasks are ready.
//!
//! ## Data Structure
//!
//! A `BTreeMap<(u64, TaskId), DeadlineEntry>` ordered by absolute
//! deadline.  O(log n) for all operations.
//!
//! ## Performance
//!
//! Designed for a small number of real-time tasks (typically < 20).
//! Audio pipelines have 2-4 deadline tasks; video has 1-2.  The
//! BTreeMap overhead is negligible at this scale.
//!
//! ## References
//!
//! - C.L. Liu & J.W. Layland, "Scheduling Algorithms for
//!   Multiprogramming in a Hard-Real-Time Environment", JACM 1973.
//! - Linux SCHED_DEADLINE (kernel/sched/deadline.c).
//! - LITMUS^RT research scheduler.

use alloc::collections::BTreeMap;
use super::task::{TaskId, NUM_PRIORITIES};

// ---------------------------------------------------------------------------
// Deadline task parameters
// ---------------------------------------------------------------------------

/// Parameters for a deadline-scheduled task.
///
/// These are set when the task is registered with the deadline scheduler
/// and define its real-time requirements.
#[derive(Debug, Clone, Copy)]
pub struct DeadlineParams {
    /// CPU time budget per period (in timer ticks).
    /// The task will be throttled after consuming this many ticks.
    pub budget_ticks: u32,

    /// Relative deadline from period start (in timer ticks).
    /// The task must complete its work within this many ticks of being
    /// released.  Must be ≤ `period_ticks`.
    pub deadline_ticks: u32,

    /// Period length (in timer ticks).
    /// After each period, the budget is replenished and a new absolute
    /// deadline is set.  This is the task's repetition rate.
    /// For audio at 48kHz with 10ms buffers: period = 1 tick (at 100Hz).
    pub period_ticks: u32,
}

impl DeadlineParams {
    /// Utilization of this task as a fraction × 10000 (fixed-point).
    ///
    /// `utilization = budget / period × 10000`
    ///
    /// A task with budget=1 and period=10 has utilization 1000 (10%).
    #[inline]
    fn utilization_x10000(&self) -> u64 {
        if self.period_ticks == 0 {
            return 10000; // Treat zero period as 100% utilization.
        }
        (self.budget_ticks as u64)
            .saturating_mul(10000)
            .checked_div(self.period_ticks as u64)
            .unwrap_or(10000)
    }
}

// ---------------------------------------------------------------------------
// Run queue entry
// ---------------------------------------------------------------------------

/// Per-task deadline scheduling state.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields used for diagnostics and throttle recovery.
struct DeadlineEntry {
    /// Task identifier.
    id: TaskId,
    /// Static priority (used for fallback ordering when deadlines are equal).
    priority: u8,
    /// Deadline parameters (budget, deadline, period).
    params: DeadlineParams,
    /// Absolute deadline (monotonic tick count when work must be done).
    abs_deadline: u64,
    /// Remaining budget ticks in the current period.
    remaining_budget: u32,
    /// Tick when the current period started.
    period_start: u64,
}

// ---------------------------------------------------------------------------
// Deadline Scheduler
// ---------------------------------------------------------------------------

/// Maximum utilization allowed (fixed-point × 10000).
///
/// 9500 = 95%.  Leaving 5% headroom ensures non-deadline tasks can still
/// make progress (timer ISR, scheduler bookkeeping, etc.).
const MAX_UTILIZATION_X10000: u64 = 9500;

/// Deadline scheduler state for a single CPU.
pub struct DeadlineScheduler {
    /// Run queue: tasks ordered by (absolute_deadline, task_id).
    tree: BTreeMap<(u64, TaskId), DeadlineEntry>,

    /// Reverse index: task_id → absolute_deadline.
    deadlines: BTreeMap<TaskId, u64>,

    /// Registered deadline parameters for each task.
    /// Persists across throttle/replenish cycles.
    params: BTreeMap<TaskId, DeadlineParams>,

    /// Throttled tasks: task_id → (abs_deadline, next_period_start).
    /// These tasks exhausted their budget and are waiting for the next
    /// period to begin.
    throttled: BTreeMap<TaskId, (u64, u64)>,

    /// Current monotonic tick counter (advances on each tick() call).
    current_tick: u64,

    /// Total utilization of all registered tasks (× 10000).
    total_utilization: u64,

    /// Number of runnable tasks (not throttled).
    nr_running: u32,

    /// Time slices per priority (for fallback non-deadline tasks).
    time_slices: [u32; NUM_PRIORITIES],

    /// Remaining budget ticks for the currently-running task.
    pub current_remaining: u32,

    /// Priority of the currently-running task.
    current_priority: u8,

    /// ID of the currently-running task (0 = none).
    current_id: TaskId,

    /// Whether the current task is a registered deadline task.
    current_is_deadline: bool,

    /// The absolute deadline of the current task (for re-insertion).
    current_abs_deadline: u64,
}

/// Default base time slice (for non-deadline tasks passing through).
const BASE_TIME_SLICE: u32 = 2;

/// Time slice increment per priority level.
const SLICE_INCREMENT: u32 = 1;

#[allow(dead_code)] // Public API for selectable scheduler backend.
impl DeadlineScheduler {
    /// Const constructor for static initialization.
    #[must_use]
    pub const fn new_const() -> Self {
        Self {
            tree: BTreeMap::new(),
            deadlines: BTreeMap::new(),
            params: BTreeMap::new(),
            throttled: BTreeMap::new(),
            current_tick: 0,
            total_utilization: 0,
            nr_running: 0,
            time_slices: [0; NUM_PRIORITIES],
            current_remaining: 0,
            current_priority: 0,
            current_id: 0,
            current_is_deadline: false,
            current_abs_deadline: 0,
        }
    }

    /// Create a new deadline scheduler with default configuration.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    #[must_use]
    pub fn new() -> Self {
        let mut time_slices = [0u32; NUM_PRIORITIES];
        for (i, slot) in time_slices.iter_mut().enumerate() {
            *slot = BASE_TIME_SLICE.saturating_add((i as u32).saturating_mul(SLICE_INCREMENT));
        }

        Self {
            tree: BTreeMap::new(),
            deadlines: BTreeMap::new(),
            params: BTreeMap::new(),
            throttled: BTreeMap::new(),
            current_tick: 0,
            total_utilization: 0,
            nr_running: 0,
            time_slices,
            current_remaining: 0,
            current_priority: 0,
            current_id: 0,
            current_is_deadline: false,
            current_abs_deadline: 0,
        }
    }

    /// Register a task with deadline parameters.
    ///
    /// Performs admission control: if adding this task would exceed
    /// the CPU utilization limit, returns `false` (task rejected).
    ///
    /// Must be called before `enqueue()` for deadline-aware scheduling.
    /// Tasks enqueued without registration are treated as best-effort
    /// (scheduled by deadline based on priority-derived timeslice).
    pub fn register(&mut self, id: TaskId, params: DeadlineParams) -> bool {
        // Admission control: check utilization.
        let task_util = params.utilization_x10000();
        let new_total = self.total_utilization.saturating_add(task_util);
        if new_total > MAX_UTILIZATION_X10000 {
            return false; // Rejected — would overload CPU.
        }

        // Remove old registration if re-registering.
        if let Some(old) = self.params.remove(&id) {
            self.total_utilization = self.total_utilization
                .saturating_sub(old.utilization_x10000());
        }

        self.params.insert(id, params);
        self.total_utilization = self.total_utilization.saturating_add(task_util);
        true
    }

    /// Unregister a deadline task.
    ///
    /// Removes the task's deadline parameters and frees its utilization
    /// quota.  The task remains in the run queue (if present) but will
    /// be treated as best-effort on next enqueue.
    pub fn unregister(&mut self, id: TaskId) {
        if let Some(old) = self.params.remove(&id) {
            self.total_utilization = self.total_utilization
                .saturating_sub(old.utilization_x10000());
        }
        self.throttled.remove(&id);
    }

    /// Check and replenish any throttled tasks whose period has elapsed.
    fn replenish_throttled(&mut self) {
        // Collect IDs of tasks ready to be replenished (period elapsed).
        // Stack-allocated buffer for small number of RT tasks.
        let mut to_replenish: [(TaskId, u8); 32] = [(0, 0); 32];
        let mut count = 0usize;

        for (&id, &(_abs_dl, next_period)) in &self.throttled {
            if self.current_tick >= next_period && count < 32 {
                // Look up priority from params (default 0 if not found).
                let prio = 0u8; // Deadline tasks are highest priority.
                to_replenish[count] = (id, prio);
                count += 1;
            }
        }

        // Replenish and re-enqueue.
        for &(id, prio) in &to_replenish[..count] {
            self.throttled.remove(&id);
            self.enqueue(id, prio);
        }
    }

    /// Pick the next task: the one with the earliest absolute deadline.
    #[must_use]
    pub fn pick_next(&mut self) -> Option<TaskId> {
        // First, check if any throttled tasks are ready for replenishment.
        self.replenish_throttled();

        if self.tree.is_empty() {
            return None;
        }

        // Take the first entry (earliest deadline).
        let key = *self.tree.keys().next()?;
        let entry = self.tree.remove(&key)?;
        self.deadlines.remove(&entry.id);
        self.nr_running = self.nr_running.saturating_sub(1);

        // Set up current task state.
        self.current_id = entry.id;
        self.current_priority = entry.priority;
        self.current_is_deadline = self.params.contains_key(&entry.id);
        self.current_abs_deadline = entry.abs_deadline;

        if self.current_is_deadline {
            // Use remaining budget from the entry.
            self.current_remaining = entry.remaining_budget;
        } else {
            // Non-deadline task: use priority-based time slice.
            let idx = (entry.priority as usize).min(NUM_PRIORITIES.saturating_sub(1));
            self.current_remaining = self.time_slices.get(idx).copied()
                .unwrap_or(BASE_TIME_SLICE);
        }

        Some(entry.id)
    }

    /// Enqueue a task.
    ///
    /// For registered deadline tasks: computes absolute deadline from
    /// current_tick + deadline_ticks, sets budget.
    /// For unregistered tasks: uses priority-derived deadline.
    #[allow(clippy::cast_possible_truncation)]
    pub fn enqueue(&mut self, id: TaskId, priority: u8) {
        // Remove any stale entry.
        if let Some(old_deadline) = self.deadlines.remove(&id) {
            self.tree.remove(&(old_deadline, id));
            self.nr_running = self.nr_running.saturating_sub(1);
        }

        let (abs_deadline, remaining_budget) = if self.current_id == id && self.current_is_deadline {
            // Re-enqueuing the currently-running deadline task (preempted
            // but not throttled).  Keep its existing deadline and budget.
            let dl = self.current_abs_deadline;
            let budget = self.current_remaining;
            self.current_id = 0;
            self.current_is_deadline = false;
            (dl, budget)
        } else if let Some(params) = self.params.get(&id) {
            // New activation of a registered deadline task.
            let abs_dl = self.current_tick.saturating_add(params.deadline_ticks as u64);
            (abs_dl, params.budget_ticks)
        } else {
            // Non-deadline task: derive a pseudo-deadline from priority.
            let idx = (priority as usize).min(NUM_PRIORITIES.saturating_sub(1));
            let slice = self.time_slices.get(idx).copied().unwrap_or(BASE_TIME_SLICE);
            let abs_dl = self.current_tick.saturating_add(slice as u64);
            (abs_dl, slice)
        };

        let period_start = if let Some(params) = self.params.get(&id) {
            // Align to the current period boundary.
            let _ = params; // Use current_tick as period start.
            self.current_tick
        } else {
            self.current_tick
        };

        let entry = DeadlineEntry {
            id,
            priority,
            params: self.params.get(&id).copied().unwrap_or(DeadlineParams {
                budget_ticks: remaining_budget,
                deadline_ticks: abs_deadline.saturating_sub(self.current_tick) as u32,
                period_ticks: abs_deadline.saturating_sub(self.current_tick) as u32,
            }),
            abs_deadline,
            remaining_budget,
            period_start,
        };

        self.tree.insert((abs_deadline, id), entry);
        self.deadlines.insert(id, abs_deadline);
        self.nr_running = self.nr_running.saturating_add(1);
    }

    /// Remove a task from the run queue.
    #[allow(clippy::cast_possible_truncation)]
    pub fn dequeue(&mut self, id: TaskId, _priority: u8) -> bool {
        if let Some(deadline) = self.deadlines.remove(&id) {
            if self.tree.remove(&(deadline, id)).is_some() {
                self.nr_running = self.nr_running.saturating_sub(1);
                return true;
            }
        }

        if self.current_id == id {
            self.current_id = 0;
            self.current_remaining = 0;
            self.current_is_deadline = false;
            return true;
        }

        false
    }

    /// Remove a task by id regardless of priority level.
    ///
    /// The deadline scheduler keys its run queue by absolute deadline + id, so
    /// `dequeue` already locates the task purely by id and ignores the
    /// `_priority` argument.  This wrapper satisfies the
    /// `SchedulerBackend::dequeue_any` dispatch used by the anti-starvation
    /// booster; it is a priority-agnostic `dequeue`.
    pub fn dequeue_any(&mut self, id: TaskId) -> bool {
        self.dequeue(id, 0)
    }

    /// Handle a timer tick.
    ///
    /// For deadline tasks: decrements budget and throttles if exhausted.
    /// For non-deadline tasks: standard time slice decrement.
    /// Returns `true` when a reschedule is needed.
    pub fn tick(&mut self) -> bool {
        self.current_tick = self.current_tick.saturating_add(1);

        if self.current_id == 0 {
            return false;
        }

        if self.current_remaining > 0 {
            self.current_remaining = self.current_remaining.saturating_sub(1);
        }

        if self.current_remaining == 0 {
            // Budget exhausted.
            if self.current_is_deadline {
                // Throttle: don't re-enqueue until next period.
                if let Some(params) = self.params.get(&self.current_id) {
                    let next_period = self.current_tick
                        .saturating_add(params.period_ticks as u64);
                    let next_deadline = next_period
                        .saturating_add(params.deadline_ticks as u64);
                    self.throttled.insert(
                        self.current_id,
                        (next_deadline, next_period),
                    );
                }
                self.current_id = 0;
                self.current_is_deadline = false;
            }
            return true; // Reschedule needed.
        }

        // Check if we've missed the deadline (for diagnostics).
        if self.current_is_deadline && self.current_tick > self.current_abs_deadline {
            // Deadline miss — task is still running past its deadline.
            // In a production system this would trigger a warning/event.
            // For now, we let it continue but force a reschedule so
            // higher-priority deadline tasks can preempt.
            return true;
        }

        false
    }

    /// Check if any task is ready.
    #[must_use]
    pub fn has_ready(&self) -> bool {
        !self.tree.is_empty()
    }

    /// Check if any non-idle task is ready.
    #[must_use]
    pub fn has_real_work(&self) -> bool {
        self.tree.values().any(|e| e.priority != super::task::IDLE_PRIORITY)
    }

    /// Total runnable tasks.
    #[must_use]
    pub fn total_tasks(&self) -> usize {
        self.nr_running as usize
    }

    /// Current tick counter.
    #[must_use]
    pub fn current_tick(&self) -> u64 {
        self.current_tick
    }

    /// Total utilization (fixed-point × 10000).
    #[must_use]
    pub fn utilization(&self) -> u64 {
        self.total_utilization
    }

    /// Number of throttled tasks.
    #[must_use]
    pub fn throttled_count(&self) -> usize {
        self.throttled.len()
    }

    /// Steal tasks (for work-stealing compatibility).
    pub fn steal(&mut self, count: usize) -> super::priority_rr::StolenTasks {
        let mut stolen = super::priority_rr::StolenTasks::new();
        if count == 0 || self.tree.is_empty() {
            return stolen;
        }

        let capped = count.min(super::priority_rr::MAX_STEAL);
        let mut remaining = capped;

        // Steal from the back (latest deadlines = least urgent).
        let mut keys_to_steal: [Option<(u64, TaskId)>; super::priority_rr::MAX_STEAL] =
            [None; super::priority_rr::MAX_STEAL];
        let mut steal_idx = 0;

        for (key, entry) in self.tree.iter().rev() {
            // Don't steal registered deadline tasks — they have CPU
            // affinity requirements for their utilization guarantees.
            if self.params.contains_key(&entry.id) {
                continue;
            }
            if remaining == 0 || steal_idx >= super::priority_rr::MAX_STEAL {
                break;
            }
            keys_to_steal[steal_idx] = Some(*key);
            steal_idx += 1;
            remaining = remaining.saturating_sub(1);
        }

        for key in keys_to_steal[..steal_idx].iter().flatten() {
            if let Some(entry) = self.tree.remove(key) {
                self.deadlines.remove(&entry.id);
                self.nr_running = self.nr_running.saturating_sub(1);
                stolen.push(entry.id, entry.priority);
            }
        }

        stolen
    }

    // -----------------------------------------------------------------------
    // Time slice configuration (for non-deadline tasks)
    // -----------------------------------------------------------------------

    /// Set time slice for a priority level.
    pub fn set_time_slice(&mut self, level: usize, ticks: u32) -> bool {
        if level >= NUM_PRIORITIES || ticks == 0 {
            return false;
        }
        self.time_slices[level] = ticks;
        true
    }

    /// Get time slice for a priority level.
    #[must_use]
    pub fn time_slice(&self, level: usize) -> Option<u32> {
        self.time_slices.get(level).copied()
    }

    /// Reconfigure all time slices.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    pub fn reconfigure_slices(&mut self, base: u32, increment: u32) -> bool {
        if base == 0 {
            return false;
        }
        for (i, slot) in self.time_slices.iter_mut().enumerate() {
            *slot = base.saturating_add((i as u32).saturating_mul(increment));
        }
        true
    }

    /// Apply a workload profile.
    pub fn apply_profile(&mut self, profile: super::priority_rr::WorkloadProfile) {
        let ok = self.reconfigure_slices(profile.base(), profile.increment());
        debug_assert!(ok, "WorkloadProfile base must be >= 1");
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the deadline scheduler.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
pub fn self_test() -> crate::error::KernelResult<()> {
    use crate::serial_println;

    serial_println!("  deadline: admission control...");
    {
        let mut sched = DeadlineScheduler::new();

        // Register a task that uses 50% CPU: budget=5, period=10.
        let ok = sched.register(1, DeadlineParams {
            budget_ticks: 5,
            deadline_ticks: 10,
            period_ticks: 10,
        });
        assert!(ok, "50% utilization should be accepted");
        assert_eq!(sched.utilization(), 5000); // 50% × 10000

        // Register another at 40%: should still fit (90% total).
        let ok = sched.register(2, DeadlineParams {
            budget_ticks: 4,
            deadline_ticks: 10,
            period_ticks: 10,
        });
        assert!(ok, "40% should be accepted (90% total)");
        assert_eq!(sched.utilization(), 9000);

        // Register at 10%: would be 100%, exceeds 95% limit.
        let ok = sched.register(3, DeadlineParams {
            budget_ticks: 1,
            deadline_ticks: 10,
            period_ticks: 10,
        });
        assert!(!ok, "10% should be rejected (would be 100%)");
    }

    serial_println!("  deadline: earliest deadline first...");
    {
        let mut sched = DeadlineScheduler::new();

        // Task A: deadline in 5 ticks.
        sched.register(10, DeadlineParams {
            budget_ticks: 2,
            deadline_ticks: 5,
            period_ticks: 10,
        });
        // Task B: deadline in 3 ticks (tighter).
        sched.register(11, DeadlineParams {
            budget_ticks: 2,
            deadline_ticks: 3,
            period_ticks: 10,
        });

        sched.enqueue(10, 0);
        sched.enqueue(11, 0);

        // Task B has the earlier deadline — should be picked first.
        let first = sched.pick_next();
        assert_eq!(first, Some(11), "tighter deadline picked first");
    }

    serial_println!("  deadline: budget throttling...");
    {
        let mut sched = DeadlineScheduler::new();

        sched.register(20, DeadlineParams {
            budget_ticks: 2,
            deadline_ticks: 5,
            period_ticks: 10,
        });
        sched.enqueue(20, 0);

        let _ = sched.pick_next(); // Pick task 20.
        assert_eq!(sched.current_remaining, 2);

        // Tick twice to exhaust budget.
        assert!(!sched.tick()); // tick 1: remaining=1
        assert!(sched.tick());  // tick 2: remaining=0, throttled

        // Task should now be throttled.
        assert_eq!(sched.throttled_count(), 1, "task should be throttled");
        assert!(!sched.has_ready(), "no tasks in run queue");
    }

    serial_println!("  deadline: replenishment after period...");
    {
        let mut sched = DeadlineScheduler::new();

        sched.register(30, DeadlineParams {
            budget_ticks: 1,
            deadline_ticks: 3,
            period_ticks: 5,
        });
        sched.enqueue(30, 0);

        // Pick and exhaust budget.
        let _ = sched.pick_next();
        assert!(sched.tick()); // Budget exhausted, throttled.
        assert_eq!(sched.throttled_count(), 1);

        // Advance time past the period.
        for _ in 0..6 {
            sched.tick();
        }

        // pick_next should replenish the throttled task.
        let replenished = sched.pick_next();
        assert_eq!(replenished, Some(30), "task should be replenished");
        assert_eq!(sched.throttled_count(), 0);
    }

    serial_println!("  deadline: non-deadline task fallback...");
    {
        let mut sched = DeadlineScheduler::new();

        // Enqueue without registering — treated as best-effort.
        sched.enqueue(40, 15);

        let picked = sched.pick_next();
        assert_eq!(picked, Some(40));
        assert!(!sched.current_is_deadline);
    }

    serial_println!("  deadline: dequeue...");
    {
        let mut sched = DeadlineScheduler::new();
        sched.enqueue(50, 10);
        sched.enqueue(51, 10);
        assert_eq!(sched.total_tasks(), 2);

        let ok = sched.dequeue(50, 10);
        assert!(ok, "should dequeue task 50");
        assert_eq!(sched.total_tasks(), 1);

        let nope = sched.dequeue(99, 10);
        assert!(!nope, "should not find task 99");
    }

    serial_println!("  deadline: unregister frees utilization...");
    {
        let mut sched = DeadlineScheduler::new();
        sched.register(60, DeadlineParams {
            budget_ticks: 5,
            deadline_ticks: 10,
            period_ticks: 10,
        });
        assert_eq!(sched.utilization(), 5000);

        sched.unregister(60);
        assert_eq!(sched.utilization(), 0, "utilization should be freed");
    }

    serial_println!("  deadline: has_ready / has_real_work...");
    {
        let mut sched = DeadlineScheduler::new();
        assert!(!sched.has_ready());

        sched.enqueue(70, 31); // Idle priority.
        assert!(sched.has_ready());
        assert!(!sched.has_real_work());

        sched.enqueue(71, 5);
        assert!(sched.has_real_work());
    }

    serial_println!("  deadline: workload profile...");
    {
        let mut sched = DeadlineScheduler::new();
        sched.apply_profile(super::priority_rr::WorkloadProfile::Gaming);
        assert_eq!(sched.time_slice(0), Some(1));
        assert_eq!(sched.time_slice(31), Some(63));
    }

    serial_println!("  deadline: steal skips registered deadline tasks...");
    {
        let mut sched = DeadlineScheduler::new();

        // Register task 80 as deadline.
        sched.register(80, DeadlineParams {
            budget_ticks: 2,
            deadline_ticks: 5,
            period_ticks: 10,
        });
        sched.enqueue(80, 0);

        // Enqueue task 81 as non-deadline.
        sched.enqueue(81, 15);

        let stolen = sched.steal(2);
        // Should only steal the non-deadline task.
        assert_eq!(stolen.len(), 1, "only non-deadline task stolen");
        assert_eq!(sched.total_tasks(), 1, "deadline task remains");
    }

    serial_println!("  deadline: all tests passed.");
    Ok(())
}
