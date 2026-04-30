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
//! - **Default**: priority round-robin with 32 levels.  Per-CPU queues
//!   and work stealing will be added when SMP support is implemented.
//! - **Cooperative for now**: tasks yield explicitly via [`yield_now`].
//!   Preemptive scheduling will be added when the APIC timer is wired
//!   up (§2.2 Hardware Foundation in the roadmap).
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
//! section.
//!
//! Lock ordering: `SCHED` → frame allocator (via task stack allocation).

pub mod context;
pub mod priority_rr;
pub mod task;

use alloc::collections::BTreeMap;
use crate::cpu;
use crate::error::{KernelError, KernelResult};
use crate::serial_println;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use self::context::switch_context;
use self::priority_rr::PriorityRoundRobin;
use self::task::{Context, Task, TaskId, TaskState, NUM_PRIORITIES};

// ---------------------------------------------------------------------------
// Scheduler trait
// ---------------------------------------------------------------------------

/// Trait for scheduler implementations.
///
/// The scheduler decides which task runs next.  It does NOT own the
/// tasks — tasks are stored in the global [`TASKS`] table.  The
/// scheduler only holds `TaskId` values and priority information.
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
struct SchedState {
    /// The scheduler implementation.
    scheduler: PriorityRoundRobin,
    /// All tasks indexed by ID.
    tasks: BTreeMap<TaskId, Task>,
    /// Whether the scheduler has been initialized.
    initialized: bool,
}

static SCHED: Mutex<SchedState> = Mutex::new(SchedState {
    scheduler: PriorityRoundRobin::new_const(),
    tasks: BTreeMap::new(),
    initialized: false,
});

/// ID of the task currently running on this CPU.
///
/// For SMP, this would be per-CPU.  For now, a single global.
static CURRENT_TASK_ID: AtomicU64 = AtomicU64::new(0);

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

    // Replace the placeholder scheduler with a properly initialized one.
    state.scheduler = PriorityRoundRobin::new();

    // Create the idle task.  It represents the current execution
    // context (kmain), using the bootloader-provided stack.
    let idle = Task::new_idle();
    state.tasks.insert(0, idle);
    CURRENT_TASK_ID.store(0, Ordering::Release);

    state.initialized = true;
    serial_println!("[sched] Scheduler initialized (priority round-robin, {} levels)", NUM_PRIORITIES);
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

    let mut state = SCHED.lock();
    if !state.initialized {
        return Err(KernelError::NotSupported);
    }

    state.tasks.insert(id, new_task);
    state.scheduler.enqueue(id, prio);

    serial_println!("[sched] Spawned task {} (priority {})", id, prio);
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
    let current_id = CURRENT_TASK_ID.load(Ordering::Acquire);
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
    CURRENT_TASK_ID.load(Ordering::Acquire)
}

/// Block the current task and yield to the next runnable task.
///
/// The current task is set to [`Blocked`](TaskState::Blocked) and is
/// NOT placed in the run queue.  It must be explicitly woken via
/// [`wake`] to become runnable again.
///
/// This is used by IPC channels, futexes, and other blocking
/// primitives.
pub fn block_current() {
    let current_id = CURRENT_TASK_ID.load(Ordering::Acquire);
    {
        let mut state = SCHED.lock();
        if let Some(task) = state.tasks.get_mut(&current_id) {
            task.state = TaskState::Blocked;
        }
    }
    // Yield without re-enqueuing (requeue = false).
    schedule_inner(false);
}

/// Wake a blocked task, making it runnable again.
///
/// Sets the task's state to [`Ready`](TaskState::Ready) and places
/// it in the run queue at its original priority.
///
/// Returns `true` if the task was blocked and is now ready.
/// Returns `false` if the task was not in the Blocked state.
pub fn wake(task_id: TaskId) -> bool {
    let mut state = SCHED.lock();
    if let Some(task) = state.tasks.get_mut(&task_id)
        && task.state == TaskState::Blocked
    {
        task.state = TaskState::Ready;
        let prio = task.priority;
        state.scheduler.enqueue(task_id, prio);
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
            let prio = task.priority;
            state.scheduler.enqueue(task_id, prio);
            return true;
        }
    }
    false
}

