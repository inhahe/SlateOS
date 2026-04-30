//! Thread management — the bridge between processes and the scheduler.
//!
//! A *thread* is a schedulable unit within a process.  Each thread is
//! backed by a scheduler *task* (`TaskId`) and belongs to exactly one
//! process (`ProcessId`).
//!
//! ## Relationship Between Threads, Tasks, and Processes
//!
//! ```text
//! Process (pid=5)
//!   ├─ Thread (task_id=10)  → scheduler task 10
//!   ├─ Thread (task_id=11)  → scheduler task 11
//!   └─ Thread (task_id=12)  → scheduler task 12
//! ```
//!
//! - The scheduler only knows about tasks (it has no concept of processes).
//! - A process is a container: address space + capability table + threads.
//! - This module creates the link: spawning a thread allocates a scheduler
//!   task AND registers it with the owning process.
//!
//! ## Thread Lifecycle
//!
//! 1. `spawn()` — create a scheduler task, register with process, set
//!    process to Running if it was Creating.
//! 2. Thread runs its entry function.
//! 3. Entry function returns → `task_exit()` fires in the scheduler.
//! 4. `on_thread_exit()` — unregisters from process, triggers zombie
//!    transition if last thread.
//!
//! ## Current Limitations
//!
//! - All threads run in kernel mode (ring 0).  Userspace threads require
//!   per-process address space switching and ring 3 transition (future).
//! - Thread-local storage (TLS) is not yet supported.
//! - Thread join/detach semantics are not yet implemented.

use crate::error::{KernelError, KernelResult};
use crate::proc::pcb::{self, ProcessId, ProcessState};
use crate::sched::{self, task::TaskId};
use crate::serial_println;
use alloc::collections::BTreeMap;
use spin::Mutex;

// ---------------------------------------------------------------------------
// Thread → Process mapping
// ---------------------------------------------------------------------------

/// Maps task IDs to their owning process ID.
///
/// This is the reverse mapping of `Process::threads`.  It allows
/// `on_thread_exit()` (called from the scheduler's task-finished
/// path) to find the owning process without holding `PROCESS_TABLE`
/// during scheduling.
///
/// Lock ordering: `THREAD_OWNERS` → `PROCESS_TABLE` → `SCHED`.
static THREAD_OWNERS: Mutex<BTreeMap<TaskId, ProcessId>> =
    Mutex::new(BTreeMap::new());

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Spawn a new thread within a process.
///
/// Creates a scheduler task that runs `entry(arg)` and registers it
/// with the given process.  If the process was in `Creating` state,
/// it transitions to `Running`.
///
/// Returns the new thread's task ID.
///
/// # Arguments
///
/// - `pid` — owning process ID (must exist in the process table).
/// - `name` — human-readable name for debug output.
/// - `priority` — scheduling priority (0 = highest, 31 = lowest).
/// - `entry` — function the thread will execute.
/// - `arg` — argument passed to `entry`.
///
/// # Errors
///
/// - [`KernelError::NoSuchProcess`] if `pid` doesn't exist.
/// - [`KernelError::OutOfMemory`] if stack allocation fails.
pub fn spawn(
    pid: ProcessId,
    name: &[u8],
    priority: u8,
    entry: extern "C" fn(u64),
    arg: u64,
) -> KernelResult<TaskId> {
    // Verify the process exists before allocating resources.
    let proc_state = pcb::state(pid)
        .ok_or(KernelError::NoSuchProcess)?;

    // Don't spawn threads into zombie processes.
    if proc_state == ProcessState::Zombie {
        return Err(KernelError::NoSuchProcess);
    }

    // Look up the process's PML4 so the scheduler can switch CR3
    // on context switch.  pml4_phys == 0 means "kernel address space."
    let pml4 = pcb::get_pml4(pid).unwrap_or(0);

    // Create the scheduler task.
    let task_id = sched::spawn(name, priority, entry, arg, pml4)?;

    // Register the thread with the process.
    if let Err(e) = pcb::add_thread(pid, task_id) {
        // Process disappeared between our check and the add — very
        // unlikely with single-CPU, but handle defensively.
        // The scheduler task is already spawned; mark it for cleanup.
        serial_println!(
            "[thread] Failed to register task {} with process {}: {:?}",
            task_id, pid, e
        );
        return Err(e);
    }

    // Record the reverse mapping.
    {
        let mut owners = THREAD_OWNERS.lock();
        owners.insert(task_id, pid);
    }

    // Transition process from Creating to Running on first thread.
    if proc_state == ProcessState::Creating {
        // Ignore error — race with another thread doing the same.
        let _ = pcb::set_running(pid);
    }

    serial_println!(
        "[thread] Spawned thread (task {}) in process {}",
        task_id, pid
    );

    Ok(task_id)
}

