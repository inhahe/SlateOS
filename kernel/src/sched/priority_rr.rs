//! Priority round-robin scheduler with per-CPU queues.
//!
//! This is the default (and currently only) scheduler implementation.
//! Tasks are organized into 32 priority levels, with round-robin
//! scheduling within each level.  The highest-priority non-empty
//! queue is always serviced first.
//!
//! ## O(1) `pick_next_task`
//!
//! A 32-bit bitmap tracks which priority levels have runnable tasks.
//! Finding the highest-priority level is a single `trailing_zeros()`
//! operation (compiled to the `BSF` or `TZCNT` instruction on
//! `x86_64`).
//!
//! ## Per-CPU Queues
//!
//! Each CPU has its own [`PriorityRoundRobin`] run queue set, wrapped
//! by [`PerCpuScheduler`].  Tasks are enqueued on their `last_cpu`
//! (cache-warm scheduling).  When a CPU's local queue is empty, it
//! steals work from the most-loaded CPU (work stealing).
//!
//! Currently only CPU 0 is online (single-CPU boot).  The per-CPU
//! infrastructure is ready for SMP — when AP bootstrap is implemented,
//! `PerCpuScheduler::init(num_cpus)` is called with the actual CPU
//! count and each CPU runs its own scheduling loop.
//!
//! ## Time Slices
//!
//! Each priority level has a configurable time slice (in timer ticks).
//! Higher priorities get shorter slices for lower latency; lower
//! priorities get longer slices for better throughput.  Time slices
//! are applied per-CPU.

use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Mutex;
use super::task::{TaskId, NUM_PRIORITIES};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default time slice for priority 0 (highest), in timer ticks.
const BASE_TIME_SLICE: u32 = 2;

/// Time slice increment per priority level.  Each level gets
/// `BASE_TIME_SLICE + level * SLICE_INCREMENT` ticks.
const SLICE_INCREMENT: u32 = 1;

// ---------------------------------------------------------------------------
// Workload profiles
// ---------------------------------------------------------------------------

/// Predefined workload profiles that tune scheduler time slices.
///
/// From the design spec (§ "Workload profiles"):
/// > Workload profiles would just be named presets of these runtime
/// > parameters.  The user selects a profile, the OS applies the preset
/// > values, no recompile needed.
///
/// Each profile tunes the time slice formula `base + level * increment`
/// for the 32 priority levels.  At 100 Hz timer tick rate, 1 tick = 10 ms.
///
/// The numeric encoding matches the syscall argument (0–3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WorkloadProfile {
    /// Balanced for general desktop use.  Moderate time slices across
    /// all priorities; interactive tasks (detected via burst tracking)
    /// get a priority boost.  Good for mixed workloads: browsing,
    /// document editing, background builds.
    ///
    /// base=2 (20 ms), increment=1 → level 0: 20 ms, level 31: 330 ms.
    Desktop = 0,

    /// Database / server workloads.  Longer time slices to reduce
    /// context switch overhead and maximize throughput.  Tasks run
    /// longer before preemption, reducing scheduling jitter for
    /// sustained CPU-bound work.
    ///
    /// base=4 (40 ms), increment=2 → level 0: 40 ms, level 31: 660 ms.
    Server = 1,

    /// Software development workloads.  Many short-lived processes
    /// (compiler invocations, test runners, build scripts) benefit
    /// from quick scheduling.  Short base slices keep context-switch
    /// latency low for parallel `make -j` or `cargo build` runs.
    ///
    /// base=1 (10 ms), increment=1 → level 0: 10 ms, level 31: 320 ms.
    Development = 2,

    /// Gaming and real-time workloads.  Very short slices at high
    /// priorities for minimal input-to-frame latency.  Low-priority
    /// background tasks get generous slices to avoid starving them
    /// entirely, but the foreground game (high priority) preempts
    /// quickly.
    ///
    /// base=1 (10 ms), increment=2 → level 0: 10 ms, level 31: 630 ms.
    Gaming = 3,
}

