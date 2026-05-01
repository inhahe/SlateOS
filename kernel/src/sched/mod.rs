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
//! The scheduler state is protected by a single spinlock (`SCHED`).
//! During context switch, the lock is dropped *before* calling
//! `switch_context` so the new task doesn't resume inside a critical
//! section.  When SMP is fully implemented, the global lock will be
//! split into per-CPU locks for the fast path.
//!
//! Lock ordering: `SCHED` → frame allocator (via task stack allocation).

pub mod context;
pub mod io_sched;
pub mod priority_rr;
pub mod task;

use alloc::collections::BTreeMap;
use crate::cpu;
use crate::error::{KernelError, KernelResult};
use crate::serial_println;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Global scheduler state: the scheduler implementation + task table.
///
/// Protected by a spinlock.  Lock ordering: this lock before any
/// memory allocator locks.
///
/// The per-CPU scheduler holds independent run queues for each CPU.
/// With a single CPU (current state), all tasks go to CPU 0's queue.
/// When SMP is implemented, each CPU will have its own queue with
/// work stealing for load balance.
pub(crate) struct SchedState {
    /// Per-CPU scheduler (run queues + work stealing).
    pub(crate) scheduler: PerCpuScheduler,
    /// All tasks indexed by ID.
    tasks: BTreeMap<TaskId, Task>,
    /// Whether the scheduler has been initialized.
    initialized: bool,
}