/// Notify that a thread has exited.
///
/// Called from the scheduler's task-exit path (or explicitly for thread
/// cleanup).  Removes the thread from its owning process.  If this was
/// the last thread, the process becomes a zombie.
///
/// Returns `Some(pid)` if the owning process was found, `None` if the
/// thread was not registered (e.g., a bare kernel task not owned by any
/// process).
pub fn on_thread_exit(task_id: TaskId) -> Option<ProcessId> {
    // Look up and remove the reverse mapping.
    let pid = {
        let mut owners = THREAD_OWNERS.lock();
        owners.remove(&task_id)?
    };

    // Remove from the process's thread list.
    match pcb::remove_thread(pid, task_id) {
        Ok((is_zombie, wake_task)) => {
            if is_zombie {
                serial_println!(
                    "[thread] Process {} has no threads left — now zombie",
                    pid
                );

                // Wake any task waiting to reap this process.
                if let Some(waiter) = wake_task {
                    crate::sched::wake(waiter);
                }
            }
        }
        Err(e) => {
            serial_println!(
                "[thread] Failed to remove task {} from process {}: {:?}",
                task_id, pid, e
            );
        }
    }

    Some(pid)
}

/// Get the process ID that owns a given thread.
///
/// Returns `None` if the task is not registered as a thread (bare
/// kernel task or already exited).
#[allow(dead_code)]
pub fn owner_process(task_id: TaskId) -> Option<ProcessId> {
    let owners = THREAD_OWNERS.lock();
    owners.get(&task_id).copied()
}