impl WorkloadProfile {
    /// Try to convert a raw u8 to a profile.
    #[must_use]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Desktop),
            1 => Some(Self::Server),
            2 => Some(Self::Development),
            3 => Some(Self::Gaming),
            _ => None,
        }
    }

    /// Get the time slice base for this profile (in timer ticks).
    #[must_use]
    pub const fn base(self) -> u32 {
        match self {
            Self::Desktop     => 2,
            Self::Server      => 4,
            Self::Development => 1,
            Self::Gaming      => 1,
        }
    }

    /// Get the time slice increment per priority level.
    #[must_use]
    pub const fn increment(self) -> u32 {
        match self {
            Self::Desktop     => 1,
            Self::Server      => 2,
            Self::Development => 1,
            Self::Gaming      => 2,
        }
    }

    /// Human-readable name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Desktop     => "Desktop",
            Self::Server      => "Server",
            Self::Development => "Development",
            Self::Gaming      => "Gaming",
        }
    }
}

// ---------------------------------------------------------------------------
// Priority round-robin scheduler
// ---------------------------------------------------------------------------

/// Priority round-robin scheduler state.
///
/// Holds 32 per-priority FIFO queues and a bitmap for O(1)
/// highest-priority lookup.
pub struct PriorityRoundRobin {
    /// Per-priority FIFO queues.  Index 0 = highest priority.
    queues: [VecDeque<TaskId>; NUM_PRIORITIES],
    /// Bitmap: bit `i` set → `queues[i]` is non-empty.
    bitmap: u32,
    /// Time slice configuration per priority level (in timer ticks).
    time_slices: [u32; NUM_PRIORITIES],
    /// Remaining ticks for the currently-running task.
    pub current_remaining: u32,
}

impl PriorityRoundRobin {
    /// Const constructor for use in static initialization.
    ///
    /// Queues start empty; the scheduler should be replaced via
    /// [`new`](Self::new) after the heap is initialized.
    #[must_use]
    pub const fn new_const() -> Self {
        Self {
            queues: [const { VecDeque::new() }; NUM_PRIORITIES],
            bitmap: 0,
            time_slices: [0; NUM_PRIORITIES],
            current_remaining: 0,
        }
    }

    /// Create a new scheduler with default time slice configuration.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    #[must_use]
    pub fn new() -> Self {
        // Build default time slices: higher priority = shorter slice.
        // Truncation: NUM_PRIORITIES is 32, so `i as u32` is always safe.
        let mut time_slices = [0u32; NUM_PRIORITIES];
        for (i, slot) in time_slices.iter_mut().enumerate() {
            *slot = BASE_TIME_SLICE + (i as u32) * SLICE_INCREMENT;
        }

        // VecDeque::new() is const, but [VecDeque::new(); N] isn't
        // allowed for non-Copy types.  Build the array explicitly.
        //
        // core::array::from_fn generates all 32 queues.
        let queues = core::array::from_fn(|_| VecDeque::new());

        Self {
            queues,
            bitmap: 0,
            time_slices,
            current_remaining: 0,
        }
    }

    /// Pick the next task to run.
    ///
    /// Returns the `TaskId` of the highest-priority ready task, or
    /// `None` if all queues are empty.  The task is removed from its
    /// queue (the caller must set it to Running).
    ///
    /// **O(1)**: bitmap scan + dequeue from head.
    #[must_use]
    pub fn pick_next(&mut self) -> Option<TaskId> {
        if self.bitmap == 0 {
            return None;
        }

        // Highest priority = lowest set bit.
        let level = self.bitmap.trailing_zeros() as usize;

        // Pop the front task from this priority's queue.
        let queue = self.queues.get_mut(level)?;
        let id = queue.pop_front()?;

        // If the queue is now empty, clear the bitmap bit.
        if queue.is_empty() {
            self.bitmap &= !(1 << level);
        }

        // Set the time slice for this task.
        self.current_remaining = self.time_slices.get(level).copied().unwrap_or(BASE_TIME_SLICE);

        Some(id)
    }

    /// Add a task to its priority level's queue.
    ///
    /// The task is placed at the back of its queue (round-robin
    /// fairness).
    #[allow(clippy::cast_possible_truncation)]
    pub fn enqueue(&mut self, id: TaskId, priority: u8) {
        let level = (priority as usize).min(NUM_PRIORITIES.saturating_sub(1));
        if let Some(queue) = self.queues.get_mut(level) {
            queue.push_back(id);
            self.bitmap |= 1 << level;
        }
    }

