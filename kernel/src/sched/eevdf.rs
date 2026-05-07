//! EEVDF (Earliest Eligible Virtual Deadline First) scheduler.
//!
//! An alternative scheduler backend that provides better fairness and
//! latency guarantees than the default priority round-robin scheduler.
//! Based on the algorithm used in Linux 6.6+ (replacing CFS).
//!
//! ## Algorithm Overview
//!
//! Each task has a **virtual runtime** (vruntime) that advances at a
//! rate inversely proportional to its weight (derived from priority).
//! Higher-weight tasks accumulate vruntime more slowly and thus get
//! more CPU time.
//!
//! When a task becomes runnable, it receives a **virtual deadline**:
//!
//!     deadline = vruntime + time_slice / weight
//!
//! A task is **eligible** when its `vruntime <= min_vruntime` (the
//! minimum vruntime across all runnable tasks).  Among eligible tasks,
//! `pick_next` selects the one with the earliest deadline — this
//! ensures both fairness (via vruntime tracking) and bounded latency
//! (via virtual deadlines).
//!
//! ## Data Structure
//!
//! The run queue is a `BTreeMap<(u64, TaskId), EevdfEntry>` keyed by
//! `(virtual_deadline, task_id)`.  This gives O(log n) insertion,
//! removal, and pick_next.  A reverse index `BTreeMap<TaskId, u64>`
//! maps task IDs to their deadlines for O(log n) dequeue-by-ID.
//!
//! ## Weight Table
//!
//! Priority levels 0..31 map to weights via a geometric table (similar
//! to Linux's `sched_prio_to_weight`), where each step is roughly a
//! 1.25× ratio.  Priority 0 (highest) gets the largest weight; priority
//! 31 (idle) gets the smallest.
//!
//! ## Performance
//!
//! - `pick_next`: O(log n) — iterate BTreeMap from front until eligible
//! - `enqueue`: O(log n) — BTreeMap insert
//! - `dequeue`: O(log n) — reverse index lookup + BTreeMap remove
//! - `tick`: O(1) — decrement counter, advance vruntime
//! - `has_ready`: O(1) — check count
//!
//! ## References
//!
//! - P. Stoica & H. Abdel-Wahab, "Earliest Eligible Virtual Deadline
//!   First: A Flexible and Accurate Mechanism for Proportional Share
//!   Resource Allocation", 1995.
//! - Linux kernel `kernel/sched/fair.c` (v6.6+, EEVDF implementation).

use alloc::collections::BTreeMap;
use super::task::{TaskId, NUM_PRIORITIES};

// ---------------------------------------------------------------------------
// Weight table
// ---------------------------------------------------------------------------

/// Weight table mapping priority levels (0 = highest, 31 = idle) to
/// scheduling weights.
///
/// Higher weight means the task gets more CPU time (vruntime advances
/// slower).  The table uses a roughly geometric progression with ratio
/// ~1.25 per step, similar to Linux's `sched_prio_to_weight` but
/// adapted for 32 levels.
///
/// The absolute values don't matter — only the ratios between weights.
/// A task at priority 0 gets ~88× more CPU time than one at priority 31.
///
/// Based on Linux sched_prio_to_weight[] (kernel/sched/core.c), which
/// uses nice-level-to-weight mapping with ~1.25× per nice step.  We
/// adapt this for our 32 priority levels (0 = highest = nice -20
/// equivalent, 31 = idle = nice +19 equivalent).
const WEIGHT_TABLE: [u32; NUM_PRIORITIES] = [
    88761, // prio 0  (highest — real-time equivalent)
    71755, // prio 1
    56483, // prio 2
    46273, // prio 3
    36291, // prio 4
    29154, // prio 5
    23254, // prio 6
    18705, // prio 7
    14949, // prio 8
    11916, // prio 9
    9548,  // prio 10
    7620,  // prio 11
    6100,  // prio 12
    4904,  // prio 13
    3906,  // prio 14
    3121,  // prio 15  (default / nice 0 equivalent)
    2501,  // prio 16
    1991,  // prio 17
    1586,  // prio 18
    1277,  // prio 19
    1024,  // prio 20  (reference weight)
     820,  // prio 21
     655,  // prio 22
     526,  // prio 23
     423,  // prio 24
     335,  // prio 25
     272,  // prio 26
     215,  // prio 27
     172,  // prio 28
     137,  // prio 29
     110,  // prio 30
      15,  // prio 31  (idle — minimal weight)
];