/// Get the number of registered thread→process mappings.
///
/// Useful for debugging and self-tests.
#[allow(dead_code)]
pub fn thread_count() -> usize {
    let owners = THREAD_OWNERS.lock();
    owners.len()
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Kernel task entry function for thread tests.
extern "C" fn test_thread_entry(arg: u64) {
    // Simple task: just increment the shared counter and exit.
    // The arg encodes a pointer to an AtomicU64 counter.
    // SAFETY: arg was set from a valid &AtomicU64 in the test.
    let counter = unsafe {
        &*(arg as *const core::sync::atomic::AtomicU64)
    };
    counter.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
}

/// Run thread management self-tests.
pub fn self_test() -> KernelResult<()> {
    test_spawn_thread()?;
    test_thread_exit_zombies_process()?;
    test_spawn_into_zombie_fails()?;

    Ok(())
}

/// Test 1: Spawn a thread within a process and verify registration.
fn test_spawn_thread() -> KernelResult<()> {
    use core::sync::atomic::AtomicU64;

    // Create a process.
    let pid = pcb::create("thread-test-1", 0);

    // Track the counter.
    let counter = AtomicU64::new(0);
    let counter_ptr = &counter as *const AtomicU64 as u64;

    // Spawn a thread in the process.
    let task_id = spawn(
        pid,
        b"test-thread-1",
        sched::task::DEFAULT_PRIORITY,
        test_thread_entry,
        counter_ptr,
    )?;

    // Verify registration.
    let owner = owner_process(task_id);
    if owner != Some(pid) {
        serial_println!("[thread]   FAIL: thread owner should be {}, got {:?}", pid, owner);
        pcb::destroy(pid);
        return Err(KernelError::InternalError);
    }

    // Process should now be Running (was Creating → first thread).
    let s = pcb::state(pid);
    if s != Some(ProcessState::Running) {
        serial_println!("[thread]   FAIL: process should be Running, got {:?}", s);
        pcb::destroy(pid);
        return Err(KernelError::InternalError);
    }

    // Let the thread run.
    sched::yield_now();
    sched::yield_now();

    // Counter should have been incremented.
    if counter.load(core::sync::atomic::Ordering::Relaxed) != 1 {
        serial_println!("[thread]   FAIL: counter should be 1");
        pcb::destroy(pid);
        return Err(KernelError::InternalError);
    }

    // Thread exited — notify the thread system.
    on_thread_exit(task_id);

    // Clean up.
    pcb::destroy(pid);
    serial_println!("[thread]   Spawn thread: OK");
    Ok(())
}

/// Test 2: Thread exit causes process to become zombie.
fn test_thread_exit_zombies_process() -> KernelResult<()> {
    use core::sync::atomic::AtomicU64;

    let pid = pcb::create("thread-test-2", 0);

    let counter = AtomicU64::new(0);
    let counter_ptr = &counter as *const AtomicU64 as u64;

    // Spawn two threads.
    let t1 = spawn(pid, b"t2-a", sched::task::DEFAULT_PRIORITY, test_thread_entry, counter_ptr)?;
    let t2 = spawn(pid, b"t2-b", sched::task::DEFAULT_PRIORITY, test_thread_entry, counter_ptr)?;

    // Let both run.
    sched::yield_now();
    sched::yield_now();
    sched::yield_now();

    // Both counters fired.
    if counter.load(core::sync::atomic::Ordering::Relaxed) != 2 {
        serial_println!("[thread]   FAIL: counter should be 2");
        pcb::destroy(pid);
        return Err(KernelError::InternalError);
    }

    // First thread exits — process should still be Running.
    on_thread_exit(t1);
    let s = pcb::state(pid);
    if s != Some(ProcessState::Running) {
        serial_println!("[thread]   FAIL: should still be Running after first exit, got {:?}", s);
        pcb::destroy(pid);
        return Err(KernelError::InternalError);
    }

    // Second thread exits — process should now be Zombie.
    on_thread_exit(t2);
    let s = pcb::state(pid);
    if s != Some(ProcessState::Zombie) {
        serial_println!("[thread]   FAIL: should be Zombie after last exit, got {:?}", s);
        pcb::destroy(pid);
        return Err(KernelError::InternalError);
    }

    pcb::destroy(pid);
    serial_println!("[thread]   Thread exit → zombie: OK");
    Ok(())
}

/// Test 3: Cannot spawn thread into a zombie process.
fn test_spawn_into_zombie_fails() -> KernelResult<()> {
    use core::sync::atomic::AtomicU64;

    let pid = pcb::create("thread-test-3", 0);
    let counter = AtomicU64::new(0);
    let counter_ptr = &counter as *const AtomicU64 as u64;

    // Spawn and run a thread.
    let t1 = spawn(pid, b"t3", sched::task::DEFAULT_PRIORITY, test_thread_entry, counter_ptr)?;
    sched::yield_now();
    sched::yield_now();

    // Exit the thread → process becomes zombie.
    on_thread_exit(t1);

    // Try to spawn into the zombie.
    match spawn(pid, b"t3-late", sched::task::DEFAULT_PRIORITY, test_thread_entry, counter_ptr) {
        Err(KernelError::NoSuchProcess) => {} // Expected.
        other => {
            serial_println!("[thread]   FAIL: spawn into zombie should fail, got {:?}", other);
            pcb::destroy(pid);
            return Err(KernelError::InternalError);
        }
    }

    pcb::destroy(pid);
    serial_println!("[thread]   Reject spawn into zombie: OK");
    Ok(())
}