    /// Remove a specific task from its queue.
    ///
    /// Used when a task blocks or is suspended.  Returns `true` if
    /// the task was found and removed.
    #[allow(clippy::cast_possible_truncation)]
    pub fn dequeue(&mut self, id: TaskId, priority: u8) -> bool {
        let level = (priority as usize).min(NUM_PRIORITIES.saturating_sub(1));
        let Some(queue) = self.queues.get_mut(level) else {
            return false;
        };

        // Linear scan within the priority queue.  Each individual
        // queue should be short (a few tasks), so this is acceptable.
        if let Some(pos) = queue.iter().position(|&tid| tid == id) {
            queue.remove(pos);
            if queue.is_empty() {
                self.bitmap &= !(1 << level);
            }
            return true;
        }

        false
    }

    /// Handle a timer tick for the current task.
    ///
    /// Decrements the remaining time slice.  Returns `true` if the
    /// time slice has expired and a reschedule is needed.
    pub fn tick(&mut self) -> bool {
        if self.current_remaining > 0 {
            self.current_remaining = self.current_remaining.saturating_sub(1);
        }
        self.current_remaining == 0
    }

    /// Check if any task is ready to run.
    #[must_use]
    #[allow(dead_code)] // Used by idle loop to decide hlt vs spin.
    pub fn has_ready(&self) -> bool {
        self.bitmap != 0
    }

    /// Check if any task above IDLE_PRIORITY is ready.
    ///
    /// The idle task (priority 31) is always in the queue when its CPU
    /// is running, but it shouldn't count as "real work" for load
    /// balance decisions.  Without this distinction, a CPU with only
    /// its idle task never triggers work stealing — it looks busy.
    #[must_use]
    pub fn has_real_work(&self) -> bool {
        // Mask out the IDLE_PRIORITY bit (bit 31) from the bitmap.
        let idle_bit = 1u32 << super::task::IDLE_PRIORITY;
        (self.bitmap & !idle_bit) != 0
    }

    /// Set the time slice for a specific priority level.
    ///
    /// `level` must be in `0..NUM_PRIORITIES` and `ticks` must be at
    /// least 1 (a zero-tick time slice would starve the task).
    ///
    /// Returns `true` on success, `false` if the level is out of range
    /// or ticks is 0.
    pub fn set_time_slice(&mut self, level: usize, ticks: u32) -> bool {
        if level >= NUM_PRIORITIES || ticks == 0 {
            return false;
        }
        self.time_slices[level] = ticks;
        true
    }

    /// Get the current time slice for a priority level (in timer ticks).
    ///
    /// Returns `None` if the level is out of range.
    #[must_use]
    pub fn time_slice(&self, level: usize) -> Option<u32> {
        self.time_slices.get(level).copied()
    }

    /// Reconfigure all time slices with a new base and increment.
    ///
    /// Formula: `time_slice[level] = base + level * increment`.
    /// Both `base` and `increment` must be >= 1.
    ///
    /// Returns `true` on success.
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
    ///
    /// Reconfigures all time slices to the profile's base and increment.
    /// The currently-running task's remaining slice is NOT changed — the
    /// new configuration takes effect on the next `pick_next()`.
    pub fn apply_profile(&mut self, profile: WorkloadProfile) {
        // apply_profile delegates to reconfigure_slices, which always
        // succeeds because profile bases are all >= 1.
        let ok = self.reconfigure_slices(profile.base(), profile.increment());
        debug_assert!(ok, "WorkloadProfile base must be >= 1");
    }

    /// Count the total number of tasks across all priority queues.
    ///
    /// Used by work stealing to find the longest queue (most loaded CPU).
    #[must_use]
    pub fn total_tasks(&self) -> usize {
        if self.bitmap == 0 {
            return 0;
        }
        let mut total = 0usize;
        let mut bits = self.bitmap;
        while bits != 0 {
            let level = bits.trailing_zeros() as usize;
            if let Some(q) = self.queues.get(level) {
                total = total.saturating_add(q.len());
            }
            bits &= bits.wrapping_sub(1); // clear lowest set bit
        }
        total
    }