static SCHED: Mutex<SchedState> = Mutex::new(SchedState {
    scheduler: PerCpuScheduler::new_const(),
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
/// OPT: Each AtomicU64 should ideally be on its own cache line to
/// avoid false sharing.  For now, the 16-element array (128 bytes)
/// fits in 2 cache lines, which is acceptable for ≤16 CPUs.
static CURRENT_TASK_IDS: [AtomicU64; priority_rr::MAX_CPUS] = {
    const INIT: AtomicU64 = AtomicU64::new(0);
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
static IDLE_FLAGS: [AtomicBool; priority_rr::MAX_CPUS] = {
    const INIT: AtomicBool = AtomicBool::new(false);
    [INIT; priority_rr::MAX_CPUS]
};

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

/// Acquire the scheduler lock (for SMP bootstrap to update CPU count).
///
/// The returned guard provides mutable access to `SchedState`.
/// This is intentionally `pub(crate)` — only SMP bootstrap uses it.
pub(crate) fn sched_lock() -> spin::MutexGuard<'static, SchedState> {
    SCHED.lock()
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

    let mut state = SCHED.lock();

    // Initialize per-CPU scheduler with 1 CPU (boot CPU).
    // When SMP is implemented, this will be updated with the actual
    // number of online CPUs after AP bootstrap.
    let num_cpus = 1;
    state.scheduler.init(num_cpus);

    // Create the idle task.  It represents the current execution
    // context (kmain), using the bootloader-provided stack.
    let idle = Task::new_idle();
    state.tasks.insert(0, idle);
    set_current_task(0, 0); // BSP (CPU 0) starts with idle task 0.

    state.initialized = true;
    serial_println!(
        "[sched] Scheduler initialized (priority round-robin, {} levels, {} CPU{})",
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
    state.tasks.insert(id, idle);
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
        .map_or(false, |f| f.load(Ordering::Acquire))
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
    let new_task = Task::new_kernel(name, priority, entry, arg, pml4_phys)?;
    let id = new_task.id;
    let prio = new_task.priority;
    let target_cpu = new_task.last_cpu;

    let mut state = SCHED.lock();
    if !state.initialized {
        return Err(KernelError::NotSupported);
    }

    state.tasks.insert(id, new_task);
    state.scheduler.enqueue(id, prio, target_cpu);

    serial_println!("[sched] Spawned task {} (priority {}, cpu {})", id, prio, target_cpu);
    Ok(id)
}

/// Yield the current task's CPU time.
///
/// The current task is placed back in the run queue and the highest-
/// priority ready task is scheduled.  If no other task is ready, the
/// current task continues running.
pub fn yield_now() {
    schedule_inner(true);
}

/// Mark the current task as dead and yield to the next task.
///
/// Called by `task_finished` (the context trampoline) when a task's
/// entry function returns.  The task is NOT placed back in the run
/// queue.
pub fn task_exit() {
    let current_id = load_current_task();
    serial_println!("[sched] Task {} exiting", current_id);

    {
        let mut state = SCHED.lock();
        if let Some(task) = state.tasks.get_mut(&current_id) {
            task.state = TaskState::Dead;
        }
    }

    // Yield without re-enqueuing.
    schedule_inner(false);

    // Should never reach here — the task is dead and won't be
    // scheduled again.  If somehow we do, halt.
    cpu::halt_loop();
}

/// Get the ID of the currently running task.
#[must_use]
pub fn current_task_id() -> TaskId {
    load_current_task()
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
    let current_id = load_current_task();
    {
        let mut state = SCHED.lock();
        if let Some(task) = state.tasks.get_mut(&current_id) {
            // Record burst length for interactive task detection.
            task.record_block();
            task.state = TaskState::Blocked;
        }
    }
    // Yield without re-enqueuing (requeue = false).
    schedule_inner(false);
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
    let mut state = SCHED.lock();
    if let Some(task) = state.tasks.get_mut(&task_id)
        && task.state == TaskState::Blocked
    {
        task.state = TaskState::Ready;
        // Reset burst counter for the new wake cycle.
        // Enqueue on the CPU the task last ran on (cache warmth).
        task.burst_ticks = 0;
        let prio = task.effective_priority();
        let target_cpu = task.last_cpu;
        state.scheduler.enqueue(task_id, prio, target_cpu);
        return true;
    }
    false
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
        if let Some(task) = state.tasks.get_mut(&task_id)
            && task.state == TaskState::Blocked
        {
            task.state = TaskState::Ready;
            task.burst_ticks = 0;
            let prio = task.effective_priority();
            let target_cpu = task.last_cpu;
            state.scheduler.enqueue(task_id, prio, target_cpu);
            return true;
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
static BALANCE_TICKS: [AtomicU64; priority_rr::MAX_CPUS] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; priority_rr::MAX_CPUS]
};

/// Handle a timer tick from the APIC timer interrupt.
///
/// Called from the timer ISR with interrupts disabled.  Uses `try_lock`
/// to avoid deadlock — if the scheduler lock is already held (e.g.,
/// the timer fired while `schedule_inner` was running), we skip this
/// tick.  The next timer interrupt will catch it.
///
/// Also increments the current task's burst tick counter for
/// interactive task detection.
///
/// Periodically checks load balance: if this CPU's local queue is
/// empty but other CPUs have work, returns `true` to trigger a
/// preempt (which does work stealing via `schedule_inner`).
///
/// Returns `true` if the current task's time slice has expired
/// (or a load balance steal is warranted) and a reschedule is needed.
pub fn timer_tick() -> bool {
    let cpu = current_cpu_id();

    // Use try_lock to avoid deadlock with code that holds SCHED
    // when the timer fires.
    if let Some(mut state) = SCHED.try_lock() {
        if !state.initialized {
            return false;
        }

        // Track CPU burst length for interactive task detection.
        let current_id = load_current_task();
        if let Some(task) = state.tasks.get_mut(&current_id) {
            task.tick_burst();
        }

        let time_slice_expired = state.scheduler.tick(cpu);
        if time_slice_expired {
            return true;
        }

        // Periodic load balance: check if this CPU is idle while
        // others have work.  Only check every BALANCE_INTERVAL ticks
        // to avoid overhead on every 10ms tick.
        //
        // OPT: This proactive check means idle CPUs pull work within
        // 100ms instead of waiting for the next yield/block event.
        // Without this, a CPU that enters the idle loop stays idle
        // until another CPU yields a task (which may never happen if
        // the busy CPU's tasks don't yield).
        // SAFETY: cpu < MAX_CPUS (guaranteed by smp::current_cpu_index).
        let Some(balance_counter) = BALANCE_TICKS.get(cpu) else { return false; };
        let tick_count = balance_counter.fetch_add(1, Ordering::Relaxed);
        if tick_count % BALANCE_INTERVAL == 0 {
            // Check: does our local queue have real work (above idle)?
            // Using has_real_work instead of has_ready so the idle task
            // doesn't mask an empty-queue condition — otherwise APs with
            // only their idle task never trigger work stealing.
            if !state.scheduler.local_has_real_work(cpu) {
                // Is anyone else overloaded with real tasks?
                if state.scheduler.others_have_real_work(cpu) {
                    // Trigger a reschedule — schedule_inner will try_steal.
                    return true;
                }
            }
        }

        false
    } else {
        // Couldn't acquire lock — skip this tick.
        false
    }
}

/// Preempt the current task (called from timer ISR after time slice
/// expiry).
///
/// This is equivalent to `yield_now()` but called from interrupt
/// context.  The current task is re-enqueued and the highest-priority
/// ready task is scheduled.
pub fn preempt() {
    schedule_inner(true);
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
                state.scheduler.dequeue(task_id, prio, cpu);
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
    if task_id == current {
        schedule_inner(false);
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
    let mut state = SCHED.lock();
    let Some(task) = state.tasks.get_mut(&task_id) else {
        return false;
    };

    if task.state != TaskState::Suspended {
        return false;
    }

    task.state = TaskState::Ready;
    let prio = task.effective_priority();
    let target_cpu = task.last_cpu;
    state.scheduler.enqueue(task_id, prio, target_cpu);

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
        state.scheduler.dequeue(task_id, old_effective, task_cpu);
        state.scheduler.enqueue(task_id, new_effective, task_cpu);
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
            state.scheduler.dequeue(task_id, prio, cpu);
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

    serial_println!("[sched] Killed task {}", task_id);
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
            // Drop the lock before freeing the stack (free_order
            // acquires the frame allocator lock — safe since our lock
            // ordering is SCHED → frame allocator, and we just dropped
            // SCHED).
            drop(state);

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
    let mut state = SCHED.lock();
    state.scheduler.set_time_slice(level, ticks)
}

/// Get the time slice (in timer ticks) for a specific priority level.
///
/// Returns `None` if the level is out of range.
#[must_use]
pub fn get_time_slice(level: usize) -> Option<u32> {
    let state = SCHED.lock();
    state.scheduler.time_slice(level)
}

/// Reconfigure all time slices with a new base and increment.
///
/// Applies to all CPUs.  Formula: `time_slice[level] = base + level * increment`.
/// `base` must be >= 1 (zero would starve priority-0 tasks).
///
/// Returns `true` on success, `false` if `base` is 0.
pub fn reconfigure_time_slices(base: u32, increment: u32) -> bool {
    let mut state = SCHED.lock();
    state.scheduler.reconfigure_slices(base, increment)
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
    let mut state = SCHED.lock();
    state.scheduler.apply_profile(profile);
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
    let state = SCHED.lock();
    // Check each profile by comparing the time slice at level 0 and 1.
    // This identifies the (base, increment) pair.
    for profile_id in 0..=3u8 {
        if let Some(profile) = WorkloadProfile::from_u8(profile_id) {
            let base = profile.base();
            let inc = profile.increment();
            // Verify level 0 and level 1 match this profile's formula.
            let l0 = state.scheduler.time_slice(0);
            let l1 = state.scheduler.time_slice(1);
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
    state.tasks.get(&task_id).map(Task::effective_priority)
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
        state.scheduler.dequeue(task_id, old_effective, task_cpu);
        state.scheduler.enqueue(task_id, new_effective, task_cpu);
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
        state.scheduler.dequeue(task_id, old_effective, task_cpu);
        state.scheduler.enqueue(task_id, new_effective, task_cpu);
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
    /// Number of times this task was scheduled.
    pub schedule_count: u64,
    /// CPU this task last ran on.
    pub last_cpu: usize,
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
            schedule_count: task.schedule_count,
            last_cpu: task.last_cpu,
        })
        .collect()
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

/// Scan the sleep queue and wake tasks whose sleep deadline has passed.
///
/// Called from the APIC timer ISR on every tick.  Must be lock-free
/// in the fast path (only atomic loads/stores, no mutexes).
///
/// Uses [`try_wake`] to safely wake tasks even from interrupt context.
/// If `try_wake` fails (scheduler lock contended), the entry stays in
/// the queue and will be retried on the next tick.
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

        // Deadline passed.  Try to wake the task.
        let task_id = entry.task_id.load(Ordering::Acquire);
        if try_wake(task_id) {
            // Woken successfully — clear the slot.
            entry.wake_tick.store(0, Ordering::Release);
        }
        // If try_wake fails (lock contended), we leave the entry
        // and will retry on the next tick.
    }
}

// ---------------------------------------------------------------------------
// Core scheduling logic
// ---------------------------------------------------------------------------

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
fn schedule_inner(requeue: bool) {
    let current_id = load_current_task();
    let cpu = current_cpu_id();

    // Data extracted under the single lock acquisition for the switch.
    let old_ctx_ptr: *mut Context;
    let new_ctx_ptr: *const Context;
    let old_pml4: u64;
    let new_pml4: u64;
    let new_stack_top: u64;
    let next_id: TaskId;

    {
        let mut state = SCHED.lock();

        if !state.initialized {
            return;
        }

        // Re-enqueue the current task if requested (on its current CPU).
        //
        // Guard: only re-enqueue if the task is still Running.  Another
        // CPU may have called kill_task() or suspend() while we were
        // executing, changing the state to Dead or Suspended.  If we
        // blindly overwrite to Ready, the task would be re-enqueued
        // despite being killed/suspended — a correctness bug on SMP.
        if requeue {
            if let Some(task) = state.tasks.get_mut(&current_id) {
                if task.state == TaskState::Running {
                    task.state = TaskState::Ready;
                    let prio = task.effective_priority();
                    state.scheduler.enqueue(current_id, prio, cpu);
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
        let mut migrated = alloc::vec::Vec::new();
        let picked = match state.scheduler.pick_next_local(cpu) {
            Some(id) => Some(id),
            None => state.scheduler.try_steal(cpu, &mut migrated),
        };
        for &id in &migrated {
            if let Some(task) = state.tasks.get_mut(&id) {
                task.last_cpu = cpu;
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

                    let mut idle_migrated = alloc::vec::Vec::new();
                    let ready_id = match s.scheduler.pick_next_local(cpu) {
                        Some(id) => id,
                        None => match s.scheduler.try_steal(cpu, &mut idle_migrated) {
                            Some(id) => id,
                            None => { drop(s); continue; }
                        },
                    };
                    // Update last_cpu for all stolen tasks so future
                    // wake()/kill_task() target the correct CPU queue.
                    for &id in &idle_migrated {
                        if let Some(task) = s.tasks.get_mut(&id) {
                            task.last_cpu = cpu;
                        }
                    }

                    // Found a ready task — set it up for switching.
                    if let Some(task) = s.tasks.get_mut(&ready_id) {
                        task.state = TaskState::Running;
                        task.last_cpu = cpu;
                        task.schedule_count = task.schedule_count.saturating_add(1);
                    }

                    // Extract context pointers for old (blocked/dead)
                    // and new tasks.  The old task's entry still exists
                    // in the BTreeMap — it's Blocked or Dead, not removed.
                    let old_data = s.tasks.get_mut(&current_id)
                        .map(|t| {
                            t.check_stack_canary();
                            (&raw mut t.context, t.pml4_phys)
                        });
                    let new_data = s.tasks.get(&ready_id)
                        .map(|t| (&raw const t.context, t.pml4_phys, t.stack_bottom));

                    if let (Some((old_p, o_pml4)), Some((new_p, n_pml4, n_sb))) =
                        (old_data, new_data)
                    {
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

                        // SAFETY: Both pointers valid (from task table
                        // under lock).  old is &mut (exclusive), new is
                        // & (shared), pointing to different tasks.
                        unsafe { switch_context(&mut *old_p, &*new_p); }

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
            next_task.state = TaskState::Running;
            next_task.last_cpu = cpu;
            next_task.schedule_count = next_task.schedule_count.saturating_add(1);
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
                (&raw mut t.context, t.pml4_phys)
            });
        let new_data = state.tasks.get(&next_id)
            .map(|t| (&raw const t.context, t.pml4_phys, t.stack_bottom));

        if let (Some((old, o_pml4)), Some((new, n_pml4, n_stack_bottom))) =
            (old_data, new_data)
        {
            old_ctx_ptr = old;
            new_ctx_ptr = new;
            old_pml4 = o_pml4;
            new_pml4 = n_pml4;
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
    // - Both pointers are valid (extracted from the task table under lock).
    // - The BTreeMap nodes won't be freed during the switch because no
    //   other code runs on this CPU until switch_context returns.
    // - old_ctx_ptr is &mut (exclusive write) and new_ctx_ptr is &
    //   (shared read), pointing to different tasks' contexts.
    unsafe {
        switch_context(&mut *old_ctx_ptr, &*new_ctx_ptr);
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
            .map_or(false, |f| f.load(Ordering::Acquire))
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

    serial_println!("[sched] Scheduler self-test PASSED");
    Ok(())
}

/// Test 0: Stack canary — verify canary is planted and survives execution.
fn test_stack_canary() -> KernelResult<()> {
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
                if canary != task::STACK_CANARY {
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
        Some(p) if p == WorkloadProfile::Desktop => {}
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
        Some(p) if p == WorkloadProfile::Server => {}
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
        Some(p) if p == WorkloadProfile::Development => {}
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
        Some(p) if p == WorkloadProfile::Gaming => {}
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
    let mut sched = Box::new(PerCpuScheduler::new_const());
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
    let mut migrated = alloc::vec::Vec::new();
    let stolen = sched.try_steal(0, &mut migrated);
    if stolen.is_none() {
        serial_println!("[sched]   FAIL: work stealing returned None");
        return Err(KernelError::InternalError);
    }

    // The migrated vec should contain all stolen task IDs.
    // The first one is also in `stolen`; the rest were enqueued on CPU 0.
    if migrated.is_empty() {
        serial_println!("[sched]   FAIL: migrated vec empty after steal");
        return Err(KernelError::InternalError);
    }
    if migrated.first().copied() != stolen {
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