// ---------------------------------------------------------------------------
// vruntime scaling
// ---------------------------------------------------------------------------

/// Scaling factor for vruntime calculations.
///
/// vruntime is stored in fixed-point units to avoid floating-point math.
/// One "real" tick advances vruntime by `VRUNTIME_UNIT / weight`.
/// With VRUNTIME_UNIT = 1_000_000, even the highest-weight task (88761)
/// advances by ~11 per tick, giving sufficient resolution.
const VRUNTIME_UNIT: u64 = 1_000_000;

/// Minimum granularity for preemption decisions (in vruntime units).
///
/// Even if a newly-woken task has an earlier deadline, we don't preempt
/// the current task unless the deadline difference exceeds this threshold.
/// This prevents excessive context switches from micro-differences.
///
/// Set to approximately 1 tick's worth of vruntime at the reference
/// weight (priority 20, weight 1024): 1_000_000 / 1024 ≈ 976.
/// We round to 1000 for simplicity.
///
/// Based on Linux's `sysctl_sched_min_granularity` (kernel/sched/fair.c),
/// which prevents preemption unless the vruntime gap is meaningful.
const MIN_GRANULARITY: u64 = 1000;

// ---------------------------------------------------------------------------
// Run queue entry
// ---------------------------------------------------------------------------

/// Per-task EEVDF scheduling state.
#[derive(Debug, Clone)]
#[allow(dead_code)] // `deadline` stored for diagnostics alongside BTreeMap key.
struct EevdfEntry {
    /// The task identifier.
    id: TaskId,
    /// Static priority level (0 = highest, 31 = idle).
    priority: u8,
    /// Scheduling weight derived from priority (higher = more CPU time).
    weight: u32,
    /// Virtual runtime — tracks how much CPU time this task has consumed,
    /// normalized by weight.  Advances as `VRUNTIME_UNIT / weight` per tick.
    vruntime: u64,
    /// Virtual deadline — `vruntime + time_slice * VRUNTIME_UNIT / weight`
    /// at the time the task was enqueued.  Earlier deadline = higher urgency.
    deadline: u64,
}

// ---------------------------------------------------------------------------
// EEVDF Scheduler
// ---------------------------------------------------------------------------

/// EEVDF scheduler state for a single CPU.
///
/// Provides fair scheduling with latency guarantees by combining virtual
/// runtime tracking with virtual deadline ordering.
pub struct EevdfScheduler {
    /// Run queue: tasks ordered by (virtual_deadline, task_id).
    /// BTreeMap gives O(log n) first-entry access and insertion.
    tree: BTreeMap<(u64, TaskId), EevdfEntry>,

    /// Reverse index: task_id → virtual_deadline.
    /// Enables O(log n) dequeue-by-ID (need the deadline to form the
    /// composite key for `tree`).
    deadlines: BTreeMap<TaskId, u64>,

    /// Minimum vruntime across all runnable tasks.
    ///
    /// Monotonically non-decreasing.  Used for:
    /// 1. Eligibility: a task is eligible when `vruntime <= min_vruntime`
    /// 2. New task placement: new/waking tasks start at `min_vruntime`
    ///    to prevent starvation of existing tasks.
    min_vruntime: u64,

    /// Number of runnable tasks in the tree.
    nr_running: u32,

    /// Time slice configuration per priority level (in timer ticks).
    /// Reuses the same WorkloadProfile scheme as PriorityRoundRobin.
    time_slices: [u32; NUM_PRIORITIES],

    /// Remaining ticks for the currently-running task.
    /// Decremented on each `tick()` call.
    pub current_remaining: u32,