    /// Steal up to `count` tasks from the back of the highest-priority
    /// non-empty queue.  Returns stolen (task_id, priority) pairs.
    ///
    /// Steals from the back (most recently enqueued) to minimize
    /// cache disruption — the front tasks are likely cache-warm on
    /// the victim CPU.
    pub fn steal(&mut self, count: usize) -> alloc::vec::Vec<(super::task::TaskId, u8)> {
        let mut stolen = alloc::vec::Vec::new();
        if count == 0 || self.bitmap == 0 {
            return stolen;
        }

        let mut remaining = count;
        let mut bits = self.bitmap;
        while bits != 0 && remaining > 0 {
            let level = bits.trailing_zeros() as usize;
            if let Some(q) = self.queues.get_mut(level) {
                // Steal from the back of this priority queue.
                while remaining > 0 && !q.is_empty() {
                    if let Some(id) = q.pop_back() {
                        #[allow(clippy::cast_possible_truncation)]
                        stolen.push((id, level as u8));
                        remaining = remaining.saturating_sub(1);
                    }
                }
                if q.is_empty() {
                    self.bitmap &= !(1 << level);
                }
            }
            bits &= bits.wrapping_sub(1);
        }

        stolen
    }
}

// ---------------------------------------------------------------------------
// Per-CPU scheduler (multi-CPU wrapper with work stealing)
// ---------------------------------------------------------------------------

/// Maximum number of CPUs supported.
///
/// Sized for desktop/workstation use (design spec targets x86_64 desktops).
/// 16 CPUs covers 8-core/16-thread consumer CPUs with headroom.
/// Server-class systems with more cores can increase this constant.
///
/// Keep this small enough that `PerCpuScheduler` (~900 bytes per CPU
/// entry) doesn't blow kernel stacks when allocated in tests.
pub const MAX_CPUS: usize = 16;

/// Multi-CPU scheduler with per-CPU run queues and work stealing.
///
/// Each CPU has its own independent [`PriorityRoundRobin`] queue set.
/// When a CPU's queues are empty, it steals tasks from the most-loaded
/// CPU's queues (work stealing).  This avoids global lock contention
/// while maintaining load balance.
///
/// ## Work Stealing Algorithm
///
/// 1. CPU tries its own queue first (fast path, no cross-CPU interaction).
/// 2. If local queue is empty, scan all other CPUs to find the one with
///    the most queued tasks (the "victim").
/// 3. Steal half the victim's tasks (amortizes migration overhead).
/// 4. Stolen tasks are placed in the stealing CPU's local queue.
///
/// ## Cache Warmth
///
/// Each task tracks `last_cpu`.  When enqueuing, the task is placed on
/// its `last_cpu` queue when possible (cache-warm scheduling).  Stolen
/// tasks update `last_cpu` to the new CPU.
///
/// ## Locking
///
/// The entire `PerCpuScheduler` is currently under the global `SCHED`
/// spinlock (inherited from the single-CPU design).  When SMP is fully
/// implemented, this will be split into per-CPU locks for the fast path
/// with a global lock only for cross-CPU operations (work stealing,
/// profile changes).
/// Per-CPU scheduler with internal per-CPU locks.
///
/// Each CPU has its own `PriorityRoundRobin` run queue protected by
/// an independent spinlock.  This allows multiple CPUs to schedule
/// simultaneously without contending on a global lock — the primary
/// SMP scalability improvement over a single global scheduler lock.
///
/// ## Lock ordering
///
/// When multiple CPU queues must be locked (work stealing), locks are
/// acquired in CPU index order (lower index first) to prevent deadlock.
/// The caller's task-table lock (if any) must be acquired AFTER any
/// per-CPU queue lock:  `RQ[i] < RQ[j] (i < j) < TASK_TABLE`.
pub struct PerCpuScheduler {
    /// Per-CPU run queues, each with its own lock.
    queues: [Mutex<PriorityRoundRobin>; MAX_CPUS],
    /// Number of online (active) CPUs.  Atomic for lock-free reads.
    num_cpus: AtomicUsize,
}

// SAFETY: PerCpuScheduler uses internal Mutex for synchronization.
// All mutable state is behind per-CPU spinlocks.
unsafe impl Sync for PerCpuScheduler {}