/// Handle a timer tick from the APIC timer interrupt.
///
/// Called from the timer ISR with interrupts disabled.  Uses `try_lock`
/// to avoid deadlock — if the scheduler lock is already held (e.g.,
/// the timer fired while `schedule_inner` was running), we skip this
/// tick.  The next timer interrupt will catch it.
///
/// Returns `true` if the current task's time slice has expired and a
/// reschedule is needed.
pub fn timer_tick() -> bool {
    // Use try_lock to avoid deadlock with code that holds SCHED
    // when the timer fires.
    if let Some(mut state) = SCHED.try_lock() {
        if !state.initialized {
            return false;
        }
        state.scheduler.tick()
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
    let current = CURRENT_TASK_ID.load(Ordering::Acquire);
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
            let prio = task.priority;
            task.state = TaskState::Dead;
            state.scheduler.dequeue(task_id, prio);
        }
        TaskState::Blocked | TaskState::Suspended => {
            // Not in the run queue — just mark Dead.
            // If anything tries to wake() this task later, it'll
            // see it's not Blocked and return false.
            task.state = TaskState::Dead;
        }
        TaskState::Running => {
            // On single-CPU, Running means it's the current task.
            // We already checked for that above.  If we get here,
            // something is wrong, but handle it defensively.
            serial_println!(
                "[sched] BUG: kill_task: task {} is Running but not current (current={})",
                task_id, current
            );
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
    let current_id = CURRENT_TASK_ID.load(Ordering::Acquire);
    let mut reaped = 0;

    // Collect IDs of dead tasks first, then remove them one by one.
    // We do this in two passes because we need the lock to inspect
    // state but also need to call free_stack which does allocation-
    // related work.
    let dead_ids: alloc::vec::Vec<TaskId> = {
        let state = SCHED.lock();
        state.tasks.iter()
            .filter(|(id, task)| {
                task.state == TaskState::Dead && **id != current_id
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

            // SAFETY: The task is Dead, was removed from the table,
            // and is not the current task, so no CPU is using its stack.
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

/// Snapshot of a task's key fields for diagnostic display.
pub struct TaskInfo {
    /// Task ID.
    pub id: TaskId,
    /// Scheduling state.
    pub state: TaskState,
    /// Priority level (0 = highest).
    pub priority: u8,
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
            state: task.state,
            priority: task.priority,
        })
        .collect()
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
    let task_id = CURRENT_TASK_ID.load(Ordering::Acquire);

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
fn schedule_inner(requeue: bool) {
    // We need two contexts: the old one (current task) and the new
    // one (next task).  We extract raw pointers to avoid holding the
    // lock across the context switch.
    let current_id = CURRENT_TASK_ID.load(Ordering::Acquire);

    let mut next_id: TaskId = current_id;
    let mut should_switch = false;

    {
        let mut state = SCHED.lock();

        if !state.initialized {
            return;
        }

        // Re-enqueue the current task if requested.
        if requeue && let Some(task) = state.tasks.get_mut(&current_id) {
            task.state = TaskState::Ready;
            let prio = task.priority;
            state.scheduler.enqueue(current_id, prio);
        }

        // Pick the next task.
        if let Some(picked_id) = state.scheduler.pick_next() {
            if picked_id != current_id || !requeue {
                // Switching to a different task (or the current task
                // exited and we must switch regardless).
                next_id = picked_id;

                if let Some(next_task) = state.tasks.get_mut(&next_id) {
                    next_task.state = TaskState::Running;
                }

                should_switch = true;
            } else {
                // Same task picked — no switch needed.
                if let Some(task) = state.tasks.get_mut(&current_id) {
                    task.state = TaskState::Running;
                }
            }
        } else if !requeue {
            // No task ready and we can't run the current one (it's
            // exiting/blocking).  This shouldn't happen if the idle
            // task is always ready, but handle it defensively.
            serial_println!("[sched] No runnable tasks — halting");
            cpu::halt_loop();
        }
        // Lock is dropped here before the context switch.
    }

    if should_switch {
        CURRENT_TASK_ID.store(next_id, Ordering::Release);
        do_switch(current_id, next_id);
    }
}

/// Perform the actual context switch between two tasks.
///
/// Gets raw pointers into the task table's Context fields and calls
/// the assembly `switch_context`.
fn do_switch(old_id: TaskId, new_id: TaskId) {
    // We need simultaneous mutable access to two different tasks'
    // contexts.  Since BTreeMap doesn't allow two &mut borrows, we
    // extract raw pointers under the lock, then call switch_context
    // outside the lock.
    let (old_ctx_ptr, new_ctx_ptr): (*mut Context, *const Context);
    let old_pml4: u64;
    let new_pml4: u64;
    let new_stack_top: u64;

    {
        let mut state = SCHED.lock();

        // Get pointers to both contexts.
        //
        // SAFETY: We're getting raw pointers to fields within the
        // BTreeMap.  The BTreeMap won't be modified during the
        // switch (the lock is dropped, but no other code runs on
        // this CPU until switch_context returns — interrupts are
        // disabled).
        let old_data = state.tasks.get_mut(&old_id)
            .map(|t| (&raw mut t.context, t.pml4_phys));
        let new_data = state.tasks.get(&new_id)
            .map(|t| (&raw const t.context, t.pml4_phys, t.stack_bottom));

        if let (Some((old, o_pml4)), Some((new, n_pml4, n_stack_bottom))) =
            (old_data, new_data)
        {
            old_ctx_ptr = old;
            new_ctx_ptr = new;
            old_pml4 = o_pml4;
            new_pml4 = n_pml4;
            // Kernel stack top = bottom + stack size.
            // Zero means idle task (no kernel stack switch needed).
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
                old_id, new_id
            );
            return;
        }
        // Lock dropped here.
    }

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
        // SAFETY: Single-CPU, interrupts are disabled (we're in
        // the context switch path).  No concurrent access.
        unsafe {
            crate::syscall::entry::set_kernel_stack(new_stack_top);
            crate::gdt::set_kernel_stack(new_stack_top);
        }
    }

    // SAFETY:
    // - Both pointers are valid (we just got them from the task table).
    // - The task table won't move or reallocate during the switch
    //   because interrupts are disabled and no other CPU is running.
    // - old_ctx_ptr is &mut (exclusive) and new_ctx_ptr is & (shared),
    //   and they point to different tasks' contexts.
    unsafe {
        switch_context(&mut *old_ctx_ptr, &*new_ctx_ptr);
    }

    // When we return here, it means some other task has switched back
    // to us.  We're now running as old_id again.
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Counter for self-test verification.
static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Self-test: spawn tasks, yield between them, verify execution.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[sched] Running scheduler self-test...");

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

    serial_println!("[sched] Scheduler self-test PASSED");
    Ok(())
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