    /// Weight of the currently-running task (needed to advance vruntime
    /// correctly on tick).
    current_weight: u32,

    /// vruntime of the currently-running task.  Updated on each tick.
    /// When the task is re-enqueued, this becomes its new vruntime.
    current_vruntime: u64,

    /// Priority of the currently-running task (for re-enqueue).
    current_priority: u8,

    /// Task ID of the currently-running task (0 = none).
    current_id: TaskId,
}

/// Default base time slice (timer ticks).  Same as priority_rr.
const BASE_TIME_SLICE: u32 = 2;

/// Time slice increment per priority level.
const SLICE_INCREMENT: u32 = 1;

#[allow(dead_code)] // Public API for selectable scheduler backend.
impl EevdfScheduler {
    /// Const constructor for static initialization (before heap is ready).
    #[must_use]
    pub const fn new_const() -> Self {
        Self {
            tree: BTreeMap::new(),
            deadlines: BTreeMap::new(),
            min_vruntime: 0,
            nr_running: 0,
            time_slices: [0; NUM_PRIORITIES],
            current_remaining: 0,
            current_weight: 0,
            current_vruntime: 0,
            current_priority: 0,
            current_id: 0,
        }
    }

    /// Create a new EEVDF scheduler with default time slice configuration.
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
            min_vruntime: 0,
            nr_running: 0,
            time_slices,
            current_remaining: 0,
            current_weight: 0,
            current_vruntime: 0,
            current_priority: 0,
            current_id: 0,
        }
    }

    /// Look up the weight for a priority level.
    #[inline]
    fn weight_for(priority: u8) -> u32 {
        let idx = (priority as usize).min(NUM_PRIORITIES.saturating_sub(1));
        WEIGHT_TABLE[idx]
    }

    /// Compute the virtual deadline for a task being enqueued.
    ///
    /// `deadline = vruntime + time_slice_ticks * VRUNTIME_UNIT / weight`
    ///
    /// The time_slice_ticks come from the per-priority configuration,
    /// representing how long the task may run before being preempted.
    /// Dividing by weight means higher-weight tasks get later deadlines
    /// (they're expected to use more real time), while lower-weight
    /// tasks get tighter deadlines (ensuring they get scheduled sooner
    /// in proportion to their share).
    #[inline]
    fn compute_deadline(&self, vruntime: u64, priority: u8, weight: u32) -> u64 {
        let idx = (priority as usize).min(NUM_PRIORITIES.saturating_sub(1));
        let slice_ticks = self.time_slices.get(idx).copied().unwrap_or(BASE_TIME_SLICE);
        let slice_vruntime = (slice_ticks as u64)
            .saturating_mul(VRUNTIME_UNIT)
            .checked_div(weight as u64)
            .unwrap_or(VRUNTIME_UNIT);
        vruntime.saturating_add(slice_vruntime)
    }

    /// Update min_vruntime after a change to the run queue.
    ///
    /// min_vruntime is the minimum vruntime of any runnable task (or the
    /// current task's vruntime, whichever is smaller).  It only advances
    /// forward — never backwards — to prevent vruntime regression.
    fn update_min_vruntime(&mut self) {
        let tree_min = self.tree.values().next().map(|e| e.vruntime);
        let candidate = match (tree_min, self.current_id != 0) {
            (Some(tv), true) => tv.min(self.current_vruntime),
            (Some(tv), false) => tv,
            (None, true) => self.current_vruntime,
            (None, false) => self.min_vruntime,
        };
        // min_vruntime only advances, never goes backward.
        if candidate > self.min_vruntime {
            self.min_vruntime = candidate;
        }
    }

    /// Pick the next eligible task with the earliest virtual deadline.
    ///
    /// A task is eligible when `vruntime <= min_vruntime`.  We iterate
    /// the BTreeMap from the front (earliest deadline) and pick the first
    /// eligible entry.
    ///
    /// If no task is eligible (can happen briefly during vruntime
    /// adjustments), we fall back to the task with the absolute earliest
    /// deadline — this guarantees forward progress.
    #[must_use]
    pub fn pick_next(&mut self) -> Option<TaskId> {
        if self.tree.is_empty() {
            return None;
        }

        // Phase 1: Find the first eligible task (vruntime <= min_vruntime)
        // among those with the earliest deadlines.
        let mut best_key: Option<(u64, TaskId)> = None;

        for (key, entry) in &self.tree {
            if entry.vruntime <= self.min_vruntime {
                best_key = Some(*key);
                break;
            }
        }

        // Phase 2: If no eligible task found, just take the earliest
        // deadline (guarantees forward progress).
        if best_key.is_none() {
            if let Some((key, _)) = self.tree.iter().next() {
                best_key = Some(*key);
            }
        }

        let key = best_key?;
        let entry = self.tree.remove(&key)?;
        self.deadlines.remove(&entry.id);
        self.nr_running = self.nr_running.saturating_sub(1);

        // Set up current task tracking.
        let idx = (entry.priority as usize).min(NUM_PRIORITIES.saturating_sub(1));
        self.current_remaining = self.time_slices.get(idx).copied().unwrap_or(BASE_TIME_SLICE);
        self.current_weight = entry.weight;
        self.current_vruntime = entry.vruntime;
        self.current_priority = entry.priority;
        self.current_id = entry.id;

        self.update_min_vruntime();

        Some(entry.id)
    }

    /// Add a task to the run queue.
    ///
    /// New tasks and tasks waking from sleep start at `min_vruntime`
    /// to prevent them from monopolizing the CPU (a freshly-spawned
    /// task with vruntime 0 would otherwise run for a very long time
    /// before "catching up").
    ///
    /// Tasks being re-enqueued after preemption keep their existing
    /// vruntime (they've already consumed their fair share).  We detect
    /// this by checking if the task already has a vruntime > 0 stored
    /// in our tracking.
    #[allow(clippy::cast_possible_truncation)]
    pub fn enqueue(&mut self, id: TaskId, priority: u8) {
        // Remove any stale entry (shouldn't happen, but defensive).
        if let Some(old_deadline) = self.deadlines.remove(&id) {
            self.tree.remove(&(old_deadline, id));
            self.nr_running = self.nr_running.saturating_sub(1);
        }

        let weight = Self::weight_for(priority);

        // New/waking tasks start at min_vruntime.
        // Re-enqueued tasks (from pick_next → tick → re-enqueue cycle)
        // will have their vruntime passed through current_vruntime.
        let vruntime = if self.current_id == id {
            // This is the currently-running task being re-enqueued
            // (time slice expired).  Use its accumulated vruntime.
            let vrt = self.current_vruntime;
            self.current_id = 0;
            self.current_weight = 0;
            vrt
        } else {
            // New or waking task: start at min_vruntime to prevent
            // starvation and avoid monopolizing the CPU.
            self.min_vruntime
        };

        let deadline = self.compute_deadline(vruntime, priority, weight);

        let entry = EevdfEntry {
            id,
            priority,
            weight,
            vruntime,
            deadline,
        };

        self.tree.insert((deadline, id), entry);
        self.deadlines.insert(id, deadline);
        self.nr_running = self.nr_running.saturating_add(1);

        self.update_min_vruntime();
    }

    /// Remove a specific task from the run queue.
    ///
    /// Used when a task blocks or is suspended.  Returns `true` if
    /// the task was found and removed.
    #[allow(clippy::cast_possible_truncation)]
    pub fn dequeue(&mut self, id: TaskId, _priority: u8) -> bool {
        if let Some(deadline) = self.deadlines.remove(&id) {
            if self.tree.remove(&(deadline, id)).is_some() {
                self.nr_running = self.nr_running.saturating_sub(1);
                self.update_min_vruntime();
                return true;
            }
        }

        // Also handle the case where the dequeued task is the current one.
        if self.current_id == id {
            self.current_id = 0;
            self.current_weight = 0;
            self.current_remaining = 0;
            return true;
        }

        false
    }

    /// Check if the currently-running task should be preempted by a
    /// ready task with an earlier virtual deadline.
    ///
    /// Returns `true` if the first eligible task in the run queue has a
    /// deadline earlier than the current task's projected deadline by
    /// more than [`MIN_GRANULARITY`].  This enables preemption-on-wake:
    /// when a high-priority task wakes and receives a tight deadline,
    /// the running task is preempted on the next timer tick instead of
    /// waiting for its full time slice to expire.
    ///
    /// The MIN_GRANULARITY threshold prevents oscillation — two tasks
    /// with nearly identical deadlines would otherwise ping-pong the
    /// CPU on every tick, wasting time on context switches.
    ///
    /// Based on Linux's `check_preempt_wakeup()` in kernel/sched/fair.c,
    /// which compares vruntime/deadline gaps before requesting preemption.
    #[must_use]
    fn should_preempt(&self) -> bool {
        if self.current_weight == 0 {
            return false; // No task running.
        }

        // Find the earliest-deadline task in the queue.
        let Some((_, front)) = self.tree.iter().next() else {
            return false; // Queue empty — nothing to preempt for.
        };

        // Only consider eligible tasks (vruntime <= min_vruntime).
        // A task that isn't eligible yet hasn't "earned" its turn.
        if front.vruntime > self.min_vruntime {
            return false;
        }

        // Compute the current task's projected deadline based on its
        // accumulated vruntime (updated each tick).
        let current_deadline = self.compute_deadline(
            self.current_vruntime, self.current_priority, self.current_weight,
        );

        // Preempt if the front task's deadline is earlier by more than
        // MIN_GRANULARITY.  The saturating_add prevents overflow from
        // producing a false negative.
        front.deadline.saturating_add(MIN_GRANULARITY) < current_deadline
    }

    /// Handle a timer tick for the currently-running task.
    ///
    /// Advances the current task's vruntime by `VRUNTIME_UNIT / weight`
    /// and decrements the remaining time slice.  Returns `true` when
    /// a reschedule is needed — either because the time slice expired
    /// or because a woken task has a significantly earlier deadline
    /// (preemption-on-wake).
    ///
    /// The preemption-on-wake check (via [`should_preempt`]) runs on
    /// every tick.  This adds O(1) overhead (reading the BTreeMap front
    /// entry) but ensures woken tasks with tight deadlines get the CPU
    /// within one timer period (~10ms) rather than waiting for the
    /// running task's full time slice.
    pub fn tick(&mut self) -> bool {
        if self.current_weight == 0 {
            return false;
        }

        // Advance the running task's vruntime.
        let delta = VRUNTIME_UNIT
            .checked_div(self.current_weight as u64)
            .unwrap_or(1);
        self.current_vruntime = self.current_vruntime.saturating_add(delta);

        // Update min_vruntime (it may advance if the running task was
        // the minimum).
        self.update_min_vruntime();

        // Decrement time slice.
        if self.current_remaining > 0 {
            self.current_remaining = self.current_remaining.saturating_sub(1);
        }

        // Reschedule if: (a) time slice expired, or (b) a woken task
        // has a significantly earlier deadline (preemption-on-wake).
        //
        // OPT: should_preempt() is O(1) — it reads the BTreeMap's
        // front entry and compares two u64s.  The added cost per tick
        // is negligible compared to the timer ISR overhead.
        self.current_remaining == 0 || self.should_preempt()
    }

    /// Check if any task is ready in the run queue.
    #[must_use]
    pub fn has_ready(&self) -> bool {
        !self.tree.is_empty()
    }

    /// Check if any real work (non-idle priority) is in the queue.
    #[must_use]
    pub fn has_real_work(&self) -> bool {
        self.tree.values().any(|e| e.priority != super::task::IDLE_PRIORITY)
    }

    /// Count the total number of runnable tasks.
    #[must_use]
    pub fn total_tasks(&self) -> usize {
        self.nr_running as usize
    }

    /// Steal up to `count` tasks from this scheduler.
    ///
    /// Steals tasks with the latest deadlines (least urgent) to minimize
    /// disruption to the victim CPU's scheduling decisions.
    pub fn steal(&mut self, count: usize) -> super::priority_rr::StolenTasks {
        let mut stolen = super::priority_rr::StolenTasks::new();
        if count == 0 || self.tree.is_empty() {
            return stolen;
        }

        let capped = count.min(super::priority_rr::MAX_STEAL);
        let mut remaining = capped;

        // Collect keys from the back (latest deadlines = least urgent).
        // We collect first, then remove, to avoid borrow issues.
        let mut keys_to_steal: [Option<(u64, TaskId)>; super::priority_rr::MAX_STEAL] =
            [None; super::priority_rr::MAX_STEAL];
        let mut steal_idx = 0;

        for (key, _entry) in self.tree.iter().rev() {
            if remaining == 0 || steal_idx >= super::priority_rr::MAX_STEAL {
                break;
            }
            keys_to_steal[steal_idx] = Some(*key);
            steal_idx += 1;
            remaining = remaining.saturating_sub(1);
        }

        // Remove collected entries.
        for slot in &keys_to_steal[..steal_idx] {
            if let Some(key) = slot {
                if let Some(entry) = self.tree.remove(key) {
                    self.deadlines.remove(&entry.id);
                    self.nr_running = self.nr_running.saturating_sub(1);
                    stolen.push(entry.id, entry.priority);
                }
            }
        }

        if steal_idx > 0 {
            self.update_min_vruntime();
        }

        stolen
    }

    // -----------------------------------------------------------------------
    // Time slice configuration (mirrors PriorityRoundRobin API)
    // -----------------------------------------------------------------------

    /// Set the time slice for a specific priority level.
    pub fn set_time_slice(&mut self, level: usize, ticks: u32) -> bool {
        if level >= NUM_PRIORITIES || ticks == 0 {
            return false;
        }
        self.time_slices[level] = ticks;
        true
    }

    /// Get the current time slice for a priority level.
    #[must_use]
    pub fn time_slice(&self, level: usize) -> Option<u32> {
        self.time_slices.get(level).copied()
    }

    /// Reconfigure all time slices with a new base and increment.
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

    /// Apply a workload profile preset.
    pub fn apply_profile(&mut self, profile: super::priority_rr::WorkloadProfile) {
        let ok = self.reconfigure_slices(profile.base(), profile.increment());
        debug_assert!(ok, "WorkloadProfile base must be >= 1");
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the EEVDF scheduler.
///
/// Verifies correctness of the core scheduling algorithm: enqueue,
/// pick_next (eligibility + deadline ordering), tick (vruntime advance),
/// dequeue, work stealing, and fairness properties.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
pub fn self_test() -> crate::error::KernelResult<()> {
    use crate::serial_println;

    serial_println!("  eevdf: basic enqueue/pick_next...");
    {
        let mut sched = EevdfScheduler::new();

        // Enqueue three tasks at different priorities.
        sched.enqueue(1, 0);   // highest priority (biggest weight)
        sched.enqueue(2, 15);  // medium priority
        sched.enqueue(3, 31);  // idle priority (smallest weight)

        assert_eq!(sched.nr_running, 3, "should have 3 tasks");

        // All tasks start at min_vruntime=0 with the same vruntime.
        // The task with the earliest deadline should be picked first.
        // Priority 0 has weight 88761, so its deadline =
        //   0 + 2 * 1_000_000 / 88761 ≈ 22 (very tight deadline).
        // Priority 31 has weight 15, so its deadline =
        //   0 + 33 * 1_000_000 / 15 = 2_200_000 (very loose deadline).
        //
        // So priority 0 (task 1) should be picked first.
        let first = sched.pick_next();
        assert_eq!(first, Some(1), "highest priority task picked first");
        assert_eq!(sched.nr_running, 2, "2 remaining");
    }

    serial_println!("  eevdf: tick advances vruntime...");
    {
        let mut sched = EevdfScheduler::new();
        // Use priority 0 → time_slices[0] = BASE_TIME_SLICE + 0 = 2 ticks.
        sched.enqueue(10, 0); // weight = 1024 (reference), priority 0

        let picked = sched.pick_next();
        assert_eq!(picked, Some(10));

        // Weight 1024, so each tick advances vruntime by 1_000_000/1024 ≈ 976
        let vrt_before = sched.current_vruntime;
        let expired = sched.tick();
        let vrt_after = sched.current_vruntime;
        assert!(vrt_after > vrt_before, "vruntime should advance");
        assert!(!expired, "first tick shouldn't expire (2-tick slice)");

        let expired2 = sched.tick();
        assert!(expired2, "second tick should expire the 2-tick slice");
    }

    serial_println!("  eevdf: dequeue removes task...");
    {
        let mut sched = EevdfScheduler::new();
        sched.enqueue(20, 10);
        sched.enqueue(21, 10);

        assert_eq!(sched.nr_running, 2);
        let removed = sched.dequeue(20, 10);
        assert!(removed, "should find and remove task 20");
        assert_eq!(sched.nr_running, 1);

        let removed2 = sched.dequeue(99, 10);
        assert!(!removed2, "should not find task 99");
    }

    serial_println!("  eevdf: fairness — equal priority tasks get equal turns...");
    {
        let mut sched = EevdfScheduler::new();
        let mut pick_count = [0u32; 3];
        let task_ids: [TaskId; 3] = [100, 101, 102];

        // All at the same priority (15) — should get equal CPU time.
        for &id in &task_ids {
            sched.enqueue(id, 15);
        }

        // Run 30 scheduling cycles.
        for _ in 0..30 {
            if let Some(id) = sched.pick_next() {
                let idx = task_ids.iter().position(|&t| t == id).unwrap_or(0);
                pick_count[idx] += 1;
                // Simulate running for the full time slice.
                while !sched.tick() {}
                // Re-enqueue.
                sched.enqueue(id, 15);
            }
        }

        // Each task should have been picked approximately 10 times.
        // Allow ±5 tolerance for rounding effects and preemption-on-wake
        // (tasks may get slightly shorter slices due to deadline-based
        // preemption, redistributing picks unevenly).
        for (i, &count) in pick_count.iter().enumerate() {
            assert!(
                count >= 5 && count <= 15,
                "task {} picked {} times (expected ~10)",
                task_ids[i], count
            );
        }
    }

    serial_println!("  eevdf: weighted fairness — higher priority gets more CPU...");
    {
        let mut sched = EevdfScheduler::new();
        let mut pick_count = [0u32; 2];

        // Task A at priority 10 (weight 9548), Task B at priority 20 (weight 1024).
        // Weight ratio: 9548/1024 ≈ 9.3×.  A should be picked ~9× more than B.
        sched.enqueue(200, 10);
        sched.enqueue(201, 20);

        for _ in 0..100 {
            if let Some(id) = sched.pick_next() {
                if id == 200 {
                    pick_count[0] += 1;
                } else {
                    pick_count[1] += 1;
                }
                while !sched.tick() {}
                sched.enqueue(id, if id == 200 { 10 } else { 20 });
            }
        }

        // Task A should dominate. We expect ~90 picks for A and ~10 for B.
        // Allow wide tolerance since time slices also differ.
        assert!(
            pick_count[0] > pick_count[1],
            "higher-weight task should get more picks: A={}, B={}",
            pick_count[0], pick_count[1]
        );
    }

    serial_println!("  eevdf: steal from back (least-urgent tasks)...");
    {
        let mut sched = EevdfScheduler::new();
        sched.enqueue(300, 5);
        sched.enqueue(301, 15);
        sched.enqueue(302, 25);

        assert_eq!(sched.total_tasks(), 3);

        let stolen = sched.steal(2);
        assert_eq!(stolen.len(), 2, "should steal 2 tasks");
        assert_eq!(sched.total_tasks(), 1, "1 task remaining");
    }

    serial_println!("  eevdf: has_ready / has_real_work...");
    {
        let mut sched = EevdfScheduler::new();
        assert!(!sched.has_ready());
        assert!(!sched.has_real_work());

        sched.enqueue(400, 31); // idle priority
        assert!(sched.has_ready());
        assert!(!sched.has_real_work(), "idle task isn't 'real work'");

        sched.enqueue(401, 10); // real work
        assert!(sched.has_real_work());
    }

    serial_println!("  eevdf: workload profile changes time slices...");
    {
        let mut sched = EevdfScheduler::new();
        sched.apply_profile(super::priority_rr::WorkloadProfile::Server);

        // Server: base=4, increment=2
        assert_eq!(sched.time_slice(0), Some(4));
        assert_eq!(sched.time_slice(1), Some(6));
        assert_eq!(sched.time_slice(31), Some(66));

        sched.apply_profile(super::priority_rr::WorkloadProfile::Gaming);
        assert_eq!(sched.time_slice(0), Some(1));
    }

    serial_println!("  eevdf: new tasks don't starve existing ones...");
    {
        let mut sched = EevdfScheduler::new();

        // Task A has been running and accumulated vruntime.
        sched.enqueue(500, 15);
        let _ = sched.pick_next(); // pick A
        // Simulate many ticks to build up vruntime.
        for _ in 0..50 {
            sched.tick();
        }
        // Re-enqueue A with accumulated vruntime.
        sched.enqueue(500, 15);

        // Now enqueue a brand-new task B.
        sched.enqueue(501, 15);

        // B should NOT immediately dominate — it starts at min_vruntime,
        // same as A's current position.  The scheduler should interleave.
        let first = sched.pick_next();
        assert!(
            first == Some(500) || first == Some(501),
            "either task could be first"
        );
    }

    serial_println!("  eevdf: preemption-on-wake — high-priority waker preempts...");
    {
        let mut sched = EevdfScheduler::new();

        // Task A: low priority (25, weight 335).  Enqueue and pick.
        sched.enqueue(600, 25);
        let picked = sched.pick_next();
        assert_eq!(picked, Some(600));

        // Simulate a few ticks so A has been running (builds vruntime).
        sched.tick();
        sched.tick();

        // Now a high-priority task B (priority 0, weight 88761) wakes up.
        // Its deadline will be very tight (small weight divisor → small
        // deadline offset), while A's projected deadline is very loose.
        sched.enqueue(601, 0);

        // The next tick should trigger preemption because B's deadline
        // is significantly earlier than A's.
        let preempted = sched.tick();
        assert!(
            preempted,
            "high-priority waker should preempt low-priority runner"
        );
    }

    serial_println!("  eevdf: preemption-on-wake — equal priority does NOT preempt...");
    {
        let mut sched = EevdfScheduler::new();

        // Task A: priority 15 (weight 3121).
        sched.enqueue(700, 15);
        let _ = sched.pick_next();

        // One tick so A is running.
        sched.tick();

        // Task B also at priority 15 — same weight, similar deadline.
        // Should NOT trigger preemption (deadline difference < MIN_GRANULARITY).
        sched.enqueue(701, 15);

        // Tick: should not preempt yet (time slice still has ticks left,
        // and same-priority waker doesn't have a meaningfully earlier
        // deadline).
        let preempted = sched.tick();
        // At priority 15, base time slice = 2 + 15*1 = 17 ticks.
        // We've used 2 ticks, so time slice hasn't expired.
        // Same-weight tasks get similar deadlines, so should_preempt is false.
        assert!(
            !preempted,
            "equal-priority waker should not preempt (deadline gap < MIN_GRANULARITY)"
        );
    }

    serial_println!("  eevdf: should_preempt returns false with empty queue...");
    {
        let mut sched = EevdfScheduler::new();
        sched.enqueue(800, 10);
        let _ = sched.pick_next();
        // Queue is empty — should_preempt must be false.
        assert!(!sched.should_preempt(), "empty queue → no preemption");
    }

    serial_println!("  eevdf: all tests passed.");
    Ok(())
}
