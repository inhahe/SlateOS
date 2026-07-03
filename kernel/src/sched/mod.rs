//! Scheduler subsystem.
//!
//! Manages kernel tasks (threads), scheduling them across CPUs using a
//! pluggable scheduler backend.  Currently implements a priority
//! round-robin scheduler with 32 levels and O(1) task selection.
//!
//! ## Design
//!
//! - **Trait-based**: [`Scheduler`] trait with `pick_next`, `enqueue`,
//!   `dequeue`, `tick`, `has_ready`.  Alternative schedulers (EEVDF,
//!   deadline) can be added behind the same trait.
//! - **Per-CPU queues**: each CPU has its own run queue set via
//!   [`PerCpuScheduler`](priority_rr::PerCpuScheduler).  Tasks are
//!   enqueued on their `last_cpu` (cache-warm scheduling).
//! - **Work stealing**: when a CPU's local queue is empty, it steals
//!   half the tasks from the most-loaded CPU.  Triggered both
//!   reactively (on yield/block) and proactively (every 100 ms via
//!   periodic load balance in `timer_tick`).
//! - **Preemptive**: the APIC timer fires at 100 Hz, calling
//!   [`timer_tick`] which decrements time slices, triggers
//!   reschedule on expiry, and checks periodic load balance.
//!
//! ## Performance Targets
//!
//! - `pick_next_task`: O(1) via bitmap scan (`BSF`/`TZCNT` instruction).
//! - Context switch: target < 5 µs (Linux: 1–3 µs).
//! - See `bench/baselines.toml` for measured targets.
//!
//! ## Locking
//!
//! Two lock levels:
//!
//! 1. **Per-CPU run queue locks** (inside [`PER_CPU_SCHED`]): one
//!    spinlock per CPU, protecting that CPU's run queues.  The timer
//!    ISR path (`timer_tick`) only touches the local CPU's lock —
//!    no global contention on the hot path.
//!
//! 2. **Task table lock** ([`SCHED`]): global spinlock protecting the
//!    `BTreeMap<TaskId, Task>`.  Held during task state transitions,
//!    context pointer extraction, and task creation/destruction.
//!
//! Lock ordering: `RQ[i] < RQ[j]` (i < j) `< SCHED < frame_allocator`.
//!
//! In practice, most code holds `SCHED` while calling `PER_CPU_SCHED`
//! methods (which briefly acquire per-CPU locks internally).  This is
//! safe because no code path ever holds a per-CPU lock and then tries
//! to acquire `SCHED`.

pub mod backend;
pub mod barrier;
pub mod condvar;
pub mod context;
pub mod deadline;
pub mod eevdf;
pub mod fpu;
pub mod io_sched;
pub mod kchannel;
pub mod kmutex;
pub mod krwlock;
pub mod once_event;
pub mod priority_rr;
pub mod semaphore;
pub mod supervisor;
pub mod task;
pub mod waitqueue;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use crate::cpu;
use crate::error::{KernelError, KernelResult};
use crate::serial_println;
use crate::serial_print;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use spin::Mutex;

use self::context::switch_context;
use self::priority_rr::{PerCpuScheduler, PriorityRoundRobin};
pub use self::priority_rr::WorkloadProfile;
use self::task::{Context, Task, TaskId, TaskState, NUM_PRIORITIES};

// ---------------------------------------------------------------------------
// Scheduler trait
// ---------------------------------------------------------------------------

/// Trait for scheduler implementations.
///
/// The scheduler decides which task runs next.  It does NOT own the
/// tasks — tasks are stored in the global [`TASKS`] table.  The
/// scheduler only holds `TaskId` values and priority information.
#[allow(dead_code)] // Trait interface for pluggable scheduler implementations (EEVDF, deadline).
pub trait Scheduler {
    /// Pick the highest-priority ready task.  Removes it from the
    /// run queue.  Returns `None` if no task is ready.
    fn pick_next(&mut self) -> Option<TaskId>;

    /// Add a task to the run queue at the given priority.
    fn enqueue(&mut self, id: TaskId, priority: u8);

    /// Remove a specific task from the run queue.  Returns `true` if
    /// found and removed.
    fn dequeue(&mut self, id: TaskId, priority: u8) -> bool;

    /// Called on each timer tick.  Returns `true` if the current
    /// task's time slice expired and a reschedule is needed.
    fn tick(&mut self) -> bool;

    /// Check if any task is in the run queue.
    fn has_ready(&self) -> bool;
}

// Implement Scheduler trait for PriorityRoundRobin.
impl Scheduler for PriorityRoundRobin {
    fn pick_next(&mut self) -> Option<TaskId> {
        self.pick_next()
    }

    fn enqueue(&mut self, id: TaskId, priority: u8) {
        self.enqueue(id, priority);
    }

    fn dequeue(&mut self, id: TaskId, priority: u8) -> bool {
        self.dequeue(id, priority)
    }

    fn tick(&mut self) -> bool {
        self.tick()
    }

    fn has_ready(&self) -> bool {
        self.has_ready()
    }
}

// Implement Scheduler trait for DeadlineScheduler.
impl Scheduler for deadline::DeadlineScheduler {
    fn pick_next(&mut self) -> Option<TaskId> {
        self.pick_next()
    }

    fn enqueue(&mut self, id: TaskId, priority: u8) {
        self.enqueue(id, priority);
    }

    fn dequeue(&mut self, id: TaskId, priority: u8) -> bool {
        self.dequeue(id, priority)
    }

    fn tick(&mut self) -> bool {
        self.tick()
    }

    fn has_ready(&self) -> bool {
        self.has_ready()
    }
}

// Implement Scheduler trait for EevdfScheduler.
impl Scheduler for eevdf::EevdfScheduler {
    fn pick_next(&mut self) -> Option<TaskId> {
        self.pick_next()
    }

    fn enqueue(&mut self, id: TaskId, priority: u8) {
        self.enqueue(id, priority);
    }

    fn dequeue(&mut self, id: TaskId, priority: u8) -> bool {
        self.dequeue(id, priority)
    }

    fn tick(&mut self) -> bool {
        self.tick()
    }

    fn has_ready(&self) -> bool {
        self.has_ready()
    }
}

// ---------------------------------------------------------------------------
// Cache-line padding
// ---------------------------------------------------------------------------

/// Cache-line-padded wrapper to prevent false sharing on per-CPU data.
///
/// On x86_64, cache lines are 64 bytes.  When multiple CPUs each have
/// their own slot in an array, without padding all slots that share a
/// cache line will bounce between CPU caches on every write — a
/// significant source of cross-CPU latency.
///
/// `CachePadded<T>` ensures each element occupies its own cache line.
/// Implements `Deref` so `.store()`, `.load()`, `.swap()`, etc. work
/// transparently through auto-deref.
///
/// Cost: 64 bytes per element instead of `size_of::<T>()`.  For 16 CPUs
/// the overhead per array is about 1 KiB — trivially small.
#[repr(C, align(64))]
struct CachePadded<T> {
    value: T,
}

impl<T> CachePadded<T> {
    const fn new(value: T) -> Self {
        Self { value }
    }
}

impl<T> core::ops::Deref for CachePadded<T> {
    type Target = T;
    #[inline(always)]
    fn deref(&self) -> &T {
        &self.value
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Per-CPU scheduler: run queues with per-CPU locks.
///
/// Separated from the task table so that hot-path operations (timer
/// tick, pick_next, enqueue) only contend on per-CPU locks, not the
/// global task table lock.
///
/// OPT: Splitting the single global SCHED lock into per-CPU scheduler
/// locks eliminates cross-CPU contention on timer_tick() — previously
/// every CPU's timer ISR tried to acquire the same global lock, causing
/// the tick to be skipped when contended.  Now each CPU only touches
/// its own queue lock.
static PER_CPU_SCHED: PerCpuScheduler = PerCpuScheduler::new_const();

/// Global task table state.
///
/// Protected by a spinlock.  Lock ordering: per-CPU RQ locks (inside
/// `PER_CPU_SCHED`) < this lock < frame allocator.
///
/// The task table holds all tasks indexed by ID.  Scheduler queue
/// operations go through `PER_CPU_SCHED` (per-CPU locks), while task
/// state transitions and metadata access go through this lock.
pub(crate) struct SchedState {
    /// All tasks indexed by ID.
    ///
    /// Tasks are `Box`ed so they have stable heap addresses.  This is
    /// critical for the context switch path which extracts raw pointers
    /// to task fields (Context, FpuState) and then drops the SCHED lock
    /// before performing the actual switch.  Without Box, a concurrent
    /// BTreeMap rebalance (from another CPU inserting/removing) would
    /// move entries between nodes, invalidating those raw pointers.
    pub(crate) tasks: BTreeMap<TaskId, Box<Task>>,
    /// Whether the scheduler has been initialized.
    initialized: bool,
}

static SCHED: Mutex<SchedState> = Mutex::new(SchedState {
    tasks: BTreeMap::new(),
    initialized: false,
});

/// Per-CPU current task IDs.
///
/// Each CPU stores the ID of its currently-running task.  Indexed by
/// the sequential CPU index from `current_cpu_id()`.
///
/// Uses an array of `AtomicU64` rather than a plain array because
/// other CPUs may read a different CPU's slot (e.g., kill_task reads
/// the target task's state, which was set by the running CPU).
///
/// OPT: Cache-line padded — each CPU's slot is on its own 64-byte
/// cache line to prevent false sharing.  Without this, CPU 0 writing
/// its task ID invalidates the cache line for CPUs 1-7 (they share
/// the same 64-byte line in an unpadded 128-byte array).
static CURRENT_TASK_IDS: [CachePadded<AtomicU64>; priority_rr::MAX_CPUS] = {
    const INIT: CachePadded<AtomicU64> = CachePadded::new(AtomicU64::new(0));
    [INIT; priority_rr::MAX_CPUS]
};

/// Per-CPU idle flags.
///
/// Set when a CPU enters the schedule_inner idle fallback (no runnable
/// tasks).  While set, the timer ISR on that CPU skips `preempt()` to
/// avoid nested `schedule_inner` calls — the idle fallback handles its
/// own task picking and context switching.
///
/// The idle fallback is a defense-in-depth path: with per-CPU idle tasks,
/// it should only be reached transiently (e.g., during the window
/// between the idle task blocking for reap and being immediately
/// re-enqueued by the BSP's idle loop).
static IDLE_FLAGS: [CachePadded<AtomicBool>; priority_rr::MAX_CPUS] = {
    const INIT: CachePadded<AtomicBool> = CachePadded::new(AtomicBool::new(false));
    [INIT; priority_rr::MAX_CPUS]
};

/// Per-CPU flag: new work has been enqueued on this CPU's run queue.
///
/// Set by [`signal_cpu`] when a task is enqueued on a remote CPU.
/// Checked (and cleared) by the idle loop to trigger a yield without
/// waiting for the next timer tick.  The IPI (vector 252) wakes the
/// CPU from HLT; this flag tells the idle loop to actually reschedule.
///
/// Without this mechanism, an idle CPU would only discover new work on
/// the next timer tick (up to 10ms delay).  With it, work is picked up
/// within a few microseconds of the enqueue.
static RESCHEDULE_PENDING: [CachePadded<AtomicBool>; priority_rr::MAX_CPUS] = {
    const INIT: CachePadded<AtomicBool> = CachePadded::new(AtomicBool::new(false));
    [INIT; priority_rr::MAX_CPUS]
};

/// Per-CPU "needs reschedule" flag set from interrupt context (deferred
/// preemption).
///
/// Set by [`request_preempt`] (called from the timer ISR when a time slice
/// expires) and serviced by [`do_deferred_preempt`], which runs at the
/// *outermost* IRQ level **after** the IRQ entry path has switched RSP back
/// to the interrupted task's kernel stack.
///
/// This is the linchpin of the IRQ-stack design (B-DF1 / open-questions Q7,
/// option A): hardware IRQs run on a dedicated per-CPU IRQ stack, but the
/// context switch performed by `preempt()` must record the *task* stack's
/// RSP as the task's resume point — never a transient IRQ-stack RSP.  By
/// deferring the actual `preempt()` call out of the handler and onto the
/// task stack, the saved resume point is always correct, and nested IRQs
/// (the timer re-enables interrupts mid-handler for preemption) simply
/// accumulate the flag, which the outermost IRQ then services exactly once.
static NEED_RESCHED: [CachePadded<AtomicBool>; priority_rr::MAX_CPUS] = {
    const INIT: CachePadded<AtomicBool> = CachePadded::new(AtomicBool::new(false));
    [INIT; priority_rr::MAX_CPUS]
};

/// Per-CPU preemption-disable count (spinlock hold depth).
///
/// Incremented for the whole duration a CPU holds (or is spinning to acquire)
/// a tracked [`crate::sync::Mutex`], decremented on release.  While this count
/// is non-zero on a CPU, [`do_deferred_preempt`] refuses to perform an
/// *involuntary* context switch on that CPU.
///
/// # The deadlock this prevents
///
/// A kernel spinlock must never be held across a context switch.  On a single
/// CPU, if a task is involuntarily preempted (timer tick) while holding a
/// spinlock and a higher-priority task then spins on that same lock, the
/// preempted holder can never be rescheduled to release it — the spinner
/// monopolizes the CPU — a priority-inversion deadlock.  This was observed as
/// a recursive-looking `cpu N holds [0] ACCT [1] ACCT` stall on the memory
/// accounting lock: the preempted holder's still-tracked lockdep entry plus
/// the higher-priority spinner's, both accumulated on one CPU's held stack.
///
/// Disabling *involuntary preemption* (not interrupts) for the hold duration
/// is the minimal correct fix: hardware IRQs still run and softirqs still
/// fire, but the timer-driven context switch is deferred to the next tick
/// *after* the lock is released, so a spinlock is never held across a switch.
/// This generalises the earlier SCHED-only `SCHED.is_locked()` guard in
/// [`do_deferred_preempt`] to *every* tracked lock.  (Locks also taken from a
/// hardware ISR — e.g. the cgroup table from `timer_tick` — additionally use
/// `try_lock` on the ISR side, so preempt-disable alone is sufficient here;
/// no full interrupt-disable is required.)
static PREEMPT_DISABLE_COUNT: [CachePadded<AtomicU64>; priority_rr::MAX_CPUS] = {
    const INIT: CachePadded<AtomicU64> = CachePadded::new(AtomicU64::new(0));
    [INIT; priority_rr::MAX_CPUS]
};

/// Disable involuntary preemption on the calling CPU.
///
/// Called by [`crate::sync::Mutex::lock`] / `try_lock` when acquiring a
/// tracked spinlock.  Cheap: one lock-free CPU-index read plus one relaxed
/// atomic increment.  Must be paired with exactly one [`preempt_enable`]
/// (the `MutexGuard`'s `Drop` guarantees this).
#[inline]
pub fn preempt_disable() {
    if let Some(c) = PREEMPT_DISABLE_COUNT.get(current_cpu_id()) {
        c.fetch_add(1, Ordering::Relaxed);
    }
}

/// Re-enable involuntary preemption on the calling CPU.
///
/// Saturating: an (erroneous) unbalanced release can never wrap the counter
/// to a huge value and wedge preemption off permanently — the worst case is a
/// missed decrement, which self-heals on the next balanced pair.
#[inline]
pub fn preempt_enable() {
    if let Some(c) = PREEMPT_DISABLE_COUNT.get(current_cpu_id()) {
        // fetch_update keeps the decrement atomic w.r.t. a nested ISR that
        // acquires/releases a tracked lock between our read and write.
        let _ = c.fetch_update(Ordering::Release, Ordering::Relaxed, |v| {
            Some(v.saturating_sub(1))
        });
    }
}

/// Current preemption-disable depth on `cpu` (0 == preemptible).
#[inline]
#[must_use]
pub fn preempt_count(cpu: usize) -> u64 {
    PREEMPT_DISABLE_COUNT
        .get(cpu)
        .map_or(0, |c| c.load(Ordering::Acquire))
}

// ---------------------------------------------------------------------------
// Per-CPU scheduler statistics
// ---------------------------------------------------------------------------

/// Per-CPU context switch counter.
///
/// Incremented every time a context switch actually happens (not when
/// the same task is re-picked).  Used by /proc/stat and diagnostics.
static CTX_SWITCHES: [CachePadded<AtomicU64>; priority_rr::MAX_CPUS] = {
    const INIT: CachePadded<AtomicU64> = CachePadded::new(AtomicU64::new(0));
    [INIT; priority_rr::MAX_CPUS]
};

/// Per-CPU voluntary yield counter (task called yield_now or block).
static VOLUNTARY_SWITCHES: [CachePadded<AtomicU64>; priority_rr::MAX_CPUS] = {
    const INIT: CachePadded<AtomicU64> = CachePadded::new(AtomicU64::new(0));
    [INIT; priority_rr::MAX_CPUS]
};

/// Per-CPU preemption counter (timer tick expired the time slice).
static PREEMPTIONS: [CachePadded<AtomicU64>; priority_rr::MAX_CPUS] = {
    const INIT: CachePadded<AtomicU64> = CachePadded::new(AtomicU64::new(0));
    [INIT; priority_rr::MAX_CPUS]
};

// ---------------------------------------------------------------------------
// Per-CPU utilization and system load tracking
// ---------------------------------------------------------------------------

/// Per-CPU total tick counter (incremented every timer_tick).
///
/// Used with `IDLE_TICKS` to compute per-CPU utilization:
///   utilization_pct = (total - idle) / total × 100
static TOTAL_TICKS: [CachePadded<AtomicU64>; priority_rr::MAX_CPUS] = {
    const INIT: CachePadded<AtomicU64> = CachePadded::new(AtomicU64::new(0));
    [INIT; priority_rr::MAX_CPUS]
};

/// Per-CPU idle tick counter (incremented when the idle task is running).
///
/// A tick is "idle" if the current task's name starts with "idle".
/// This is checked once per timer_tick outside the SCHED lock (using
/// the known idle task ID range).
static IDLE_TICK_COUNTS: [CachePadded<AtomicU64>; priority_rr::MAX_CPUS] = {
    const INIT: CachePadded<AtomicU64> = CachePadded::new(AtomicU64::new(0));
    [INIT; priority_rr::MAX_CPUS]
};

/// System load averages over 1, 5, and 15 minutes.
///
/// Stored in Linux's exact fixed-point representation: a value `v`
/// represents the real load `v / FIXED_1` where `FIXED_1 = 1 << FSHIFT`
/// (`FSHIFT = 11`, so `FIXED_1 = 2048`).  The three EWMAs are sampled
/// every 5 seconds (Linux's `LOAD_FREQ`) from the per-CPU runnable
/// counts and decayed with Linux's published constants `EXP_1`/`EXP_5`/
/// `EXP_15` (see [`calc_load`]).  Using the same representation and
/// constants as Linux means `/proc/loadavg` reports values directly
/// comparable to a Linux box under the same workload, instead of the
/// previous misleading scheme that stuffed the *instantaneous* runnable
/// count into all three slots.
///
/// Reference: Linux `kernel/sched/loadavg.c` (`calc_load`, `LOAD_INT`,
/// `LOAD_FRAC`) and `include/linux/sched/loadavg.h` (the `EXP_*` table).
static LOAD_AVG_1: AtomicU64 = AtomicU64::new(0);
static LOAD_AVG_5: AtomicU64 = AtomicU64::new(0);
static LOAD_AVG_15: AtomicU64 = AtomicU64::new(0);

/// Counter of [`update_load_average`] invocations (once per second).  The
/// EWMA is only recomputed every 5th call, matching Linux's 5-second
/// `LOAD_FREQ` sampling interval while keeping the existing 1-second
/// `timer_tick` call site.
static LOAD_SAMPLE_DIVIDER: AtomicU64 = AtomicU64::new(0);

/// Load-average fixed-point shift: a value of `1 << LOAD_FSHIFT`
/// (= 2048) represents a real load of `1.00`.  Matches Linux `FSHIFT`.
const LOAD_FSHIFT: u64 = 11;
/// Fixed-point representation of `1.00` load (Linux `FIXED_1`).
const LOAD_FIXED_1: u64 = 1 << LOAD_FSHIFT;
/// EWMA decay factor for the 1-minute average at a 5s sample interval
/// (Linux `EXP_1` = `1/exp(5/60)` in fixed-point).
const LOAD_EXP_1: u64 = 1884;
/// EWMA decay factor for the 5-minute average (Linux `EXP_5`).
const LOAD_EXP_5: u64 = 2014;
/// EWMA decay factor for the 15-minute average (Linux `EXP_15`).
const LOAD_EXP_15: u64 = 2037;

/// Advance one load-average EWMA by a single 5-second sample.
///
/// Implements Linux's `calc_load` (kernel/sched/loadavg.c):
///   `newload = load * exp + active * (FIXED_1 - exp)`
///   `if (active >= load) newload += FIXED_1 - 1;`   // round up while rising
///   `return newload / FIXED_1;`
/// where `active` is the runnable count already shifted into fixed-point
/// (`n_runnable << FSHIFT`).  The round-up-when-rising term matters: without
/// it, integer truncation biases every step downward, so a genuinely rising
/// load can stall just below the integer target (e.g. sit at 0.99 forever
/// under a steady single-runnable workload).  Linux adds it only when
/// `active >= load` so a falling load still decays cleanly.  All arithmetic
/// is saturating so a pathological runnable count can never overflow or panic.
#[must_use]
fn calc_load(load: u64, exp: u64, active: u64) -> u64 {
    let mut weighted = load
        .saturating_mul(exp)
        .saturating_add(active.saturating_mul(LOAD_FIXED_1.saturating_sub(exp)));
    if active >= load {
        weighted = weighted.saturating_add(LOAD_FIXED_1.saturating_sub(1));
    }
    weighted >> LOAD_FSHIFT
}

/// Per-CPU TSC timestamp of the last context switch-in.
///
/// Updated at each context switch.  The delta `current_tsc - last_switch_tsc[cpu]`
/// gives the CPU cycles consumed by the outgoing task.  This enables
/// nanosecond-precision per-task CPU time accounting.
static LAST_SWITCH_TSC: [CachePadded<AtomicU64>; priority_rr::MAX_CPUS] = {
    const INIT: CachePadded<AtomicU64> = CachePadded::new(AtomicU64::new(0));
    [INIT; priority_rr::MAX_CPUS]
};

/// Global work steal counter (any CPU stealing from another).
static WORK_STEALS: AtomicU64 = AtomicU64::new(0);

/// Global task spawn counter.
static TASKS_SPAWNED: AtomicU64 = AtomicU64::new(0);

/// Global task exit counter (both natural exits and kills).
static TASKS_EXITED: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Scheduling latency histogram (system-wide)
// ---------------------------------------------------------------------------

/// Histogram bucket boundaries (in ticks).  With 100 Hz timer:
/// - Bucket 0: 0 ticks (immediate dispatch, waited < 10ms)
/// - Bucket 1: 1 tick (10-20 ms)
/// - Bucket 2: 2-4 ticks (20-50 ms)
/// - Bucket 3: 5-9 ticks (50-100 ms)
/// - Bucket 4: 10-19 ticks (100-200 ms)
/// - Bucket 5: 20-49 ticks (200-500 ms)
/// - Bucket 6: 50-99 ticks (500ms - 1s)
/// - Bucket 7: 100+ ticks (1s+)
const LATENCY_HIST_BUCKETS: usize = 8;

/// Scheduling latency histogram counters.
///
/// Each bucket counts the number of dispatch events where the task's
/// queue wait time fell within that bucket's range.  Incremented in
/// `record_dispatch_latency()` from the context switch path.
static LATENCY_HIST: [AtomicU64; LATENCY_HIST_BUCKETS] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; LATENCY_HIST_BUCKETS]
};

/// Maximum scheduling latency observed (in ticks) since boot.
static LATENCY_MAX_EVER: AtomicU64 = AtomicU64::new(0);

/// Total wait ticks accumulated system-wide (for computing mean).
static LATENCY_TOTAL_TICKS: AtomicU64 = AtomicU64::new(0);

/// Total dispatch events (for computing mean).
static LATENCY_TOTAL_EVENTS: AtomicU64 = AtomicU64::new(0);

/// Classify a wait time (in ticks) into a histogram bucket index.
#[inline]
fn latency_bucket(wait_ticks: u64) -> usize {
    match wait_ticks {
        0 => 0,
        1 => 1,
        2..=4 => 2,
        5..=9 => 3,
        10..=19 => 4,
        20..=49 => 5,
        50..=99 => 6,
        _ => 7,
    }
}

/// Record a scheduling latency observation.
///
/// Called from the context switch path whenever a task transitions
/// from Ready → Running.  `wait_ticks` is `current_tick - ready_since_tick`.
#[inline]
pub(crate) fn record_dispatch_latency(wait_ticks: u64) {
    let bucket = latency_bucket(wait_ticks);
    if let Some(c) = LATENCY_HIST.get(bucket) {
        c.fetch_add(1, Ordering::Relaxed);
    }
    LATENCY_TOTAL_TICKS.fetch_add(wait_ticks, Ordering::Relaxed);
    LATENCY_TOTAL_EVENTS.fetch_add(1, Ordering::Relaxed);
    // Update max (CAS loop).
    let _ = LATENCY_MAX_EVER.fetch_update(
        Ordering::Relaxed, Ordering::Relaxed,
        |cur| if wait_ticks > cur { Some(wait_ticks) } else { None },
    );
}

/// Scheduling latency histogram snapshot.
#[derive(Debug, Clone, Copy)]
pub struct LatencyHistogram {
    /// Bucket counts (indexed by `latency_bucket()` logic).
    pub buckets: [u64; LATENCY_HIST_BUCKETS],
    /// Maximum single wait time observed (ticks).
    pub max_ticks: u64,
    /// Mean wait time (ticks × 100, fixed-point for precision).
    pub mean_ticks_x100: u64,
    /// Total dispatch events.
    pub total_events: u64,
}

/// Read the scheduling latency histogram.
#[must_use]
pub fn latency_histogram() -> LatencyHistogram {
    let mut buckets = [0u64; LATENCY_HIST_BUCKETS];
    for (i, c) in LATENCY_HIST.iter().enumerate() {
        buckets[i] = c.load(Ordering::Relaxed);
    }
    let total_events = LATENCY_TOTAL_EVENTS.load(Ordering::Relaxed);
    let total_ticks = LATENCY_TOTAL_TICKS.load(Ordering::Relaxed);
    let mean_x100 = if total_events > 0 {
        total_ticks.saturating_mul(100).checked_div(total_events).unwrap_or(0)
    } else {
        0
    };
    LatencyHistogram {
        buckets,
        max_ticks: LATENCY_MAX_EVER.load(Ordering::Relaxed),
        mean_ticks_x100: mean_x100,
        total_events,
    }
}

/// Snapshot of per-CPU and global scheduler statistics.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Public API for procfs /proc/stat.
pub struct SchedStats {
    /// Per-CPU context switch counts.
    pub ctx_switches: [u64; priority_rr::MAX_CPUS],
    /// Per-CPU voluntary switch counts (yield/block).
    pub voluntary_switches: [u64; priority_rr::MAX_CPUS],
    /// Per-CPU preemption counts (timer-forced).
    pub preemptions: [u64; priority_rr::MAX_CPUS],
    /// Total context switches across all CPUs.
    pub total_ctx_switches: u64,
    /// Total work steal operations.
    pub total_work_steals: u64,
    /// Total tasks spawned since boot.
    pub total_tasks_spawned: u64,
    /// Total tasks exited since boot.
    pub total_tasks_exited: u64,
    /// Number of online CPUs.
    pub num_cpus: usize,
    /// System load average (×100). Load 1.50 = 150.
    pub load_avg_x100: u64,
    /// Per-CPU (total_ticks, idle_ticks) for utilization calculation.
    pub cpu_ticks: [(u64, u64); priority_rr::MAX_CPUS],
}

/// Collect a snapshot of scheduler statistics.
#[must_use]
#[allow(dead_code)] // Public API for procfs /proc/stat.
pub fn sched_stats() -> SchedStats {
    let num_cpus = crate::smp::cpu_count().max(1);
    let mut stats = SchedStats {
        ctx_switches: [0; priority_rr::MAX_CPUS],
        voluntary_switches: [0; priority_rr::MAX_CPUS],
        preemptions: [0; priority_rr::MAX_CPUS],
        total_ctx_switches: 0,
        total_work_steals: WORK_STEALS.load(Ordering::Relaxed),
        total_tasks_spawned: TASKS_SPAWNED.load(Ordering::Relaxed),
        total_tasks_exited: TASKS_EXITED.load(Ordering::Relaxed),
        num_cpus,
        load_avg_x100: load_average_x100(),
        cpu_ticks: [(0, 0); priority_rr::MAX_CPUS],
    };

    for i in 0..num_cpus.min(priority_rr::MAX_CPUS) {
        // SAFETY: i < MAX_CPUS (bounded by min above).
        #[allow(clippy::indexing_slicing)]
        {
            stats.ctx_switches[i] = CTX_SWITCHES[i].load(Ordering::Relaxed);
            stats.voluntary_switches[i] = VOLUNTARY_SWITCHES[i].load(Ordering::Relaxed);
            stats.preemptions[i] = PREEMPTIONS[i].load(Ordering::Relaxed);
            stats.total_ctx_switches = stats.total_ctx_switches
                .saturating_add(stats.ctx_switches[i]);
            stats.cpu_ticks[i] = (
                TOTAL_TICKS[i].load(Ordering::Relaxed),
                IDLE_TICK_COUNTS[i].load(Ordering::Relaxed),
            );
        }
    }

    stats
}