impl PerCpuScheduler {
    /// Create a new per-CPU scheduler (const-initializable for static use).
    #[must_use]
    pub const fn new_const() -> Self {
        Self {
            queues: [const { Mutex::new(PriorityRoundRobin::new_const()) }; MAX_CPUS],
            num_cpus: AtomicUsize::new(0),
        }
    }

    /// Initialize with a given number of CPUs.
    ///
    /// Each CPU's queue gets default time slice configuration.
    /// Call once during scheduler init.
    pub fn init(&self, num_cpus: usize) {
        let n = num_cpus.min(MAX_CPUS).max(1);
        self.num_cpus.store(n, Ordering::Release);
        for i in 0..n {
            *self.queues[i].lock() = PriorityRoundRobin::new();
        }
    }

    /// Number of online CPUs.
    #[must_use]
    pub fn num_cpus(&self) -> usize {
        self.num_cpus.load(Ordering::Acquire)
    }

    /// Pick the next task from the given CPU's local queue.
    ///
    /// Does NOT perform work stealing — call [`try_steal`] if this
    /// returns `None` and other CPUs might have work.
    #[must_use]
    pub fn pick_next_local(&self, cpu: usize) -> Option<super::task::TaskId> {
        self.queues.get(cpu)?.lock().pick_next()
    }