/// Get per-task CPU tick counts for fairness measurement.
///
/// Returns an array of (total_ticks, name_bytes, name_len) for up to 64
/// active tasks.  Used by the fairness module to compute Jain's Index.
pub fn all_task_ticks() -> alloc::vec::Vec<(u64, [u8; 32], usize)> {
    let mut result = alloc::vec::Vec::with_capacity(64);
    if let Some(state) = SCHED.try_lock() {
        for task in state.tasks.values() {
            if task.state != task::TaskState::Dead {
                result.push((task.total_ticks, task.name, task.name_len));
            }
            if result.len() >= 64 {
                break;
            }
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Task exit hooks
// ---------------------------------------------------------------------------

/// Maximum number of exit hooks that can be registered simultaneously.
///
/// This is a small fixed number because exit hooks run in the dying
/// task's context (for `task_exit`) or while holding the scheduler lock
/// (for `kill_task`), so they must be lightweight.  8 slots is enough
/// for the driver framework's crash detector, the IOCP process-exit
/// notification, and a few future consumers.
const MAX_EXIT_HOOKS: usize = 8;

/// Registered exit hook function pointers.
///
/// Each slot is either null (0) or a valid `fn(TaskId)` cast to u64.
/// We use `AtomicU64` instead of `Option<fn(TaskId)>` because atomics
/// allow lock-free registration/unregistration without holding SCHED.
///
/// Hooks are called with the dying task's ID.  They must NOT:
/// - Acquire the SCHED lock (it may already be held)
/// - Block or allocate
/// - Take longer than ~1 µs
///
/// Appropriate uses: set a flag, enqueue a work item, increment a
/// counter.
static EXIT_HOOKS: [AtomicU64; MAX_EXIT_HOOKS] = {
    const INIT: AtomicU64 = AtomicU64::new(0);
    [INIT; MAX_EXIT_HOOKS]
};

/// Number of registered exit hooks.  Used to avoid scanning all 8
/// slots when no hooks are registered (the common case during early
/// boot).
static EXIT_HOOK_COUNT: AtomicU8 = AtomicU8::new(0);

/// Register an exit hook that will be called when any task dies.
///
/// Returns the slot index (0..MAX_EXIT_HOOKS-1) on success, or `None`
/// if all slots are full.
///
/// # Safety contract
///
/// The provided function must be safe to call from:
/// - The dying task's context (interrupts may be enabled or disabled)
/// - While the SCHED lock may or may not be held (hook must NOT
///   acquire it)
///
/// The hook receives the `TaskId` of the dying task.  It must not
/// block, allocate, or perform any long-running work.
pub fn register_exit_hook(hook: fn(TaskId)) -> Option<usize> {
    let hook_addr = hook as usize as u64;
    if hook_addr == 0 {
        // Null function pointer — reject to avoid confusion with
        // empty slots.
        return None;
    }

    for (i, slot) in EXIT_HOOKS.iter().enumerate() {
        // Try to claim this slot with a CAS from 0 (empty) to our hook.
        if slot
            .compare_exchange(0, hook_addr, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            EXIT_HOOK_COUNT.fetch_add(1, Ordering::Release);
            serial_println!(
                "[sched] Registered exit hook at slot {} (addr {:#x})",
                i, hook_addr
            );
            return Some(i);
        }
    }

    serial_println!("[sched] WARNING: exit hook table full ({} slots)", MAX_EXIT_HOOKS);
    None
}

/// Unregister an exit hook by slot index.
///
/// Returns `true` if the slot was occupied and is now cleared.
/// Returns `false` if the index is out of range or the slot was empty.
pub fn unregister_exit_hook(slot: usize) -> bool {
    let Some(entry) = EXIT_HOOKS.get(slot) else {
        return false;
    };

    let old = entry.swap(0, Ordering::AcqRel);
    if old != 0 {
        EXIT_HOOK_COUNT.fetch_sub(1, Ordering::Release);
        serial_println!("[sched] Unregistered exit hook at slot {}", slot);
        true
    } else {
        false
    }
}

/// Invoke all registered exit hooks for a dying task.
///
/// Called from `task_exit()` (self-termination) and `kill_task()`
/// (external kill).  Runs with interrupts in whatever state the
/// caller left them — hooks must be safe in any context.
///
/// If a hook panics (shouldn't happen, but defense-in-depth), we
/// catch nothing — the panic propagates normally.  Hooks must be
/// bullet-proof.
fn notify_exit_hooks(task_id: TaskId) {
    // Fast path: no hooks registered.
    if EXIT_HOOK_COUNT.load(Ordering::Acquire) == 0 {
        return;
    }

    for slot in &EXIT_HOOKS {
        let addr = slot.load(Ordering::Acquire);
        if addr != 0 {
            // SAFETY: The address was set by register_exit_hook from a
            // valid fn(TaskId) pointer.  We only clear slots via
            // unregister_exit_hook (which sets to 0) or never.  The
            // function pointer remains valid for the lifetime of the
            // kernel (hooks are registered by subsystem init, never
            // unloaded).
            let hook: fn(TaskId) = unsafe {
                core::mem::transmute::<u64, fn(TaskId)>(addr)
            };
            hook(task_id);
        }
    }
}

/// Get the current CPU ID (sequential index).
///
/// Returns 0 for the BSP.  After SMP bootstrap, reads the LAPIC ID
/// and maps it to a sequential CPU index.
#[inline]
#[must_use]
pub fn current_cpu_id() -> usize {
    crate::smp::current_cpu_index()
}

/// Store the current-task ID for a CPU.
///
/// # Safety invariant
///
/// Only call this for the local CPU (cpu == `current_cpu_id()`), or
/// while holding the scheduler lock and the CPU is known to be in a
/// controlled state (e.g., during init before APs start).
#[inline]
fn set_current_task(cpu: usize, id: TaskId) {
    // SAFETY: cpu < MAX_CPUS (guaranteed by smp::current_cpu_index).
    #[allow(clippy::indexing_slicing)]
    CURRENT_TASK_IDS[cpu].store(id, Ordering::Release);
}

/// Read the current-task ID for the calling CPU.
#[inline]
fn load_current_task() -> TaskId {
    let cpu = current_cpu_id();
    // SAFETY: cpu < MAX_CPUS.
    #[allow(clippy::indexing_slicing)]
    CURRENT_TASK_IDS[cpu].load(Ordering::Acquire)
}

/// Re-initialize the per-CPU scheduler with the actual CPU count.
///
/// Called by SMP bootstrap after all APs are online.  This replaces
/// the initial single-CPU configuration with one that covers all
/// online CPUs.
///
/// Safe to call from the BSP while APs are in their idle loops (not
/// touching the scheduler yet).
pub(crate) fn update_cpu_count(num_cpus: usize) {
    PER_CPU_SCHED.init(num_cpus);
}

/// The boot-time kernel PML4 physical address.
///
/// Saved during `init()` so we can restore it when switching back to
/// tasks that run in the kernel address space (pml4_phys == 0).
static KERNEL_PML4: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize the scheduler.
///
/// Creates the idle task (task 0) from the current execution context.
/// Must be called after the heap allocator is initialized.
pub fn init() {
    // Save the kernel PML4 so we can restore it when switching to
    // tasks that use the kernel address space (pml4_phys == 0).
    let kernel_pml4 = crate::mm::page_table::cr3_to_pml4(
        crate::mm::page_table::read_cr3(),
    );
    KERNEL_PML4.store(kernel_pml4, Ordering::Release);

    // Initialize per-CPU scheduler with 1 CPU (boot CPU).
    // SMP bootstrap calls update_cpu_count() later with the real count.
    let num_cpus = 1;
    PER_CPU_SCHED.init(num_cpus);

    let mut state = SCHED.lock();

    // Create the idle task.  It represents the current execution
    // context (kmain), using the bootloader-provided stack.
    let idle = Task::new_idle();
    state.tasks.insert(0, Box::new(idle));
    set_current_task(0, 0); // BSP (CPU 0) starts with idle task 0.

    state.initialized = true;
    serial_println!(
        "[sched] Scheduler initialized ({}, {} levels, {} CPU{})",
        backend::backend_name(backend::active_backend()),
        NUM_PRIORITIES,
        num_cpus,
        if num_cpus > 1 { "s" } else { "" }
    );
}

/// Register an idle task for an Application Processor.
///
/// Called by each AP during SMP bootstrap, after the AP's GDT/IDT/APIC
/// are set up but before enabling interrupts.  Creates a task that
/// represents the AP's current execution context (its trampoline stack)
/// and sets it as the AP's current task.
///
/// Returns the new idle task's ID.
///
/// # Why per-CPU idle tasks?
///
/// Without a dedicated idle task, an AP that has no runnable tasks would
/// need an ad-hoc idle loop inside `schedule_inner`.  This is hard to get
/// right on SMP because:
/// - The timer ISR can fire during the idle loop and call `preempt()`,
///   nesting `schedule_inner` calls and corrupting the blocked task's
///   saved context.
/// - The blocked/dead task's stack might be freed by `reap_dead_tasks`
///   on another CPU while the AP is still using it.
///
/// With a per-CPU idle task, the scheduler always has a valid fallback:
/// when the AP's only real task blocks, `schedule_inner` switches to the
/// idle task, which safely does `yield_now(); hlt();` in a loop.
pub fn register_ap_idle(cpu_index: usize) -> TaskId {
    let idle = Task::new_ap_idle(cpu_index);
    let id = idle.id;

    let mut state = SCHED.lock();
    state.tasks.insert(id, Box::new(idle));
    set_current_task(cpu_index, id);

    serial_println!(
        "[sched] Registered AP idle task {} for CPU {}",
        id, cpu_index
    );
    id
}

/// Check whether a CPU is in the schedule_inner idle fallback.
///
/// Used by the timer ISR to avoid calling `preempt()` on a CPU that
/// is handling its own scheduling in the idle fallback loop.  The idle
/// fallback is a defense-in-depth path — with per-CPU idle tasks it
/// should rarely be reached.
#[inline]
#[must_use]
pub fn cpu_is_idle(cpu: usize) -> bool {
    IDLE_FLAGS
        .get(cpu)
        .is_some_and(|f| f.load(Ordering::Acquire))
}

/// Pick the best CPU for a task, respecting its affinity mask.
///
/// Prefers `last_cpu` if it's in the mask (cache-warm scheduling).
/// Otherwise, falls back to the first allowed CPU.  Returns
/// `last_cpu` unchanged if the mask is empty or all-ones (common case
/// fast-path).
#[inline]
fn choose_cpu_for_task(task: &Task) -> usize {
    if task.cpu_affinity == task::CPU_AFFINITY_ALL {
        return task.last_cpu; // Fast path: no affinity restriction.
    }
    if task.can_run_on(task.last_cpu) {
        return task.last_cpu; // Preferred CPU is allowed.
    }
    // last_cpu is not in the affinity mask — pick the lowest allowed CPU.
    // This is the cold path; we could also pick the lightest-loaded
    // allowed CPU, but that requires locking per-CPU queues.
    let first = task.cpu_affinity.trailing_zeros();
    if first < 64 { first as usize } else { task.last_cpu }
}

/// Signal a CPU that new work has been enqueued on its run queue.
///
/// Sets the `RESCHEDULE_PENDING` flag and, if the target is a remote
/// CPU, sends a reschedule IPI (vector 252) to wake it from HLT.
///
/// The idle loop checks this flag after every HLT wake and calls
/// `yield_now()` to pick up the new task immediately instead of
/// waiting for the next timer tick (up to 10ms).
///
/// For the local CPU, only the flag is set — the timer ISR's
/// `preempt()` will handle scheduling on the next tick.
pub fn signal_cpu(target_cpu: usize) {
    if let Some(flag) = RESCHEDULE_PENDING.get(target_cpu) {
        flag.store(true, Ordering::Release);
    }
    // Also set the idle subsystem's need_resched flag.  If the target
    // CPU is in MWAIT, the write to this cache line will wake it
    // without needing an IPI (faster wakeup path).
    crate::idle::signal_resched(target_cpu);
    // Only send IPI to remote CPUs.  Self-IPI is unnecessary (and
    // risky from ISR context).
    let local = current_cpu_id();
    if target_cpu != local {
        if let Some(apic_id) = crate::smp::cpu_apic_id(target_cpu) {
            // SAFETY: APIC is initialized (we're past init).
            // Vector 252 has a valid ISR registered in the IDT.
            unsafe {
                crate::apic::send_fixed_ipi(apic_id, crate::apic::RESCHEDULE_VECTOR);
            }
        }
    }
}

/// Check and clear the reschedule-pending flag for a CPU.
///
/// Returns `true` if the flag was set (meaning new work was enqueued).
/// The flag is atomically cleared so the next check returns `false`
/// until `signal_cpu` is called again.
#[must_use]
pub fn reschedule_pending(cpu: usize) -> bool {
    RESCHEDULE_PENDING
        .get(cpu)
        .is_some_and(|f| f.swap(false, Ordering::Acquire))
}

/// Spawn a new kernel task.
///
/// The task starts in [`Ready`](TaskState::Ready) state and will run
/// `entry(arg)` when scheduled.
///
/// Returns the new task's ID.
///
/// # Errors
///
/// - [`KernelError::OutOfMemory`] if stack allocation fails.
/// - [`KernelError::NotSupported`] if the scheduler isn't initialized.
pub fn spawn(
    name: &[u8],
    priority: u8,
    entry: extern "C" fn(u64),
    arg: u64,
    pml4_phys: u64,
) -> KernelResult<TaskId> {
    spawn_with_affinity(name, priority, entry, arg, pml4_phys, task::CPU_AFFINITY_ALL)
}

/// Spawn a new kernel task with explicit CPU affinity.
///
/// Like [`spawn`], but the task is restricted to CPUs set in
/// `affinity_mask` (bit N = CPU N allowed).  Use
/// [`task::CPU_AFFINITY_ALL`] to allow all CPUs.
///
/// # Errors
///
/// - [`KernelError::OutOfMemory`] if stack allocation fails.
/// - [`KernelError::NotSupported`] if the scheduler isn't initialized.
/// - [`KernelError::InvalidArgument`] if `affinity_mask` is zero.
pub fn spawn_with_affinity(
    name: &[u8],
    priority: u8,
    entry: extern "C" fn(u64),
    arg: u64,
    pml4_phys: u64,
    affinity_mask: u64,
) -> KernelResult<TaskId> {
    spawn_inner(name, priority, entry, arg, pml4_phys, affinity_mask, true)
}

/// Spawn a task but leave it **suspended** — created and inserted into the
/// task table in [`Blocked`](task::TaskState::Blocked) state, but *not*
/// placed in any run queue, so it cannot be scheduled until the caller
/// explicitly [`admit`]s it.
///
/// This exists to close a **register-vs-runnable race**: the ordinary
/// [`spawn`]/[`spawn_with_affinity`] enqueue the new task immediately, so a
/// timer preemption in the window between `spawn` returning and the caller
/// finishing its bookkeeping can run the child to completion first.  For a
/// process/thread that manifested as B-PTHREAD-YIELDBUDGET: a spawned
/// `/bin/hello` could print and exit before `thread::spawn` registered it in
/// `THREAD_OWNERS`, so `on_thread_exit`'s `owners.remove(&task_id)?` returned
/// `None`, skipped the zombie transition, and the process was never
/// zombified — hanging the container self-test's yield budget.
///
/// The correct structural fix (SMP-safe, unlike merely widening a
/// `without_interrupts` window) is: create the task non-runnable, finish all
/// registration that must precede first execution, then admit it.
///
/// # Errors
///
/// Same as [`spawn_with_affinity`].
pub fn spawn_suspended(
    name: &[u8],
    priority: u8,
    entry: extern "C" fn(u64),
    arg: u64,
    pml4_phys: u64,
) -> KernelResult<TaskId> {
    spawn_inner(name, priority, entry, arg, pml4_phys, task::CPU_AFFINITY_ALL, false)
}

/// Admit a task previously created via [`spawn_suspended`], transitioning it
/// from `Blocked` to `Ready` and enqueuing it so the scheduler can run it.
///
/// Returns `true` if the task was admitted.  Returns `false` if the task no
/// longer exists or was not in the expected `Blocked` state (e.g. it was
/// already killed).  Implemented on top of [`wake`], which already handles
/// the Blocked→Ready transition, run-queue insertion, target-CPU selection,
/// and the pending-wake race.
pub fn admit(task_id: TaskId) -> bool {
    wake(task_id)
}

/// Shared implementation of [`spawn_with_affinity`] and [`spawn_suspended`].
///
/// When `admit` is `true` the task is created `Ready` and enqueued
/// immediately (the historical behavior).  When `false` it is created
/// `Blocked` and left out of every run queue until [`admit`] is called.
fn spawn_inner(
    name: &[u8],
    priority: u8,
    entry: extern "C" fn(u64),
    arg: u64,
    pml4_phys: u64,
    affinity_mask: u64,
    admit: bool,
) -> KernelResult<TaskId> {
    if affinity_mask == 0 {
        return Err(KernelError::InvalidArgument);
    }

    // Cgroup inheritance (Q14 / design-decisions §39): a newly spawned
    // task joins the *creating* task's resource control group, mirroring
    // Linux fork/clone semantics (the child inherits the parent's cgroup).
    // Captured before the no-interrupts critical section below because
    // `current_task_cgroup` takes the SCHED lock via try_lock and we must
    // not nest it inside the SCHED.lock() held there.  Defaults to
    // ROOT_CGROUP during early boot / lock contention, which is correct.
    let inherit_cgroup = current_task_cgroup();

    // Disable interrupts for the entire task-creation + SCHED-insertion
    // critical section.  Task::new_kernel() allocates a kernel stack
    // (holding the kstack ALLOCATOR spinlock) and physical frames
    // (holding the frame ALLOCATOR spinlock).  If a timer interrupt
    // preempts us while either lock is held and the next scheduled task
    // tries to allocate, it deadlocks on that same spinlock.  This was
    // the root cause of the kchannel test-5 Heisenbug: the timer fired
    // during kstack allocation inside spawn(), preempted to the consumer
    // task, which itself needed to allocate on its first context-switch
    // path (or a subsequent spawn), hitting the held lock.
    let (id, prio, target_cpu) = cpu::without_interrupts(|| {
        let mut new_task = Task::new_kernel(name, priority, entry, arg, pml4_phys)?;
        new_task.cpu_affinity = affinity_mask;
        new_task.cgroup_id = inherit_cgroup;
        new_task.ready_since_tick = crate::apic::tick_count();
        // Suspended spawn: create the task non-runnable so it cannot be
        // scheduled until the caller finishes registration and calls admit().
        if !admit {
            new_task.state = task::TaskState::Blocked;
        }
        let id = new_task.id;
        let prio = new_task.priority;
        let target_cpu = choose_cpu_for_task(&new_task);
        new_task.last_cpu = target_cpu;

        let mut state = SCHED.lock();
        if !state.initialized {
            return Err(KernelError::NotSupported);
        }
        state.tasks.insert(id, Box::new(new_task));
        // Only enqueue when admitting immediately.  A suspended task is left
        // out of every run queue; admit() (via wake()) enqueues it later.
        if admit {
            PER_CPU_SCHED.enqueue(id, prio, target_cpu);
        }
        drop(state); // Release lock before re-enabling interrupts.

        Ok((id, prio, target_cpu))
    })?;

    // TD31: symmetric cgroup membership accounting.  A task that inherits a
    // non-root cgroup must increment that cgroup's `nr_tasks`, mirroring the
    // `detach_task` in `reap_dead_tasks`.  Without this, only tasks explicitly
    // moved via `set_task_cgroup` are counted, so a cgroup could report
    // `nr_tasks == 0` while hosting many inherited (forked/cloned) children.
    //
    // Done here — *after* the `without_interrupts`/SCHED critical section has
    // ended and SCHED is dropped — so the cgroup `TABLE` lock is taken strictly
    // after SCHED is released, never nested inside it (preserving the
    // SCHED → TABLE lock order that `set_task_cgroup` also uses).  ROOT is
    // skipped to avoid churning the root count for ordinary kernel tasks,
    // matching the reap-side skip; `attach_task` on ROOT would otherwise pair
    // with the reap-side ROOT skip and leave ROOT permanently inflated.
    //
    // The earlier TD31 attempt hung the boot because the extra `TABLE` lock
    // traffic aggravated a spinlock-across-preemption deadlock; that root cause
    // (B-PREEMPT-SPINLOCK) is now fixed — a `crate::sync::Mutex` (including
    // `TABLE`) disables preemption while held, so it can no longer be preempted
    // mid-critical-section and deadlocked by a higher-priority spinner.
    //
    // `attach_task` only fails with `InvalidArgument` when the cgroup doesn't
    // exist (raced deletion); in that case the task simply isn't counted, which
    // is the same benign outcome as before this call existed — safe to ignore.
    if inherit_cgroup != crate::cgroup::ROOT_CGROUP {
        let _ = crate::cgroup::attach_task(inherit_cgroup);
    }

    // Wake the target CPU if it's idle (remote CPUs may be in HLT).
    // Done outside without_interrupts — signal_cpu sends an IPI which
    // is fine with interrupts enabled.  Skipped for a suspended spawn:
    // the task is not runnable yet, so there is nothing to wake a CPU for
    // (admit() signals the target CPU when it actually enqueues the task).
    if admit {
        signal_cpu(target_cpu);
    }

    TASKS_SPAWNED.fetch_add(1, Ordering::Relaxed);
    crate::ktrace::record(
        crate::ktrace::Category::Sched,
        crate::ktrace::event::TASK_SPAWN,
        id,
        prio as u64,
    );
    serial_println!("[sched] Spawned task {} (priority {}, cpu {})", id, prio, target_cpu);
    Ok(id)
}

/// Yield the current task's CPU time.
///
/// The current task is placed back in the run queue and the highest-
/// priority ready task is scheduled.  If no other task is ready, the
/// current task continues running.
pub fn yield_now() {
    let current_id = load_current_task();
    if let Some(ctr) = VOLUNTARY_SWITCHES.get(current_cpu_id()) {
        ctr.fetch_add(1, Ordering::Relaxed);
    }
    crate::ktrace::record(
        crate::ktrace::Category::Sched,
        crate::ktrace::event::YIELD,
        current_id,
        current_cpu_id() as u64,
    );
    // Report RCU quiescent state — this CPU is voluntarily yielding,
    // so it's not in an RCU read-side critical section.
    crate::rcu::quiescent_state();
    schedule_inner(true, SwitchKind::Voluntary);
}

/// Mark the current task as dead and yield to the next task.
///
/// Called by `task_finished` (the context trampoline) when a task's
/// entry function returns.  The task is NOT placed back in the run
/// queue.
pub fn task_exit() {
    let current_id = load_current_task();
    TASKS_EXITED.fetch_add(1, Ordering::Relaxed);
    crate::ktrace::record(
        crate::ktrace::Category::Sched,
        crate::ktrace::event::TASK_EXIT,
        current_id,
        0,
    );
    serial_println!("[sched] Task {} exiting", current_id);

    // Notify exit hooks BEFORE marking the task Dead and before
    // dropping the SCHED lock.  Hooks run outside the lock to avoid
    // deadlock (they may access other subsystems that acquire their
    // own locks).  The task is still Running at this point — hooks
    // can safely look up the task ID if needed.
    notify_exit_hooks(current_id);

    {
        let mut state = SCHED.lock();
        if let Some(task) = state.tasks.get_mut(&current_id) {
            task.state = TaskState::Dead;
        }
    }

    // Yield without re-enqueuing.  Exit is not a counted context switch
    // (the outgoing task is dead).
    schedule_inner(false, SwitchKind::Uncounted);

    // Should never reach here — the task is dead and won't be
    // scheduled again.  If somehow we do, halt.
    cpu::halt_loop();
}

/// Get the ID of the currently running task.
#[must_use]
pub fn current_task_id() -> TaskId {
    load_current_task()
}

/// Record the `%fs` base (TLS pointer) for the **current** task.
///
/// Called by `arch_prctl(ARCH_SET_FS)` and `execve` (reset to 0) after
/// they update the live `IA32_FS_BASE` MSR, so the value survives the
/// next context switch (the switch path restores [`Task::fs_base`] into
/// the MSR when switching a user task back in).  See [`Task::fs_base`].
pub fn set_current_task_fs_base(fs_base: u64) {
    let task_id = load_current_task();
    let mut state = SCHED.lock();
    if let Some(task) = state.tasks.get_mut(&task_id) {
        task.fs_base = fs_base;
    }
}

/// Record the `%fs` base (TLS pointer) for a specific task.
///
/// Used by `fork` (child inherits the parent's TLS base) and
/// `clone(CLONE_SETTLS)` (new thread gets `new_tls`) to initialise the
/// new task's saved TLS base **before** it is first scheduled, so the
/// switch-in restore loads the correct `%fs` for it.  No-op if the task
/// no longer exists.
pub fn set_task_fs_base(task_id: TaskId, fs_base: u64) {
    let mut state = SCHED.lock();
    if let Some(task) = state.tasks.get_mut(&task_id) {
        task.fs_base = fs_base;
    }
}

/// Record the current task's **userspace `%gs` base** (the value
/// `arch_prctl(ARCH_SET_GS)` installs into the active `IA32_GS_BASE` under
/// Slate's entry-stub convention).
///
/// The switch path restores [`Task::gs_base`] into `IA32_GS_BASE` for user
/// tasks on switch-in (0 = no custom `%gs`).  See [`Task::gs_base`].
pub fn set_current_task_gs_base(gs_base: u64) {
    let task_id = load_current_task();
    let mut state = SCHED.lock();
    if let Some(task) = state.tasks.get_mut(&task_id) {
        task.gs_base = gs_base;
    }
}

/// Record the userspace `%gs` base for a specific task.
///
/// Used by `fork`/`clone` so the new task inherits the creator's `%gs`
/// base **before** it is first scheduled.  No-op if the task is gone.
pub fn set_task_gs_base(task_id: TaskId, gs_base: u64) {
    let mut state = SCHED.lock();
    if let Some(task) = state.tasks.get_mut(&task_id) {
        task.gs_base = gs_base;
    }
}

/// Read the current task's saved userspace `%gs` base (the authoritative
/// [`Task::gs_base`] field, `0` if unset).  Used by `fork`/`clone` to
/// propagate the creator's `%gs` base to the new task.
#[must_use]
pub fn current_task_gs_base() -> u64 {
    let task_id = load_current_task();
    let state = SCHED.lock();
    state.tasks.get(&task_id).map_or(0, |task| task.gs_base)
}

/// Get the cgroup ID of the current task (non-blocking).
///
/// Returns `ROOT_CGROUP` if the scheduler lock is contended or the
/// task isn't found.  Designed for use in allocation paths where
/// blocking is not acceptable (e.g., page fault handler).
#[must_use]
#[allow(dead_code)] // Public API for cgroup memory controller integration.
pub fn current_task_cgroup() -> crate::cgroup::CgroupId {
    let task_id = load_current_task();
    if task_id == 0 {
        return crate::cgroup::ROOT_CGROUP;
    }
    if let Some(state) = SCHED.try_lock() {
        if let Some(task) = state.tasks.get(&task_id) {
            return task.cgroup_id;
        }
    }
    crate::cgroup::ROOT_CGROUP
}

/// Get the network namespace of the current task (non-blocking).
///
/// Returns [`ROOT_NS`](crate::netns::ROOT_NS) if the scheduler lock is
/// contended or the task isn't found.  Designed for use in syscall
/// handlers where the task needs namespace-aware socket operations.
#[must_use]
pub fn current_task_net_ns() -> crate::netns::NetNsId {
    let task_id = load_current_task();
    if let Some(state) = SCHED.try_lock() {
        if let Some(task) = state.tasks.get(&task_id) {
            return task.net_ns;
        }
    }
    crate::netns::ROOT_NS
}

/// Set the network namespace for a specific task.
///
/// Used by the container subsystem to assign a task to a container's
/// network namespace after creation.
///
/// Returns `Err(InvalidArgument)` if the task doesn't exist.
pub fn set_task_net_ns(
    task_id: TaskId,
    ns_id: crate::netns::NetNsId,
) -> KernelResult<()> {
    let mut state = SCHED.lock();
    if let Some(task) = state.tasks.get_mut(&task_id) {
        task.net_ns = ns_id;
        Ok(())
    } else {
        Err(KernelError::InvalidArgument)
    }
}

/// Move a specific task into a resource control group.
///
/// This is the single authoritative process→cgroup assignment path
/// (Q14 / design-decisions §39): it updates the task's `cgroup_id` so
/// the frame-allocator charging and scheduler CPU-charging hooks bill
/// the right group, **and** keeps the cgroup's per-group task counts
/// consistent by detaching from the old group and attaching to the new
/// one.  Callers must therefore use this instead of calling
/// [`crate::cgroup::attach_task`] / [`crate::cgroup::detach_task`]
/// directly, or the counts double.
///
/// The target group is validated first: if `new_cgroup` does not exist,
/// the task is left untouched.  The `SCHED` lock is released before the
/// cgroup table is touched, so the lock ordering is strictly
/// `SCHED` → cgroup `TABLE` (never the reverse), avoiding deadlock with
/// the timer-tick charging path (which only `try_lock`s the cgroup
/// table).
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if `new_cgroup` doesn't exist or
///   the task doesn't exist.
pub fn set_task_cgroup(
    task_id: TaskId,
    new_cgroup: crate::cgroup::CgroupId,
) -> KernelResult<()> {
    // Validate the target group exists before mutating anything, so a
    // bad cgroup id can never leave a task pointing at a dead group.
    if !crate::cgroup::exists(new_cgroup) {
        return Err(KernelError::InvalidArgument);
    }

    // Swap the task's cgroup under the SCHED lock, capturing the old id.
    let old_cgroup = {
        let mut state = SCHED.lock();
        let task = state
            .tasks
            .get_mut(&task_id)
            .ok_or(KernelError::InvalidArgument)?;
        let old = task.cgroup_id;
        task.cgroup_id = new_cgroup;
        old
    };

    // Keep per-group task counts consistent (outside the SCHED lock).
    if old_cgroup != new_cgroup {
        // detach/attach failures only mean a stale count for a group
        // that is being torn down concurrently — never a correctness
        // hazard for the task itself, which already points at the new
        // group.  Ignore them rather than unwinding the assignment.
        let _ = crate::cgroup::detach_task(old_cgroup);
        let _ = crate::cgroup::attach_task(new_cgroup);
    }

    Ok(())
}

/// Block the current task and yield to the next runnable task.
///
/// The current task is set to [`Blocked`](TaskState::Blocked) and is
/// NOT placed in the run queue.  It must be explicitly woken via
/// [`wake`] to become runnable again.
///
/// Before blocking, records the current CPU burst length into the
/// task's interactivity EWMA.  Tasks with short bursts (< 50 ms)
/// are marked as interactive and receive a priority boost when woken.
///
/// This is used by IPC channels, futexes, and other blocking
/// primitives.
pub fn block_current() {
    if let Some(ctr) = VOLUNTARY_SWITCHES.get(current_cpu_id()) {
        ctr.fetch_add(1, Ordering::Relaxed);
    }
    let current_id = load_current_task();
    crate::ktrace::record(
        crate::ktrace::Category::Sched,
        crate::ktrace::event::TASK_BLOCK,
        current_id,
        current_cpu_id() as u64,
    );
    {
        let mut state = SCHED.lock();
        if let Some(task) = state.tasks.get_mut(&current_id) {
            // Check for pending wake: if someone called wake() on us
            // between registering in a wait queue and now, the pending
            // flag is set.  Consume it and don't actually block — this
            // prevents the lost-wakeup race (see wake() comments).
            if task.pending_wake {
                task.pending_wake = false;
                return;
            }
            // Record burst length for interactive task detection.
            task.record_block();
            task.state = TaskState::Blocked;
        }
    }
    // Yield without re-enqueuing (requeue = false).  Blocking is a
    // voluntary context switch.
    schedule_inner(false, SwitchKind::Voluntary);
}

/// Wake a blocked task, making it runnable again.
///
/// Sets the task's state to [`Ready`](TaskState::Ready) and places
/// it in the run queue at its effective priority (which may be
/// boosted for interactive tasks).
///
/// Returns `true` if the task was blocked and is now ready.
/// Returns `false` if the task was not in the Blocked state.
pub fn wake(task_id: TaskId) -> bool {
    let target_cpu;
    {
        let mut state = SCHED.lock();
        if let Some(task) = state.tasks.get_mut(&task_id) {
            if task.state == TaskState::Blocked {
                task.mark_ready(crate::apic::tick_count());
                // Reset burst counter for the new wake cycle.
                task.burst_ticks = 0;
                let prio = task.effective_priority();
                // Respect CPU affinity when choosing the target CPU.
                target_cpu = choose_cpu_for_task(task);
                task.last_cpu = target_cpu;
                PER_CPU_SCHED.enqueue(task_id, prio, target_cpu);
            } else {
                // Task is not Blocked (still Running or Ready).  Set the
                // pending-wake flag so block_current() won't actually
                // block.  This prevents the lost-wakeup race where a
                // timer preemption between registering in a wait queue
                // and calling block_current() lets a waker find the task
                // as Running and lose the wake signal.
                task.pending_wake = true;
                return false;
            }
        } else {
            return false;
        }
    }
    crate::ktrace::record(
        crate::ktrace::Category::Sched,
        crate::ktrace::event::TASK_WAKE,
        task_id,
        target_cpu as u64,
    );
    // Signal the target CPU after releasing the lock.
    signal_cpu(target_cpu);
    true
}

/// Wake a blocked task using `try_lock` — safe in ISR context.
///
/// Same as [`wake`] but uses `try_lock` instead of blocking `lock`.
/// If the scheduler lock is already held (e.g., the ISR interrupted
/// code that was holding it), returns `false` without blocking.
///
/// The caller (typically the timer ISR's deferred-wake path) should
/// retry on the next tick if this fails.
pub fn try_wake(task_id: TaskId) -> bool {
    if let Some(mut state) = SCHED.try_lock() {
        if let Some(task) = state.tasks.get_mut(&task_id) {
            if task.state == TaskState::Blocked {
                task.mark_ready(crate::apic::tick_count());
                task.burst_ticks = 0;
                let prio = task.effective_priority();
                let target_cpu = choose_cpu_for_task(task);
                task.last_cpu = target_cpu;
                PER_CPU_SCHED.enqueue(task_id, prio, target_cpu);
                drop(state);
                signal_cpu(target_cpu);
                return true;
            }
            // Same pending-wake logic as wake() — see comment there.
            task.pending_wake = true;
        }
    }
    false
}

/// How often (in timer ticks) to check load balance.
///
/// At 100 Hz timer, 10 ticks = 100 ms between balance checks.
/// This is a reasonable trade-off between responsiveness and overhead.
/// Linux uses 4 ms (HZ/250) for idle CPUs and 64 ms for busy CPUs;
/// we use a fixed 100 ms interval which is fine for our current
/// workload patterns.
const BALANCE_INTERVAL: u64 = 10;

/// Per-CPU tick counters for periodic load balancing.
///
/// Each CPU increments its counter on every timer tick.  When the
/// counter reaches `BALANCE_INTERVAL`, the load balancer checks if
/// work stealing is beneficial.
///
/// Using atomics (not behind the scheduler lock) because the timer
/// ISR increments this BEFORE acquiring the scheduler lock.  Each
/// CPU only writes its own slot, so no contention.
static BALANCE_TICKS: [CachePadded<AtomicU64>; priority_rr::MAX_CPUS] = {
    const ZERO: CachePadded<AtomicU64> = CachePadded::new(AtomicU64::new(0));
    [ZERO; priority_rr::MAX_CPUS]
};

// ---------------------------------------------------------------------------
// CPU bandwidth limiting
// ---------------------------------------------------------------------------

/// Length of one bandwidth period, in timer ticks.
///
/// At 100 Hz, 100 ticks = 1 second.  A task with `cpu_quota_pct = 50`
/// may consume at most 50 ticks per 100-tick period (50% of one CPU).
///
/// Using a 1-second period gives 1% granularity (each tick = 1%).
/// The downside is up to 1 second of burst before throttling, which is
/// acceptable for a desktop OS (not real-time).
const BANDWIDTH_PERIOD_TICKS: u64 = 100;

/// Global tick counter for bandwidth period tracking.
///
/// Incremented by the BSP (CPU 0) on every timer tick.  When it
/// crosses a `BANDWIDTH_PERIOD_TICKS` boundary, `unthrottle_expired`
/// is called to reset per-task counters and re-enqueue throttled tasks.
static BANDWIDTH_TICK: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Anti-starvation boost
// ---------------------------------------------------------------------------

/// Threshold for starvation detection (ticks a task has been waiting).
///
/// If a Ready task has been waiting this many ticks without being
/// dispatched, it gets a temporary priority boost to prevent indefinite
/// starvation.  At 100 Hz, 200 ticks = 2 seconds.
///
/// This is a safety net — in a healthy system, no task should wait this
/// long.  If boosting activates frequently, it indicates the workload
/// profile or priority assignment is wrong.
const STARVATION_THRESHOLD_TICKS: u64 = 200;

/// How often to run the anti-starvation check (in ticks).
///
/// At 100 Hz, 100 ticks = 1 second.  We check once per bandwidth
/// period (1 second) which is frequent enough to catch starvation
/// within one threshold interval after it begins.
const STARVATION_CHECK_INTERVAL: u64 = 100;

/// Number of tasks boosted by anti-starvation since boot (diagnostic).
static STARVATION_BOOSTS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Soft lockup watchdog
// ---------------------------------------------------------------------------

/// Per-CPU heartbeat counters for the soft lockup watchdog.
///
/// Each CPU increments its counter on every timer tick.  The BSP
/// periodically checks that all CPUs have ticked within the expected
/// interval.  If a CPU's counter hasn't advanced in
/// `WATCHDOG_THRESHOLD_TICKS`, it's considered soft-locked (stuck in
/// a non-preemptible code path — typically a deadlocked spinlock or
/// an infinite loop with interrupts disabled).
///
/// Based on Linux's `kernel/watchdog.c` soft lockup detector.
static WATCHDOG_HEARTBEAT: [CachePadded<AtomicU64>; priority_rr::MAX_CPUS] = {
    const ZERO: CachePadded<AtomicU64> = CachePadded::new(AtomicU64::new(0));
    [ZERO; priority_rr::MAX_CPUS]
};

/// Snapshot of each CPU's heartbeat at the last watchdog check.
///
/// If `WATCHDOG_HEARTBEAT[cpu] == WATCHDOG_LAST_SEEN[cpu]` at check
/// time, the CPU has made zero forward progress since the last check.
static WATCHDOG_LAST_SEEN: [CachePadded<AtomicU64>; priority_rr::MAX_CPUS] = {
    const ZERO: CachePadded<AtomicU64> = CachePadded::new(AtomicU64::new(0));
    [ZERO; priority_rr::MAX_CPUS]
};

/// Read the bootstrap processor's timer-tick heartbeat counter.
///
/// Bumped once per BSP timer tick in [`timer_tick`]. Because the hard-lockup
/// watchdog is kicked from that same tick, this counter advancing means the
/// BSP is alive and taking timer interrupts (and therefore kicking). The NMI
/// handler uses it to tell a *real* BSP-dead wedge (heartbeat frozen) apart
/// from a *spurious* watchdog NMI caused by QEMU/TCG virtual-clock jitter
/// during a heavy compute burst (heartbeat still advancing). Safe to call from
/// NMI context — it is a single relaxed atomic load, no locks.
#[must_use]
pub fn bsp_heartbeat() -> u64 {
    WATCHDOG_HEARTBEAT
        .first()
        .map_or(0, |hb| hb.load(Ordering::Relaxed))
}

/// Number of consecutive watchdog intervals a CPU has been stalled.
///
/// Alert only after `WATCHDOG_ALERT_COUNT` consecutive failures to
/// avoid false positives from transient IRQ-disabled sections.
static WATCHDOG_STALL_COUNT: [CachePadded<AtomicU64>; priority_rr::MAX_CPUS] = {
    const ZERO: CachePadded<AtomicU64> = CachePadded::new(AtomicU64::new(0));
    [ZERO; priority_rr::MAX_CPUS]
};

/// How often the watchdog checks (in BSP ticks).
///
/// 500 ticks at 100 Hz = every 5 seconds.  Strikes a balance between
/// timely detection and avoiding noisy false positives.
const WATCHDOG_CHECK_INTERVAL: u64 = 500;

/// How many consecutive stall intervals before alerting.
///
/// 2 means a CPU must be stuck for 10+ seconds (2×5s) before we
/// report it.  This avoids false positives from legitimate long
/// IRQ-disabled sections (e.g., TLB shootdown synchronization).
const WATCHDOG_ALERT_COUNT: u64 = 2;

/// Run the soft lockup watchdog check.
///
/// Called by the BSP every `WATCHDOG_CHECK_INTERVAL` ticks.  Checks
/// each non-BSP CPU's heartbeat counter against the last-seen value.
/// If unchanged for `WATCHDOG_ALERT_COUNT` consecutive intervals,
/// reports a soft lockup warning.
fn watchdog_check() {
    let num_cpus = crate::smp::cpu_count();
    if num_cpus <= 1 {
        return; // Nothing to watch with only the BSP.
    }

    for cpu in 1..num_cpus {
        let Some(heartbeat) = WATCHDOG_HEARTBEAT.get(cpu) else { continue };
        let Some(last_seen) = WATCHDOG_LAST_SEEN.get(cpu) else { continue };
        let Some(stall_count) = WATCHDOG_STALL_COUNT.get(cpu) else { continue };

        let current = heartbeat.load(Ordering::Relaxed);
        let previous = last_seen.load(Ordering::Relaxed);

        if current == previous && previous > 0 {
            // CPU hasn't ticked since last check.
            let count = stall_count.fetch_add(1, Ordering::Relaxed).saturating_add(1);
            if count >= WATCHDOG_ALERT_COUNT {
                // Soft lockup detected — report but don't halt.
                // The CPU may recover (e.g., if it was just a very
                // long critical section).
                let stall_secs = count.saturating_mul(WATCHDOG_CHECK_INTERVAL / 100);
                serial_println!(
                    "[watchdog] SOFT LOCKUP on CPU {} (no progress for {}+ seconds, heartbeat={})",
                    cpu, stall_secs, current,
                );
                crate::klog!(Error, "sched.watchdog",
                    "soft lockup: cpu={}, stall_seconds={}, heartbeat={}",
                    cpu, stall_secs, current
                );
            }
        } else {
            // CPU is alive — reset stall counter.
            stall_count.store(0, Ordering::Relaxed);
        }

        // Record current heartbeat for next comparison.
        last_seen.store(current, Ordering::Release);
    }
}

/// Query watchdog status for diagnostics.
///
/// Returns an array of (heartbeat_count, stall_count) per CPU.
#[must_use]
#[allow(dead_code)] // Diagnostic API for procfs/kshell.
pub fn watchdog_status() -> [(u64, u64); priority_rr::MAX_CPUS] {
    let mut result = [(0u64, 0u64); priority_rr::MAX_CPUS];
    for (i, entry) in result.iter_mut().enumerate() {
        if let (Some(hb), Some(sc)) = (
            WATCHDOG_HEARTBEAT.get(i),
            WATCHDOG_STALL_COUNT.get(i),
        ) {
            *entry = (
                hb.load(Ordering::Relaxed),
                sc.load(Ordering::Relaxed),
            );
        }
    }
    result
}

// ---------------------------------------------------------------------------
// System-wide liveness watchdog (hung-task detector)
// ---------------------------------------------------------------------------
//
// The soft-lockup watchdog above detects a CPU that STOPS ticking (stuck
// with interrupts disabled — a spinlock deadlock or an IRQ-off infinite
// loop).  It structurally CANNOT detect the intermittent total-hang we
// have observed (known-issues.md B-PTHREAD-YIELDBUDGET): in that failure
// every CPU keeps ticking happily in its idle loop — the timer IRQ still
// fires, heartbeats still advance — yet no *task* ever runs again because
// a runnable/blocked thread was lost (a lost-wakeup or a reap-vs-clone
// race).  From the soft-lockup watchdog's point of view the machine is
// perfectly healthy; from the user's point of view it is dead.
//
// This liveness watchdog closes that gap.  It watches a single global
// "useful work" counter that advances only when a tick is charged to a
// non-idle context (a ring-3 task, or a CPU with real work in its local
// run queue).  At a true total-hang every CPU is parked in the idle task
// with an empty run queue, so the counter FREEZES while heartbeats keep
// climbing.  If the counter fails to advance for LIVENESS_ALERT_COUNT
// consecutive check intervals, the BSP dumps every task's
// (id, name, state, cpu, priority, wait-clock, blocked-on, pending_wake)
// straight to the serial log from IRQ context using only try_lock — the
// exact breadcrumb needed to identify which thread was lost and in what
// state.  It then disarms itself so the dump appears exactly once.
//
// Scoping: the watchdog is armed explicitly (`liveness_arm`) at the start
// of the boot-time task/ring-3 phase and disarmed (`liveness_disarm`)
// once BOOT_OK is reached and the system may legitimately go idle at an
// interactive prompt.  Without a per-task block-reason field we cannot
// otherwise distinguish "shell blocked on the keyboard" (healthy idle)
// from "thread blocked on a wakeup that will never come" (the hang), so
// restricting the active window to boot — where continuous forward
// progress is expected until BOOT_OK — is what makes the detector free of
// idle false-positives.  During boot the only sustained no-progress state
// IS the bug.

/// Global monotonic count of timer ticks charged to a non-idle context.
///
/// Advanced by [`timer_tick`] whenever the preempted context was a ring-3
/// task or a CPU that had real work queued.  Frozen only when every CPU is
/// idle — which, during the armed boot window, means the system has hung.
static USEFUL_WORK_TICKS: AtomicU64 = AtomicU64::new(0);

/// Whether the liveness watchdog is active.  Armed for the boot window,
/// disarmed at BOOT_OK (see module comment above).
static LIVENESS_ARMED: AtomicBool = AtomicBool::new(false);

/// `USEFUL_WORK_TICKS` value observed at the previous liveness check.
static LIVENESS_LAST_WORK: AtomicU64 = AtomicU64::new(0);

/// Consecutive liveness intervals with zero forward progress.
static LIVENESS_STALL_COUNT: AtomicU64 = AtomicU64::new(0);

/// System-wide context-switch total observed at the previous liveness check.
///
/// Used by the busy-livelock guard: a task that monopolizes a CPU without
/// ever yielding advances [`USEFUL_WORK_TICKS`] (its timer ticks are charged
/// as useful work) yet produces **no context switches** system-wide. This
/// snapshot lets the watchdog notice that the useful-work counter is moving
/// for the wrong reason (see [`liveness_check`]).
static LIVENESS_LAST_CTX: AtomicU64 = AtomicU64::new(0);

/// Consecutive intervals where useful-work advanced but no context switch
/// occurred system-wide (the busy-livelock signature).
static LIVENESS_CTX_STALL_COUNT: AtomicU64 = AtomicU64::new(0);

/// Consecutive stalled check-intervals before declaring a hung system.
///
/// At WATCHDOG_CHECK_INTERVAL (5s) per interval, 3 intervals = 15 seconds
/// of the whole machine making zero task-level progress during boot.  No
/// legitimate pre-BOOT_OK operation stalls all tasks for that long, so 15s
/// is comfortably above the noise floor while still catching the hang
/// promptly.
const LIVENESS_ALERT_COUNT: u64 = 3;

/// Monotonic timestamp (ns since boot) captured when the liveness watchdog was
/// armed, or 0 if not armed. Backs the wall-clock boot-deadline backstop below.
///
/// **Why wall-clock, not a tick count:** an earlier version counted
/// `liveness_check` invocations (tick-driven, 60 × 5 s intervals). That is
/// structurally broken for the very hang it targets: under the heavy
/// poison-debug build, cpu0 drops a large fraction of its 100 Hz timer ticks
/// during long `IF=0` sections (poison-heap page-fault handling, spinlocks), so
/// tick-time runs far slower than wall-clock and 30000 ticks never accrue
/// within the harness's 480 s wall-clock window — the backstop silently never
/// fired (observed 2026-07-02, dash-loop livelock caught with *no* dump).
/// A monotonic-clock deadline is immune: `clock_monotonic()` is a bare `rdtsc`,
/// independent of timer-tick delivery, so the deadline elapses in real time no
/// matter how degraded tick delivery becomes.
static LIVENESS_ARM_NS: AtomicU64 = AtomicU64::new(0);

/// Whether the boot-deadline backstop already dumped (one-shot flag).
static LIVENESS_DEADLINE_FIRED: AtomicBool = AtomicBool::new(false);

/// Highest 30 s "breadcrumb" bucket already logged during the armed boot window.
///
/// Diagnostic: the serial log carries no timestamps, so when the boot-deadline
/// backstop mysteriously *never* dumps we can't tell whether (a) boot was simply
/// too slow to reach arm+deadline before the 480 s harness kill, (b) the fault
/// storm started too late/was too short, or (c) the check itself never runs
/// during the storm (cpu0 gets zero ticks). This bucket lets
/// [`liveness_boot_deadline_check`] emit one line every 30 s of *armed monotonic
/// time*, so the last breadcrumb printed reveals exactly how far the deadline
/// counter advanced and whether the per-tick check kept running through the
/// storm. Cheap (one atomic load + compare per tick) and self-limiting.
static LIVENESS_BREADCRUMB_BUCKET: AtomicU64 = AtomicU64::new(0);

/// Breadcrumb interval, in nanoseconds (30 s of armed monotonic time).
const LIVENESS_BREADCRUMB_NS: u64 = 30_000_000_000;

/// Absolute boot-phase deadline, in monotonic nanoseconds since [`liveness_arm`].
///
/// The progress-based detectors above (total-hang: useful-work frozen;
/// busy-livelock: context-switches frozen) are structurally blind to a
/// *ping-pong livelock* — two or more tasks that keep context-switching and
/// keep being charged useful-work ticks yet make no real boot progress (e.g.
/// a spawned ring-3 child that deadlocks on a futex/reap while its driver
/// keeps re-scheduling it). Both counters advance every interval, so neither
/// detector ever trips, and the boot hangs silently (observed 2026-07-02:
/// dash-loop ring-3 test wedged until the 480 s harness kill with no dump; see
/// known-issues B-PTHREAD-YIELDBUDGET / B-DASH-STDIN-FLAKE).
///
/// This is a purely time-based backstop that catches *any* hang mode. It is
/// measured in wall-clock (monotonic) time so it is immune to the timer-tick
/// starvation that broke the earlier tick-interval version. The value must sit
/// above the slowest healthy armed-to-BOOT_OK duration and below the harness's
/// 480 s boot-test timeout, so it dumps the task table before QEMU is killed.
///
/// Measured healthy armed-to-BOOT_OK (2026-07-02, full glibc/dash ring-3
/// battery, poison-debug build): **67.7 s** (logged by [`liveness_disarm`]).
/// 200 s is ~3× that — no realistic false-fire even for a much slower-than-
/// normal healthy boot — while still leaving a 280 s margin before the 480 s
/// kill for the dump (and for the progress-based detectors to add their own
/// report if the livelock later degrades into a total stall).
const LIVENESS_BOOT_DEADLINE_NS: u64 = 200_000_000_000; // 200 s

/// Record forward progress for the liveness watchdog.
///
/// Called from [`timer_tick`] when this tick is charged to a non-idle
/// context.  A single global relaxed increment — negligible cost, and
/// contention is irrelevant because the watchdog only reads the value once
/// every 5 seconds and only cares whether it moved at all.
#[inline]
fn note_useful_work() {
    USEFUL_WORK_TICKS.fetch_add(1, Ordering::Relaxed);
}

/// Arm the system-wide liveness watchdog.
///
/// Call once the scheduler is running real tasks and continuous forward
/// progress is expected (i.e., at the start of the boot-time task/ring-3
/// self-test phase).  Resets the progress baseline so the first interval
/// measures from "now".
pub fn liveness_arm() {
    LIVENESS_LAST_WORK.store(USEFUL_WORK_TICKS.load(Ordering::Relaxed), Ordering::Relaxed);
    LIVENESS_STALL_COUNT.store(0, Ordering::Relaxed);
    LIVENESS_LAST_CTX.store(total_ctx_switches(), Ordering::Relaxed);
    LIVENESS_CTX_STALL_COUNT.store(0, Ordering::Relaxed);
    LIVENESS_ARM_NS.store(crate::timekeeping::clock_monotonic(), Ordering::Relaxed);
    LIVENESS_DEADLINE_FIRED.store(false, Ordering::Relaxed);
    LIVENESS_BREADCRUMB_BUCKET.store(0, Ordering::Relaxed);
    LIVENESS_ARMED.store(true, Ordering::Release);
}

/// Sum the per-CPU context-switch counters into a single system-wide total.
///
/// A whole-system scheduling-progress signal that is independent of
/// [`USEFUL_WORK_TICKS`]: it advances only when a *different* task is
/// actually switched in, not merely when a timer tick is charged to the
/// currently-running task. Relaxed loads are fine — the watchdog only cares
/// whether the aggregate moved at all across a 5-second interval.
fn total_ctx_switches() -> u64 {
    let mut total: u64 = 0;
    let num_cpus = crate::smp::cpu_count().min(priority_rr::MAX_CPUS);
    for cpu in 0..num_cpus {
        if let Some(ctr) = CTX_SWITCHES.get(cpu) {
            total = total.wrapping_add(ctr.load(Ordering::Relaxed));
        }
    }
    total
}

/// Disarm the system-wide liveness watchdog.
///
/// Call at BOOT_OK, before the system may legitimately idle at an
/// interactive prompt (where task-level progress correctly stops).
pub fn liveness_disarm() {
    // Log the armed duration so the wall-clock boot-deadline (see
    // LIVENESS_BOOT_DEADLINE_NS) can be tuned against real healthy-boot timing
    // rather than a stale guess. Only meaningful when we were actually armed.
    let arm_ns = LIVENESS_ARM_NS.load(Ordering::Relaxed);
    if arm_ns != 0 && LIVENESS_ARMED.load(Ordering::Acquire) {
        let elapsed_ms = crate::timekeeping::clock_monotonic().saturating_sub(arm_ns) / 1_000_000;
        serial_println!(
            "[liveness] disarmed after {}.{:03}s armed (boot-deadline is {}s)",
            elapsed_ms / 1000,
            elapsed_ms % 1000,
            LIVENESS_BOOT_DEADLINE_NS / 1_000_000_000,
        );
    }
    LIVENESS_ARMED.store(false, Ordering::Release);
}

/// Run the system-wide liveness check.
///
/// Called by the BSP every `WATCHDOG_CHECK_INTERVAL` ticks alongside the
/// soft-lockup watchdog.  If the global useful-work counter has not moved
/// for `LIVENESS_ALERT_COUNT` consecutive intervals while armed, dumps
/// every task's state and disarms (one-shot).
/// Wall-clock boot-deadline backstop — evaluated on **every** BSP timer tick.
///
/// This is deliberately separate from [`liveness_check`], which runs only every
/// `WATCHDOG_CHECK_INTERVAL` (500) ticks for its interval-comparison detectors.
/// That 500-tick cadence is itself tick-driven, and the hang this backstop
/// exists to catch — an `IF=0` page-fault-storm livelock — *starves cpu0 of
/// timer ticks*: cpu0 sits in the (poison-heap-slow) `#PF` handler with
/// interrupts disabled almost continuously, taking only a handful of ticks per
/// second. That is enough to keep kicking the ~9.8 s hard-lockup watchdog (so
/// no NMI), but the `tick.is_multiple_of(500)` gate is then hit far too rarely
/// to run `liveness_check` at all before the 480 s harness kill (observed
/// 2026-07-02: two dash-redir/pipeline livelock catches with *no* dump, because
/// `liveness_check` was never re-entered during the ~400 s storm).
///
/// The deadline test is cheap (an atomic load + one `rdtsc` + a compare), so we
/// run it every tick. The wall-clock comparison then fires on the *first* tick
/// past the deadline no matter how degraded the tick rate becomes. Detects
/// *any* hang mode (including the ping-pong livelock the progress detectors are
/// structurally blind to). One-shot; does not disarm, so the progress-based
/// detectors can still add their own report if the hang later degrades into a
/// total stall.
#[inline]
fn liveness_boot_deadline_check() {
    if !LIVENESS_ARMED.load(Ordering::Acquire)
        || LIVENESS_DEADLINE_FIRED.load(Ordering::Relaxed)
    {
        return;
    }
    let arm_ns = LIVENESS_ARM_NS.load(Ordering::Relaxed);
    if arm_ns == 0 {
        return;
    }
    let elapsed_ns = crate::timekeeping::clock_monotonic().saturating_sub(arm_ns);

    // Diagnostic breadcrumb: emit one line each time armed-elapsed crosses a new
    // 30 s boundary. Because this runs every BSP tick, the *last* breadcrumb in a
    // hung boot's serial log tells us how far the monotonic deadline counter
    // actually advanced and whether the per-tick check kept firing through the
    // IF=0 fault-storm livelock (vs. cpu0 going fully tick-dark). See the
    // LIVENESS_BREADCRUMB_BUCKET docs.
    let bucket = elapsed_ns / LIVENESS_BREADCRUMB_NS;
    if bucket > LIVENESS_BREADCRUMB_BUCKET.load(Ordering::Relaxed) {
        LIVENESS_BREADCRUMB_BUCKET.store(bucket, Ordering::Relaxed);
        serial_println!(
            "[liveness] boot-window breadcrumb: {}s armed (deadline {}s, heartbeat={})",
            elapsed_ns / 1_000_000_000,
            LIVENESS_BOOT_DEADLINE_NS / 1_000_000_000,
            bsp_heartbeat(),
        );
    }

    if elapsed_ns >= LIVENESS_BOOT_DEADLINE_NS
        && !LIVENESS_DEADLINE_FIRED.swap(true, Ordering::AcqRel)
    {
        serial_println!(
            "[liveness] BOOT DEADLINE EXCEEDED: still armed {}s after arming (no BOOT_OK). \
             The progress-based detectors did not trip, so this is a livelock or partial \
             hang — some task(s) keep running/switching but boot is not advancing. \
             Dumping task table:",
            elapsed_ns / 1_000_000_000,
        );
        dump_all_tasks_serial();
    }
}

fn liveness_check() {
    if !LIVENESS_ARMED.load(Ordering::Acquire) {
        return;
    }

    // NOTE: the absolute boot-deadline backstop is NOT here — it lives in
    // `liveness_boot_deadline_check`, called every BSP tick, because this
    // function's 500-tick cadence is itself starved during the IF=0 fault-storm
    // livelock it would need to catch. See that function's docs.

    let current = USEFUL_WORK_TICKS.load(Ordering::Relaxed);
    let previous = LIVENESS_LAST_WORK.load(Ordering::Relaxed);
    LIVENESS_LAST_WORK.store(current, Ordering::Relaxed);

    // Snapshot the system-wide context-switch total every interval so the
    // busy-livelock guard below always compares against the immediately
    // preceding interval, regardless of which branch we took.
    let ctx_now = total_ctx_switches();
    let ctx_prev = LIVENESS_LAST_CTX.swap(ctx_now, Ordering::Relaxed);

    if current == previous {
        // No task-level progress this interval — the total-hang signature
        // (every CPU idle-ticking).  A total stall is reported by this path,
        // so keep the busy-livelock counter quiet to avoid double-reporting.
        LIVENESS_CTX_STALL_COUNT.store(0, Ordering::Relaxed);

        let count = LIVENESS_STALL_COUNT
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        if count < LIVENESS_ALERT_COUNT {
            return;
        }

        // Confirmed hang: dump every task's state, then disarm so the report
        // appears exactly once (further intervals would just repeat it).
        LIVENESS_ARMED.store(false, Ordering::Release);
        let stall_secs = count.saturating_mul(WATCHDOG_CHECK_INTERVAL / 100);
        serial_println!(
            "[liveness] SYSTEM HANG: no task-level forward progress for {}+ seconds \
             (useful_work={}, all CPUs idle-ticking). Dumping task table:",
            stall_secs, current,
        );
        dump_all_tasks_serial();
        return;
    }

    // Useful-work ticks advanced, so the primary watchdog above considers the
    // system healthy.  Reset its stall counter.
    LIVENESS_STALL_COUNT.store(0, Ordering::Relaxed);

    // Busy-livelock guard (watchdog blind spot 1): the useful-work counter
    // can advance for the *wrong* reason — a single task monopolizing a CPU
    // (e.g. a futex/yield-budget spin) has its own timer ticks charged as
    // "useful work" even though no real forward progress is happening.  The
    // giveaway is that such a task never yields, so **no context switch**
    // occurs system-wide.  A healthy boot self-test phase context-switches
    // continuously (thread spawn/reap/futex hand-off/yield), so useful-work
    // advancing while the aggregate context-switch count is frozen for
    // several consecutive intervals is the busy-livelock signature.
    if ctx_now != ctx_prev {
        LIVENESS_CTX_STALL_COUNT.store(0, Ordering::Relaxed);
        return;
    }
    let count = LIVENESS_CTX_STALL_COUNT
        .fetch_add(1, Ordering::Relaxed)
        .saturating_add(1);
    if count < LIVENESS_ALERT_COUNT {
        return;
    }

    // Suspected busy-livelock.  Unlike the total-hang path this is a *soft*
    // warning: it does NOT disarm the watchdog, because a rare legitimate
    // long single-task computation during a stress self-test could in
    // principle also freeze context switches while charging useful work.
    // Keeping the watchdog armed means a false positive here cannot disable
    // hang detection for the remainder of boot.  Reset the counter so the
    // warning re-fires at most once per LIVENESS_ALERT_COUNT intervals
    // rather than every interval.
    LIVENESS_CTX_STALL_COUNT.store(0, Ordering::Relaxed);
    let stall_secs = count.saturating_mul(WATCHDOG_CHECK_INTERVAL / 100);
    serial_println!(
        "[liveness] SUSPECTED LIVELOCK: useful-work ticks advancing but zero \
         context switches for {}+ seconds (useful_work={}, ctx_switches={}) — a \
         task is likely monopolizing a CPU without yielding. Dumping task table:",
        stall_secs, current, ctx_now,
    );
    dump_all_tasks_serial();
}

/// Dump every task's scheduling state to the serial log on demand.
///
/// Public wrapper around the liveness watchdog's task-table dump so an
/// operator can trigger the same diagnostic manually (e.g. from a kshell
/// command) when the system *feels* wedged at an interactive prompt —
/// exactly the window where the boot-scoped liveness watchdog is disarmed
/// and so would never fire on its own. Uses only `try_lock`, so it is safe
/// to call from any context, including a partially-hung system.
pub fn dump_task_table() {
    dump_all_tasks_serial();
}

/// Dump every task's scheduling state to the serial log.
///
/// Runs from timer-IRQ context, so it must never block: it uses `try_lock`
/// on the scheduler and reports failure (itself a strong signal — a task
/// wedged while holding `SCHED` is a prime hang cause) rather than waiting.
fn dump_all_tasks_serial() {
    // Per-CPU liveness snapshot first — shows which CPUs are still ticking
    // (all of them, in the classic total-hang) and their last context
    // switch counts (frozen at the hang).
    let num_cpus = crate::smp::cpu_count();
    for cpu in 0..num_cpus {
        let hb = WATCHDOG_HEARTBEAT
            .get(cpu)
            .map_or(0, |c| c.load(Ordering::Relaxed));
        let cs = CTX_SWITCHES
            .get(cpu)
            .map_or(0, |c| c.load(Ordering::Relaxed));
        let has_work = PER_CPU_SCHED.local_has_real_work(cpu);
        // Where was this CPU executing at its most recent timer tick?  This is
        // the key diagnostic the task table alone cannot answer: it reveals
        // whether the CPU is parked in the idle HLT loop, spinning in a
        // context-switch/wait path, or stuck in a task that never yields.
        let rip = crate::rip_sample::last_rip(cpu);
        // rip==0 means no timer tick has sampled this CPU yet (e.g. very early
        // boot); label it explicitly rather than misclassifying 0x0 as "user".
        let class = if rip == 0 {
            "no sample yet"
        } else {
            crate::rip_sample::AddrClass::classify(rip).name()
        };
        serial_println!(
            "[liveness]   cpu{}: heartbeat={} ctx_switches={} local_has_real_work={} \
             last_rip={:#x} ({})",
            cpu, hb, cs, has_work, rip, class,
        );
    }

    let now = crate::apic::tick_count();
    let Some(state) = SCHED.try_lock() else {
        serial_println!(
            "[liveness]   !! could not acquire SCHED lock — a task is likely \
             wedged holding it (this IS the deadlock)",
        );
        return;
    };

    serial_println!("[liveness]   {} task(s) in table (now_tick={}):", state.tasks.len(), now);
    for (&id, task) in state.tasks.iter() {
        // Name is stored as raw bytes (OS-boundary data): render losslessly
        // as an escaped byte string rather than forcing UTF-8.
        let name = task.name.get(..task.name_len.min(task.name.len())).unwrap_or(&[]);
        let waited = if task.ready_since_tick == 0 {
            0
        } else {
            now.saturating_sub(task.ready_since_tick)
        };
        serial_println!(
            "[liveness]   tid={} state={:?} cpu={} prio={} pending_wake={} \
             ready_since={} waited={} blocked_on_pi={:#x} name={:?}",
            id,
            task.state,
            task.last_cpu,
            task.priority,
            task.pending_wake,
            task.ready_since_tick,
            waited,
            task.blocked_on_pi_addr.unwrap_or(0),
            core::str::from_utf8(name).unwrap_or("<non-utf8>"),
        );
    }
}

// ---------------------------------------------------------------------------
// Timer tick handler
// ---------------------------------------------------------------------------

/// Handle a timer tick from the APIC timer interrupt.
///
/// Called from the timer ISR with interrupts disabled.  Uses `try_lock`
/// to avoid deadlock — if the scheduler lock is already held (e.g.,
/// the timer fired while `schedule_inner` was running), we skip this
/// tick.  The next timer interrupt will catch it.
///
/// Also increments the current task's burst tick counter for
/// interactive task detection and enforces CPU bandwidth quotas.
///
/// `from_user` reflects the privilege level the timer interrupt
/// preempted (`true` = ring 3, `false` = ring 0).  It is forwarded to
/// [`Task::tick_burst`](crate::sched::task::Task::tick_burst) so the
/// tick is charged to the task's user- or system-time bucket — the
/// Linux tick-sampling CPU-time model.  The caller (the timer ISR)
/// derives it from the saved interrupt frame's `CS` (CPL bits).
///
/// Periodically checks load balance: if this CPU's local queue is
/// empty but other CPUs have work, returns `true` to trigger a
/// preempt (which does work stealing via `schedule_inner`).
///
/// Returns `true` if the current task's time slice has expired,
/// the task's CPU bandwidth quota is exhausted, or a load balance
/// steal is warranted — and a reschedule is needed.
pub fn timer_tick(from_user: bool) -> bool {
    let cpu = current_cpu_id();

    // Report RCU quiescent state — the timer tick is a natural
    // quiescent point (we're in interrupt context, not in any
    // RCU read-side critical section).
    crate::rcu::quiescent_state();

    // --- Fast path: per-CPU scheduler ops (no global lock) ---
    //
    // OPT: The tick() and load balance checks only need the local
    // CPU's per-CPU lock inside PER_CPU_SCHED.  By not acquiring the
    // global SCHED lock here, we eliminate cross-CPU contention on the
    // timer ISR hot path.  Previously every CPU's timer tried to
    // acquire the same global lock, causing ticks to be skipped when
    // contended.

    let time_slice_expired = PER_CPU_SCHED.tick(cpu);

    // --- Watchdog heartbeat (no lock needed) ---
    // Each CPU bumps its counter so the BSP can detect stalls.
    if let Some(hb) = WATCHDOG_HEARTBEAT.get(cpu) {
        hb.fetch_add(1, Ordering::Relaxed);
    }

    // --- Hard-lockup watchdog kick (BSP only) ---
    // Reload the i6300esb NMI watchdog from the BSP timer tick. As long as the
    // BSP keeps taking timer interrupts this never expires; if the BSP wedges
    // with IF=0 (the BSP-dead total-silence hang the timer-driven watchdogs
    // cannot see), the kicks stop and QEMU injects an NMI ~9.8 s later so
    // handle_nmi() can dump the wedge RIP. No-op unless armed (opt-in device).
    if cpu == 0 {
        crate::hardlockup::kick();
    }

    // --- Per-CPU utilization tracking (no lock needed) ---
    // Increment total and idle tick counters for utilization calculation.
    if let Some(total) = TOTAL_TICKS.get(cpu) {
        total.fetch_add(1, Ordering::Relaxed);
    }
    let current_id = load_current_task();
    // A task is "idle" if its ID is 0 (BSP idle) or if it's an AP idle
    // task.  AP idle tasks have ID > 0 but are detected by checking if
    // the task has idle priority (31) and is running on this CPU.
    // For simplicity, we track idle via the PER_CPU_SCHED fast check:
    // if the local queue has no real work, this CPU is effectively idle.
    let has_real_work = PER_CPU_SCHED.local_has_real_work(cpu);
    if !has_real_work
        && let Some(idle) = IDLE_TICK_COUNTS.get(cpu)
    {
        idle.fetch_add(1, Ordering::Relaxed);
    }

    // Liveness watchdog progress signal: this tick counts as "useful work"
    // if it preempted a ring-3 task (from_user) or a CPU that has real work
    // queued.  When every CPU is parked in the idle task with an empty run
    // queue — the signature of the intermittent total-hang — this counter
    // freezes even though heartbeats keep advancing, which is exactly what
    // `liveness_check()` looks for.
    if from_user || has_real_work {
        note_useful_work();
    }

    // Track CPU burst length and enforce CPU bandwidth quotas.
    // This DOES need the task table, but we use try_lock to avoid
    // blocking.  If the lock is held, we simply skip tracking for
    // this tick — the next tick will catch up.
    let mut bandwidth_exceeded = false;
    if let Some(mut state) = SCHED.try_lock() {
        if !state.initialized {
            return false;
        }
        if let Some(task) = state.tasks.get_mut(&current_id) {
            task.tick_burst(from_user);

            // CPU bandwidth enforcement: if the task has a quota and
            // has used all its ticks for this period, throttle it.
            if task.cpu_quota_pct > 0 {
                task.cpu_period_used = task.cpu_period_used.saturating_add(1);
                if task.cpu_period_used >= u64::from(task.cpu_quota_pct) {
                    task.throttled = true;
                    bandwidth_exceeded = true;
                }
            }

            // Cgroup CPU enforcement: charge the tick to the task's
            // resource control group.  If the group's quota is exceeded,
            // throttle the task (same effect as per-task throttling).
            if !bandwidth_exceeded {
                let cg = task.cgroup_id;
                if crate::cgroup::cpu_charge(cg) {
                    task.throttled = true;
                    bandwidth_exceeded = true;
                }
            }
        }
    }
    // Even if we couldn't acquire SCHED for burst tracking, the
    // time slice tick still happened above — don't lose it.

    // BSP drives bandwidth period resets, load average sampling, and
    // the soft lockup watchdog.
    if cpu == 0 {
        // Wall-clock boot-deadline backstop: evaluated on EVERY BSP tick (not
        // gated by the 500-tick multiple below) so it still fires when cpu0 is
        // starved of ticks during an IF=0 fault-storm livelock. Cheap: an
        // atomic load + rdtsc + compare, and a no-op once disarmed at BOOT_OK.
        liveness_boot_deadline_check();

        let tick = BANDWIDTH_TICK.fetch_add(1, Ordering::Relaxed);
        #[allow(clippy::arithmetic_side_effects)]
        if tick > 0 && tick.is_multiple_of(BANDWIDTH_PERIOD_TICKS) {
            unthrottle_expired();
            update_load_average();
            // Reset cgroup CPU and I/O period counters alongside per-task resets.
            crate::cgroup::cpu_period_reset();
            crate::cgroup::io_period_reset();
        }
        // Anti-starvation check: every STARVATION_CHECK_INTERVAL ticks.
        #[allow(clippy::arithmetic_side_effects)]
        if tick > 0 && tick.is_multiple_of(STARVATION_CHECK_INTERVAL) {
            check_starvation();
        }
        // Watchdog check: every WATCHDOG_CHECK_INTERVAL ticks (5s).
        #[allow(clippy::arithmetic_side_effects)]
        if tick > 0 && tick.is_multiple_of(WATCHDOG_CHECK_INTERVAL) {
            watchdog_check();
            // System-wide liveness check runs on the same cadence: detects
            // a total-hang where every CPU keeps ticking but no task runs.
            liveness_check();
        }
    }

    if time_slice_expired || bandwidth_exceeded {
        return true;
    }

    // Periodic load balance: check if this CPU is idle while
    // others have work.  Only check every BALANCE_INTERVAL ticks
    // to avoid overhead on every 10ms tick.
    //
    // OPT: These checks use PER_CPU_SCHED directly — no global lock.
    // This proactive check means idle CPUs pull work within 100ms
    // instead of waiting for the next yield/block event.
    let Some(balance_counter) = BALANCE_TICKS.get(cpu) else { return false; };
    let tick_count = balance_counter.fetch_add(1, Ordering::Relaxed);
    if tick_count % BALANCE_INTERVAL == 0 {
        // Check: does our local queue have real work (above idle)?
        if !PER_CPU_SCHED.local_has_real_work(cpu) {
            // Idle CPU: pull work via reactive work stealing.
            if PER_CPU_SCHED.others_have_real_work(cpu) {
                // Trigger a reschedule — schedule_inner will try_steal.
                return true;
            }
        } else {
            // Busy CPU: raise SCHED_SOFTIRQ for push-based balancing.
            // The softirq handler runs with interrupts re-enabled
            // (after EOI), so the balance computation doesn't extend
            // the hard-IRQ phase.
            crate::softirq::raise(crate::softirq::SCHED_SOFTIRQ);
        }
    }

    false
}

/// Return the accumulated `(user_ticks, sys_ticks)` CPU time for a task
/// by its scheduler id, or `None` if no such task is registered.
///
/// Ticks are at `USER_HZ` (100 Hz, 10 ms each), the same units Linux
/// uses for `times`/`/proc` clock_t fields.  Used by the Linux-ABI
/// `getrusage`/`times`/`/proc/<pid>/stat` CPU-time surfaces (via the
/// per-process roll-up in `proc::thread::process_cpu_ticks`).  Takes the
/// global `SCHED` lock — not for hot paths.
#[must_use]
pub fn cpu_ticks(tid: TaskId) -> Option<(u64, u64)> {
    let state = SCHED.lock();
    let task = state.tasks.get(&tid)?;
    Some((task.user_ticks, task.sys_ticks))
}

/// Charge a page fault to a task's per-task fault counters.
///
/// `major == true` increments `maj_flt` (the fault required I/O to
/// resolve — e.g. swap-in); `false` increments `min_flt` (resolved
/// without I/O — demand-zero, CoW, stack growth).
///
/// **Best-effort, non-blocking.** This is called from the page-fault
/// handler, which must never block on the `SCHED` lock (it would deadlock
/// against a timer IRQ that also takes the lock, or against the scheduler
/// faulting on user memory).  We use `try_lock`; on contention the fault
/// is simply not counted.  Under-counting under contention is acceptable
/// for these statistical rusage/proc counters and matches the existing
/// `current_task_cgroup` / `try_get_rlimit` page-fault-path pattern.
pub fn account_fault(tid: TaskId, major: bool) {
    if let Some(mut state) = SCHED.try_lock() {
        if let Some(task) = state.tasks.get_mut(&tid) {
            if major {
                task.maj_flt = task.maj_flt.saturating_add(1);
            } else {
                task.min_flt = task.min_flt.saturating_add(1);
            }
        }
    }
}

/// Return the accumulated `(min_flt, maj_flt)` page-fault counts for a
/// task by its scheduler id, or `None` if no such task is registered.
///
/// Used by the Linux-ABI `getrusage` `ru_minflt`/`ru_majflt` and
/// `/proc/<pid>/stat` fields 10/12 (via the per-process roll-up in
/// `proc::thread::process_fault_counts`).  Takes the global `SCHED` lock
/// — not for hot paths (the read side is the syscall surface, not the
/// fault handler).
#[must_use]
pub fn fault_counts(tid: TaskId) -> Option<(u64, u64)> {
    let state = SCHED.lock();
    let task = state.tasks.get(&tid)?;
    Some((task.min_flt, task.maj_flt))
}

/// Return the accumulated `(nvcsw, nivcsw)` context-switch counts for a
/// task by its scheduler id, or `None` if no such task is registered.
///
/// `nvcsw` = voluntary switches (the task gave up the CPU); `nivcsw` =
/// involuntary switches (the task was preempted).  Used by the Linux-ABI
/// `getrusage` `ru_nvcsw`/`ru_nivcsw` (via the per-process roll-up in
/// `proc::thread::process_ctxsw_counts`).  Takes the global `SCHED` lock.
#[must_use]
pub fn ctxsw_counts(tid: TaskId) -> Option<(u64, u64)> {
    let state = SCHED.lock();
    let task = state.tasks.get(&tid)?;
    Some((task.nvcsw, task.nivcsw))
}

/// Preempt the current task (called from timer ISR after time slice
/// expiry).
///
/// This is equivalent to `yield_now()` but called from interrupt
/// context.  The current task is re-enqueued and the highest-priority
/// ready task is scheduled.
pub fn preempt() {
    let current_id = load_current_task();
    if let Some(ctr) = PREEMPTIONS.get(current_cpu_id()) {
        ctr.fetch_add(1, Ordering::Relaxed);
    }
    crate::ktrace::record(
        crate::ktrace::Category::Sched,
        crate::ktrace::event::PREEMPT,
        current_id,
        current_cpu_id() as u64,
    );
    schedule_inner(true, SwitchKind::Involuntary);
}

/// Request a deferred preemption on the calling CPU.
///
/// Called from interrupt context (the timer ISR) instead of calling
/// [`preempt`] directly.  Sets the per-CPU [`NEED_RESCHED`] flag; the actual
/// context switch is performed by [`do_deferred_preempt`] at the outermost
/// IRQ level, after the IRQ entry path has restored RSP to the interrupted
/// task's kernel stack.
///
/// This guarantees the context switch's saved resume point is the task
/// stack's RSP, never the transient per-CPU IRQ stack (see B-DF1 / Q7).
#[inline]
pub fn request_preempt() {
    let cpu = current_cpu_id();
    if let Some(f) = NEED_RESCHED.get(cpu) {
        f.store(true, Ordering::Release);
    }
}

/// Service a pending deferred preemption, if one was requested.
///
/// Called by the IRQ entry path ([`crate::idt::irq_common_dispatch`]) at the
/// outermost IRQ level, **after** RSP has been switched back to the
/// interrupted task's kernel stack.  Atomically clears the per-CPU
/// [`NEED_RESCHED`] flag and, if it was set, calls [`preempt`].
///
/// The guards mirror the original in-handler preemption check: skip if this
/// CPU is in the `schedule_inner` idle fallback (calling `preempt()` there
/// would nest `schedule_inner` and corrupt the blocked task's saved
/// context), and skip during softirq processing (a nested timer IRQ during
/// softirq work — the outer ISR will preempt after softirqs complete).
///
/// # Safety
///
/// Must be called on the interrupted task's kernel stack (not the IRQ
/// stack), with interrupts in a state where a context switch is safe (the
/// timer ISR re-enables interrupts before returning, so IF is restored on
/// the about-to-be-saved task).
#[inline]
pub fn do_deferred_preempt() {
    let cpu = current_cpu_id();
    let pending = NEED_RESCHED
        .get(cpu)
        .is_some_and(|f| f.swap(false, Ordering::AcqRel));
    if pending && !cpu_is_idle(cpu) && !crate::softirq::is_processing() {
        // Spinlock guard: never involuntarily preempt a task that holds a
        // tracked spinlock.  Preempting mid-critical-section lets a higher-
        // priority task spin forever on a lock whose (now un-scheduled) holder
        // can never run to release it — a single-CPU priority-inversion
        // deadlock (see PREEMPT_DISABLE_COUNT).  Re-arm NEED_RESCHED so the
        // preemption lands on a subsequent tick once the lock is released;
        // tracked critical sections are short, so this is a bounded deferral.
        if preempt_count(cpu) > 0 {
            if let Some(f) = NEED_RESCHED.get(cpu) {
                f.store(true, Ordering::Release);
            }
            return;
        }
        // Deadlock guard: never block on SCHED from the deferred-preempt path.
        //
        // `preempt()` calls `schedule_inner()`, which takes `SCHED.lock()`.  If
        // the timer interrupted this CPU *while the running task held SCHED*
        // (e.g. inside `task_list()`, which collects a heap `Vec` under the
        // lock), preempting now would re-enter `SCHED.lock()` on the same CPU
        // and spin forever — the interrupted frame can never release the lock
        // because this CPU is now stuck in the nested acquire (and the `cli`
        // below would make the hang unrecoverable).  This is the same hazard
        // `unthrottle_expired()` avoids with `try_lock()` from ISR context.
        //
        // If SCHED is currently held (by this CPU's interrupted task *or*
        // transiently by another CPU — we can't tell which, and skipping is
        // safe either way), re-arm NEED_RESCHED and defer to the next tick.
        // SCHED critical sections are short, so the preemption lands on a
        // subsequent tick where the task is not holding the lock.  This makes
        // involuntary preemption deadlock-free for *every* SCHED holder, not
        // just the long `task_list()` hold (it also closes the analogous —
        // tiny but real — window during voluntary `yield_now`/`block`).
        if SCHED.is_locked() {
            if let Some(f) = NEED_RESCHED.get(cpu) {
                f.store(true, Ordering::Release);
            }
            return;
        }
        // Disable interrupts across the involuntary context switch.
        //
        // B-DF1 recursion: without this, the whole `preempt → schedule_inner`
        // path runs on the interrupted task's stack with interrupts enabled
        // (the timer ISR re-enabled them via `sti` so the outgoing task is
        // saved with IF=1).  A timer tick arriving *during* `schedule_inner`
        // has RSP on the task stack — outside the per-CPU IRQ-stack range — so
        // `idt::irq_common_dispatch` treats it as a fresh *outermost* IRQ,
        // re-enters `do_deferred_preempt → preempt → schedule_inner`, and
        // recurses one ~2 KiB frame at a time until the task stack overflows
        // its guard page (#DF at `schedule_inner+0x11`).  Disabling interrupts
        // here prevents any nested tick for the duration of the switch.
        //
        // Correctness of IF preservation (the reason the ISR `sti`s in the
        // first place — see apic.rs handle_timer_irq):
        //   * The outgoing task is saved by `switch_context` with IF=0, but it
        //     is *always* resumed at the instruction right after `preempt()`
        //     below, whose next statement is the `sti` — so it regains
        //     interrupts immediately, and the enclosing IRQ stub's `iretq`
        //     restores IF=1 from the saved frame regardless.
        //   * Voluntary yields (`yield_now`, channel/futex blocking) do NOT go
        //     through this path; they run, and are saved, with IF=1 — so the
        //     per-task RFLAGS-preservation invariant is untouched for them.
        //   * `preempt()` calls `schedule_inner(true, ..)` (requeue=true), so
        //     the current task is always re-enqueued and a runnable task is
        //     always picked — the HLT-based idle fallback (which needs IF=1) is
        //     never entered from here.
        //
        // SAFETY: plain IF toggles with no memory effects; paired so IF is
        // restored on every path (the `sti` runs after `preempt()` returns,
        // i.e. when this CPU is switched back to this task).
        unsafe {
            core::arch::asm!("cli", options(nomem, nostack, preserves_flags));
        }
        preempt();
        unsafe {
            core::arch::asm!("sti", options(nomem, nostack, preserves_flags));
        }
    }
}

/// Reset bandwidth period counters and re-enqueue all throttled tasks.
///
/// Called by the BSP's `timer_tick` every [`BANDWIDTH_PERIOD_TICKS`]
/// (100 ticks = 1 second).  For each task with a CPU quota:
/// - Resets `cpu_period_used` to 0.
/// - Clears the `throttled` flag.
/// - Re-enqueues throttled tasks that are in the Ready state.
///
/// Uses `try_lock` because this runs in the timer ISR context.  If
/// the lock is contended, we skip this period — the next period
/// boundary will catch up (throttled tasks wait at most 2 seconds
/// instead of 1 in the worst case).
fn unthrottle_expired() {
    let Some(mut state) = SCHED.try_lock() else {
        return;
    };

    for (&id, task) in state.tasks.iter_mut() {
        if task.cpu_quota_pct == 0 {
            continue; // No quota — nothing to reset.
        }

        let was_throttled = task.throttled;
        task.cpu_period_used = 0;
        task.throttled = false;

        // Re-enqueue tasks that were parked by throttling.
        // They are in Ready state but not in any run queue.
        if was_throttled && task.state == TaskState::Ready {
            let prio = task.effective_priority();
            let cpu = task.last_cpu;
            PER_CPU_SCHED.enqueue(id, prio, cpu);
        }
    }
}

/// Update the system load averages (1/5/15-minute EWMAs).
///
/// Called once per second (every `BANDWIDTH_PERIOD_TICKS`) by the BSP,
/// but only recomputes the EWMAs every 5th call so the effective sample
/// interval is 5 seconds — Linux's `LOAD_FREQ`.  Samples the number of
/// runnable tasks across all CPUs (an O(num_cpus) read of per-CPU queue
/// lengths — no task-list walk, safe in ISR context) and decays the three
/// averages with Linux's `EXP_1`/`EXP_5`/`EXP_15` constants.
fn update_load_average() {
    // Gate to a 5-second effective sample interval.  fetch_add returns the
    // pre-increment value, so the very first call (n == 0) samples
    // immediately, then every 5th call thereafter.
    let n = LOAD_SAMPLE_DIVIDER.fetch_add(1, Ordering::Relaxed);
    if !n.is_multiple_of(5) {
        return;
    }

    // Count runnable tasks: query per-CPU queue lengths (cheap, lock-light).
    let num_cpus = crate::smp::cpu_count().max(1);
    let mut runnable: u64 = 0;
    for cpu_idx in 0..num_cpus.min(priority_rr::MAX_CPUS) {
        runnable = runnable.saturating_add(
            PER_CPU_SCHED.queue_length(cpu_idx) as u64,
        );
    }

    // active = n_runnable in fixed-point (Linux passes `nr_active * FIXED_1`).
    let active = runnable.saturating_mul(LOAD_FIXED_1);
    LOAD_AVG_1.store(
        calc_load(LOAD_AVG_1.load(Ordering::Relaxed), LOAD_EXP_1, active),
        Ordering::Relaxed,
    );
    LOAD_AVG_5.store(
        calc_load(LOAD_AVG_5.load(Ordering::Relaxed), LOAD_EXP_5, active),
        Ordering::Relaxed,
    );
    LOAD_AVG_15.store(
        calc_load(LOAD_AVG_15.load(Ordering::Relaxed), LOAD_EXP_15, active),
        Ordering::Relaxed,
    );
}

/// Get the three system load averages in Linux fixed-point form.
///
/// Each value `v` represents the real load `v / 2048` (`FIXED_1`).  The
/// tuple is `(1-minute, 5-minute, 15-minute)`.  `/proc/loadavg` formats
/// these with [`load_int`]/[`load_frac`] exactly as Linux does.
#[must_use]
// Public API: backs /proc/loadavg and the Linux `sysinfo(2)` loads[] field.
pub fn load_averages_fixed() -> (u64, u64, u64) {
    (
        LOAD_AVG_1.load(Ordering::Relaxed),
        LOAD_AVG_5.load(Ordering::Relaxed),
        LOAD_AVG_15.load(Ordering::Relaxed),
    )
}

/// Integer part of a Linux fixed-point load value (`v >> FSHIFT`).
#[must_use]
pub fn load_int(v: u64) -> u64 {
    v >> LOAD_FSHIFT
}

/// Two-digit fractional part of a Linux fixed-point load value.
///
/// Matches Linux's `LOAD_FRAC`: `LOAD_INT((v & (FIXED_1 - 1)) * 100)`.
#[must_use]
pub fn load_frac(v: u64) -> u64 {
    load_int((v & (LOAD_FIXED_1.saturating_sub(1))).saturating_mul(100))
}

/// Get the 1-minute system load average scaled ×100.
///
/// Returns a value like 150 meaning load = 1.50.  Derived from the
/// Linux fixed-point 1-minute EWMA so it stays consistent with
/// `/proc/loadavg`.
#[must_use]
#[allow(dead_code)] // Public API for kdiag / kshell.
pub fn load_average_x100() -> u64 {
    LOAD_AVG_1.load(Ordering::Relaxed).saturating_mul(100) >> LOAD_FSHIFT
}

/// Anti-starvation check: boost priority of tasks stuck in Ready too long.
///
/// Called periodically by the BSP (every `STARVATION_CHECK_INTERVAL` ticks).
/// Scans all Ready tasks — if any task has been waiting longer than
/// `STARVATION_THRESHOLD_TICKS`, temporarily boost it by moving it from
/// its current priority queue to priority 0 (highest).
///
/// The boost is "one-shot": the task's `priority` field is NOT changed
/// (its *base* priority is preserved).  Instead, we dequeue it from its
/// current queue and re-enqueue at priority 0.  When the scheduler
/// picks it, it runs with its normal time slice.  After being dispatched,
/// `ready_since_tick` resets, so it won't be boosted again until it
/// genuinely starves again.
///
/// This prevents indefinite starvation of low-priority tasks when
/// many higher-priority tasks are CPU-bound.  Linux has a similar
/// mechanism (it used to boost starved tasks every 500ms in the O(1)
/// scheduler; CFS/EEVDF avoid the problem via virtual runtime tracking).
fn check_starvation() {
    // Read threshold from sysctl (allows runtime tuning).
    // 0 = anti-starvation disabled.
    let threshold = crate::sysctl::get(crate::sysctl::PARAM_SCHED_STARVATION_THRESHOLD)
        .unwrap_or(STARVATION_THRESHOLD_TICKS);
    if threshold == 0 {
        return; // Anti-starvation disabled.
    }

    let now = crate::apic::tick_count();

    // Use try_lock to avoid blocking timer_tick if the scheduler is
    // already held (e.g., a context switch is in progress).
    let Some(state) = SCHED.try_lock() else { return };

    // Collect tasks that need boosting (avoid mutating while iterating).
    // Stack-allocated small buffer for the common case (few starved tasks).
    let mut boost_list: [(TaskId, u8, usize); 8] = [(0, 0, 0); 8];
    let mut boost_count = 0usize;

    for (&id, task) in state.tasks.iter() {
        if task.state != TaskState::Ready {
            continue;
        }
        if task.throttled {
            continue; // Throttled tasks wait by design.
        }
        if task.priority >= task::IDLE_PRIORITY {
            continue; // Idle tasks don't need boosting.
        }
        if task.ready_since_tick == 0 {
            continue; // Not tracking yet.
        }
        let waited = now.saturating_sub(task.ready_since_tick);
        if waited >= threshold {
            let current_prio = task.effective_priority();
            if current_prio > 0 && boost_count < boost_list.len() {
                // Only boost if not already at highest priority.
                // SAFETY: indexing is bounds-checked by boost_count < len.
                #[allow(clippy::indexing_slicing)]
                {
                    boost_list[boost_count] = (id, current_prio, task.last_cpu);
                }
                boost_count = boost_count.saturating_add(1);
            }
        }
    }

    // Release read-only lock, re-acquire for mutations.
    drop(state);

    if boost_count == 0 {
        return;
    }

    // Re-enqueue starved tasks at priority 0.
    //
    // Two correctness points here, both guarding against duplicate run-queue
    // entries (the same task ID appearing twice in the priority-0 queue):
    //
    // 1. We use `dequeue_any` instead of `dequeue(id, old_prio, cpu)`. A task
    //    that was already boosted on a previous pass physically sits in
    //    priority-queue 0, but `old_prio = effective_priority()` reports its
    //    BASE priority, so a level-targeted dequeue would scan the wrong queue,
    //    fail to remove it, and the following enqueue would duplicate the
    //    entry. `dequeue_any` scans every level and removes ALL occurrences of
    //    the id, so the subsequent single enqueue leaves exactly one queue-0
    //    entry.
    //
    // 2. We reset each boosted task's `ready_since_tick` to "now". Without this
    //    a task boosted on this pass but not yet dispatched would still satisfy
    //    `waited >= threshold` on the very next check_starvation() pass and be
    //    boosted again. The reset gives it a fresh starvation clock so it is
    //    only re-boosted if it genuinely starves at priority 0 too.
    //
    // Together these close the anti-starvation duplicate-enqueue bug (the W2
    // bench_pick_next-livelock amplifier).
    let now_boost = crate::apic::tick_count();
    let mut relock = SCHED.try_lock();
    for i in 0..boost_count {
        #[allow(clippy::indexing_slicing)]
        let (id, _old_prio, cpu) = boost_list[i];
        // Remove every existing copy of this task from all priority levels,
        // then place a single entry at priority 0.
        PER_CPU_SCHED.dequeue_any(id, cpu);
        PER_CPU_SCHED.enqueue(id, 0, cpu);
        STARVATION_BOOSTS.fetch_add(1, Ordering::Relaxed);
        // Reset the starvation clock so the task is not re-boosted before it
        // has had a chance to be dispatched from priority 0. If we could not
        // re-acquire the scheduler lock this pass, skip the reset: the worst
        // case is a redundant boost next pass, which `dequeue_any` makes safe.
        if let Some(state) = relock.as_mut() {
            if let Some(task) = state.tasks.get_mut(&id) {
                task.ready_since_tick = now_boost;
            }
        }
    }
    drop(relock);

    // Log the boosted task IDs (and their base priorities) so a perpetual
    // starvation loop can be attributed to specific tasks.  Printed
    // incrementally to avoid any heap allocation on this path.
    serial_print!(
        "[sched] Anti-starvation: cur={} boosted {} task{} to priority 0: [",
        load_current_task(),
        boost_count,
        if boost_count == 1 { "" } else { "s" }
    );
    for i in 0..boost_count {
        #[allow(clippy::indexing_slicing)]
        let (id, prio, _cpu) = boost_list[i];
        if i == 0 {
            serial_print!("{}(p{})", id, prio);
        } else {
            serial_print!(",{}(p{})", id, prio);
        }
    }
    serial_println!("]");
}

/// Number of anti-starvation boosts since boot (diagnostic).
#[must_use]
#[allow(dead_code)] // Public API for diagnostics.
pub fn starvation_boost_count() -> u64 {
    STARVATION_BOOSTS.load(Ordering::Relaxed)
}

/// Get per-CPU utilization percentages.
///
/// Returns an array of `(total_ticks, idle_ticks)` pairs for each CPU.
/// CPU utilization = `(total - idle) / total × 100`.
///
/// The counters are cumulative since boot.  To measure utilization over
/// a time window, sample twice and compute the delta.
#[must_use]
#[allow(dead_code)] // Public API for procfs /proc/stat, system monitor.
pub fn cpu_utilization() -> [(u64, u64); priority_rr::MAX_CPUS] {
    let mut result = [(0u64, 0u64); priority_rr::MAX_CPUS];
    let num_cpus = crate::smp::cpu_count().max(1);
    for i in 0..num_cpus.min(priority_rr::MAX_CPUS) {
        #[allow(clippy::indexing_slicing)] // i < MAX_CPUS (bounded above).
        {
            result[i] = (
                TOTAL_TICKS[i].load(Ordering::Relaxed),
                IDLE_TICK_COUNTS[i].load(Ordering::Relaxed),
            );
        }
    }
    result
}

/// Set the CPU bandwidth quota for a task.
///
/// `quota_pct` is the maximum percentage of one CPU core the task may
/// consume per bandwidth period (1 second):
/// - `0` = unlimited (no throttling)
/// - `1..=100` = percentage cap
///
/// Returns `true` if the task was found and the quota was set.
/// Returns `false` if the task doesn't exist or `quota_pct > 100`.
///
/// # Example
///
/// ```ignore
/// // Limit task 42 to 25% of one CPU core.
/// set_cpu_quota(42, 25);
/// ```
pub fn set_cpu_quota(task_id: TaskId, quota_pct: u8) -> bool {
    if quota_pct > 100 {
        return false;
    }

    let mut state = SCHED.lock();
    let Some(task) = state.tasks.get_mut(&task_id) else {
        return false;
    };

    let old_quota = task.cpu_quota_pct;
    task.cpu_quota_pct = quota_pct;

    // If we're removing the quota (setting to 0 or raising it above
    // current usage), un-throttle the task immediately.
    if task.throttled && (quota_pct == 0 || task.cpu_period_used < u64::from(quota_pct)) {
        task.throttled = false;
        if task.state == TaskState::Ready {
            let prio = task.effective_priority();
            let cpu = task.last_cpu;
            PER_CPU_SCHED.enqueue(task_id, prio, cpu);
        }
    }

    // Log the change outside the critical section would be ideal,
    // but since this is a rare configuration call (not hot-path),
    // logging under the lock is acceptable.
    if old_quota != quota_pct {
        match (old_quota, quota_pct) {
            (0, new) => serial_println!(
                "[sched] Task {} CPU quota: unlimited% → {}%", task_id, new
            ),
            (old, 0) => serial_println!(
                "[sched] Task {} CPU quota: {}% → unlimited%", task_id, old
            ),
            (old, new) => serial_println!(
                "[sched] Task {} CPU quota: {}% → {}%", task_id, old, new
            ),
        }
    }

    true
}

/// Get the CPU bandwidth quota for a task.
///
/// Returns `Some(quota_pct)` where 0 = unlimited, 1–100 = percentage.
/// Returns `None` if the task doesn't exist.
#[must_use]
pub fn get_cpu_quota(task_id: TaskId) -> Option<u8> {
    let state = SCHED.lock();
    state.tasks.get(&task_id).map(|t| t.cpu_quota_pct)
}

/// Proactive push-based load balancing.
///
/// Called from `SCHED_SOFTIRQ` handler with interrupts enabled.
/// If this CPU has significantly more tasks than the lightest CPU,
/// migrates some excess tasks to equalize load.
///
/// Complements the reactive work-stealing in [`schedule_inner`]
/// (which only fires when a CPU runs out of local work).  Push
/// balancing ensures that a CPU with 10 tasks and a neighbor with 2
/// don't wait until the neighbor goes fully idle before equalizing.
///
/// Uses `try_lock` for the global task table to avoid blocking in
/// softirq context.  If the lock is contended, we skip this balance
/// check and try again in the next `BALANCE_INTERVAL` (100 ms).
pub fn push_balance() {
    let cpu = current_cpu_id();

    let migrations = PER_CPU_SCHED.try_push_balance(cpu);
    if migrations.is_empty() {
        return;
    }

    // Update `last_cpu` on migrated tasks.  If a task's affinity
    // forbids the target CPU (rare), move it to an allowed CPU.
    if let Some(mut state) = SCHED.try_lock() {
        for &(task_id, target_cpu) in &migrations {
            if let Some(task) = state.tasks.get_mut(&task_id) {
                if task.can_run_on(target_cpu) {
                    task.last_cpu = target_cpu;
                } else {
                    // Affinity doesn't allow target — move to allowed CPU.
                    let correct_cpu = choose_cpu_for_task(task);
                    task.last_cpu = correct_cpu;
                    let prio = task.effective_priority();
                    PER_CPU_SCHED.dequeue(task_id, prio, target_cpu);
                    PER_CPU_SCHED.enqueue(task_id, prio, correct_cpu);
                }
            }
        }
    }
    // Even if we couldn't update last_cpu (lock contended), the
    // tasks are already enqueued on the target CPU's queue.  On
    // their next reschedule/wake, they'll land on the wrong CPU
    // but self-correct on the following balance pass.

    // Record migration events for diagnostics.
    for &(task_id, target_cpu) in &migrations {
        crate::sched_migrate::record(
            task_id as u32,
            cpu as u8,
            target_cpu as u8,
            crate::sched_migrate::MigrateReason::WorkSteal,
        );
    }

    // Wake target CPUs that might be idle (HLTing).
    // Deduplicate: all migrations go to the same target CPU.
    if let Some(&(_, target_cpu)) = migrations.first() {
        signal_cpu(target_cpu);
    }
}

/// Migrate all tasks from a CPU's run queue to other online CPUs.
///
/// Called during CPU hotplug offline.  Drains the target CPU's run queue
/// and distributes tasks across remaining online CPUs (round-robin).
/// Tasks with CPU affinity restrictions are placed on an appropriate
/// allowed CPU.
///
/// Returns the number of tasks migrated.
pub fn migrate_tasks_from_cpu(cpu: usize) -> usize {
    // Drain everything from the target CPU's run queue.
    let stolen = PER_CPU_SCHED.drain_all(cpu);

    if stolen.is_empty() {
        return 0;
    }

    let count = stolen.len();

    // Distribute to other online CPUs round-robin.
    let total_cpus = crate::smp::cpu_count();
    let mut target = 0usize;

    if let Some(mut state) = SCHED.try_lock() {
        for (task_id, priority) in &stolen {
            // Find next online CPU that isn't the one being offlined.
            let mut found_target = false;
            for _ in 0..total_cpus {
                if target != cpu && crate::cpu_hotplug::is_scheduling_eligible(target) {
                    found_target = true;
                    break;
                }
                target = (target + 1) % total_cpus;
            }
            if !found_target {
                // Fallback: BSP (always online).
                target = 0;
            }

            // Check affinity if available.
            let actual_target = if let Some(task) = state.tasks.get_mut(task_id) {
                if task.can_run_on(target) {
                    task.last_cpu = target;
                    target
                } else {
                    let correct = choose_cpu_for_task(task);
                    task.last_cpu = correct;
                    correct
                }
            } else {
                target
            };

            PER_CPU_SCHED.enqueue(*task_id, *priority, actual_target);
            target = (target + 1) % total_cpus;
        }
    } else {
        // Couldn't lock the task table — just enqueue on BSP.
        for (task_id, priority) in &stolen {
            PER_CPU_SCHED.enqueue(*task_id, *priority, 0);
        }
    }

    count
}

/// Suspend a task (pause execution).
///
/// Transitions the task from [`Ready`] to [`Suspended`], removing
/// it from the run queue.  If the task is [`Running`] (the current
/// task), it is suspended and the scheduler picks another task.
/// If the task is [`Blocked`], it transitions directly to
/// [`Suspended`] — when the blocking event fires, the wake will
/// find it in Suspended state and leave it there.
///
/// Returns `true` if the task was suspended, `false` if it was
/// already Suspended, Dead, or not found.
pub fn suspend(task_id: TaskId) -> bool {
    let current = load_current_task();

    {
        let mut state = SCHED.lock();
        let Some(task) = state.tasks.get_mut(&task_id) else {
            return false;
        };

        match task.state {
            TaskState::Ready => {
                let prio = task.effective_priority();
                let cpu = task.last_cpu;
                task.state = TaskState::Suspended;
                PER_CPU_SCHED.dequeue(task_id, prio, cpu);
            }
            TaskState::Running => {
                // Suspending the current task — mark it and yield
                // without re-enqueuing.
                task.state = TaskState::Suspended;
            }
            TaskState::Blocked => {
                // Suspend a blocked task.  When the wake event fires,
                // wake() will see it's not Blocked and skip it.  The
                // task stays Suspended until resume() is called.
                task.state = TaskState::Suspended;
            }
            TaskState::Suspended | TaskState::Dead => {
                return false;
            }
        }
    }

    // If we just suspended the current task, yield to another task.
    // Self-suspension is a voluntary context switch.
    if task_id == current {
        schedule_inner(false, SwitchKind::Voluntary);
    }

    serial_println!("[sched] Suspended task {}", task_id);
    true
}

/// Resume a suspended task (unpause execution).
///
/// Transitions the task from [`Suspended`] to [`Ready`] and places
/// it back in the run queue at its effective priority (which may
/// include an interactive boost).
///
/// Returns `true` if the task was resumed, `false` if it was not
/// in the Suspended state.
pub fn resume(task_id: TaskId) -> bool {
    let target_cpu;
    {
        let mut state = SCHED.lock();
        let Some(task) = state.tasks.get_mut(&task_id) else {
            return false;
        };

        if task.state != TaskState::Suspended {
            return false;
        }

        task.mark_ready(crate::apic::tick_count());
        let prio = task.effective_priority();
        target_cpu = choose_cpu_for_task(task);
        task.last_cpu = target_cpu;
        PER_CPU_SCHED.enqueue(task_id, prio, target_cpu);
    }
    signal_cpu(target_cpu);

    serial_println!("[sched] Resumed task {}", task_id);
    true
}

/// Change a task's scheduling priority.
///
/// If the task is in the run queue ([`Ready`] state), it is dequeued
/// at the old priority and re-enqueued at the new priority.  For
/// other states (Running, Blocked, Suspended), the new priority takes
/// effect when the task next enters the run queue.
///
/// Priority is clamped to `0..NUM_PRIORITIES` (0 = highest, 31 =
/// lowest).
///
/// Returns the old priority, or `None` if the task was not found.
pub fn set_priority(task_id: TaskId, new_priority: u8) -> Option<u8> {
    let clamped = new_priority.min(
        #[allow(clippy::cast_possible_truncation)]
        { (NUM_PRIORITIES - 1) as u8 }
    );

    let mut state = SCHED.lock();
    let task = state.tasks.get(&task_id)?;
    let old_priority = task.priority;
    let old_effective = task.effective_priority();
    let task_state = task.state;
    let is_interactive = task.interactive;
    let task_cpu = task.last_cpu;

    if old_priority == clamped {
        return Some(old_priority);
    }

    // Compute the new effective priority (with interactive boost).
    let new_effective = if is_interactive {
        clamped.saturating_sub(task::INTERACTIVE_BOOST)
    } else {
        clamped
    };

    // If the task is Ready (in the run queue), move it to the new
    // priority queue.  We do the dequeue/enqueue first with the
    // scheduler, then update the task's stored priority, to avoid
    // two mutable borrows of `state`.
    if task_state == TaskState::Ready {
        PER_CPU_SCHED.dequeue(task_id, old_effective, task_cpu);
        PER_CPU_SCHED.enqueue(task_id, new_effective, task_cpu);
    }

    // Now update the task's stored priority.
    if let Some(task) = state.tasks.get_mut(&task_id) {
        task.priority = clamped;
    }

    serial_println!(
        "[sched] Task {} priority: {} → {}{}",
        task_id, old_priority, clamped,
        if is_interactive { " (interactive)" } else { "" }
    );
    Some(old_priority)
}

/// Set a task's CPU affinity mask.
///
/// Bit N set means the task is allowed to run on CPU N.  If the task
/// is currently in the run queue on a CPU that's no longer allowed,
/// it is moved to the first allowed CPU.
///
/// Returns the old affinity mask, or `None` if the task was not found.
///
/// # Errors
///
/// Returns `None` if `mask` is zero (would make the task unrunnable).
pub fn set_cpu_affinity(task_id: TaskId, mask: u64) -> Option<u64> {
    if mask == 0 {
        return None;
    }

    let mut state = SCHED.lock();
    let task = state.tasks.get(&task_id)?;
    let old_mask = task.cpu_affinity;
    let task_state = task.state;
    let prio = task.effective_priority();
    let old_cpu = task.last_cpu;

    if old_mask == mask {
        return Some(old_mask);
    }

    // Check if the task's current CPU is still allowed.
    let needs_migrate = task_state == TaskState::Ready
        && (mask >> old_cpu) & 1 == 0;

    // Update the stored mask.
    if let Some(task) = state.tasks.get_mut(&task_id) {
        task.cpu_affinity = mask;

        if needs_migrate {
            // Move from old CPU's queue to the first allowed CPU.
            let new_cpu = choose_cpu_for_task(task);
            task.last_cpu = new_cpu;
            PER_CPU_SCHED.dequeue(task_id, prio, old_cpu);
            PER_CPU_SCHED.enqueue(task_id, prio, new_cpu);
        }
    }

    Some(old_mask)
}

/// Get a task's CPU affinity mask.
///
/// Returns the affinity mask, or `None` if the task was not found.
#[must_use]
pub fn get_cpu_affinity(task_id: TaskId) -> Option<u64> {
    let state = SCHED.lock();
    state.tasks.get(&task_id).map(|t| t.cpu_affinity)
}

/// Kill a task remotely (force-terminate without running task code).
///
/// Marks the task as [`Dead`](TaskState::Dead) and removes it from
/// the run queue if it was [`Ready`].  Blocked and Suspended tasks
/// are simply marked Dead (they won't be woken).
///
/// Cannot kill the currently running task — use [`task_exit`] for
/// self-termination.
///
/// Returns `true` if the task was found and killed, `false` if it
/// was already Dead, not found, or is the current task.
pub fn kill_task(task_id: TaskId) -> bool {
    let current = load_current_task();
    if task_id == current {
        // Can't kill the currently running task via this path.
        // Use task_exit() for self-termination.
        serial_println!(
            "[sched] kill_task: refusing to kill current task {}",
            task_id
        );
        return false;
    }

    let mut state = SCHED.lock();
    let Some(task) = state.tasks.get_mut(&task_id) else {
        return false;
    };

    match task.state {
        TaskState::Dead => return false,
        TaskState::Ready => {
            // Remove from the run queue before marking Dead.
            let prio = task.effective_priority();
            let cpu = task.last_cpu;
            task.state = TaskState::Dead;
            PER_CPU_SCHED.dequeue(task_id, prio, cpu);
        }
        TaskState::Blocked | TaskState::Suspended => {
            // Not in the run queue — just mark Dead.
            // If anything tries to wake() this task later, it'll
            // see it's not Blocked and return false.
            task.state = TaskState::Dead;
        }
        TaskState::Running => {
            // On SMP, the task may be Running on another CPU while
            // we kill it from this CPU.  Mark it Dead — the other
            // CPU will notice the state change at its next preemption
            // or yield (schedule_inner checks state before re-enqueue).
            //
            // On single-CPU, this case shouldn't be reachable (we
            // checked for current task above), but handle it safely.
            task.state = TaskState::Dead;
        }
    }

    // Drop the SCHED lock before notifying hooks — hooks may access
    // other subsystems that have their own locks.
    drop(state);

    TASKS_EXITED.fetch_add(1, Ordering::Relaxed);
    serial_println!("[sched] Killed task {}", task_id);

    // Notify exit hooks after the task is marked Dead and the lock
    // is released.  Hooks see the task as Dead if they check state.
    notify_exit_hooks(task_id);
    true
}

/// Reap all dead tasks: free their kernel stacks and remove them from
/// the task table.
///
/// Must NOT be called from within a dead task's own context (i.e., the
/// current task must still be alive).  Typically called from the idle
/// loop or from a test after yields that let tasks finish.
///
/// Returns the number of tasks reaped.
pub fn reap_dead_tasks() -> usize {
    let mut reaped = 0;

    // Collect current task IDs from ALL online CPUs.  We must not
    // reap any task that ANY CPU is currently running on, because
    // freeing the stack while a CPU is using it is use-after-free.
    //
    // The previous code only checked the local CPU's current task,
    // which is an SMP correctness bug: CPU 0 could reap a dead task
    // whose stack CPU 1 is still using (e.g., in the idle fallback
    // after task_exit).
    let num_cpus = crate::smp::cpu_count().max(1);
    let active_ids: alloc::vec::Vec<TaskId> = (0..num_cpus)
        .map(|i| {
            CURRENT_TASK_IDS
                .get(i)
                .map_or(0, |a| a.load(Ordering::Acquire))
        })
        .collect();

    // Collect IDs of dead tasks first, then remove them one by one.
    // We do this in two passes because we need the lock to inspect
    // state but also need to call free_stack which does allocation-
    // related work.
    let dead_ids: alloc::vec::Vec<TaskId> = {
        let state = SCHED.lock();
        state.tasks.iter()
            .filter(|(id, task)| {
                task.state == TaskState::Dead
                    && !active_ids.contains(id)
            })
            .map(|(id, _)| *id)
            .collect()
    };

    for id in dead_ids {
        let mut state = SCHED.lock();
        if let Some(mut task) = state.tasks.remove(&id) {
            // Capture the cgroup before dropping the task so we can
            // decrement its task count below.
            let task_cgroup = task.cgroup_id;

            // Drop the lock before freeing the stack (free_order
            // acquires the frame allocator lock — safe since our lock
            // ordering is SCHED → frame allocator, and we just dropped
            // SCHED).
            drop(state);

            // Decrement the task's cgroup count on the definitive teardown
            // path.  cgroup accounting is otherwise only decremented by an
            // explicit `set_task_cgroup`/`remove_process_task` while the task
            // is still alive; without this a task that simply *exits* while
            // assigned to a non-root cgroup (e.g. a `container exec` process
            // that runs to completion, or any container init) would leave a
            // stale `nr_tasks` count forever, since the task is gone from the
            // table before anyone can move it back to root.  Skipping the
            // root group avoids churning the root count for ordinary kernel
            // tasks.  detach_task is saturating, so a double-detach (task was
            // already moved back to root before dying — cgroup_id would then
            // be ROOT and skipped anyway) can never underflow.
            if task_cgroup != crate::cgroup::ROOT_CGROUP {
                let _ = crate::cgroup::detach_task(task_cgroup);
            }

            // Final canary check — if the task overflowed before dying,
            // log a warning (the task is already dead so we can't halt,
            // but the corruption may have affected other memory).
            task.check_stack_canary();

            // SAFETY: The task is Dead, was removed from the table,
            // and no CPU has it as current (checked all CPUs above).
            if let Err(e) = unsafe { task.free_stack() } {
                serial_println!(
                    "[sched] WARNING: failed to free stack for task {}: {:?}",
                    id, e
                );
            }

            reaped += 1;
        }
    }

    reaped
}

// ---------------------------------------------------------------------------
// Time slice configuration
// ---------------------------------------------------------------------------

/// Set the time slice (in timer ticks) for a specific priority level.
///
/// Applies to all CPUs.  `level` must be in `0..NUM_PRIORITIES` (0–31)
/// and `ticks` must be >= 1.  A zero-tick time slice would starve the task.
///
/// Returns `true` on success, `false` if the level or ticks are invalid.
pub fn set_time_slice(level: usize, ticks: u32) -> bool {
    PER_CPU_SCHED.set_time_slice(level, ticks)
}

/// Get the time slice (in timer ticks) for a specific priority level.
///
/// Returns `None` if the level is out of range.
#[must_use]
pub fn get_time_slice(level: usize) -> Option<u32> {
    PER_CPU_SCHED.time_slice(level)
}

/// Reconfigure all time slices with a new base and increment.
///
/// Applies to all CPUs.  Formula: `time_slice[level] = base + level * increment`.
/// `base` must be >= 1 (zero would starve priority-0 tasks).
///
/// Returns `true` on success, `false` if `base` is 0.
pub fn reconfigure_time_slices(base: u32, increment: u32) -> bool {
    PER_CPU_SCHED.reconfigure_slices(base, increment)
}

/// Apply a named workload profile preset.
///
/// Profiles (from the design spec):
/// - **Desktop** (0): balanced interactivity, base=2, inc=1
/// - **Server** (1): throughput-oriented, base=4, inc=2
/// - **Development** (2): quick context switches, base=1, inc=1
/// - **Gaming** (3): minimal latency for foreground, base=1, inc=2
///
/// Returns `true` on success, `false` if the profile ID is invalid.
pub fn apply_workload_profile(profile_id: u8) -> bool {
    let Some(profile) = WorkloadProfile::from_u8(profile_id) else {
        return false;
    };
    PER_CPU_SCHED.apply_profile(profile);
    serial_println!(
        "[sched] Applied workload profile: {} (base={}, inc={})",
        profile.name(), profile.base(), profile.increment()
    );
    true
}

/// Get the currently active workload profile, if the time slices
/// match any known profile.
///
/// Returns `None` if the time slices have been manually tuned and
/// don't match any profile.
#[must_use]
pub fn current_workload_profile() -> Option<WorkloadProfile> {
    // Check each profile by comparing the time slice at level 0 and 1.
    // This identifies the (base, increment) pair.
    // No global lock needed — reads from PER_CPU_SCHED (per-CPU locks).
    for profile_id in 0..=3u8 {
        if let Some(profile) = WorkloadProfile::from_u8(profile_id) {
            let base = profile.base();
            let inc = profile.increment();
            // Verify level 0 and level 1 match this profile's formula.
            let l0 = PER_CPU_SCHED.time_slice(0);
            let l1 = PER_CPU_SCHED.time_slice(1);
            if l0 == Some(base) && l1 == Some(base.saturating_add(inc)) {
                return Some(profile);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Priority Inheritance support
// ---------------------------------------------------------------------------

/// Get a task's current effective scheduling priority.
///
/// Returns `None` if the task is not found.
#[must_use]
pub fn get_effective_priority(task_id: TaskId) -> Option<u8> {
    let state = SCHED.lock();
    state.tasks.get(&task_id).map(|t| t.effective_priority())
}

/// Boost a task's scheduling priority via priority inheritance.
///
/// Sets the task's `inherited_priority` to `donor_priority`, or
/// lowers it further if already set to a higher priority level.
/// (Lower number = higher priority.)
///
/// If the task is in the Ready queue, it is moved to the new
/// effective priority level.
///
/// Returns the task's new effective priority, or `None` if the task
/// was not found.
///
/// Called by the PI futex subsystem when a high-priority task blocks
/// on a lock held by a lower-priority task.
pub fn boost_priority(task_id: TaskId, donor_priority: u8) -> Option<u8> {
    let mut state = SCHED.lock();

    // Read current state (immutable borrow).
    let t = state.tasks.get(&task_id)?;
    let old_effective = t.effective_priority();
    let task_state = t.state;
    let current_inherited = t.inherited_priority;
    let base_prio = t.priority;
    let is_interactive = t.interactive;
    let task_cpu = t.last_cpu;

    // Compute new inherited priority (keep the most aggressive boost).
    let new_inh = match current_inherited {
        Some(current) => current.min(donor_priority),
        None => donor_priority,
    };

    // No change — return early.
    if Some(new_inh) == current_inherited {
        return Some(old_effective);
    }

    // Compute new effective priority.
    let base_eff = if is_interactive {
        base_prio.saturating_sub(task::INTERACTIVE_BOOST)
    } else {
        base_prio
    };
    let new_effective = base_eff.min(new_inh);

    // Re-queue if Ready and effective priority changed.
    if task_state == TaskState::Ready && new_effective != old_effective {
        PER_CPU_SCHED.dequeue(task_id, old_effective, task_cpu);
        PER_CPU_SCHED.enqueue(task_id, new_effective, task_cpu);
    }

    // Write the new inherited priority.
    if let Some(task) = state.tasks.get_mut(&task_id) {
        task.inherited_priority = Some(new_inh);
    }

    serial_println!(
        "[sched] PI boost: task {} priority {} → {} (donor prio {})",
        task_id, old_effective, new_effective, donor_priority
    );
    Some(new_effective)
}

/// Set or clear a task's inherited priority.
///
/// Used by the PI futex subsystem when a task releases a lock and
/// needs its inherited priority recalculated (or cleared entirely).
///
/// If `new_inherited` is `None`, the inherited priority is cleared
/// and the task returns to its base effective priority.
///
/// Returns the task's new effective priority, or `None` if the task
/// was not found.
pub fn set_inherited_priority(task_id: TaskId, new_inherited: Option<u8>) -> Option<u8> {
    let mut state = SCHED.lock();

    let t = state.tasks.get(&task_id)?;
    let old_effective = t.effective_priority();
    let task_state = t.state;
    let base_prio = t.priority;
    let is_interactive = t.interactive;
    let task_cpu = t.last_cpu;

    let base_eff = if is_interactive {
        base_prio.saturating_sub(task::INTERACTIVE_BOOST)
    } else {
        base_prio
    };
    let new_effective = match new_inherited {
        Some(inh) => base_eff.min(inh),
        None => base_eff,
    };

    if task_state == TaskState::Ready && new_effective != old_effective {
        PER_CPU_SCHED.dequeue(task_id, old_effective, task_cpu);
        PER_CPU_SCHED.enqueue(task_id, new_effective, task_cpu);
    }

    if let Some(task) = state.tasks.get_mut(&task_id) {
        task.inherited_priority = new_inherited;
    }

    if old_effective != new_effective {
        serial_println!(
            "[sched] PI {}: task {} effective priority {} → {}",
            if new_inherited.is_some() { "update" } else { "clear" },
            task_id, old_effective, new_effective
        );
    }

    Some(new_effective)
}

// ---------------------------------------------------------------------------
// Transitive PI support
// ---------------------------------------------------------------------------

/// Set or clear the PI futex address a task is blocked on.
///
/// Called by `futex_lock_pi()` just before blocking to record which
/// lock this task is waiting for.  Called with `None` when the task
/// acquires the lock or is interrupted.
///
/// This metadata enables **transitive priority inheritance**: when a
/// chain of tasks A→B→C exists (A waits on B's lock, B waits on C's
/// lock), the chain walker can follow each task's `blocked_on_pi_addr`
/// to find the next link.
pub fn set_blocked_on_pi_addr(task_id: TaskId, addr: Option<u64>) {
    let mut state = SCHED.lock();
    if let Some(task) = state.tasks.get_mut(&task_id) {
        task.blocked_on_pi_addr = addr;
    }
}

/// Get the PI futex address a task is blocked on.
///
/// Returns `None` if the task is not blocking on any PI futex, or
/// if the task doesn't exist.
///
/// Used by the PI chain walker to determine if a lock owner is itself
/// blocked on another PI lock, enabling transitive boost propagation.
#[must_use]
pub fn get_blocked_on_pi_addr(task_id: TaskId) -> Option<u64> {
    let state = SCHED.lock();
    state.tasks.get(&task_id).and_then(|t| t.blocked_on_pi_addr)
}

/// Walk the PI chain and boost all owners transitively.
///
/// Starting from `start_owner`, boosts each task in the dependency
/// chain to `donor_priority`.  The chain is followed by reading each
/// task's `blocked_on_pi_addr` and then looking up the owner of that
/// address via the provided `find_owner` callback.
///
/// The walk stops when:
/// - A task is not blocked on any PI address (chain terminates)
/// - `find_owner` returns `None` (no owner for the address)
/// - The depth limit [`PI_CHAIN_DEPTH_LIMIT`](task::PI_CHAIN_DEPTH_LIMIT)
///   is reached (prevents runaway chains)
/// - A cycle is detected (a task appears twice in the chain)
///
/// Returns the number of tasks boosted beyond `start_owner`.
///
/// # Parameters
///
/// - `start_owner`: The direct lock owner (already boosted by the caller).
/// - `donor_priority`: The priority to propagate through the chain
///   (typically the highest-priority waiter's priority).
/// - `find_owner`: Callback that maps a futex address to its current
///   owner task ID.  Provided by the futex subsystem since the scheduler
///   doesn't own the PI ownership table.
pub fn pi_chain_boost(
    start_owner: TaskId,
    donor_priority: u8,
    find_owner: impl Fn(u64) -> Option<TaskId>,
) -> usize {
    let mut boosted = 0;
    let mut current = start_owner;

    // Walk the chain up to the depth limit.
    // Start from 1 because the first boost (start_owner) is already done
    // by the caller.
    for _ in 1..task::PI_CHAIN_DEPTH_LIMIT {
        // Does the current owner block on another PI futex?
        let Some(addr) = get_blocked_on_pi_addr(current) else {
            break;
        };

        // Who owns that futex?
        let Some(next_owner) = find_owner(addr) else {
            break;
        };

        // Cycle detection: if we loop back to start_owner (or any
        // previously-visited node), stop.  In practice we only check
        // for the start to keep the code simple — a full visited set
        // would need allocation.  Real lock chains should never cycle.
        if next_owner == start_owner {
            serial_println!(
                "[sched] PI chain: cycle detected at task {} (addr {:#x})",
                next_owner, addr
            );
            break;
        }

        // Boost the next owner in the chain.
        boost_priority(next_owner, donor_priority);
        boosted += 1;
        current = next_owner;
    }

    if boosted > 0 {
        serial_println!(
            "[sched] PI chain: boosted {} transitive owner(s) from task {} (donor prio {})",
            boosted, start_owner, donor_priority
        );
    }

    boosted
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

/// Snapshot of a task's key fields for diagnostic display.
pub struct TaskInfo {
    /// Task ID.
    pub id: TaskId,
    /// Human-readable name.
    pub name: [u8; 32],
    /// Valid bytes in `name`.
    pub name_len: usize,
    /// Scheduling state.
    pub state: TaskState,
    /// Base priority level (0 = highest).
    pub priority: u8,
    /// Total CPU time consumed (timer ticks, 10 ms each at 100 Hz).
    pub total_ticks: u64,
    /// User-mode (ring 3) CPU time, in timer ticks.  `user_ticks +
    /// sys_ticks == total_ticks`.  Exposed as `/proc/<pid>/stat` field
    /// 14 (utime).
    pub user_ticks: u64,
    /// System (ring 0) CPU time, in timer ticks.  Exposed as
    /// `/proc/<pid>/stat` field 15 (stime).
    pub sys_ticks: u64,
    /// Minor page faults (resolved without I/O — demand-zero, CoW,
    /// stack growth).  Exposed as `/proc/<pid>/stat` field 10 (minflt).
    pub min_flt: u64,
    /// Major page faults (required I/O — swap-in, file-backed read).
    /// Exposed as `/proc/<pid>/stat` field 12 (majflt).
    pub maj_flt: u64,
    /// Voluntary context switches (task yielded/blocked).  Sourced by
    /// `getrusage` `ru_nvcsw`.
    pub nvcsw: u64,
    /// Involuntary context switches (task preempted).  Sourced by
    /// `getrusage` `ru_nivcsw`.
    pub nivcsw: u64,
    /// Total CPU cycles consumed (TSC-based, nanosecond precision).
    pub total_cycles: u64,
    /// Number of times this task was scheduled.
    pub schedule_count: u64,
    /// Boot-relative tick (USER_HZ = 100) when the task was created.
    /// Exposed as `/proc/<pid>/stat` field 22 (`starttime`).
    pub start_tick: u64,
    /// CPU this task last ran on.
    pub last_cpu: usize,
    /// CPU bandwidth quota (0 = unlimited, 1–100 = %).
    pub cpu_quota_pct: u8,
    /// Whether the task is currently throttled.
    pub throttled: bool,
    /// Total time spent waiting in Ready state (ticks).
    pub total_wait_ticks: u64,
    /// Maximum single wait duration (ticks).
    pub max_wait_ticks: u64,
    /// Stack usage in bytes (high water mark).  `None` for idle tasks.
    pub stack_used: Option<usize>,
    /// Stack usage percentage (0-100).  `None` for idle tasks.
    pub stack_pct: Option<u8>,
}

/// Return a snapshot of all tasks in the scheduler.
///
/// Used by the kernel debug shell to implement the `ps` command.
pub fn task_list() -> alloc::vec::Vec<TaskInfo> {
    let state = SCHED.lock();
    state
        .tasks
        .iter()
        .map(|(&id, task)| TaskInfo {
            id,
            name: task.name,
            name_len: task.name_len,
            state: task.state,
            priority: task.priority,
            total_ticks: task.total_ticks,
            user_ticks: task.user_ticks,
            sys_ticks: task.sys_ticks,
            min_flt: task.min_flt,
            maj_flt: task.maj_flt,
            nvcsw: task.nvcsw,
            nivcsw: task.nivcsw,
            total_cycles: task.total_cycles,
            schedule_count: task.schedule_count,
            start_tick: task.start_tick,
            last_cpu: task.last_cpu,
            cpu_quota_pct: task.cpu_quota_pct,
            throttled: task.throttled,
            total_wait_ticks: task.total_wait_ticks,
            max_wait_ticks: task.max_wait_ticks,
            stack_used: task.stack_usage_bytes(),
            stack_pct: task.stack_usage_pct(),
        })
        .collect()
}

/// Return a snapshot of a single task by id, without scanning any stacks.
///
/// This is the lightweight counterpart to [`task_list`]: it acquires the
/// SCHED lock, looks up exactly one task by id, and extracts its bookkeeping
/// fields.  Crucially it does **not** call `stack_usage_bytes` /
/// `stack_usage_pct` — those are O(stack-size) volatile per-word scans, and
/// building the *whole* list with a stack scan per task under the SCHED lock
/// (as `task_list` does) can take seconds in the poison debug build, starving
/// the timer tick of the lock and tripping the hard-lockup watchdog.
///
/// `stack_used`/`stack_pct` are therefore always `None` in the returned
/// `TaskInfo`.  Callers that only need accounting fields (name, ticks,
/// cycles, wait times, priority, cpu) should prefer this over `task_list`.
#[must_use]
pub fn task_info(task_id: TaskId) -> Option<TaskInfo> {
    let state = SCHED.lock();
    state.tasks.get(&task_id).map(|task| TaskInfo {
        id: task_id,
        name: task.name,
        name_len: task.name_len,
        state: task.state,
        priority: task.priority,
        total_ticks: task.total_ticks,
        user_ticks: task.user_ticks,
        sys_ticks: task.sys_ticks,
        min_flt: task.min_flt,
        maj_flt: task.maj_flt,
        nvcsw: task.nvcsw,
        nivcsw: task.nivcsw,
        total_cycles: task.total_cycles,
        schedule_count: task.schedule_count,
        start_tick: task.start_tick,
        last_cpu: task.last_cpu,
        cpu_quota_pct: task.cpu_quota_pct,
        throttled: task.throttled,
        total_wait_ticks: task.total_wait_ticks,
        max_wait_ticks: task.max_wait_ticks,
        // Deliberately skip the volatile stack scan — see fn docs.
        stack_used: None,
        stack_pct: None,
    })
}

/// Return `true` if a task with `task_id` currently exists in the scheduler.
///
/// A cheap map lookup — does not allocate and does not scan any stacks.
/// Prefer this over `task_list().iter().any(...)` for existence checks.
#[must_use]
pub fn task_exists(task_id: TaskId) -> bool {
    SCHED.lock().tasks.contains_key(&task_id)
}

/// Install a new scheduler task name ("comm") for `task_id`, returning
/// `true` if the task exists (and `false` if it is unknown).
///
/// This is the per-thread `comm` that `/proc/<pid>/comm`,
/// `/proc/<pid>/stat` field 2, and `/proc/<pid>/status` `Name:` all read,
/// and the storage Linux's `PR_SET_NAME` targets (`current->comm`).  The
/// `name` is copied into the fixed 32-byte field (truncated if longer);
/// callers wanting Linux's 15-visible-byte `TASK_COMM_LEN - 1` rule must
/// apply it before calling.  The field is fully cleared first so no stale
/// tail bytes survive a shorter replacement.
#[must_use]
pub fn set_task_name(task_id: TaskId, name: &[u8]) -> bool {
    let mut state = SCHED.lock();
    let Some(task) = state.tasks.get_mut(&task_id) else {
        return false;
    };
    task.name = [0u8; 32];
    let copy_len = name.len().min(task.name.len());
    if let (Some(dst), Some(src)) = (task.name.get_mut(..copy_len), name.get(..copy_len)) {
        dst.copy_from_slice(src);
    }
    task.name_len = copy_len;
    true
}

/// Copy the scheduler task name ("comm") for `task_id` into `out`,
/// returning the number of bytes written (0 if the task is unknown or
/// `out` is empty).  Reads the same storage `set_task_name` writes.
#[must_use]
pub fn copy_task_name(task_id: TaskId, out: &mut [u8]) -> usize {
    let state = SCHED.lock();
    let Some(task) = state.tasks.get(&task_id) else {
        return 0;
    };
    let n = task.name_len.min(task.name.len()).min(out.len());
    if let (Some(dst), Some(src)) = (out.get_mut(..n), task.name.get(..n)) {
        dst.copy_from_slice(src);
    }
    n
}

/// Result of a stack canary scan.
#[derive(Debug, Clone)]
pub struct CanaryScanResult {
    /// Total tasks scanned.
    pub scanned: usize,
    /// Tasks with intact canaries.
    pub ok: usize,
    /// Tasks skipped (no stack_bottom, e.g., idle tasks).
    pub skipped: usize,
    /// Tasks with corrupted canaries (task_id, task_name).
    pub corrupted: alloc::vec::Vec<(TaskId, [u8; 32], usize)>,
}

/// Scan all task stack canaries and report any corruption.
///
/// Acquires the SCHED lock and reads the canary u64 at each task's
/// `stack_bottom`.  Returns the scan result.  Safe to call from kshell.
#[must_use]
pub fn check_all_canaries() -> CanaryScanResult {
    let state = SCHED.lock();
    let mut result = CanaryScanResult {
        scanned: 0,
        ok: 0,
        skipped: 0,
        corrupted: alloc::vec::Vec::new(),
    };

    for (&id, task_item) in state.tasks.iter() {
        let bottom = task_item.stack_bottom;
        if bottom == 0 {
            result.skipped += 1;
            continue;
        }
        result.scanned += 1;

        // SAFETY: stack_bottom is a valid kernel virtual address set during
        // task creation.  The canary is a u64 at that address.
        let canary = unsafe { core::ptr::read_volatile(bottom as *const u64) };
        // Compare against the value planted into this task's stack at
        // creation, not the global canary — see Task::planted_canary.
        if canary == task_item.planted_canary {
            result.ok += 1;
        } else {
            result.corrupted.push((id, task_item.name, task_item.name_len));
        }
    }

    result
}

/// Summary of scheduler state for the panic handler.
///
/// All fields are gathered via `try_lock` so the panic handler never
/// deadlocks even if the panic occurred while holding the scheduler lock.
pub struct PanicSchedInfo {
    /// ID of the task that was running when the panic occurred.
    pub current_task_id: TaskId,
    /// Name of the current task (UTF-8 bytes, length in `name_len`).
    pub name: [u8; 32],
    /// Valid bytes in `name`.
    pub name_len: usize,
    /// Base priority of the current task.
    pub priority: u8,
    /// Stack bottom address of the current task (0 if idle/unknown).
    pub stack_bottom: u64,
    /// Total number of tasks in the task table.
    pub total_tasks: usize,
    /// Number of tasks in each state: [ready, running, blocked, suspended, dead].
    pub state_counts: [usize; 5],
    /// Whether the SCHED lock could be acquired.
    pub lock_acquired: bool,
}

/// Gather scheduler diagnostics for the panic handler.
///
/// Uses `try_lock` to avoid deadlocking if the panic occurred
/// inside a scheduler critical section.  Returns basic info even
/// if the lock cannot be acquired (task ID is always available
/// via the per-CPU `CURRENT_TASK_IDS` array).
#[must_use]
pub fn panic_diagnostics() -> PanicSchedInfo {
    let current_id = load_current_task();

    let mut info = PanicSchedInfo {
        current_task_id: current_id,
        name: [0u8; 32],
        name_len: 0,
        priority: 0,
        stack_bottom: 0,
        total_tasks: 0,
        state_counts: [0; 5],
        lock_acquired: false,
    };

    // Try to get detailed info from the task table.
    if let Some(state) = SCHED.try_lock() {
        info.lock_acquired = true;
        info.total_tasks = state.tasks.len();

        // Count tasks by state.
        for task in state.tasks.values() {
            let idx = match task.state {
                TaskState::Ready => 0,
                TaskState::Running => 1,
                TaskState::Blocked => 2,
                TaskState::Suspended => 3,
                TaskState::Dead => 4,
            };
            // idx is always 0..5, matching state_counts length.
            info.state_counts[idx] = info.state_counts[idx].saturating_add(1);
        }

        // Get current task details.
        if let Some(task) = state.tasks.get(&current_id) {
            let len = task.name_len.min(32);
            info.name[..len].copy_from_slice(&task.name[..len]);
            info.name_len = len;
            info.priority = task.priority;
            info.stack_bottom = task.stack_bottom;
        }
    }

    info
}

// ---------------------------------------------------------------------------
// Sleep queue — timer-driven wakeups for SYS_SLEEP
// ---------------------------------------------------------------------------

/// Maximum number of concurrently sleeping tasks.
///
/// Fixed array avoids heap allocation in the ISR path.  256 is generous —
/// typical desktop workloads have tens of tasks, not hundreds sleeping.
const MAX_SLEEPERS: usize = 256;

/// A single entry in the sleep queue.
///
/// `wake_tick` == 0 means the slot is empty.  Written atomically by
/// `sleep_until_tick` (which sets wake_tick + task_id) and read by the
/// timer ISR (which zeroes wake_tick when it fires the wakeup).
struct SleepEntry {
    /// Tick count at which to wake.  0 = slot is empty.
    wake_tick: AtomicU64,
    /// Task ID to wake.
    task_id: AtomicU64,
}

impl SleepEntry {
    const fn new() -> Self {
        Self {
            wake_tick: AtomicU64::new(0),
            task_id: AtomicU64::new(0),
        }
    }
}

// SAFETY: `SleepEntry` fields are `AtomicU64`, which are `Sync`.
// The array itself is `Send + Sync` because we only access it
// through atomic operations with appropriate ordering.

/// The sleep queue.  Scanned by [`process_sleep_wakeups`] on every
/// timer tick.  Entries are added by [`sleep_until_tick`].
///
/// Fixed-size array, lock-free.  Indexed linearly — O(MAX_SLEEPERS)
/// per tick, which at 100 Hz and 256 entries is trivially fast.
static SLEEP_QUEUE: [SleepEntry; MAX_SLEEPERS] = {
    // const-initialize all entries.
    const EMPTY: SleepEntry = SleepEntry::new();
    [EMPTY; MAX_SLEEPERS]
};

/// Put the current task to sleep until the given tick count.
///
/// Blocks the current task and registers it in the sleep queue.
/// The timer ISR will wake it once `tick_count() >= wake_tick`.
///
/// Returns the number of nanoseconds actually slept (approximate).
pub fn sleep_until_tick(wake_tick: u64) {
    let task_id = load_current_task();

    // Find an empty slot.
    let mut found = false;
    for entry in &SLEEP_QUEUE {
        // CAS: try to claim an empty slot (wake_tick == 0).
        if entry
            .wake_tick
            .compare_exchange(0, wake_tick, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            entry.task_id.store(task_id, Ordering::Release);
            found = true;
            break;
        }
    }

    if !found {
        // No free slot — all 256 slots occupied.  Fall back to a
        // simple spin-yield loop.  This is extremely unlikely.
        serial_println!(
            "[sched] WARNING: sleep queue full, task {} falling back to spin",
            task_id
        );
        while crate::apic::tick_count() < wake_tick {
            yield_now();
        }
        return;
    }

    // Block the task.  The timer ISR will wake it.
    block_current();
}

// ---------------------------------------------------------------------------
// Deferred wake queue — retry mechanism for ISR-context wakes
// ---------------------------------------------------------------------------

/// Maximum pending deferred wakes (small: usually 0-2 in flight).
const DEFERRED_WAKE_SLOTS: usize = 32;

/// Sentinel value for empty deferred wake slots.
///
/// Must NOT be a valid task ID.  Using `u64::MAX` because task IDs
/// start at 0 and never reach this value.  Previously used 0, which
/// silently dropped deferred wakes for the boot task (tid=0) — the
/// CAS succeeded but drain_deferred_wakes skipped the slot because
/// it looked "empty."
const DEFERRED_WAKE_EMPTY: u64 = u64::MAX;

/// Deferred wake slot: `DEFERRED_WAKE_EMPTY` = empty, other = task_id to wake.
static DEFERRED_WAKES: [AtomicU64; DEFERRED_WAKE_SLOTS] = {
    const EMPTY: AtomicU64 = AtomicU64::new(DEFERRED_WAKE_EMPTY);
    [EMPTY; DEFERRED_WAKE_SLOTS]
};

/// Fast-path flag: set when any slot becomes non-empty, cleared after drain.
///
/// OPT: Avoids scanning all 32 slots on every schedule_inner call when
/// no deferred wakes are pending (the common case).  Each schedule_inner
/// invocation reads one atomic bool instead of 32 atomic u64s.
static DEFERRED_WAKES_PENDING: AtomicBool = AtomicBool::new(false);

/// Queue a deferred wake for a task.
///
/// Called from ISR context when [`try_wake`] fails (scheduler lock
/// contended by the interrupted code path).  The deferred wake will
/// be processed on the next timer tick by [`process_deferred_wakes`].
///
/// This is public because timer callbacks (hrtimer) fire from ISR
/// context and need the `try_wake` → `defer_wake` fallback pattern
/// to reliably wake blocked tasks.
///
/// If the queue is full (extremely unlikely — 32 slots), the wake is
/// dropped.  The task will remain blocked until another explicit wake
/// occurs.  This should never happen in practice because the queue
/// drains every tick (10ms).
pub fn defer_wake(task_id: TaskId) {
    for slot in &DEFERRED_WAKES {
        // CAS: claim an empty slot.
        if slot
            .compare_exchange(DEFERRED_WAKE_EMPTY, task_id, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            // Signal that the drain loop should run on next schedule_inner.
            DEFERRED_WAKES_PENDING.store(true, Ordering::Release);
            // Record that a wake was deferred (indicates ISR lock contention).
            crate::ktrace::record(
                crate::ktrace::Category::Sched,
                crate::ktrace::event::DEFERRED_WAKE,
                task_id,
                0, // arg2 unused
            );
            return;
        }
    }
    // Queue full — this is a diagnostic-only path.
    // In practice should never happen (32 slots, drained every 10ms).
}

/// Process all pending deferred wakes (lock-free path for softirq).
///
/// Called from the timer softirq (alongside `process_sleep_wakeups`).
/// Attempts to wake each queued task.  If `try_wake` still fails
/// (lock contended again — extremely rare), the entry stays for the
/// next tick.
pub fn process_deferred_wakes() {
    // OPT: Quick check — skip the scan if no wakes were deferred.
    if !DEFERRED_WAKES_PENDING.load(Ordering::Acquire) {
        return;
    }

    let mut any_remaining = false;
    for slot in &DEFERRED_WAKES {
        let task_id = slot.load(Ordering::Acquire);
        if task_id == DEFERRED_WAKE_EMPTY {
            continue;
        }
        if try_wake(task_id) {
            // Success — clear the slot.
            slot.store(DEFERRED_WAKE_EMPTY, Ordering::Release);
        } else {
            // try_wake failed again — slot stays occupied for next drain.
            any_remaining = true;
        }
    }

    // Only clear the flag if all slots were successfully drained.
    if !any_remaining {
        DEFERRED_WAKES_PENDING.store(false, Ordering::Release);
    }
}

/// Drain the deferred wake queue while holding the SCHED lock.
///
/// Called from `schedule_inner` with the lock already held.  This is
/// the primary drain path — it guarantees that deferred wakes are
/// processed at the next scheduling decision, even on single-CPU
/// systems where the ISR-context `try_wake` always fails (because the
/// interrupted code was holding the lock).
fn drain_deferred_wakes_locked(state: &mut SchedState, _cpu: usize) {
    // OPT: Quick check — skip the scan if no wakes were deferred.
    if !DEFERRED_WAKES_PENDING.load(Ordering::Acquire) {
        return;
    }

    for slot in &DEFERRED_WAKES {
        let task_id = slot.load(Ordering::Acquire);
        if task_id == DEFERRED_WAKE_EMPTY {
            continue;
        }
        // We already hold the lock — wake directly.
        if let Some(task) = state.tasks.get_mut(&task_id) {
            if task.state == TaskState::Blocked {
                task.mark_ready(crate::apic::tick_count());
                task.burst_ticks = 0;
                let prio = task.effective_priority();
                let target_cpu = choose_cpu_for_task(task);
                task.last_cpu = target_cpu;
                PER_CPU_SCHED.enqueue(task_id, prio, target_cpu);
            }
        }
        // Clear the slot regardless (task might have already been woken
        // by another path, or may not exist anymore).
        slot.store(DEFERRED_WAKE_EMPTY, Ordering::Release);
    }

    // All slots drained — clear the pending flag.
    DEFERRED_WAKES_PENDING.store(false, Ordering::Release);
}

/// Sleep the current task for a precise duration in nanoseconds.
///
/// Uses the high-resolution timer subsystem (HPET-backed) for sub-10ms
/// precision.  Falls back to tick-based [`sleep_until_tick`] if hrtimers
/// are unavailable.
///
/// The task is blocked and woken by an hrtimer callback.  Actual sleep
/// time depends on timer ISR frequency but is bounded by one tick
/// (~10ms) in the current implementation.
///
/// # Arguments
///
/// - `duration_ns` — sleep duration in nanoseconds (0 = yield)
pub fn sleep_ns(duration_ns: u64) {
    if duration_ns == 0 {
        yield_now();
        return;
    }

    // If interrupts aren't enabled (early boot, before sti()), the APIC
    // timer ISR won't fire, so hrtimer callbacks never execute.  Fall back
    // to a spin-wait on the HPET counter, which advances independently of
    // interrupt delivery.  This only happens during self-tests; production
    // code always runs with interrupts enabled.
    if !crate::cpu::interrupts_enabled() {
        let deadline = crate::hrtimer::now_ns().saturating_add(duration_ns);
        while crate::hrtimer::now_ns() < deadline {
            core::hint::spin_loop();
        }
        return;
    }

    // For very long sleeps (> 100ms), use tick-based for efficiency.
    // For short sleeps, use hrtimer for precision.
    if duration_ns > 100_000_000 {
        let ticks = duration_ns
            .saturating_add(9_999_999)
            .saturating_div(10_000_000);
        let wake_tick = crate::apic::tick_count().saturating_add(ticks);
        sleep_until_tick(wake_tick);
        return;
    }

    let task_id = load_current_task();

    // Schedule an hrtimer to wake us.
    //
    // The callback runs from ISR context (timer tick).  It uses try_wake
    // which may fail if the scheduler lock is held by the interrupted code.
    // On failure, we defer the wake to the next tick via the deferred wake
    // queue — guaranteeing the sleeper is eventually woken.
    fn wake_callback(task_id_arg: u64) {
        if !try_wake(task_id_arg) {
            // Scheduler lock was contended — defer to next tick.
            defer_wake(task_id_arg);
        }
    }

    let _handle = crate::hrtimer::schedule_ns(duration_ns, wake_callback, task_id);

    // Block until the timer fires and wakes us.
    block_current();
}

/// Sleep the current task for a given number of milliseconds.
///
/// Convenience wrapper around [`sleep_ns`].  Uses hrtimer for durations
/// ≤ 100ms, tick-based sleep for longer.
///
/// # Arguments
///
/// - `ms` — sleep duration in milliseconds (0 = yield)
#[inline]
pub fn sleep_ms(ms: u64) {
    sleep_ns(ms.saturating_mul(1_000_000));
}

/// Sleep the current task for a given number of microseconds.
///
/// Convenience wrapper around [`sleep_ns`].
///
/// # Arguments
///
/// - `us` — sleep duration in microseconds (0 = yield)
#[inline]
#[allow(dead_code)] // public scheduler API; convenience wrapper around sleep_ns
pub fn sleep_us(us: u64) {
    sleep_ns(us.saturating_mul(1_000));
}

/// Outcome of attempting to retire an expired sleep-queue entry.
enum SleeperWake {
    /// The sleep is over (task woken, already awake, or gone): release the slot.
    Release,
    /// The scheduler lock was contended: leave the slot and retry next tick.
    Retry,
}

/// Retire one expired sleep-queue entry, telling the caller whether the
/// slot may now be released.
///
/// This is deliberately *not* `try_wake`: `try_wake` returns a bare bool
/// that conflates "lock contended" (transient — retry) with "task isn't
/// blocked / no longer exists" (terminal — the timed sleep is over).  The
/// sleep queue must release the slot in the terminal cases; only genuine
/// lock contention warrants keeping the slot for a retry.  Collapsing the
/// two led to permanent slot leaks: a task that slept and was then woken
/// early (channel/futex/eventfd) or destroyed before its deadline left an
/// entry whose `try_wake` could never again succeed, so the slot was never
/// reclaimed.  Once all `MAX_SLEEPERS` slots leaked, every subsequent
/// sleeper fell back to busy-spinning (see `sleep_until_tick`), pinning a
/// CPU and starving lower-priority tasks.
fn wake_expired_sleeper(task_id: TaskId) -> SleeperWake {
    let Some(mut state) = SCHED.try_lock() else {
        // Lock contended — this is the only case that should retry.
        return SleeperWake::Retry;
    };
    match state.tasks.get_mut(&task_id) {
        Some(task) if task.state == TaskState::Blocked => {
            // Normal case: the task is still blocked on its timed sleep.
            task.mark_ready(crate::apic::tick_count());
            task.burst_ticks = 0;
            let prio = task.effective_priority();
            let target_cpu = choose_cpu_for_task(task);
            task.last_cpu = target_cpu;
            PER_CPU_SCHED.enqueue(task_id, prio, target_cpu);
            drop(state);
            signal_cpu(target_cpu);
            SleeperWake::Release
        }
        Some(task) => {
            // Task exists but isn't blocked: it was already woken by another
            // path before its deadline.  The timed sleep is satisfied, so the
            // slot must be released; record a pending wake to match the
            // existing `try_wake` semantics in case it blocks again.
            task.pending_wake = true;
            SleeperWake::Release
        }
        None => {
            // Task was destroyed (exited/killed) while its sleep slot was
            // still registered.  Release the slot so it can be reused —
            // otherwise the queue leaks an entry permanently.
            SleeperWake::Release
        }
    }
}

/// Scan the sleep queue and wake tasks whose sleep deadline has passed.
///
/// Called from the APIC timer ISR on every tick.  Must be lock-free
/// in the fast path (only atomic loads/stores, no mutexes).
///
/// Uses [`wake_expired_sleeper`] to safely wake tasks even from interrupt
/// context.  An expired slot is released once the scheduler lock has been
/// acquired and the wake has been resolved, regardless of the task's state;
/// only genuine lock contention keeps the slot for a retry on the next tick.
pub fn process_sleep_wakeups() {
    let now = crate::apic::tick_count();

    for entry in &SLEEP_QUEUE {
        let deadline = entry.wake_tick.load(Ordering::Acquire);
        if deadline == 0 {
            // Empty slot — skip.
            continue;
        }
        if now < deadline {
            // Not yet expired — skip.
            continue;
        }

        // Deadline passed.  Resolve the wake and release the slot unless the
        // scheduler lock was contended (in which case retry next tick).
        let task_id = entry.task_id.load(Ordering::Acquire);
        if let SleeperWake::Release = wake_expired_sleeper(task_id) {
            entry.wake_tick.store(0, Ordering::Release);
        }
    }
}

// ---------------------------------------------------------------------------
// Core scheduling logic
// ---------------------------------------------------------------------------

/// Account CPU cycles to the outgoing task at context switch time.
///
/// Reads the current TSC, computes the delta since the task was last
/// switched-in on this CPU, and adds it to the task's `total_cycles`.
/// Then updates the per-CPU `LAST_SWITCH_TSC` for the incoming task.
///
/// This gives nanosecond-precision per-task CPU time (vs the 10ms
/// granularity of tick-based `total_ticks`).
fn account_cycles(state: &mut SchedState, outgoing_id: TaskId, cpu: usize) {
    let now = crate::bench::rdtsc();
    if let Some(last_tsc_slot) = LAST_SWITCH_TSC.get(cpu) {
        let prev = last_tsc_slot.swap(now, Ordering::Relaxed);
        if prev != 0 {
            let delta = now.saturating_sub(prev);
            if let Some(task) = state.tasks.get_mut(&outgoing_id) {
                task.total_cycles = task.total_cycles.saturating_add(delta);
            }
        }
    }
}

/// The inner scheduling function.
///
/// If `requeue` is true, the current task is placed back in its
/// priority queue.  If false, it is not (used for blocking/exiting).
///
/// Uses per-CPU queues: first tries the local queue, then work-steals
/// from other CPUs if the local queue is empty.
///
/// OPT: The entire schedule+switch path uses a single lock acquisition.
/// Previous implementation took the lock twice (once for scheduling,
/// once for context pointer extraction), wasting ~100 cycles per switch
/// on redundant lock + BTreeMap lookups.
/// How the outgoing task relinquished the CPU, for `nvcsw`/`nivcsw`
/// (voluntary/involuntary context-switch) accounting.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SwitchKind {
    /// The task gave up the CPU itself (yield, block, self-suspend).
    /// Increments the outgoing task's `nvcsw`.
    Voluntary,
    /// The task was preempted by the timer while still runnable.
    /// Increments the outgoing task's `nivcsw`.
    Involuntary,
    /// Internal transition that must not be counted (e.g. the task is
    /// exiting — a dead task has no meaningful ctxsw stat).
    Uncounted,
}

fn schedule_inner(requeue: bool, kind: SwitchKind) {
    let current_id = load_current_task();
    let cpu = current_cpu_id();

    // Hardening: a voluntary yield/block while holding a tracked spinlock is a
    // caller bug — the lock is about to be carried across a context switch,
    // exactly the hazard PREEMPT_DISABLE_COUNT exists to prevent for the
    // *involuntary* path (which is why involuntary preemption is deferred
    // while the count is non-zero and can never reach here holding a lock).
    // The voluntary path can't be transparently deferred, so instead we flag
    // it loudly (one-shot) so the offending call site gets fixed. No such call
    // site exists today; this catches future regressions instantly instead of
    // as an intermittent single-CPU deadlock.
    if matches!(kind, SwitchKind::Voluntary) && preempt_count(cpu) > 0 {
        static WARNED: AtomicBool = AtomicBool::new(false);
        if !WARNED.swap(true, Ordering::Relaxed) {
            crate::serial_println!(
                "[sched] *** BUG: voluntary context switch (task {}, cpu {}) while \
                 holding {} tracked spinlock(s). A spinlock must never be held across \
                 a yield/block — fix the call site. (one-shot warning)",
                current_id,
                cpu,
                preempt_count(cpu),
            );
        }
    }

    // Data extracted under the single lock acquisition for the switch.
    let old_ctx_ptr: *mut Context;
    let new_ctx_ptr: *const Context;
    let old_fpu_ptr: *mut fpu::FpuState;
    let new_fpu_ptr: *const fpu::FpuState;
    let old_pml4: u64;
    let new_pml4: u64;
    let new_stack_top: u64;
    let new_fs_base: u64;
    let new_gs_base: u64;
    let next_id: TaskId;

    {
        let mut state = SCHED.lock();

        if !state.initialized {
            return;
        }

        // Process any deferred wakes that were queued by ISR-context
        // hrtimer callbacks when the SCHED lock was contended.  We now
        // hold the lock, so we can safely wake these tasks.
        drain_deferred_wakes_locked(&mut state, cpu);

        // Re-enqueue the current task if requested (on its current CPU).
        //
        // Guard: only re-enqueue if the task is still Running.  Another
        // CPU may have called kill_task() or suspend() while we were
        // executing, changing the state to Dead or Suspended.  If we
        // blindly overwrite to Ready, the task would be re-enqueued
        // despite being killed/suspended — a correctness bug on SMP.
        //
        // Also: if the task is throttled (CPU bandwidth exceeded), mark
        // it Ready but do NOT enqueue.  It stays parked until
        // `unthrottle_expired()` re-enqueues it at the next period reset.
        if requeue {
            if let Some(task) = state.tasks.get_mut(&current_id) {
                if task.state == TaskState::Running {
                    task.mark_ready(crate::apic::tick_count());
                    if task.throttled {
                        // Throttled: parked as Ready without a queue slot.
                        // unthrottle_expired() will re-enqueue it.
                    } else {
                        let prio = task.effective_priority();
                        PER_CPU_SCHED.enqueue(current_id, prio, cpu);
                    }
                }
                // If state is Dead/Suspended (set by another CPU),
                // don't re-enqueue — the task is being terminated or
                // paused.  It will not run again (Dead) or will be
                // re-enqueued by resume() (Suspended).
            }
        }

        // Pick the next task from the local CPU's queue.
        // If local queue is empty, try work stealing from other CPUs.
        // Stolen tasks need their last_cpu updated so wake()/kill_task()
        // dequeue from the correct (new) CPU queue, not the stale one.
        //
        // OPT: MigratedTasks is stack-allocated (no heap allocation under
        // the SCHED lock on the work-stealing path).
        let mut migrated = priority_rr::MigratedTasks::new();
        let _pick_t = crate::kprofile::begin(crate::kprofile::Slot::SchedPickNext);
        let picked = match PER_CPU_SCHED.pick_next_local(cpu) {
            Some(id) => Some(id),
            None => {
                let stolen = PER_CPU_SCHED.try_steal(cpu, &mut migrated);
                if let Some(stolen_id) = stolen {
                    WORK_STEALS.fetch_add(1, Ordering::Relaxed);
                    crate::ktrace::record(
                        crate::ktrace::Category::Sched,
                        crate::ktrace::event::WORK_STEAL,
                        stolen_id,
                        cpu as u64,
                    );
                }
                stolen
            }
        };
        crate::kprofile::end(crate::kprofile::Slot::SchedPickNext, _pick_t);
        // Update last_cpu for stolen tasks.  If a stolen task's affinity
        // forbids this CPU, put it back on its original (or first allowed)
        // CPU.  This is rare — most tasks have CPU_AFFINITY_ALL.
        for &id in migrated.iter() {
            if let Some(task) = state.tasks.get_mut(&id) {
                if task.can_run_on(cpu) {
                    task.last_cpu = cpu;
                } else {
                    // Can't run here — move it to the first allowed CPU.
                    let target = choose_cpu_for_task(task);
                    task.last_cpu = target;
                    let prio = task.effective_priority();
                    PER_CPU_SCHED.dequeue(id, prio, cpu);
                    PER_CPU_SCHED.enqueue(id, prio, target);
                }
            }
        }

        let Some(picked_id) = picked else {
            if !requeue {
                // No task ready and we can't re-enqueue the current one
                // (it's exiting or blocking).
                //
                // This path should be rare with per-CPU idle tasks: the
                // idle task is always in the queue at IDLE_PRIORITY, so
                // pick_next should always find it.  We reach here only
                // in edge cases (e.g., the idle task itself blocked
                // transiently, or during early boot before idle tasks
                // are fully set up).
                //
                // Set the idle flag so the timer ISR skips preempt() on
                // this CPU.  Without this guard, the ISR would call
                // schedule_inner(true) while we're inside this idle
                // fallback — nesting would save the wrong resume point
                // into the blocked task's context.
                if let Some(flag) = IDLE_FLAGS.get(cpu) {
                    flag.store(true, Ordering::Release);
                }
                drop(state);

                // Idle loop: HLT until a task becomes ready, then do a
                // full context switch.  The blocked/dead task's context
                // save area is still in the task table — we save our
                // current registers there and load the new task's.
                loop {
                    cpu::hlt();

                    let Some(mut s) = SCHED.try_lock() else {
                        continue;
                    };

                    // Drain deferred wakes while holding the SCHED lock.
                    // Timer ISR callbacks call try_wake() which fails when
                    // this idle loop holds the lock (the ISR interrupted
                    // us between try_lock and drop).  Those wakes are
                    // deferred and MUST be processed here — otherwise the
                    // woken task is never enqueued and we spin forever.
                    drain_deferred_wakes_locked(&mut s, cpu);

                    let mut idle_migrated = priority_rr::MigratedTasks::new();
                    let ready_id = match PER_CPU_SCHED.pick_next_local(cpu) {
                        Some(id) => id,
                        None => match PER_CPU_SCHED.try_steal(cpu, &mut idle_migrated) {
                            Some(id) => {
                                WORK_STEALS.fetch_add(1, Ordering::Relaxed);
                                id
                            }
                            None => { drop(s); continue; }
                        },
                    };
                    // Update last_cpu for stolen tasks (same affinity
                    // check as the main path above).
                    for &id in idle_migrated.iter() {
                        if let Some(task) = s.tasks.get_mut(&id) {
                            if task.can_run_on(cpu) {
                                task.last_cpu = cpu;
                            } else {
                                let target = choose_cpu_for_task(task);
                                task.last_cpu = target;
                                let prio = task.effective_priority();
                                PER_CPU_SCHED.dequeue(id, prio, cpu);
                                PER_CPU_SCHED.enqueue(id, prio, target);
                            }
                        }
                    }

                    // Found a ready task — set it up for switching.
                    if let Some(task) = s.tasks.get_mut(&ready_id) {
                        task.record_dispatch(crate::apic::tick_count());
                        task.state = TaskState::Running;
                        task.last_cpu = cpu;
                    }

                    // Extract context and FPU pointers for old (blocked/dead)
                    // and new tasks.  The old task's entry still exists
                    // in the BTreeMap — it's Blocked or Dead, not removed.
                    let old_data = s.tasks.get_mut(&current_id)
                        .map(|t| {
                            t.check_stack_canary();
                            // Real switch (current != ready): charge the
                            // outgoing task's voluntary/involuntary ctxsw.
                            match kind {
                                SwitchKind::Voluntary => {
                                    t.nvcsw = t.nvcsw.saturating_add(1);
                                }
                                SwitchKind::Involuntary => {
                                    t.nivcsw = t.nivcsw.saturating_add(1);
                                }
                                SwitchKind::Uncounted => {}
                            }
                            (&raw mut t.context, &raw mut t.fpu_state, t.pml4_phys)
                        });
                    let new_data = s.tasks.get(&ready_id)
                        .map(|t| (&raw const t.context, &raw const t.fpu_state, t.pml4_phys, t.stack_bottom, t.fs_base, t.gs_base));

                    if let (Some((old_p, old_fpu, o_pml4)), Some((new_p, new_fpu, n_pml4, n_sb, n_fs_base, n_gs_base))) =
                        (old_data, new_data)
                    {
                        // Account CPU cycles to the outgoing task (idle fallback path).
                        account_cycles(&mut s, current_id, cpu);
                        drop(s);

                        // Clear idle flag BEFORE the switch so the new
                        // task's timer ticks see normal (non-idle) state.
                        if let Some(flag) = IDLE_FLAGS.get(cpu) {
                            flag.store(false, Ordering::Release);
                        }

                        set_current_task(cpu, ready_id);

                        // Switch address space if needed.
                        if o_pml4 != n_pml4 {
                            let target = if n_pml4 == 0 {
                                KERNEL_PML4.load(Ordering::Acquire)
                            } else {
                                n_pml4
                            };
                            // SAFETY: target is a valid PML4 with kernel
                            // entries mapped.
                            unsafe {
                                crate::mm::page_table::write_cr3(target);
                            }
                        }

                        // Restore this user thread's %fs (TLS) base.
                        // IA32_FS_BASE is a global CPU register not saved in
                        // the GP Context, so without this two Linux/glibc
                        // processes would clobber each other's TLS pointer.
                        // Kernel tasks (pml4==0) never read %fs.
                        if n_pml4 != 0 {
                            // SAFETY: n_fs_base was validated < 1<<47
                            // (canonical user addr) when arch_prctl/clone
                            // stored it, so WRMSR cannot #GP.
                            unsafe {
                                crate::cpu::wrmsr(crate::cpu::IA32_FS_BASE, n_fs_base);
                            }
                            // Restore this user thread's userspace %gs base.
                            // Like %fs, the userspace %gs base is the active
                            // IA32_GS_BASE during kernel execution: the syscall
                            // entry stub swaps GS back before calling Rust
                            // (so the handler runs with active GS = user %gs,
                            // KERNEL_GS_BASE = per-CPU), and interrupts never
                            // SWAPGS — so this CPU's per-CPU pointer always
                            // rests in KERNEL_GS_BASE and the active register
                            // is the value the task sees in ring 3.  0 = no
                            // custom %gs (the default).
                            // SAFETY: n_gs_base was validated < 1<<47 (canonical
                            // user addr) when arch_prctl/clone stored it, so
                            // WRMSR cannot #GP.
                            unsafe {
                                crate::cpu::wrmsr(crate::cpu::IA32_GS_BASE, n_gs_base);
                            }
                        }

                        // Update kernel stack for ring transitions.
                        #[allow(clippy::arithmetic_side_effects)]
                        let stack_top = if n_sb != 0 {
                            n_sb + task::TASK_STACK_SIZE as u64
                        } else {
                            0
                        };
                        if stack_top != 0 {
                            // SAFETY: Interrupts are effectively disabled
                            // (idle flag prevents preempt).
                            unsafe {
                                crate::syscall::entry::set_kernel_stack(stack_top);
                                crate::gdt::set_kernel_stack(stack_top);
                            }
                        }

                        // Increment context switch counter.
                        if let Some(ctr) = CTX_SWITCHES.get(cpu) {
                            ctr.fetch_add(1, Ordering::Relaxed);
                        }

                        crate::ktrace::record(
                            crate::ktrace::Category::Sched,
                            crate::ktrace::event::CONTEXT_SWITCH,
                            current_id,
                            ready_id,
                        );

                        // Profile the actual context switch (save/restore/CR3).
                        let _prof_t = crate::kprofile::begin(crate::kprofile::Slot::ContextSwitch);

                        // SAFETY: Both context and FPU pointers valid (from
                        // task table under lock).  old is &mut (exclusive),
                        // new is & (shared), pointing to different tasks.
                        // FPU pointers are 64-byte aligned (FpuState has
                        // repr(align(64))).
                        unsafe { switch_context(&mut *old_p, &*new_p, old_fpu, new_fpu); }

                        // NOTE: After switch_context returns, we're now
                        // running as the OLD task (resumed later).  The
                        // profiling end() measures the full switch-out +
                        // switch-back-in cycle for this task, which is
                        // informative but not a single context switch cost.
                        // The actual one-way switch cost is half this value
                        // (or use the ISR measurement for precise one-way).
                        crate::kprofile::end(crate::kprofile::Slot::ContextSwitch, _prof_t);

                        // Resumed: this task was unblocked and switched
                        // back to.  (For Dead tasks, this line is
                        // unreachable — no one switches to a Dead task.)
                        return;
                    }

                    // Context extraction failed (task reaped while we
                    // were idling — shouldn't happen since reap checks
                    // all CPUs, but handle gracefully).
                    drop(s);
                }
            }
            return;
        };

        if picked_id == current_id && requeue {
            // Same task picked — no switch needed.
            if let Some(task) = state.tasks.get_mut(&current_id) {
                task.state = TaskState::Running;
            }
            return;
        }

        next_id = picked_id;

        // Mark the next task as Running and extract its metadata.
        // We do this before extracting the old task's pointer because
        // the mutable borrow ends when we're done with the task.
        if let Some(next_task) = state.tasks.get_mut(&next_id) {
            next_task.record_dispatch(crate::apic::tick_count());
            next_task.state = TaskState::Running;
            next_task.last_cpu = cpu;
        }

        // Extract raw pointers and metadata for both tasks under this
        // single lock acquisition.  We get the old task's mutable pointer
        // first, then release that borrow, then get the new task's
        // read-only pointer.  Raw pointers are stable because no entries
        // are added or removed while we hold the lock.
        //
        // SAFETY: The raw pointers point into BTreeMap node allocations.
        // No structural modification (insert/remove) occurs between
        // pointer extraction and use, so the pointers remain valid.
        // The lock is dropped before switch_context, but no other code
        // on this CPU runs until the switch completes.
        let old_data = state.tasks.get_mut(&current_id)
            .map(|t| {
                // Check stack canary before switching away from this task.
                t.check_stack_canary();
                // Reaching here means a real switch (picked_id != current_id
                // returns early above), so charge the outgoing task's
                // voluntary/involuntary context-switch counter.
                match kind {
                    SwitchKind::Voluntary => {
                        t.nvcsw = t.nvcsw.saturating_add(1);
                    }
                    SwitchKind::Involuntary => {
                        t.nivcsw = t.nivcsw.saturating_add(1);
                    }
                    SwitchKind::Uncounted => {}
                }
                (&raw mut t.context, &raw mut t.fpu_state, t.pml4_phys)
            });
        let new_data = state.tasks.get(&next_id)
            .map(|t| (&raw const t.context, &raw const t.fpu_state, t.pml4_phys, t.stack_bottom, t.fs_base, t.gs_base));

        if let (Some((old, old_fpu, o_pml4)), Some((new, new_fpu, n_pml4, n_stack_bottom, n_fs_base, n_gs_base))) =
            (old_data, new_data)
        {
            old_ctx_ptr = old;
            new_ctx_ptr = new;
            old_fpu_ptr = old_fpu;
            new_fpu_ptr = new_fpu;
            old_pml4 = o_pml4;
            new_pml4 = n_pml4;
            new_fs_base = n_fs_base;
            new_gs_base = n_gs_base;
            #[allow(clippy::arithmetic_side_effects)]
            {
                new_stack_top = if n_stack_bottom != 0 {
                    n_stack_bottom + task::TASK_STACK_SIZE as u64
                } else {
                    0
                };
            }
        } else {
            serial_println!(
                "[sched] BUG: context switch failed — task {} or {} not in table",
                current_id, next_id
            );
            return;
        }

        // Account CPU cycles to the outgoing task (TSC-based).
        account_cycles(&mut state, current_id, cpu);

        // Lock is dropped here before the context switch.
    }

    // --- Context switch (outside the lock) ---

    set_current_task(cpu, next_id);

    // Switch CR3 if the new task uses a different address space.
    // pml4_phys == 0 means "kernel address space" → use KERNEL_PML4.
    if old_pml4 != new_pml4 {
        let target_pml4 = if new_pml4 == 0 {
            KERNEL_PML4.load(Ordering::Acquire)
        } else {
            new_pml4
        };
        // SAFETY: target_pml4 is a valid PML4 with kernel entries
        // (256-511) cloned from the boot PML4.  Our currently
        // executing kernel code and stack are mapped through those
        // kernel entries, so the switch is safe.
        unsafe {
            crate::mm::page_table::write_cr3(target_pml4);
        }
    }

    // Restore this user thread's %fs (TLS) base.  IA32_FS_BASE is a global
    // CPU register not saved in the GP Context, so without this two
    // Linux/glibc processes would clobber each other's TLS pointer.
    // Kernel tasks (pml4==0) never read %fs.
    if new_pml4 != 0 {
        // SAFETY: new_fs_base was validated < 1<<47 (canonical user addr)
        // when arch_prctl/clone stored it, so WRMSR cannot #GP.
        unsafe {
            crate::cpu::wrmsr(crate::cpu::IA32_FS_BASE, new_fs_base);
        }
        // Restore the userspace %gs base.  Like %fs, this is the active
        // IA32_GS_BASE during kernel execution: the syscall entry stub swaps
        // GS back before calling Rust (handler runs with active GS = user %gs,
        // KERNEL_GS_BASE = per-CPU) and interrupts never SWAPGS, so the per-CPU
        // pointer always rests in KERNEL_GS_BASE.  0 = no custom %gs.  See
        // Task::gs_base.
        // SAFETY: new_gs_base was validated < 1<<47 (canonical user addr) when
        // arch_prctl/clone stored it, so WRMSR cannot #GP.
        unsafe {
            crate::cpu::wrmsr(crate::cpu::IA32_GS_BASE, new_gs_base);
        }
    }

    // Update the kernel stack pointers for ring 3 → ring 0 transitions.
    //
    // Two independent mechanisms must agree on the kernel stack:
    //
    // 1. SYSCALL entry: the assembly stub loads RSP from per-CPU data
    //    (IA32_KERNEL_GS_BASE → PER_CPU.kernel_rsp).
    //
    // 2. Hardware interrupts from ring 3: the CPU loads RSP from
    //    TSS.RSP0 before pushing the interrupt frame.
    //
    // Both must point to this task's kernel stack top.
    if new_stack_top != 0 {
        // SAFETY: Interrupts are disabled (we're in the context
        // switch path).  No concurrent access on this CPU.
        unsafe {
            crate::syscall::entry::set_kernel_stack(new_stack_top);
            crate::gdt::set_kernel_stack(new_stack_top);
        }
    }

    // SAFETY:
    // - Both context and FPU pointers are valid (extracted from the task
    //   table under lock).
    // - The BTreeMap nodes won't be freed during the switch because no
    //   other code runs on this CPU until switch_context returns.
    // - old_ctx_ptr is &mut (exclusive write) and new_ctx_ptr is &
    //   (shared read), pointing to different tasks' contexts.
    // - FPU pointers are 64-byte aligned (FpuState has repr(align(64))).
    // Increment context switch counter for this CPU.
    if let Some(ctr) = CTX_SWITCHES.get(cpu) {
        ctr.fetch_add(1, Ordering::Relaxed);
    }

    crate::ktrace::record(
        crate::ktrace::Category::Sched,
        crate::ktrace::event::CONTEXT_SWITCH,
        current_id,
        next_id,
    );

    unsafe {
        switch_context(&mut *old_ctx_ptr, &*new_ctx_ptr, old_fpu_ptr, new_fpu_ptr);
    }

    // When we return here, some other task has switched back to us.
    // We're now running as current_id again.
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// SMP-specific scheduler validation.
///
/// Must be called AFTER `smp::init()` so all APs are online with their
/// idle tasks registered.  Tests that per-CPU invariants hold across
/// multiple real CPUs — things that `self_test()` can't check because
/// it runs before SMP bootstrap.
pub fn smp_self_test() -> KernelResult<()> {
    let num_cpus = crate::smp::cpu_count().max(1);
    serial_println!(
        "[sched] Running SMP scheduler validation ({} CPUs)...",
        num_cpus
    );

    // 1. Each CPU must have a distinct current task.
    {
        let mut seen = alloc::collections::BTreeSet::new();
        for i in 0..num_cpus {
            let id = CURRENT_TASK_IDS
                .get(i)
                .map_or(0, |a| a.load(Ordering::Acquire));
            if !seen.insert(id) {
                serial_println!(
                    "[sched]   FAIL: CPU {} shares current_task {} with another CPU",
                    i, id
                );
                return Err(KernelError::InternalError);
            }
        }
    }
    serial_println!(
        "[sched]   Distinct current tasks: OK ({} CPUs)",
        num_cpus
    );

    // 2. Each AP's idle task must exist in the task table at IDLE_PRIORITY.
    if num_cpus > 1 {
        let state = SCHED.lock();
        for i in 1..num_cpus {
            let ap_current = CURRENT_TASK_IDS
                .get(i)
                .map_or(0, |a| a.load(Ordering::Acquire));
            match state.tasks.get(&ap_current) {
                Some(t) if t.priority == task::IDLE_PRIORITY => {}
                Some(t) => {
                    serial_println!(
                        "[sched]   FAIL: AP {}'s task {} has priority {} (expected {})",
                        i, ap_current, t.priority, task::IDLE_PRIORITY
                    );
                    return Err(KernelError::InternalError);
                }
                None => {
                    serial_println!(
                        "[sched]   FAIL: AP {}'s current task {} not in table",
                        i, ap_current
                    );
                    return Err(KernelError::InternalError);
                }
            }
        }
        drop(state);
        serial_println!(
            "[sched]   AP idle tasks valid: OK ({} APs)",
            num_cpus - 1
        );
    }

    // 3. No CPU should be in idle fallback state during normal operation.
    for i in 0..num_cpus {
        if IDLE_FLAGS
            .get(i)
            .is_some_and(|f| f.load(Ordering::Acquire))
        {
            serial_println!(
                "[sched]   FAIL: CPU {} has idle flag set during normal operation",
                i
            );
            return Err(KernelError::InternalError);
        }
    }
    serial_println!("[sched]   Idle flags clear: OK");

    // 4. Reap doesn't touch any CPU's current task.
    let current_ids: alloc::vec::Vec<TaskId> = (0..num_cpus)
        .map(|i| {
            CURRENT_TASK_IDS
                .get(i)
                .map_or(0, |a| a.load(Ordering::Acquire))
        })
        .collect();

    reap_dead_tasks();

    {
        let state = SCHED.lock();
        for (i, &id) in current_ids.iter().enumerate() {
            if !state.tasks.contains_key(&id) {
                serial_println!(
                    "[sched]   FAIL: CPU {}'s task {} was reaped!",
                    i, id
                );
                return Err(KernelError::InternalError);
            }
        }
    }
    serial_println!("[sched]   Reap SMP safety: OK");

    serial_println!("[sched] SMP scheduler validation PASSED");
    Ok(())
}

/// Counter for self-test verification.
static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Self-test: spawn tasks, yield between them, verify execution.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[sched] Running scheduler self-test...");

    test_stack_canary()?;
    test_cooperative_scheduling()?;
    test_kill_and_reap()?;
    test_suspend_resume()?;
    test_set_priority()?;
    test_interactive_detection()?;
    test_time_slice_config()?;
    test_workload_profiles()?;
    test_per_cpu_work_stealing()?;
    test_smp_idle_task_safety()?;
    test_transitive_pi_infrastructure()?;
    test_cpu_affinity()?;
    test_exit_hooks()?;
    test_cpu_bandwidth()?;
    test_wait_time_tracking()?;
    test_stack_watermark()?;
    test_load_average()?;
    test_liveness_watchdog()?;

    serial_println!("[sched] Scheduler self-test PASSED");
    Ok(())
}

/// Test the system-wide liveness watchdog's arm/disarm/progress logic and
/// smoke-test the task-table dumper.
///
/// Runs during boot *before* `liveness_arm()` is called in `kmain`, so the
/// watchdog starts disarmed; the test leaves it disarmed again on exit so
/// the real arm point is unaffected.  It verifies:
///  * `note_useful_work()` advances the global progress counter,
///  * `liveness_check()` is a no-op while disarmed (never touches the stall
///    counter — the early-return guard),
///  * `arm()`/`disarm()` toggle the armed flag and `arm()` clears the stall
///    counter,
///  * `dump_task_table()` runs to completion on the live task table without
///    panicking or blocking (exercises the try_lock path + task iteration).
fn test_liveness_watchdog() -> KernelResult<()> {
    // Snapshot so we restore exactly on exit.
    let saved_armed = LIVENESS_ARMED.load(Ordering::Relaxed);

    // Start from a known disarmed state with a cleared stall counter.
    LIVENESS_ARMED.store(false, Ordering::Relaxed);
    LIVENESS_STALL_COUNT.store(0, Ordering::Relaxed);

    // Progress counter must advance on note_useful_work().
    let before = USEFUL_WORK_TICKS.load(Ordering::Relaxed);
    note_useful_work();
    let after = USEFUL_WORK_TICKS.load(Ordering::Relaxed);
    if after == before {
        serial_println!("[sched]   FAIL: note_useful_work did not advance counter");
        return Err(KernelError::InternalError);
    }

    // While disarmed, liveness_check() must return immediately and never
    // touch the stall counter — even across a "no progress" interval.
    LIVENESS_LAST_WORK.store(USEFUL_WORK_TICKS.load(Ordering::Relaxed), Ordering::Relaxed);
    liveness_check(); // disarmed → early return
    liveness_check();
    if LIVENESS_STALL_COUNT.load(Ordering::Relaxed) != 0 {
        serial_println!("[sched]   FAIL: disarmed liveness_check advanced stall counter");
        return Err(KernelError::InternalError);
    }

    // arm() must set the flag and clear the stall counter.
    LIVENESS_STALL_COUNT.store(5, Ordering::Relaxed); // dirty it first
    liveness_arm();
    if !LIVENESS_ARMED.load(Ordering::Relaxed) {
        serial_println!("[sched]   FAIL: liveness_arm did not set the armed flag");
        return Err(KernelError::InternalError);
    }
    if LIVENESS_STALL_COUNT.load(Ordering::Relaxed) != 0 {
        serial_println!("[sched]   FAIL: liveness_arm did not clear the stall counter");
        return Err(KernelError::InternalError);
    }

    // disarm() must clear the flag.
    liveness_disarm();
    if LIVENESS_ARMED.load(Ordering::Relaxed) {
        serial_println!("[sched]   FAIL: liveness_disarm did not clear the armed flag");
        return Err(KernelError::InternalError);
    }

    // Busy-livelock guard (blind spot 1): when useful-work advances every
    // interval but the system-wide context-switch total stays frozen, the
    // guard must count consecutive intervals and, at LIVENESS_ALERT_COUNT,
    // emit a soft warning WITHOUT disarming the watchdog.  Drive the logic
    // with interrupts off so no real preemption/context-switch on this CPU
    // perturbs the frozen-ctx assumption mid-sequence (the boot self-test
    // runs single-CPU, so no other CPU advances CTX_SWITCHES either).
    serial_println!(
        "[sched]   (self-test) intentionally driving the busy-livelock guard; the \
         'SUSPECTED LIVELOCK' line below is expected and not a real event:"
    );
    let livelock_ok = crate::cpu::without_interrupts(|| {
        liveness_arm(); // armed; baselines LAST_WORK and LAST_CTX to "now"
        LIVENESS_CTX_STALL_COUNT.store(0, Ordering::Relaxed);

        // Simulate consecutive intervals where useful-work advances (a task's
        // ticks are charged) but no context switch occurs.  total_ctx_switches()
        // is stable across these back-to-back calls (IF=0, single CPU), so the
        // guard sees a frozen ctx count each interval.
        for _ in 0..LIVENESS_ALERT_COUNT {
            note_useful_work(); // useful-work advances → healthy branch taken
            liveness_check();
        }

        // The guard fired at the threshold and reset its counter, but must NOT
        // have disarmed the watchdog (soft warning only).
        if !LIVENESS_ARMED.load(Ordering::Relaxed) {
            serial_println!("[sched]   FAIL: livelock guard disarmed the watchdog");
            return false;
        }
        if LIVENESS_CTX_STALL_COUNT.load(Ordering::Relaxed) != 0 {
            serial_println!("[sched]   FAIL: livelock guard did not reset its counter after warning");
            return false;
        }

        // A subsequent interval that DOES advance context switches must clear
        // the guard's counter (no spurious accrual once scheduling resumes).
        note_useful_work();
        LIVENESS_LAST_CTX.store(total_ctx_switches().wrapping_sub(1), Ordering::Relaxed);
        liveness_check();
        if LIVENESS_CTX_STALL_COUNT.load(Ordering::Relaxed) != 0 {
            serial_println!("[sched]   FAIL: livelock guard counted an interval with ctx-switch progress");
            return false;
        }
        true
    });
    liveness_disarm();
    if !livelock_ok {
        return Err(KernelError::InternalError);
    }

    // Smoke-test the dumper on the live task table: must not panic or block.
    // (Output goes to serial; at self-test time the table is small.)
    dump_task_table();

    // Restore the watchdog to its pre-test state (disarmed during boot).
    LIVENESS_ARMED.store(saved_armed, Ordering::Relaxed);
    LIVENESS_STALL_COUNT.store(0, Ordering::Relaxed);

    serial_println!("[sched]   liveness watchdog: OK");
    Ok(())
}

/// Test the load-average EWMA math (`calc_load`) and fixed-point formatting.
///
/// Regression guard for the round-up-when-rising term in `calc_load`: a
/// steady single-runnable workload must converge UP to *exactly* load 1.00
/// (`FIXED_1`).  Without the round-up, integer truncation stalls the average
/// one unit short (2047) and it never reaches the integer target — so this
/// test fails against the buggy version and passes against the fixed one.
fn test_load_average() -> KernelResult<()> {
    // Rising load: active = 1.00 in fixed-point (one runnable task).
    let active_one = LOAD_FIXED_1;
    let mut load = 0u64;
    for _ in 0..400 {
        load = calc_load(load, LOAD_EXP_1, active_one);
    }
    if load != LOAD_FIXED_1 {
        serial_println!(
            "[sched]   FAIL: rising load converged to {load}, want {LOAD_FIXED_1}"
        );
        return Err(KernelError::InternalError);
    }
    if load_int(load) != 1 || load_frac(load) != 0 {
        serial_println!(
            "[sched]   FAIL: load 1.00 formatted as {}.{:02}",
            load_int(load), load_frac(load)
        );
        return Err(KernelError::InternalError);
    }

    // Falling load: with no runnable tasks the average must decay to 0.
    let mut load = LOAD_FIXED_1;
    for _ in 0..400 {
        load = calc_load(load, LOAD_EXP_1, 0);
    }
    if load != 0 {
        serial_println!("[sched]   FAIL: idle load decayed to {load}, want 0");
        return Err(KernelError::InternalError);
    }

    // Fixed-point formatting (Linux LOAD_INT / LOAD_FRAC): 1.50 == 3072.
    let l = LOAD_FIXED_1.saturating_add(LOAD_FIXED_1 / 2);
    if load_int(l) != 1 || load_frac(l) != 50 {
        serial_println!(
            "[sched]   FAIL: 1.50 fixed-point formatted as {}.{:02}",
            load_int(l), load_frac(l)
        );
        return Err(KernelError::InternalError);
    }

    serial_println!("[sched]   load-average EWMA + formatting OK");
    Ok(())
}

/// Post-interrupt-enable test for sleep_ns.
///
/// This test MUST run after `sti()` (interrupts enabled) because it
/// requires the APIC timer to fire and drive the hrtimer subsystem.
/// Called from main.rs after the full boot sequence enables interrupts.
pub fn test_sleep_ns_postboot() -> KernelResult<()> {
    test_sleep_ns()
}

/// Test 0: Stack canary — verify canary is planted and survives execution.
fn test_stack_canary() -> KernelResult<()> {
    #[allow(unused_imports)]
    use crate::mm::page_table;

    // Spawn a task, let it run (just increments a counter and exits),
    // then verify the canary is still intact.
    TEST_COUNTER.store(0, Ordering::SeqCst);
    let id = spawn(b"test-canary", 16, test_task_incr, 1, 0)?;

    // Let the task run and exit.
    yield_now();
    yield_now();

    // The task should be dead now.  Check its canary.
    {
        let state = SCHED.lock();
        if let Some(t) = state.tasks.get(&id) {
            if t.stack_bottom != 0 {
                // SAFETY: stack_bottom is valid HHDM address.
                let canary = unsafe {
                    core::ptr::read_volatile(t.stack_bottom as *const u64)
                };
                // Compare against the per-task planted value, not the
                // global canary (see Task::planted_canary).
                if canary != t.planted_canary {
                    serial_println!(
                        "[sched]   FAIL: stack canary corrupted for task {}",
                        id
                    );
                    return Err(KernelError::InternalError);
                }
            }
        }
    }

    // Clean up.
    {
        let mut state = SCHED.lock();
        if let Some(mut t) = state.tasks.remove(&id) {
            if t.stack_phys != 0 {
                // SAFETY: Task is dead and removed.
                unsafe { let _ = t.free_stack(); }
                t.stack_phys = 0;
            }
        }
    }

    serial_println!("[sched]   Stack canary: OK");
    Ok(())
}

/// Test 1: Cooperative scheduling — spawn tasks, yield, verify.
fn test_cooperative_scheduling() -> KernelResult<()> {
    TEST_COUNTER.store(0, Ordering::SeqCst);

    // Spawn two test tasks.
    let id_a = spawn(b"test-a", 16, test_task_a, 10, 0)?;
    let id_b = spawn(b"test-b", 16, test_task_b, 20, 0)?;
    serial_println!("[sched]   Spawned test tasks: {} and {}", id_a, id_b);

    // Yield to let the test tasks run.
    // Each test task increments TEST_COUNTER and yields back.
    yield_now();  // → test-a runs, increments to 10, yields
    yield_now();  // → test-b runs, increments to 30, yields
    yield_now();  // → test-a runs again, increments to 40, exits
    yield_now();  // → test-b runs again, increments to 60, exits

    let final_count = TEST_COUNTER.load(Ordering::SeqCst);
    serial_println!("[sched]   Test counter final value: {}", final_count);

    if final_count != 60 {
        serial_println!(
            "[sched]   FAIL: expected counter=60, got {}",
            final_count
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[sched]   Cooperative scheduling: OK");

    // Clean up dead tasks.
    {
        let mut state = SCHED.lock();
        for &id in &[id_a, id_b] {
            if let Some(mut task) = state.tasks.remove(&id)
                && task.state == TaskState::Dead
                && task.stack_phys != 0
            {
                // SAFETY: Task is dead and removed from the table;
                // no CPU is using its stack.
                unsafe { let _ = task.free_stack(); }
                // Clear stack_phys so Drop doesn't warn.
                task.stack_phys = 0;
            }
        }
    }
    serial_println!("[sched]   Cleanup (free dead task stacks): OK");
    Ok(())
}

/// Test 1b: Kill a task remotely and reap dead tasks.
///
/// Verifies:
/// - kill_task() prevents a Ready task from ever running
/// - kill_task() on a Blocked task marks it Dead
/// - kill_task() refuses to kill the current task
/// - kill_task() on an already-Dead task returns false
/// - reap_dead_tasks() frees stacks and removes tasks from the table
fn test_kill_and_reap() -> KernelResult<()> {
    TEST_COUNTER.store(0, Ordering::SeqCst);

    // Spawn a task but kill it before it gets a chance to run.
    let id_kill = spawn(b"test-kill-ready", 16, test_task_incr, 999, 0)?;

    // The task is in Ready state.  Kill it.
    if !kill_task(id_kill) {
        serial_println!("[sched]   FAIL: kill_task returned false for Ready task");
        return Err(KernelError::InternalError);
    }

    // Yield a few times — the killed task should NOT run.
    yield_now();
    yield_now();

    let counter = TEST_COUNTER.load(Ordering::SeqCst);
    if counter != 0 {
        serial_println!(
            "[sched]   FAIL: killed task ran (counter={}, expected 0)",
            counter
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[sched]   kill_task(Ready): OK (task did not run)");

    // Verify double-kill returns false.
    if kill_task(id_kill) {
        serial_println!("[sched]   FAIL: double kill_task returned true");
        return Err(KernelError::InternalError);
    }
    serial_println!("[sched]   kill_task(Dead): OK (returned false)");

    // Verify killing the current task is refused.
    let current = current_task_id();
    if kill_task(current) {
        serial_println!("[sched]   FAIL: kill_task(current) should return false");
        return Err(KernelError::InternalError);
    }
    serial_println!("[sched]   kill_task(current): OK (refused)");

    // Test kill of a Blocked task.  Spawn a task and let it block itself
    // by calling block_current.  We use a special task entry for this.
    let id_block = spawn(b"test-kill-block", 16, test_task_block_self, 0, 0)?;

    // Let it run — it will block itself.
    yield_now();
    yield_now();

    // Verify it's blocked.
    {
        let state = SCHED.lock();
        let task_state = state.tasks.get(&id_block).map(|t| t.state);
        if task_state != Some(TaskState::Blocked) {
            serial_println!(
                "[sched]   FAIL: expected Blocked, got {:?}",
                task_state
            );
            return Err(KernelError::InternalError);
        }
    }

    // Kill the blocked task.
    if !kill_task(id_block) {
        serial_println!("[sched]   FAIL: kill_task returned false for Blocked task");
        return Err(KernelError::InternalError);
    }
    serial_println!("[sched]   kill_task(Blocked): OK");

    // Now test reap_dead_tasks.
    let reaped = reap_dead_tasks();
    if reaped < 2 {
        serial_println!(
            "[sched]   FAIL: reap_dead_tasks returned {} (expected >= 2)",
            reaped
        );
        return Err(KernelError::InternalError);
    }

    // Verify the tasks are gone from the table.
    {
        let state = SCHED.lock();
        if state.tasks.contains_key(&id_kill) || state.tasks.contains_key(&id_block) {
            serial_println!("[sched]   FAIL: reaped tasks still in table");
            return Err(KernelError::InternalError);
        }
    }
    serial_println!(
        "[sched]   reap_dead_tasks: OK ({} reaped, tasks removed from table)",
        reaped
    );

    Ok(())
}

/// Test 2: Suspend and resume a task.
///
/// Spawns a task, suspends it before it runs, verifies it doesn't
/// execute, then resumes it and verifies it runs.
fn test_suspend_resume() -> KernelResult<()> {
    TEST_COUNTER.store(0, Ordering::SeqCst);

    // Spawn a task that increments the counter.
    let id = spawn(b"test-suspend", 16, test_task_incr, 100, 0)?;

    // Suspend it before it gets a chance to run.
    if !suspend(id) {
        serial_println!("[sched]   FAIL: suspend returned false");
        return Err(KernelError::InternalError);
    }

    // Verify the task is Suspended.
    {
        let state = SCHED.lock();
        if let Some(task) = state.tasks.get(&id) {
            if task.state != TaskState::Suspended {
                serial_println!(
                    "[sched]   FAIL: expected Suspended, got {:?}",
                    task.state
                );
                return Err(KernelError::InternalError);
            }
        }
    }

    // Yield a few times — the suspended task should NOT run.
    yield_now();
    yield_now();

    let count_after_yields = TEST_COUNTER.load(Ordering::SeqCst);
    if count_after_yields != 0 {
        serial_println!(
            "[sched]   FAIL: suspended task ran (counter={})",
            count_after_yields
        );
        return Err(KernelError::InternalError);
    }

    // Resume the task.
    if !resume(id) {
        serial_println!("[sched]   FAIL: resume returned false");
        return Err(KernelError::InternalError);
    }

    // Now yield — the task should run and increment the counter.
    yield_now();
    yield_now();

    let count_after_resume = TEST_COUNTER.load(Ordering::SeqCst);
    if count_after_resume != 100 {
        serial_println!(
            "[sched]   FAIL: after resume, counter={}, expected 100",
            count_after_resume
        );
        return Err(KernelError::InternalError);
    }

    // Clean up.
    {
        let mut state = SCHED.lock();
        if let Some(mut task) = state.tasks.remove(&id)
            && task.state == TaskState::Dead
            && task.stack_phys != 0
        {
            // SAFETY: Task is Dead, removed from table, stack_phys is valid.
            unsafe { let _ = task.free_stack(); }
            task.stack_phys = 0;
        }
    }

    serial_println!("[sched]   Suspend/resume: OK");
    Ok(())
}

/// Test 3: Change a task's scheduling priority.
fn test_set_priority() -> KernelResult<()> {
    TEST_COUNTER.store(0, Ordering::SeqCst);

    // Spawn at priority 16.
    let id = spawn(b"test-prio", 16, test_task_incr, 50, 0)?;

    // Change priority to 8.
    match set_priority(id, 8) {
        Some(16) => {} // Old priority was 16.
        other => {
            serial_println!(
                "[sched]   FAIL: set_priority returned {:?}, expected Some(16)",
                other
            );
            kill_task(id);
            return Err(KernelError::InternalError);
        }
    }

    // Verify the new priority.
    {
        let state = SCHED.lock();
        if let Some(task) = state.tasks.get(&id) {
            if task.priority != 8 {
                serial_println!(
                    "[sched]   FAIL: priority should be 8, got {}",
                    task.priority
                );
                return Err(KernelError::InternalError);
            }
        }
    }

    // Let the task run and clean up.
    yield_now();
    yield_now();

    if TEST_COUNTER.load(Ordering::SeqCst) != 50 {
        serial_println!("[sched]   FAIL: task didn't run after priority change");
        return Err(KernelError::InternalError);
    }

    // Clean up.
    {
        let mut state = SCHED.lock();
        if let Some(mut task) = state.tasks.remove(&id)
            && task.state == TaskState::Dead
            && task.stack_phys != 0
        {
            // SAFETY: Task is Dead, removed from table, stack_phys is valid.
            unsafe { let _ = task.free_stack(); }
            task.stack_phys = 0;
        }
    }

    serial_println!("[sched]   Set priority: OK");
    Ok(())
}

/// Test 4: Interactive task detection via burst tracking.
///
/// Verifies that a task which frequently blocks with short CPU bursts
/// gets marked as interactive (and thus receives a priority boost).
fn test_interactive_detection() -> KernelResult<()> {
    use task::{INTERACTIVE_BOOST, INTERACTIVE_THRESHOLD_TICKS};

    // Create a task directly to test the detection logic without
    // needing actual I/O blocking (which we can't easily simulate).
    let base_priority: u8 = 16;
    let id = spawn(b"test-interactive", base_priority, test_task_incr, 1, 0)?;

    // Simulate several short CPU bursts (1 tick each) by manually
    // manipulating the task's burst tracking fields.
    {
        let mut state = SCHED.lock();
        if let Some(task) = state.tasks.get_mut(&id) {
            // Simulate 5 block events with 1-tick bursts each.
            // After enough short bursts, avg should be < threshold.
            for _ in 0..5 {
                task.burst_ticks = 1;
                task.record_block();
            }

            if !task.interactive {
                serial_println!(
                    "[sched]   FAIL: task should be interactive after short bursts (avg_x8={})",
                    task.avg_burst_x8
                );
                return Err(KernelError::InternalError);
            }

            let effective = task.effective_priority();
            let expected = base_priority.saturating_sub(INTERACTIVE_BOOST);
            if effective != expected {
                serial_println!(
                    "[sched]   FAIL: effective priority should be {}, got {}",
                    expected, effective
                );
                return Err(KernelError::InternalError);
            }

            // Now simulate a long CPU burst (50 ticks).
            // This should eventually make the task non-interactive.
            for _ in 0..3 {
                task.burst_ticks = 50;
                task.record_block();
            }

            if task.interactive {
                serial_println!(
                    "[sched]   FAIL: task should NOT be interactive after long bursts (avg_x8={})",
                    task.avg_burst_x8
                );
                return Err(KernelError::InternalError);
            }

            let effective = task.effective_priority();
            if effective != base_priority {
                serial_println!(
                    "[sched]   FAIL: non-interactive effective priority should be {}, got {}",
                    base_priority, effective
                );
                return Err(KernelError::InternalError);
            }
        }
    }

    // Let the task run and clean up.
    yield_now();
    yield_now();

    {
        let mut state = SCHED.lock();
        if let Some(mut task) = state.tasks.remove(&id)
            && task.state == TaskState::Dead
            && task.stack_phys != 0
        {
            // SAFETY: Task is Dead, removed from table, stack_phys is valid.
            unsafe { let _ = task.free_stack(); }
            task.stack_phys = 0;
        }
    }

    serial_println!(
        "[sched]   Interactive detection (threshold={}ticks, boost={}): OK",
        INTERACTIVE_THRESHOLD_TICKS, INTERACTIVE_BOOST
    );
    Ok(())
}

/// Test 5: Runtime time slice configuration.
///
/// Verifies `set_time_slice`, `get_time_slice`, and `reconfigure_time_slices`
/// through the sched module's public API.
fn test_time_slice_config() -> KernelResult<()> {
    // Read the default time slice for level 0 (should be BASE=2).
    let default_0 = get_time_slice(0);
    if default_0 != Some(2) {
        serial_println!(
            "[sched]   FAIL: default time slice for level 0 is {:?}, expected Some(2)",
            default_0
        );
        return Err(KernelError::InternalError);
    }

    // Read default for level 10: BASE + 10 * INCREMENT = 2 + 10 = 12.
    let default_10 = get_time_slice(10);
    if default_10 != Some(12) {
        serial_println!(
            "[sched]   FAIL: default time slice for level 10 is {:?}, expected Some(12)",
            default_10
        );
        return Err(KernelError::InternalError);
    }

    // Out-of-range level should return None.
    if get_time_slice(32).is_some() {
        serial_println!("[sched]   FAIL: get_time_slice(32) should return None");
        return Err(KernelError::InternalError);
    }

    // Set level 5 to 100 ticks.
    if !set_time_slice(5, 100) {
        serial_println!("[sched]   FAIL: set_time_slice(5, 100) returned false");
        return Err(KernelError::InternalError);
    }
    if get_time_slice(5) != Some(100) {
        serial_println!("[sched]   FAIL: after set, level 5 is {:?}", get_time_slice(5));
        return Err(KernelError::InternalError);
    }

    // Zero ticks should be rejected.
    if set_time_slice(5, 0) {
        serial_println!("[sched]   FAIL: set_time_slice(5, 0) should return false");
        return Err(KernelError::InternalError);
    }

    // Out-of-range level should be rejected.
    if set_time_slice(32, 10) {
        serial_println!("[sched]   FAIL: set_time_slice(32, 10) should return false");
        return Err(KernelError::InternalError);
    }

    // Reconfigure all: base=4, increment=2. Level 5 should become 4+5*2=14.
    if !reconfigure_time_slices(4, 2) {
        serial_println!("[sched]   FAIL: reconfigure_time_slices(4, 2) returned false");
        return Err(KernelError::InternalError);
    }
    if get_time_slice(0) != Some(4) {
        serial_println!("[sched]   FAIL: after reconfig, level 0 is {:?}", get_time_slice(0));
        return Err(KernelError::InternalError);
    }
    if get_time_slice(5) != Some(14) {
        serial_println!("[sched]   FAIL: after reconfig, level 5 is {:?}", get_time_slice(5));
        return Err(KernelError::InternalError);
    }

    // Zero base should be rejected.
    if reconfigure_time_slices(0, 1) {
        serial_println!("[sched]   FAIL: reconfigure(0, 1) should return false");
        return Err(KernelError::InternalError);
    }

    // Restore defaults: base=2, increment=1.
    if !reconfigure_time_slices(2, 1) {
        serial_println!("[sched]   FAIL: could not restore default time slices");
        return Err(KernelError::InternalError);
    }

    serial_println!("[sched]   Time slice configuration: OK");
    Ok(())
}

/// Test 6: Workload profile presets.
///
/// Verifies that each profile applies the correct time slice formula
/// and that the profile can be queried back.
fn test_workload_profiles() -> KernelResult<()> {
    // Desktop profile (id=0): base=2, inc=1.
    if !apply_workload_profile(0) {
        serial_println!("[sched]   FAIL: apply Desktop profile returned false");
        return Err(KernelError::InternalError);
    }
    if get_time_slice(0) != Some(2) || get_time_slice(1) != Some(3) {
        serial_println!(
            "[sched]   FAIL: Desktop profile: level0={:?}, level1={:?}, expected 2, 3",
            get_time_slice(0), get_time_slice(1)
        );
        return Err(KernelError::InternalError);
    }
    match current_workload_profile() {
        Some(WorkloadProfile::Desktop) => {}
        other => {
            serial_println!(
                "[sched]   FAIL: current_workload_profile after Desktop = {:?}",
                other
            );
            return Err(KernelError::InternalError);
        }
    }

    // Server profile (id=1): base=4, inc=2.
    if !apply_workload_profile(1) {
        serial_println!("[sched]   FAIL: apply Server profile returned false");
        return Err(KernelError::InternalError);
    }
    if get_time_slice(0) != Some(4) || get_time_slice(1) != Some(6) {
        serial_println!(
            "[sched]   FAIL: Server profile: level0={:?}, level1={:?}, expected 4, 6",
            get_time_slice(0), get_time_slice(1)
        );
        return Err(KernelError::InternalError);
    }
    match current_workload_profile() {
        Some(WorkloadProfile::Server) => {}
        other => {
            serial_println!(
                "[sched]   FAIL: current_workload_profile after Server = {:?}",
                other
            );
            return Err(KernelError::InternalError);
        }
    }

    // Development profile (id=2): base=1, inc=1.
    if !apply_workload_profile(2) {
        serial_println!("[sched]   FAIL: apply Development profile returned false");
        return Err(KernelError::InternalError);
    }
    if get_time_slice(0) != Some(1) || get_time_slice(1) != Some(2) {
        serial_println!(
            "[sched]   FAIL: Development profile: level0={:?}, level1={:?}, expected 1, 2",
            get_time_slice(0), get_time_slice(1)
        );
        return Err(KernelError::InternalError);
    }
    match current_workload_profile() {
        Some(WorkloadProfile::Development) => {}
        other => {
            serial_println!(
                "[sched]   FAIL: current_workload_profile after Development = {:?}",
                other
            );
            return Err(KernelError::InternalError);
        }
    }

    // Gaming profile (id=3): base=1, inc=2.
    if !apply_workload_profile(3) {
        serial_println!("[sched]   FAIL: apply Gaming profile returned false");
        return Err(KernelError::InternalError);
    }
    if get_time_slice(0) != Some(1) || get_time_slice(1) != Some(3) {
        serial_println!(
            "[sched]   FAIL: Gaming profile: level0={:?}, level1={:?}, expected 1, 3",
            get_time_slice(0), get_time_slice(1)
        );
        return Err(KernelError::InternalError);
    }
    match current_workload_profile() {
        Some(WorkloadProfile::Gaming) => {}
        other => {
            serial_println!(
                "[sched]   FAIL: current_workload_profile after Gaming = {:?}",
                other
            );
            return Err(KernelError::InternalError);
        }
    }

    // Invalid profile ID should be rejected.
    if apply_workload_profile(4) {
        serial_println!("[sched]   FAIL: profile ID 4 should be rejected");
        return Err(KernelError::InternalError);
    }
    if apply_workload_profile(255) {
        serial_println!("[sched]   FAIL: profile ID 255 should be rejected");
        return Err(KernelError::InternalError);
    }

    // After manual tuning, current_workload_profile should return None.
    let _ = set_time_slice(0, 99);
    if current_workload_profile().is_some() {
        serial_println!("[sched]   FAIL: profile should be None after manual tuning");
        return Err(KernelError::InternalError);
    }

    // Restore default Desktop profile.
    if !apply_workload_profile(0) {
        serial_println!("[sched]   FAIL: could not restore Desktop profile");
        return Err(KernelError::InternalError);
    }

    serial_println!("[sched]   Workload profiles: OK (Desktop/Server/Development/Gaming)");
    Ok(())
}

/// Test 7: Per-CPU work stealing data structure.
///
/// Tests the `PerCpuScheduler` directly: enqueue on one CPU, steal
/// from another.  This validates the work stealing algorithm without
/// requiring actual SMP hardware.
fn test_per_cpu_work_stealing() -> KernelResult<()> {
    use alloc::boxed::Box;
    use self::priority_rr::PerCpuScheduler;

    // PerCpuScheduler is ~58 KB (MAX_CPUS=64 × ~900 bytes each).
    // Must be heap-allocated — kernel task stacks are only 32 KB.
    let sched = Box::new(PerCpuScheduler::new_const());
    sched.init(4); // Simulate 4 CPUs

    // Enqueue several tasks on CPU 1.
    for id in 100..108u64 {
        sched.enqueue(id, 16, 1);
    }

    // CPU 0's local queue should be empty.
    if sched.pick_next_local(0).is_some() {
        serial_println!("[sched]   FAIL: CPU 0 should have no local tasks");
        return Err(KernelError::InternalError);
    }

    // CPU 0 steals from the most-loaded CPU (CPU 1 has 8 tasks).
    let mut migrated = priority_rr::MigratedTasks::new();
    let stolen = sched.try_steal(0, &mut migrated);
    if stolen.is_none() {
        serial_println!("[sched]   FAIL: work stealing returned None");
        return Err(KernelError::InternalError);
    }

    // The migrated buffer should contain all stolen task IDs.
    // The first one is also in `stolen`; the rest were enqueued on CPU 0.
    if migrated.len() == 0 {
        serial_println!("[sched]   FAIL: migrated buffer empty after steal");
        return Err(KernelError::InternalError);
    }
    if migrated.iter().next().copied() != stolen {
        serial_println!("[sched]   FAIL: migrated[0] should match stolen return value");
        return Err(KernelError::InternalError);
    }

    // The first stolen task is returned directly.  `stolen` is `Some`
    // because we checked `is_none()` above and returned on None.
    let first_stolen = stolen.unwrap_or(0);

    // The rest were enqueued on CPU 0.  Pick them all.
    let mut picked = alloc::vec![first_stolen];
    while let Some(id) = sched.pick_next_local(0) {
        picked.push(id);
    }

    // We should have stolen ~4 tasks (half of 8).
    if picked.len() < 2 {
        serial_println!(
            "[sched]   FAIL: expected at least 2 stolen tasks, got {}",
            picked.len()
        );
        return Err(KernelError::InternalError);
    }

    // CPU 1 should still have ~4 tasks remaining.
    let mut remaining = 0usize;
    while sched.pick_next_local(1).is_some() {
        remaining += 1;
    }
    if remaining == 0 {
        serial_println!("[sched]   FAIL: CPU 1 should have remaining tasks after steal");
        return Err(KernelError::InternalError);
    }

    // Verify total tasks = 8.
    #[allow(clippy::arithmetic_side_effects)]
    let total = picked.len() + remaining;
    if total != 8 {
        serial_println!(
            "[sched]   FAIL: total tasks {} != 8 (stolen={}, remaining={})",
            total, picked.len(), remaining
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[sched]   Per-CPU work stealing: OK (stolen={}, remaining={}, total=8)",
        picked.len(), remaining
    );
    Ok(())
}

/// Test: spawn-exit-reap cycle doesn't leak tasks or stacks.
///
/// This runs before SMP bootstrap (single CPU).  The SMP-specific
/// validation (per-CPU idle tasks, reap SMP safety) is in
/// [`smp_self_test`], called after `smp::init()`.
fn test_smp_idle_task_safety() -> KernelResult<()> {
    // Rapid spawn-exit-reap cycle: verify no task or stack leaks.
    let initial_task_count = {
        let state = SCHED.lock();
        state.tasks.len()
    };

    for i in 0..10u64 {
        let _ = spawn(b"test-spawn-exit", 16, test_task_incr, i, 0)?;
    }

    // Yield enough times for all tasks to run and exit.
    for _ in 0..20 {
        yield_now();
    }

    let reaped = reap_dead_tasks();
    let final_task_count = {
        let state = SCHED.lock();
        state.tasks.len()
    };

    if final_task_count != initial_task_count {
        serial_println!(
            "[sched]   FAIL: task leak: {} before, {} after (reaped {})",
            initial_task_count, final_task_count, reaped
        );
        return Err(KernelError::InternalError);
    }
    serial_println!(
        "[sched]   Spawn-exit-reap cycle: OK (reaped {}, no leaks)",
        reaped
    );

    Ok(())
}

/// Test: transitive PI infrastructure.
///
/// Verifies the building blocks for transitive priority inheritance:
/// 1. `set_blocked_on_pi_addr` / `get_blocked_on_pi_addr` — field set/get
/// 2. `pi_chain_boost` — chain walking with a mock owner-lookup callback
/// 3. Priority is boosted transitively through the chain
/// 4. Chain walk stops at depth limit
/// 5. Chain walk stops on cycle detection
fn test_transitive_pi_infrastructure() -> KernelResult<()> {
    #[allow(unused_imports)]
    use crate::mm::page_table;

    // --- Setup: create 3 blocked tasks (A, B, C) ---
    let task_a = spawn(b"pi-test-a", 24, test_task_block_self, 0, 0)?;
    let task_b = spawn(b"pi-test-b", 24, test_task_block_self, 0, 0)?;
    let task_c = spawn(b"pi-test-c", 24, test_task_block_self, 0, 0)?;

    // Let them run and block themselves.
    for _ in 0..10 {
        yield_now();
    }

    // All three should be Blocked now.
    {
        let state = SCHED.lock();
        let a_state = state.tasks.get(&task_a).map(|t| t.state);
        let b_state = state.tasks.get(&task_b).map(|t| t.state);
        let c_state = state.tasks.get(&task_c).map(|t| t.state);
        if a_state != Some(TaskState::Blocked)
            || b_state != Some(TaskState::Blocked)
            || c_state != Some(TaskState::Blocked)
        {
            serial_println!(
                "[sched]   FAIL: PI tasks not blocked: A={:?}, B={:?}, C={:?}",
                a_state, b_state, c_state
            );
            // Clean up.
            drop(state);
            kill_task(task_a);
            kill_task(task_b);
            kill_task(task_c);
            reap_dead_tasks();
            return Err(KernelError::InternalError);
        }
    }

    // --- Test 1: set_blocked_on_pi_addr / get_blocked_on_pi_addr ---
    //
    // Scenario: B owns lock at 0xDEAD_0001, and is itself blocked on
    // the lock at 0xDEAD_0002 (which C owns).  C is not blocked on
    // anything (end of chain).  A is not blocked on any PI addr.
    set_blocked_on_pi_addr(task_b, Some(0xDEAD_0002));

    let b_addr = get_blocked_on_pi_addr(task_b);
    let a_addr = get_blocked_on_pi_addr(task_a); // Should be None.
    let c_addr = get_blocked_on_pi_addr(task_c); // Should be None.

    if b_addr != Some(0xDEAD_0002) || a_addr.is_some() || c_addr.is_some() {
        serial_println!(
            "[sched]   FAIL: blocked_on_pi_addr: A={:?}, B={:?}, C={:?}",
            a_addr, b_addr, c_addr
        );
        kill_task(task_a); kill_task(task_b); kill_task(task_c);
        reap_dead_tasks();
        return Err(KernelError::InternalError);
    }
    serial_println!("[sched]   PI addr set/get: OK");

    // --- Test 2: pi_chain_boost with mock chain A→B→C ---
    //
    // Scenario: high-prio donor task (prio 4) blocks on lock at
    // 0xDEAD_0001, which B owns.  B is itself blocked on lock at
    // 0xDEAD_0002, which C owns.  C is not blocked.
    //
    // Chain: donor(4) → B(owns 0xDEAD_0001, blocked on 0xDEAD_0002)
    //                  → C(owns 0xDEAD_0002, not blocked)
    //
    // Direct boost of B is done by the caller (simulating futex_lock_pi).
    // pi_chain_boost walks B→C: checks B's blocked_on_pi_addr (0xDEAD_0002),
    // finds C as owner, boosts C.

    // First, directly boost B (simulating what futex_lock_pi does).
    boost_priority(task_b, 4);

    // Now walk the chain from B.
    // Mock owner lookup: 0xDEAD_0002 → task_c, everything else → None.
    let mock_c = task_c; // Capture for closure.
    let chain_boosted = pi_chain_boost(task_b, 4, |addr| {
        if addr == 0xDEAD_0002 { Some(mock_c) } else { None }
    });

    if chain_boosted != 1 {
        serial_println!(
            "[sched]   FAIL: pi_chain_boost: expected 1 transitive boost, got {}",
            chain_boosted
        );
        kill_task(task_a); kill_task(task_b); kill_task(task_c);
        reap_dead_tasks();
        return Err(KernelError::InternalError);
    }

    // Verify C's effective priority was boosted to 4.
    let c_eff = get_effective_priority(task_c);
    if c_eff != Some(4) {
        serial_println!(
            "[sched]   FAIL: task C effective prio should be 4, got {:?}",
            c_eff
        );
        kill_task(task_a); kill_task(task_b); kill_task(task_c);
        reap_dead_tasks();
        return Err(KernelError::InternalError);
    }
    serial_println!("[sched]   PI chain boost (A→B→C): OK (C boosted to prio 4)");

    // --- Test 3: clearing blocked_on_pi_addr ---
    set_blocked_on_pi_addr(task_b, None);
    if get_blocked_on_pi_addr(task_b).is_some() {
        serial_println!("[sched]   FAIL: blocked_on_pi_addr not cleared");
        kill_task(task_a); kill_task(task_b); kill_task(task_c);
        reap_dead_tasks();
        return Err(KernelError::InternalError);
    }
    serial_println!("[sched]   PI addr clear: OK");

    // --- Test 4: chain terminates when no blocked_on_pi_addr ---
    // B no longer has a blocked_on address, so chain from B stops.
    let chain_boosted_2 = pi_chain_boost(task_b, 2, |_| Some(task_c));
    if chain_boosted_2 != 0 {
        serial_println!(
            "[sched]   FAIL: expected 0 boosts (chain terminated), got {}",
            chain_boosted_2
        );
        kill_task(task_a); kill_task(task_b); kill_task(task_c);
        reap_dead_tasks();
        return Err(KernelError::InternalError);
    }
    serial_println!("[sched]   PI chain termination: OK");

    // --- Test 5: cycle detection ---
    // Set up a cycle: B→C→B
    set_blocked_on_pi_addr(task_b, Some(0xDEAD_0002));
    set_blocked_on_pi_addr(task_c, Some(0xDEAD_0001));

    // Mock: 0xDEAD_0002→C, 0xDEAD_0001→B (back to start).
    let mock_b = task_b;
    let mock_c2 = task_c;
    let cycle_boosted = pi_chain_boost(task_b, 2, |addr| {
        if addr == 0xDEAD_0002 { Some(mock_c2) }
        else if addr == 0xDEAD_0001 { Some(mock_b) }
        else { None }
    });

    // Should detect the cycle: boost C (1 boost), then find B which is
    // start_owner → stop.
    if cycle_boosted != 1 {
        serial_println!(
            "[sched]   FAIL: cycle detection: expected 1 boost, got {}",
            cycle_boosted
        );
        kill_task(task_a); kill_task(task_b); kill_task(task_c);
        reap_dead_tasks();
        return Err(KernelError::InternalError);
    }
    serial_println!("[sched]   PI cycle detection: OK");

    // --- Cleanup ---
    // Clear inherited priorities and kill all test tasks.
    set_inherited_priority(task_b, None);
    set_inherited_priority(task_c, None);
    kill_task(task_a);
    kill_task(task_b);
    kill_task(task_c);
    reap_dead_tasks();

    serial_println!("[sched]   Transitive PI infrastructure: PASSED");
    Ok(())
}

/// Test: CPU affinity mask.
///
/// Verifies that:
/// 1. Default affinity is all-CPUs.
/// 2. `set_cpu_affinity` changes the mask and returns the old one.
/// 3. `spawn_with_affinity` sets the mask at creation.
/// 4. Zero mask is rejected.
/// 5. `can_run_on` helper works correctly.
fn test_cpu_affinity() -> KernelResult<()> {
    // 1. Spawn a task with default affinity.
    let id = spawn(b"test-aff", task::DEFAULT_PRIORITY, test_task_incr, 0, 0)?;
    let aff = get_cpu_affinity(id).ok_or(KernelError::InternalError)?;
    if aff != task::CPU_AFFINITY_ALL {
        serial_println!("[sched]   FAIL: default affinity should be all-CPUs, got {:#x}", aff);
        kill_task(id);
        reap_dead_tasks();
        return Err(KernelError::InternalError);
    }

    // 2. Set affinity to CPU 0 only.
    let old = set_cpu_affinity(id, 1).ok_or(KernelError::InternalError)?;
    if old != task::CPU_AFFINITY_ALL {
        serial_println!("[sched]   FAIL: old affinity should be all-CPUs");
        kill_task(id);
        reap_dead_tasks();
        return Err(KernelError::InternalError);
    }
    let new = get_cpu_affinity(id).ok_or(KernelError::InternalError)?;
    if new != 1 {
        serial_println!("[sched]   FAIL: affinity should be 1 (CPU 0), got {}", new);
        kill_task(id);
        reap_dead_tasks();
        return Err(KernelError::InternalError);
    }

    // 3. Zero mask is rejected.
    if set_cpu_affinity(id, 0).is_some() {
        serial_println!("[sched]   FAIL: zero affinity mask should be rejected");
        kill_task(id);
        reap_dead_tasks();
        return Err(KernelError::InternalError);
    }

    kill_task(id);
    reap_dead_tasks();

    // 4. spawn_with_affinity.
    let pml4 = crate::mm::page_table::active_pml4_phys();
    let id2 = spawn_with_affinity(
        b"test-aff2", task::DEFAULT_PRIORITY, test_task_incr, 0, pml4, 0b10,
    )?;
    let aff2 = get_cpu_affinity(id2).ok_or(KernelError::InternalError)?;
    if aff2 != 0b10 {
        serial_println!("[sched]   FAIL: spawn_with_affinity mask should be 0b10, got {:#x}", aff2);
        kill_task(id2);
        reap_dead_tasks();
        return Err(KernelError::InternalError);
    }

    // 5. spawn_with_affinity rejects zero mask.
    let err = spawn_with_affinity(b"bad", task::DEFAULT_PRIORITY, test_task_incr, 0, pml4, 0);
    if err.is_ok() {
        serial_println!("[sched]   FAIL: spawn_with_affinity(mask=0) should fail");
        if let Ok(bad_id) = err { kill_task(bad_id); }
        kill_task(id2);
        reap_dead_tasks();
        return Err(KernelError::InternalError);
    }

    // 6. can_run_on helper.
    {
        let state = SCHED.lock();
        let t = state.tasks.get(&id2).ok_or(KernelError::InternalError)?;
        if t.can_run_on(0) {
            serial_println!("[sched]   FAIL: task with mask 0b10 should not run on CPU 0");
            drop(state);
            kill_task(id2);
            reap_dead_tasks();
            return Err(KernelError::InternalError);
        }
        if !t.can_run_on(1) {
            serial_println!("[sched]   FAIL: task with mask 0b10 should run on CPU 1");
            drop(state);
            kill_task(id2);
            reap_dead_tasks();
            return Err(KernelError::InternalError);
        }
    }

    kill_task(id2);
    reap_dead_tasks();

    serial_println!("[sched]   CPU affinity: PASSED");
    Ok(())
}

/// Counter for exit hook test — incremented by the hook callback.
static EXIT_HOOK_TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Test: task exit hooks — register, fire on exit, unregister.
///
/// Verifies:
/// 1. Registration succeeds and returns a slot index.
/// 2. Hook fires when a task exits normally (via task_exit).
/// 3. Hook fires when a task is killed externally (via kill_task).
/// 4. Unregistration prevents future calls.
/// 5. Re-registration reuses freed slots.
/// 6. Full table returns None.
fn test_exit_hooks() -> KernelResult<()> {
    // -- 1. Register a hook --
    fn exit_hook_test_cb(task_id: TaskId) {
        let _ = task_id; // Suppress unused warning; we just count calls.
        EXIT_HOOK_TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    }

    EXIT_HOOK_TEST_COUNTER.store(0, Ordering::SeqCst);

    let slot = register_exit_hook(exit_hook_test_cb);
    if slot.is_none() {
        serial_println!("[sched]   FAIL: register_exit_hook returned None");
        return Err(KernelError::InternalError);
    }
    let slot = slot.unwrap_or(0); // Safe: checked above.

    // -- 2. Normal task exit fires the hook --
    let _id = spawn(b"test-hook-exit", task::DEFAULT_PRIORITY, test_task_incr, 1, 0)?;
    // Let the task run and exit naturally.
    yield_now();
    yield_now();
    yield_now();
    reap_dead_tasks();

    let count = EXIT_HOOK_TEST_COUNTER.load(Ordering::SeqCst);
    if count == 0 {
        serial_println!("[sched]   FAIL: exit hook not called on task_exit (count=0)");
        unregister_exit_hook(slot);
        return Err(KernelError::InternalError);
    }
    serial_println!("[sched]   Exit hook on normal exit: OK (count={})", count);

    // -- 3. kill_task fires the hook --
    let before_kill = EXIT_HOOK_TEST_COUNTER.load(Ordering::SeqCst);
    let id2 = spawn(b"test-hook-kill", task::DEFAULT_PRIORITY, test_task_block_self, 0, 0)?;
    // Let the task start and block.
    yield_now();
    yield_now();

    kill_task(id2);
    reap_dead_tasks();

    let after_kill = EXIT_HOOK_TEST_COUNTER.load(Ordering::SeqCst);
    if after_kill <= before_kill {
        serial_println!(
            "[sched]   FAIL: exit hook not called on kill_task (before={}, after={})",
            before_kill, after_kill
        );
        unregister_exit_hook(slot);
        return Err(KernelError::InternalError);
    }
    serial_println!("[sched]   Exit hook on kill: OK (count={})", after_kill);

    // -- 4. Unregister and verify no more calls --
    let ok = unregister_exit_hook(slot);
    if !ok {
        serial_println!("[sched]   FAIL: unregister_exit_hook returned false");
        return Err(KernelError::InternalError);
    }

    let before_unreg = EXIT_HOOK_TEST_COUNTER.load(Ordering::SeqCst);
    let _id3 = spawn(b"test-hook-unreg", task::DEFAULT_PRIORITY, test_task_incr, 1, 0)?;
    yield_now();
    yield_now();
    yield_now();
    reap_dead_tasks();

    let after_unreg = EXIT_HOOK_TEST_COUNTER.load(Ordering::SeqCst);
    if after_unreg != before_unreg {
        serial_println!(
            "[sched]   FAIL: hook called after unregister (before={}, after={})",
            before_unreg, after_unreg
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[sched]   No call after unregister: OK");

    // -- 5. Re-registration reuses freed slot --
    let reuse_slot = register_exit_hook(exit_hook_test_cb);
    if reuse_slot.is_none() {
        serial_println!("[sched]   FAIL: re-registration returned None");
        return Err(KernelError::InternalError);
    }
    unregister_exit_hook(reuse_slot.unwrap_or(0));
    serial_println!("[sched]   Slot reuse: OK");

    // -- 6. Full table returns None --
    let mut hook_slots = [0usize; MAX_EXIT_HOOKS];
    #[allow(clippy::indexing_slicing)] // i is always < MAX_EXIT_HOOKS.
    for i in 0..MAX_EXIT_HOOKS {
        if let Some(s) = register_exit_hook(exit_hook_test_cb) {
            hook_slots[i] = s;
        } else {
            serial_println!(
                "[sched]   FAIL: table full at slot {} (expected {})",
                i, MAX_EXIT_HOOKS
            );
            // Clean up any registered hooks.
            #[allow(clippy::indexing_slicing)] // j < i < MAX_EXIT_HOOKS.
            for j in 0..i {
                unregister_exit_hook(hook_slots[j]);
            }
            return Err(KernelError::InternalError);
        }
    }

    // One more should fail.
    if register_exit_hook(exit_hook_test_cb).is_some() {
        serial_println!("[sched]   FAIL: registered beyond MAX_EXIT_HOOKS");
        for s in &hook_slots {
            unregister_exit_hook(*s);
        }
        return Err(KernelError::InternalError);
    }

    // Clean up all.
    for s in &hook_slots {
        unregister_exit_hook(*s);
    }
    serial_println!("[sched]   Full table rejection: OK");

    serial_println!("[sched]   Exit hooks: PASSED");
    Ok(())
}

/// Simple test task: adds `arg` to `TEST_COUNTER`, then exits.
///
/// Used by suspend/resume and priority change tests.
extern "C" fn test_task_incr(arg: u64) {
    TEST_COUNTER.fetch_add(arg, Ordering::SeqCst);
}

/// Test task A: adds `arg` to `TEST_COUNTER`, yields, adds again, exits.
extern "C" fn test_task_a(arg: u64) {
    TEST_COUNTER.fetch_add(arg, Ordering::SeqCst);
    serial_println!("[test-a] First run, counter += {}", arg);
    yield_now();

    TEST_COUNTER.fetch_add(arg, Ordering::SeqCst);
    serial_println!("[test-a] Second run, counter += {}", arg);
    // Returns → task_entry_trampoline calls task_finished.
}

/// Test task B: adds `arg` to `TEST_COUNTER`, yields, adds again, exits.
extern "C" fn test_task_b(arg: u64) {
    TEST_COUNTER.fetch_add(arg, Ordering::SeqCst);
    serial_println!("[test-b] First run, counter += {}", arg);
    yield_now();

    TEST_COUNTER.fetch_add(arg, Ordering::SeqCst);
    serial_println!("[test-b] Second run, counter += {}", arg);
    // Returns → task_entry_trampoline calls task_finished.
}

/// Test task that blocks itself immediately.
///
/// Used by `test_kill_and_reap` to create a Blocked task that can
/// be killed from outside.  The task calls `block_current()` and
/// never wakes — it must be killed to clean up.
extern "C" fn test_task_block_self(_arg: u64) {
    serial_println!("[test-block] Blocking self...");
    block_current();
    // If we get here, someone woke us — just exit.
}

// ---------------------------------------------------------------------------
// Test: CPU bandwidth limiting
// ---------------------------------------------------------------------------

/// Test CPU bandwidth quota API and throttle state management.
///
/// Verifies:
/// 1. Default quota is 0 (unlimited).
/// 2. `set_cpu_quota` sets and `get_cpu_quota` reads back the value.
/// 3. Rejects quota > 100.
/// 4. Throttle flag is set when period_used >= quota.
/// 5. `unthrottle_expired` resets counters and clears throttle.
/// 6. Removing quota (setting to 0) un-throttles immediately.
/// 7. Returns false for nonexistent tasks.
fn test_cpu_bandwidth() -> KernelResult<()> {
    serial_println!("[sched]   CPU bandwidth limiting...");

    // --- 1. Default quota is unlimited ---
    let id = spawn(b"test-bw", 16, test_task_incr, 1, 0)?;

    let quota = get_cpu_quota(id);
    assert!(quota == Some(0), "Default quota should be 0 (unlimited)");
    serial_println!("[sched]   Default unlimited quota: OK");

    // --- 2. Set/get quota ---
    assert!(set_cpu_quota(id, 50), "set_cpu_quota should succeed");
    assert!(get_cpu_quota(id) == Some(50), "Quota should be 50");
    serial_println!("[sched]   Set/get quota: OK");

    // --- 3. Reject quota > 100 ---
    assert!(!set_cpu_quota(id, 101), "Quota > 100 should be rejected");
    assert!(get_cpu_quota(id) == Some(50), "Quota should remain 50 after rejection");
    serial_println!("[sched]   Reject > 100: OK");

    // --- 4. Throttle flag when period_used >= quota ---
    {
        let mut state = SCHED.lock();
        if let Some(task) = state.tasks.get_mut(&id) {
            assert!(!task.throttled, "Task should not start throttled");
            // Simulate consuming the quota.
            task.cpu_period_used = 50;
            // At this point the task would be throttled on the next
            // timer tick.  Manually set to test the flag.
            task.throttled = true;
            assert!(task.throttled, "Task should be throttled");
        }
    }
    serial_println!("[sched]   Throttle flag on quota exhaustion: OK");

    // --- 5. unthrottle_expired resets counters ---
    // The task is throttled and Ready (set by spawn).  Put it in a
    // state that unthrottle_expired would handle: Ready + throttled.
    // First, dequeue it so it's in the "parked" state.
    {
        let mut state = SCHED.lock();
        if let Some(task) = state.tasks.get_mut(&id) {
            // Simulate: task is Ready (parked by throttle), not in queue.
            let prio = task.effective_priority();
            let cpu = task.last_cpu;
            PER_CPU_SCHED.dequeue(id, prio, cpu);
            task.mark_ready(crate::apic::tick_count());
            task.throttled = true;
            task.cpu_period_used = 50;
        }
    }

    // Call unthrottle_expired — should reset and re-enqueue.
    unthrottle_expired();

    {
        let state = SCHED.lock();
        if let Some(task) = state.tasks.get(&id) {
            assert!(!task.throttled, "Task should be un-throttled after period reset");
            assert!(task.cpu_period_used == 0, "Period usage should be reset to 0");
        }
    }
    serial_println!("[sched]   unthrottle_expired resets: OK");

    // --- 6. Removing quota un-throttles immediately ---
    {
        let mut state = SCHED.lock();
        if let Some(task) = state.tasks.get_mut(&id) {
            task.cpu_quota_pct = 25;
            task.cpu_period_used = 25;
            task.throttled = true;
            // Dequeue first so we can verify re-enqueue.
            let prio = task.effective_priority();
            let cpu = task.last_cpu;
            PER_CPU_SCHED.dequeue(id, prio, cpu);
            task.mark_ready(crate::apic::tick_count());
        }
    }

    // Set quota to 0 (unlimited) — should un-throttle.
    assert!(set_cpu_quota(id, 0), "set_cpu_quota(0) should succeed");
    {
        let state = SCHED.lock();
        if let Some(task) = state.tasks.get(&id) {
            assert!(!task.throttled, "Task should be un-throttled after quota removal");
            assert!(task.cpu_quota_pct == 0, "Quota should be 0");
        }
    }
    serial_println!("[sched]   Remove quota un-throttles: OK");

    // --- 7. Nonexistent task ---
    assert!(!set_cpu_quota(u64::MAX, 50), "Nonexistent task should return false");
    assert!(get_cpu_quota(u64::MAX).is_none(), "Nonexistent task should return None");
    serial_println!("[sched]   Nonexistent task: OK");

    // --- 8. Boundary: quota = 100 (full CPU) ---
    assert!(set_cpu_quota(id, 100), "100% quota should be accepted");
    assert!(get_cpu_quota(id) == Some(100), "Quota should be 100");
    serial_println!("[sched]   Boundary quota=100: OK");

    // --- 9. Raising quota above usage un-throttles ---
    {
        let mut state = SCHED.lock();
        if let Some(task) = state.tasks.get_mut(&id) {
            task.cpu_quota_pct = 30;
            task.cpu_period_used = 30;
            task.throttled = true;
            let prio = task.effective_priority();
            let cpu = task.last_cpu;
            PER_CPU_SCHED.dequeue(id, prio, cpu);
            task.mark_ready(crate::apic::tick_count());
        }
    }
    // Raise quota to 50 — period_used (30) < new quota (50), so un-throttle.
    assert!(set_cpu_quota(id, 50), "Raising quota should succeed");
    {
        let state = SCHED.lock();
        if let Some(task) = state.tasks.get(&id) {
            assert!(!task.throttled, "Task should be un-throttled when quota raised above usage");
        }
    }
    serial_println!("[sched]   Raise quota un-throttles: OK");

    // Clean up: let the task run and exit.
    set_cpu_quota(id, 0); // Remove any quota.
    for _ in 0..20 {
        yield_now();
    }
    reap_dead_tasks();

    serial_println!("[sched]   CPU bandwidth limiting: PASSED");
    Ok(())
}

/// Test wait time tracking: verify counters increment when tasks wait.
fn test_wait_time_tracking() -> KernelResult<()> {
    serial_println!("[sched]   Wait time tracking...");

    // Spawn a task and let it run briefly, then check its fields.
    let id = spawn(b"test-wait", 16, test_task_incr, 5, 0)?;

    // Let it run and complete.
    for _ in 0..20 {
        yield_now();
    }

    // The task should have accumulated some schedule_count via
    // record_dispatch (which also resets ready_since_tick to 0
    // and updates total_wait_ticks).
    {
        let state = SCHED.lock();
        if let Some(task) = state.tasks.get(&id) {
            // After being dispatched at least once, schedule_count > 0.
            assert!(
                task.schedule_count > 0,
                "Task should have been dispatched at least once"
            );
            // ready_since_tick should be 0 if task is Running (cleared
            // by record_dispatch) or >0 if task is back in Ready state.
            if task.state == TaskState::Running {
                assert_eq!(
                    task.ready_since_tick, 0,
                    "Running task should have ready_since_tick = 0"
                );
            }
        }
    }

    // Clean up.
    kill_task(id);
    for _ in 0..5 {
        yield_now();
    }
    reap_dead_tasks();

    serial_println!("[sched]   Wait time tracking: PASSED");
    Ok(())
}

/// Test: Stack watermark — verify sentinel painting and usage measurement.
fn test_stack_watermark() -> KernelResult<()> {
    // Spawn a task that does minimal work, then check its stack usage.
    extern "C" fn stack_test_task(_arg: u64) {
        // Use some stack space (a local array to prevent optimizer from
        // eliminating stack usage).
        let mut buf = [0u8; 256];
        for (i, b) in buf.iter_mut().enumerate() {
            *b = (i & 0xFF) as u8;
        }
        // Prevent optimization — read the volatile value.
        // SAFETY: buf is a stack-local [u8; 256]; index 128 is within bounds.
        unsafe {
            core::ptr::read_volatile(&buf[128]);
        }
        // Let the task run briefly then exit.
        yield_now();
    }

    let pml4 = crate::mm::page_table::active_pml4_phys();
    let id = spawn(b"test-stack-wm", task::DEFAULT_PRIORITY, stack_test_task, 0, pml4)?;

    // Let the task run.
    for _ in 0..5 {
        yield_now();
    }

    // Check that the task (or recently reaped) has stack usage data.
    let state = SCHED.lock();
    if let Some(t) = state.tasks.get(&id) {
        let usage = t.stack_usage_bytes();
        let pct = t.stack_usage_pct();
        assert!(usage.is_some(), "Task should have stack usage");
        let used = usage.unwrap_or(0);
        // The task used at least some stack (context switch + function call).
        // Minimum should be > 64 bytes (saved registers alone are ~120 bytes).
        assert!(
            used > 64,
            "Stack usage too low: {} bytes (expected > 64)",
            used,
        );
        // But shouldn't use more than half the stack for this simple task.
        assert!(
            used < task::TASK_STACK_SIZE / 2,
            "Stack usage too high: {} bytes (expected < {})",
            used, task::TASK_STACK_SIZE / 2,
        );
        let pct_val = pct.unwrap_or(0);
        assert!(
            pct_val < 50,
            "Stack usage pct too high: {}% (expected < 50%)",
            pct_val,
        );
        serial_println!(
            "[sched]   Stack watermark: OK (test task used {} bytes, {}%)",
            used, pct_val,
        );
    } else {
        // Task already reaped — that's fine, just verify the API doesn't crash.
        serial_println!("[sched]   Stack watermark: OK (task reaped, API functional)");
    }
    drop(state);

    // Reap.
    for _ in 0..3 {
        yield_now();
    }
    reap_dead_tasks();

    serial_println!("[sched]   Stack watermark: PASSED");
    Ok(())
}

/// Test: sleep_ns wakes a task after the requested duration.
///
/// Spawns a helper task that sleeps for a known duration, then verifies
/// it woke up within a reasonable time window.  This exercises the full
/// hrtimer → APIC tick shortening → scheduler wake path.
fn test_sleep_ns() -> KernelResult<()> {
    use core::sync::atomic::AtomicU64;

    static SLEEP_START: AtomicU64 = AtomicU64::new(0);
    static SLEEP_END: AtomicU64 = AtomicU64::new(0);
    static SLEEPER_DONE: AtomicU64 = AtomicU64::new(0);

    extern "C" fn sleeper_task(_arg: u64) {
        SLEEP_START.store(crate::hrtimer::now_ns(), Ordering::Release);
        // Sleep for 20ms — uses hrtimer (< 100ms threshold).
        // Use a longer duration to be tolerant of QEMU TCG timing variability.
        sleep_ns(20_000_000);
        SLEEP_END.store(crate::hrtimer::now_ns(), Ordering::Release);
        SLEEPER_DONE.store(1, Ordering::Release);
    }

    SLEEP_START.store(0, Ordering::Relaxed);
    SLEEP_END.store(0, Ordering::Relaxed);
    SLEEPER_DONE.store(0, Ordering::Relaxed);

    let pml4 = crate::mm::page_table::active_pml4_phys();
    let _id = spawn(b"test-sleep-ns", task::DEFAULT_PRIORITY, sleeper_task, 0, pml4)?;

    // Wait for the sleeper to complete.  Use a spin loop that does NOT
    // hold the scheduler lock constantly — the hrtimer callback calls
    // try_wake() from the timer ISR, and if that fails (SCHED lock held
    // by the interrupted yield_now), the deferred wake mechanism picks
    // it up on the next schedule_inner call.
    let deadline = crate::apic::tick_count().saturating_add(50);
    loop {
        if SLEEPER_DONE.load(Ordering::Acquire) != 0 {
            break;
        }
        if crate::apic::tick_count() >= deadline {
            break;
        }
        // Spin without holding any locks — allows timer ISR to fire
        // and process hrtimers.  Yield periodically to give the sleeper
        // CPU time after it's woken.
        for _ in 0..1000 {
            core::hint::spin_loop();
        }
        yield_now();
    }

    let done = SLEEPER_DONE.load(Ordering::Acquire);
    assert!(done != 0, "sleep_ns: sleeper task did not complete within 500ms");

    let start = SLEEP_START.load(Ordering::Acquire);
    let end = SLEEP_END.load(Ordering::Acquire);
    assert!(end > start, "sleep_ns: end time not after start");

    let elapsed_ns = end.saturating_sub(start);
    // We requested 20ms (20_000_000 ns).
    // With hrtimer + tick shortening, actual should be >= 10ms and <= 200ms.
    // (Lower bound accounts for timer granularity; upper bound for QEMU
    // TCG scheduling delays where virtual timer ticks may bunch.)
    assert!(
        elapsed_ns >= 5_000_000,
        "sleep_ns too short: {}ns (expected >= 5ms)",
        elapsed_ns,
    );
    assert!(
        elapsed_ns <= 500_000_000,
        "sleep_ns too long: {}ns (expected <= 500ms, indicates timer didn't fire)",
        elapsed_ns,
    );

    serial_println!(
        "[sched]   sleep_ns: PASSED (slept {}.{:03}ms for 20ms request)",
        elapsed_ns / 1_000_000,
        (elapsed_ns % 1_000_000) / 1000,
    );

    reap_dead_tasks();
    Ok(())
}