    /// Enqueue a task on the specified CPU's run queue.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn enqueue(&self, id: super::task::TaskId, priority: u8, cpu: usize) {
        let n = self.num_cpus.load(Ordering::Relaxed);
        let target = cpu.min(n.saturating_sub(1));
        if let Some(q) = self.queues.get(target) {
            q.lock().enqueue(id, priority);
        }
    }

    /// Dequeue a task from the specified CPU's run queue.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn dequeue(&self, id: super::task::TaskId, priority: u8, cpu: usize) -> bool {
        let n = self.num_cpus.load(Ordering::Relaxed);
        let target = cpu.min(n.saturating_sub(1));
        self.queues.get(target)
            .is_some_and(|q| q.lock().dequeue(id, priority))
    }

    /// Handle a timer tick for the given CPU.
    ///
    /// Returns `true` if the current task's time slice expired.
    ///
    /// Uses `try_lock` so that if the per-CPU lock is held (e.g., by
    /// the SCHED_SOFTIRQ push balancer running with interrupts enabled),
    /// the timer ISR skips this tick instead of deadlocking.  The next
    /// timer tick (10 ms later) will catch up.
    pub fn tick(&self, cpu: usize) -> bool {
        self.queues.get(cpu)
            .and_then(|q| q.try_lock())
            .is_some_and(|mut guard| guard.tick())
    }

    /// Get the remaining ticks for the currently running task on a CPU.
    #[must_use]
    pub fn current_remaining(&self, cpu: usize) -> u32 {
        self.queues.get(cpu)
            .map_or(0, |q| q.lock().current_remaining)
    }

    /// Set the remaining ticks for the currently running task on a CPU.
    pub fn set_current_remaining(&self, cpu: usize, ticks: u32) {
        if let Some(q) = self.queues.get(cpu) {
            q.lock().current_remaining = ticks;
        }
    }

    /// Try to steal work from another CPU.
    ///
    /// Scans all other CPUs (using `try_lock` to avoid blocking),
    /// finds the most loaded one, and steals half its tasks.  The
    /// first stolen task is returned directly (ready to run); remaining
    /// tasks are enqueued on the thief's local queue.
    ///
    /// Also returns the IDs of ALL stolen tasks (via `migrated_out`)
    /// so the caller can update their `last_cpu` in the task table.
    ///
    /// Returns the first stolen task's ID, or `None` if nothing stolen.
    pub fn try_steal(
        &self,
        cpu: usize,
        migrated_out: &mut alloc::vec::Vec<super::task::TaskId>,
    ) -> Option<super::task::TaskId> {
        let n = self.num_cpus.load(Ordering::Relaxed);
        if n <= 1 {
            return None;
        }

        // Phase 1: Find the most loaded CPU (excluding ourselves).
        // Use try_lock to avoid blocking on busy queues.
        let mut victim_cpu = usize::MAX;
        let mut victim_count = 0usize;

        for i in 0..n {
            if i == cpu {
                continue;
            }
            if let Some(q) = self.queues.get(i).and_then(|m| m.try_lock()) {
                let count = q.total_tasks();
                if count > victim_count {
                    victim_count = count;
                    victim_cpu = i;
                }
                // Lock dropped at end of if-let scope.
            }
        }

        if victim_cpu == usize::MAX || victim_count == 0 {
            return None;
        }

        // Phase 2: Steal from victim.
        // Lock only the victim's queue, not our own — avoids holding
        // two locks simultaneously.
        let steal_count = (victim_count / 2).max(1);
        let stolen = self.queues.get(victim_cpu)?.lock().steal(steal_count);
        if stolen.is_empty() {
            return None;
        }

        // Phase 3: Enqueue stolen tasks on our queue.
        // The first stolen task is returned directly (for the caller).
        let mut first = None;
        {
            let mut our_rq = self.queues[cpu].lock();
            for (i, (id, priority)) in stolen.into_iter().enumerate() {
                migrated_out.push(id);
                if i == 0 {
                    first = Some(id);
                } else {
                    our_rq.enqueue(id, priority);
                }
            }
        }

        first
    }

    /// Try to push excess tasks from `cpu` to a lighter CPU.
    ///
    /// This is the push-based complement to [`try_steal`] (pull-based).
    /// `try_steal` runs when a CPU goes idle (reactive); `try_push_balance`
    /// runs periodically on busy CPUs via `SCHED_SOFTIRQ` (proactive).
    ///
    /// Returns a list of `(task_id, target_cpu)` migrations for the
    /// caller to update `last_cpu` fields and send reschedule IPIs.
    ///
    /// # Algorithm
    ///
    /// 1. Count local tasks.  If ≤ 1, nothing to push.
    /// 2. Scan all other CPUs (via `try_lock`) to find the lightest.
    /// 3. If the imbalance (local − lightest) < 2, skip (hysteresis).
    /// 4. Steal from our own queue's back (cache-cold tasks).
    /// 5. Enqueue on the target CPU.
    /// 6. Return migration info.
    ///
    /// # Lock safety
    ///
    /// Never holds two CPU locks simultaneously.  Phase 2 (scan) uses
    /// `try_lock` and drops immediately.  Phase 4 locks local, pops,
    /// drops.  Phase 5 locks target, pushes, drops.
    /// Try to push excess tasks from `cpu` to a lighter CPU.
    ///
    /// # Lock safety
    ///
    /// This runs from softirq context with **interrupts enabled**.
    /// A timer interrupt can fire at any point and call `timer_tick()`,
    /// which acquires the local CPU's per-CPU lock.  To avoid deadlock,
    /// ALL lock acquisitions use `try_lock`.  If any lock is contended
    /// (likely because a timer ISR is accessing it), we bail and retry
    /// on the next balance interval (100 ms).
    pub fn try_push_balance(
        &self,
        cpu: usize,
    ) -> alloc::vec::Vec<(super::task::TaskId, usize)> {
        let mut migrations = alloc::vec::Vec::new();
        let n = self.num_cpus.load(Ordering::Relaxed);
        if n <= 1 {
            return migrations;
        }

        // Phase 1: Count local tasks (excluding idle).
        // MUST use try_lock — timer ISR can acquire this lock.
        let local_count = match self.queues.get(cpu).and_then(|m| m.try_lock()) {
            Some(guard) => {
                if !guard.has_real_work() { 0 } else { guard.total_tasks() }
            }
            None => return migrations, // Lock contended (timer ISR).
        };
        if local_count <= 1 {
            return migrations;
        }

        // Phase 2: Find the lightest CPU (try_lock to avoid blocking).
        let mut lightest_cpu = usize::MAX;
        let mut lightest_count = usize::MAX;
        for i in 0..n {
            if i == cpu {
                continue;
            }
            if let Some(q) = self.queues.get(i).and_then(|m| m.try_lock()) {
                let count = q.total_tasks();
                if count < lightest_count {
                    lightest_count = count;
                    lightest_cpu = i;
                }
            }
        }
        if lightest_cpu == usize::MAX {
            return migrations;
        }

        // Phase 3: Check imbalance.  Need at least 2 tasks difference
        // to avoid oscillation (task bouncing between CPUs every 100ms).
        let imbalance = local_count.saturating_sub(lightest_count);
        if imbalance < 2 {
            return migrations;
        }

        // Phase 4+5: Hold local lock, steal, try to push to target.
        //
        // We hold the local lock throughout so that if the target lock
        // fails, we can safely put tasks back without re-acquiring.
        // This is safe from deadlock because:
        // - timer_tick() uses try_lock for its per-CPU lock
        // - We use try_lock for the target
        // - No other code holds two per-CPU locks simultaneously
        let to_push = (imbalance / 2).max(1).min(4);
        let Some(mut local_guard) = self.queues.get(cpu).and_then(|m| m.try_lock()) else {
            return migrations; // Lock contended (timer ISR).
        };
        let stolen = local_guard.steal(to_push);
        if stolen.is_empty() {
            return migrations;
        }

        // Try to lock target while holding local.
        if let Some(mut target_guard) = self.queues.get(lightest_cpu).and_then(|m| m.try_lock()) {
            // Success: move tasks to target.
            drop(local_guard); // Release local before doing work.
            for &(id, priority) in &stolen {
                target_guard.enqueue(id, priority);
                migrations.push((id, lightest_cpu));
            }
        } else {
            // Target lock contended — put tasks back locally.
            for &(id, priority) in &stolen {
                local_guard.enqueue(id, priority);
            }
            // migrations stays empty — no migration happened.
        }

        migrations
    }

    /// Check if any CPU has ready tasks.
    #[must_use]
    pub fn has_ready(&self) -> bool {
        let n = self.num_cpus.load(Ordering::Relaxed);
        self.queues.iter()
            .take(n)
            .any(|m| m.lock().has_ready())
    }

    /// Check if a specific CPU's local queue has real work
    /// (non-idle tasks).
    ///
    /// Used by the timer tick load balancer: a CPU that only has its
    /// idle task is considered "idle" and should proactively steal.
    /// Uses `try_lock` to avoid deadlock with the softirq push balancer.
    #[must_use]
    pub fn local_has_real_work(&self, cpu: usize) -> bool {
        self.queues.get(cpu)
            .and_then(|m| m.try_lock())
            .is_some_and(|guard| guard.has_real_work())
    }

    /// Check if any *other* CPU has real work that could be stolen.
    ///
    /// Lightweight probe for the timer tick load balancer: returns true
    /// if at least one CPU (other than `cpu`) has non-idle tasks.
    /// Uses `try_lock` to avoid deadlock with the softirq push balancer.
    #[must_use]
    pub fn others_have_real_work(&self, cpu: usize) -> bool {
        let n = self.num_cpus.load(Ordering::Relaxed);
        self.queues.iter()
            .take(n)
            .enumerate()
            .any(|(i, m)| i != cpu && m.try_lock().is_some_and(|g| g.has_real_work()))
    }

    // --- Global configuration (applies to all CPUs) ---

    /// Set time slice for a priority level on all CPUs.
    pub fn set_time_slice(&self, level: usize, ticks: u32) -> bool {
        let n = self.num_cpus.load(Ordering::Relaxed);
        let mut ok = true;
        for q in self.queues.iter().take(n) {
            if !q.lock().set_time_slice(level, ticks) {
                ok = false;
            }
        }
        ok
    }

    /// Get the time slice for a priority level (from CPU 0).
    #[must_use]
    pub fn time_slice(&self, level: usize) -> Option<u32> {
        self.queues.first()?.lock().time_slice(level)
    }

    /// Reconfigure time slices on all CPUs.
    pub fn reconfigure_slices(&self, base: u32, increment: u32) -> bool {
        let n = self.num_cpus.load(Ordering::Relaxed);
        let mut ok = true;
        for q in self.queues.iter().take(n) {
            if !q.lock().reconfigure_slices(base, increment) {
                ok = false;
            }
        }
        ok
    }

    /// Apply a workload profile to all CPUs.
    pub fn apply_profile(&self, profile: WorkloadProfile) {
        let n = self.num_cpus.load(Ordering::Relaxed);
        for q in self.queues.iter().take(n) {
            q.lock().apply_profile(profile);
        }
    }
}
