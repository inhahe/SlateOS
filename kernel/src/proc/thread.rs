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
use crate::sync::Mutex;
use alloc::collections::BTreeMap;

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
///
/// Tracked via [`crate::sync::Mutex`] (not raw `spin::Mutex`) so lockdep
/// validates that ordering kernel-wide and the spinlock stall detector can
/// name it if the exit/reap path ever wedges on it — this lock sits directly
/// on the suspected spawn/kill/reap hang path.
static THREAD_OWNERS: Mutex<BTreeMap<TaskId, ProcessId>> =
    Mutex::named(BTreeMap::new(), b"THRDOWN");

// ---------------------------------------------------------------------------
// Thread exit values and join waiters
// ---------------------------------------------------------------------------

/// Stores the exit value of threads that have exited.
///
/// When a thread calls `thread_exit_with_value()`, its exit value is
/// stored here.  The joining thread reads it from this map.  Entries
/// are removed when the join completes (or never, if no one joins).
///
/// This is independent of process exit codes — each thread has its
/// own exit value that another thread in the same process can retrieve.
static THREAD_EXIT_VALUES: Mutex<BTreeMap<TaskId, i64>> =
    Mutex::named(BTreeMap::new(), b"THREXITV");

/// Maps a thread being waited on → the task waiting on it.
///
/// When a thread calls `join(target_task)`, the current task is
/// registered here.  When `target_task` exits, the waiter is woken.
/// Only one thread may join on a given target at a time.
static THREAD_JOIN_WAITERS: Mutex<BTreeMap<TaskId, TaskId>> =
    Mutex::named(BTreeMap::new(), b"THRJOIN");

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
    // on context switch.  We verified the process exists above, so a
    // missing PML4 is an internal inconsistency — never silently default
    // to kernel address space (0) for a userspace process.
    let pml4 = pcb::get_pml4(pid)
        .ok_or(KernelError::InternalError)?;

    // Create the scheduler task **suspended** (not yet runnable).
    //
    // This closes a register-vs-runnable race (B-PTHREAD-YIELDBUDGET): the
    // old code used `sched::spawn`, which enqueued the task immediately.  On
    // the uniprocessor a timer preemption in the window between `spawn`
    // returning and the `THREAD_OWNERS` insert below could run the child to
    // exit *before* it was registered.  `on_thread_exit` then did
    // `owners.remove(&task_id)?` → `None`, skipped the process's zombie
    // transition, and the process was never zombified — hanging the
    // container self-test's yield budget and firing its fatal assert.  We
    // now register all process/thread ownership *before* admitting the task,
    // which is also SMP-correct (another CPU cannot pick it up early).
    let task_id = sched::spawn_suspended(name, priority, entry, arg, pml4)?;

    // Register the thread with the process.
    if let Err(e) = pcb::add_thread(pid, task_id) {
        // Process disappeared between our check and the add — very
        // unlikely with single-CPU, but handle defensively.
        // Kill the orphaned scheduler task so its stack is freed.  (Safe
        // even though the task is only suspended: kill_task handles a
        // Blocked/not-enqueued task by simply marking it Dead.)
        serial_println!(
            "[thread] Failed to register task {} with process {}: {:?}",
            task_id, pid, e
        );
        sched::kill_task(task_id);
        return Err(e);
    }

    // Record the reverse mapping.  This MUST complete before the task is
    // admitted, so `on_thread_exit` can always find the owning process.
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

    // All ownership is now registered — admit the task so it can be
    // scheduled.  Only after this point can the child run (and possibly
    // exit), guaranteeing `THREAD_OWNERS`/`add_thread` are already in place.
    if !sched::admit(task_id) {
        // The task should be exactly Blocked here (we just created it
        // suspended and nothing else touched it).  If admit failed it means
        // the task was concurrently killed; surface it as an internal error
        // after unwinding the registration we just did.
        serial_println!(
            "[thread] Failed to admit task {} in process {}",
            task_id, pid
        );
        {
            let mut owners = THREAD_OWNERS.lock();
            owners.remove(&task_id);
        }
        // Detach the thread we just registered.  The task never ran, so it
        // accrued no CPU time / faults — zero accounting is exact.  Ignore
        // the return: on this unwinding path there are no join waiters to
        // wake (the child was never observable).
        let _ = pcb::remove_thread(pid, task_id, pcb::ThreadExitAccounting::default());
        sched::kill_task(task_id);
        return Err(KernelError::InternalError);
    }

    Ok(task_id)
}

/// Spawn a new **userspace** thread within an existing process.
///
/// Creates a scheduler task that enters ring 3 at `entry_rip` with
/// stack pointer `user_rsp`, sharing the process's address space.
/// The thread gets its own kernel stack for ring 0 transitions
/// (syscalls, interrupts).
///
/// This is the syscall-facing API for `SYS_THREAD_CREATE`.
///
/// # Arguments
///
/// - `pid` — owning process ID.
/// - `name` — human-readable name for debug output.
/// - `priority` — scheduling priority (0 = highest, 31 = lowest).
/// - `entry_rip` — ring 3 instruction pointer (thread entry function).
/// - `user_rsp` — ring 3 stack pointer (top of the user stack for
///   this thread; must already be mapped in the process's address
///   space).
///
/// # Errors
///
/// - [`KernelError::NoSuchProcess`] if `pid` doesn't exist or is zombie.
/// - [`KernelError::OutOfMemory`] if stack or info allocation fails.
/// - [`KernelError::InvalidAddress`] if `entry_rip` is not in user space.
pub fn spawn_user(
    pid: ProcessId,
    name: &[u8],
    priority: u8,
    entry_rip: u64,
    user_rsp: u64,
) -> KernelResult<TaskId> {
    use alloc::boxed::Box;
    use crate::proc::spawn::{UserEntryInfo, userspace_entry_trampoline};

    // Validate that the entry point is in user space (below the
    // canonical hole at 0x0000_8000_0000_0000).
    if entry_rip >= 0x0000_8000_0000_0000 || entry_rip == 0 {
        return Err(KernelError::InvalidAddress);
    }

    // Validate that the user stack pointer is in user space.
    if user_rsp >= 0x0000_8000_0000_0000 || user_rsp == 0 {
        return Err(KernelError::InvalidAddress);
    }

    // Heap-allocate the entry info.  The trampoline will free it when
    // the thread first runs.
    let info = Box::new(UserEntryInfo {
        entry_rip,
        user_rsp,
    });
    let info_ptr = Box::into_raw(info) as u64;

    // Reuse the existing kernel-mode spawn path with the ring 3
    // trampoline.  The trampoline does IRETQ to the user entry point.
    match spawn(pid, name, priority, userspace_entry_trampoline, info_ptr) {
        Ok(task_id) => {
            serial_println!(
                "[thread] Spawned user thread (task {}) in process {}: rip={:#x}, rsp={:#x}",
                task_id, pid, entry_rip, user_rsp
            );
            Ok(task_id)
        }
        Err(e) => {
            // Thread creation failed — free the info struct.
            //
            // SAFETY: info_ptr was just created by Box::into_raw and
            // no one else has accessed it.
            drop(unsafe { Box::from_raw(info_ptr as *mut UserEntryInfo) });
            Err(e)
        }
    }
}

/// Record a thread's exit value for a later `join()`.
///
/// A **detached** thread will never be joined, so retaining its exit
/// value would leak the `THREAD_EXIT_VALUES` map entry until the owning
/// process exits.  For detached threads we therefore store nothing.  This
/// is the kernel-side counterpart of the userspace pthread self-unmap fix
/// (see `posix/src/pthread.rs` and known-issues.md
/// D-PTHREAD-DETACH-KERNEL-EXITVAL).
fn record_exit_value(task_id: TaskId, exit_value: i64, detached: bool) {
    if detached {
        // No joiner will ever retrieve this — do not retain it.  Task IDs
        // are not reused while a task is live, so there is no stale entry
        // to clear here.
        return;
    }
    let mut exit_values = THREAD_EXIT_VALUES.lock();
    exit_values.insert(task_id, exit_value);
}

/// Exit the current thread with a value, supporting join.
///
/// Stores the exit value so a joining thread can retrieve it (unless the
/// thread is `detached`, in which case nothing is stored — no one will
/// join it), wakes any thread blocked in `join()`, then notifies the
/// process system and terminates the scheduler task.
///
/// This function does **not return**.
pub fn thread_exit_with_value(exit_value: i64, detached: bool) -> ! {
    let task_id = sched::current_task_id();

    // Store exit value (skipped for detached threads to avoid leaking the
    // map entry — see `record_exit_value`).
    record_exit_value(task_id, exit_value, detached);

    // Wake any thread that is joining on us.
    {
        let mut waiters = THREAD_JOIN_WAITERS.lock();
        if let Some(waiter_task) = waiters.remove(&task_id) {
            sched::wake(waiter_task);
        }
    }

    // Notify the thread/process system (may zombie the process if
    // this was the last thread).
    on_thread_exit(task_id);

    // Terminate the scheduler task (never returns).
    sched::task_exit();

    // Unreachable, but needed for the -> ! return type.
    crate::cpu::halt_loop();
}

/// Wait for a specific thread to exit and retrieve its exit value.
///
/// If the target thread has already exited, returns the exit value
/// immediately.  Otherwise, blocks the calling task until the target
/// thread exits.
///
/// Only one thread may join on a given target at a time.  Attempting
/// to join from multiple threads returns `WouldBlock` for the second
/// joiner.
///
/// # Arguments
///
/// - `target_task` — task ID of the thread to wait for.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the target is the calling task.
/// - [`KernelError::WouldBlock`] if another thread is already joining
///   on the target.
pub fn join(target_task: TaskId) -> KernelResult<i64> {
    let caller_task = sched::current_task_id();

    // Can't join on yourself — that's a deadlock.
    if target_task == caller_task {
        return Err(KernelError::InvalidArgument);
    }

    // Check if the target has already exited.
    {
        let mut exit_values = THREAD_EXIT_VALUES.lock();
        if let Some(exit_value) = exit_values.remove(&target_task) {
            return Ok(exit_value);
        }
    }

    // Verify the target belongs to the same process as the caller.
    {
        let owners = THREAD_OWNERS.lock();
        let caller_pid = owners.get(&caller_task).copied();
        let target_pid = owners.get(&target_task).copied();

        match (caller_pid, target_pid) {
            (Some(cp), Some(tp)) if cp == tp => {} // Same process — OK.
            (_, None) => {
                // Target not registered — may have already exited and
                // been cleaned up.  Check exit values one more time.
                drop(owners);
                let mut exit_values = THREAD_EXIT_VALUES.lock();
                if let Some(exit_value) = exit_values.remove(&target_task) {
                    return Ok(exit_value);
                }
                return Err(KernelError::NoSuchProcess);
            }
            _ => {
                // Different process — not allowed.
                return Err(KernelError::PermissionDenied);
            }
        }
    }

    // Register as the waiter for the target thread.
    {
        let mut waiters = THREAD_JOIN_WAITERS.lock();
        if waiters.contains_key(&target_task) {
            // Another thread is already joining on this target.
            return Err(KernelError::WouldBlock);
        }
        waiters.insert(target_task, caller_task);
    }

    // Block until the target thread exits and wakes us.
    sched::block_current();

    // Woken up — retrieve the exit value.
    {
        let mut exit_values = THREAD_EXIT_VALUES.lock();
        if let Some(exit_value) = exit_values.remove(&target_task) {
            return Ok(exit_value);
        }
    }

    // Shouldn't happen — we were woken because the target exited.
    // Defensive fallback.
    serial_println!(
        "[thread] WARNING: join woke but no exit value for task {}",
        target_task
    );
    Ok(0)
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
    // Linux CLONE_CHILD_CLEARTID hook: if this task was created via
    // clone(CLONE_CHILD_CLEARTID, ...) and registered a `ctid`
    // address, zero it in user space and wake one futex waiter so
    // any pthread_join blocked on it can proceed.  Do this BEFORE
    // any process-state mutation while CR3 still points at this
    // thread's address space.
    super::thread_clone::on_thread_exit_hook(task_id);

    // Look up and remove the reverse mapping.
    let pid = {
        let mut owners = THREAD_OWNERS.lock();
        owners.remove(&task_id)?
    };

    // Clean up any IRQ registrations owned by this task.
    // This prevents dangling registrations when a driver process crashes.
    crate::ioapic::release_irqs_for_task(task_id);

    // Capture the exiting thread's accumulated counters while its Task is
    // still alive in the scheduler — `remove_thread` folds them into the
    // owning process's accumulators so they survive the Task's destruction.
    // (Lock ordering: read SCHED here, before taking PROCESS_TABLE inside
    // remove_thread, to avoid nesting the two locks.)
    let (exit_user, exit_sys) = sched::cpu_ticks(task_id).unwrap_or((0, 0));
    let (exit_min, exit_maj) = sched::fault_counts(task_id).unwrap_or((0, 0));
    let (exit_nv, exit_niv) = sched::ctxsw_counts(task_id).unwrap_or((0, 0));
    let acct = pcb::ThreadExitAccounting {
        user_ticks: exit_user,
        sys_ticks: exit_sys,
        min_flt: exit_min,
        maj_flt: exit_maj,
        nvcsw: exit_nv,
        nivcsw: exit_niv,
    };

    // POSIX orphaned-process-group hangup: capture the process groups this
    // process currently *guards* (children in a different group of the same
    // session) BEFORE `remove_thread` reparents them to init. If this exit
    // zombifies the process, each captured group may have just become orphaned;
    // we re-check and SIGHUP+SIGCONT any with stopped jobs after the zombie
    // bookkeeping completes (outside the PROCESS_TABLE lock).
    let guarded_pgrps = pcb::guarded_child_pgrps(pid);

    // Remove from the process's thread list.
    match pcb::remove_thread(pid, task_id, acct) {
        Ok((is_zombie, wake_task, any_waiter)) => {
            if is_zombie {
                serial_println!(
                    "[thread] Process {} has no threads left — now zombie",
                    pid
                );

                // Close all fd-bearing kernel resources NOW, at process
                // exit — matching Linux's `exit_files()` in `do_exit`.
                // This must happen before the reaper's `wait4()` (which
                // is what eventually calls `destroy()`): a pipe write end
                // held by this process has to close here so a reader
                // blocked on EOF — possibly the very task that will reap
                // us — is woken, rather than deadlocking until a reap
                // that can never come.  See `pcb::exit_close_fds`.
                pcb::exit_close_fds(pid);

                // Release namespace reference so the namespace can be cleaned up.
                crate::ipc::namespace::detach(pid);

                // Wake a task blocked in `waitpid(pid)` for this process.
                if let Some(waiter) = wake_task {
                    crate::sched::wake(waiter);
                }
                // Wake a parent blocked in `waitpid(-1)` (wait for any
                // child) so it can re-scan and reap this newly-zombied
                // child.
                if let Some(waiter) = any_waiter {
                    crate::sched::wake(waiter);
                }

                // Post SIGCHLD to the parent. This is distinct from the
                // wait4() wakeups above (which target a thread parked in
                // wait4()): SIGCHLD drives the *signal* path, used by a
                // parent running a SIGCHLD handler or parked in
                // sigsuspend()/pause() — e.g. dash's job-control `wait`
                // builtin, which arms a SIGCHLD handler then sigsuspends,
                // reaping with waitpid(WNOHANG) only after the signal wakes
                // it.  Without this the parent livelocks in sigsuspend.
                if let Some(parent) = pcb::parent(pid) {
                    if parent != 0 {
                        let info = crate::proc::signal::SigInfo::child(
                            u32::try_from(pid).unwrap_or(0),
                            0,
                        );
                        // Linux-ABI parents deliver SIGCHLD via their
                        // per-signal rt_sigaction disposition
                        // (deliver_linux_signal consults linux_disposition),
                        // so mark it pending directly. Native parents go
                        // through classify_post so a registered trampoline
                        // handler runs and a no-handler parent correctly
                        // drops it (SIGCHLD default action = ignore).
                        if pcb::get_abi_mode(parent)
                            == Some(pcb::AbiMode::Linux)
                        {
                            crate::proc::signal::set_pending_info(
                                parent, 17, info,
                            );
                        } else {
                            // Discarding the PostDecision is intentional:
                            // SIGCHLD's default is ignore, so a no-handler
                            // native parent yields Drop with no side effect;
                            // a handler yields Deliver (already marked
                            // pending). There is no Terminate case for 17.
                            let _ = crate::proc::signal::classify_post_info(
                                parent, 17, info,
                            );
                        }
                    }
                }

                // Now that this process is a zombie and its children have
                // been reparented to init, any group it used to guard may be
                // orphaned. Send SIGHUP+SIGCONT to each that is now orphaned
                // and holds stopped jobs (POSIX "Orphaned Process Group").
                for pgrp in &guarded_pgrps {
                    crate::syscall::handlers::kill_orphaned_pgrp(*pgrp);
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

/// Sum the `(user_ticks, sys_ticks)` CPU time of a process across both
/// its **live** threads and its **already-exited** threads.
///
/// Each thread's CPU time is charged tick-by-tick by the scheduler
/// (Linux tick-sampling model).  When a thread exits, `on_thread_exit`
/// folds its ticks into the per-process accumulator
/// (`Process::acct_user_ticks`/`acct_sys_ticks`) before the scheduler
/// destroys the Task, so the total here is
/// `accumulator + Σ(live thread ticks)`.  Returns `(0, 0)` if the process
/// is unknown.  Ticks are at `USER_HZ` (100 Hz).
///
/// This makes the result exact for multi-threaded processes even after
/// worker threads have exited — not just single-threaded ones.
///
/// Sourced by the Linux-ABI `getrusage(RUSAGE_SELF)` `ru_utime`/
/// `ru_stime`, `times` `tms_utime`/`tms_stime`, and `/proc/<pid>/stat`
/// utime/stime surfaces.  Children-time (`cutime`/`cstime`,
/// `RUSAGE_CHILDREN`) is tracked separately — see
/// [`crate::proc::pcb::process_child_ticks`].
#[must_use]
pub fn process_cpu_ticks(pid: ProcessId) -> (u64, u64) {
    // Exited-thread accumulator (also serves as the existence check:
    // `None` means the process is unknown).
    let Some((mut user, mut sys)) = pcb::process_acct_ticks(pid) else {
        return (0, 0);
    };
    // Add live threads' in-flight ticks.
    if let Some(task_ids) = pcb::get_threads(pid) {
        for tid in task_ids {
            if let Some((u, s)) = sched::cpu_ticks(tid) {
                user = user.saturating_add(u);
                sys = sys.saturating_add(s);
            }
        }
    }
    (user, sys)
}

/// Sum the `(min_flt, maj_flt)` page-fault counts of a process across both
/// its **live** and **already-exited** threads.
///
/// Mirrors [`process_cpu_ticks`] for page faults: live threads carry their
/// own `Task::min_flt`/`maj_flt`, and exited threads have folded theirs into
/// `Process::acct_min_flt`/`acct_maj_flt`.  Returns `(0, 0)` if the process
/// is unknown.
///
/// Sourced by `getrusage(RUSAGE_SELF)` `ru_minflt`/`ru_majflt` and
/// `/proc/<pid>/stat` fields 10/12 (minflt/majflt).  Children faults
/// (`RUSAGE_CHILDREN`, fields 11/13) are tracked separately — see
/// [`crate::proc::pcb::process_child_faults`].
#[must_use]
pub fn process_fault_counts(pid: ProcessId) -> (u64, u64) {
    let Some((mut min_flt, mut maj_flt)) = pcb::process_acct_faults(pid) else {
        return (0, 0);
    };
    if let Some(task_ids) = pcb::get_threads(pid) {
        for tid in task_ids {
            if let Some((mn, mj)) = sched::fault_counts(tid) {
                min_flt = min_flt.saturating_add(mn);
                maj_flt = maj_flt.saturating_add(mj);
            }
        }
    }
    (min_flt, maj_flt)
}

/// Sum the `(nvcsw, nivcsw)` context-switch counts of a process across both
/// its **live** and **already-exited** threads.
///
/// Mirrors [`process_cpu_ticks`]/[`process_fault_counts`]: live threads
/// carry their own `Task::nvcsw`/`nivcsw`, and exited threads have folded
/// theirs into `Process::acct_nvcsw`/`acct_nivcsw`.  Returns `(0, 0)` if the
/// process is unknown.  Sourced by `getrusage(RUSAGE_SELF)`
/// `ru_nvcsw`/`ru_nivcsw`.  Children ctxsw (`RUSAGE_CHILDREN`) is tracked
/// separately — see [`crate::proc::pcb::process_child_ctxsw`].
#[must_use]
pub fn process_ctxsw_counts(pid: ProcessId) -> (u64, u64) {
    let Some((mut nvcsw, mut nivcsw)) = pcb::process_acct_ctxsw(pid) else {
        return (0, 0);
    };
    if let Some(task_ids) = pcb::get_threads(pid) {
        for tid in task_ids {
            if let Some((nv, niv)) = sched::ctxsw_counts(tid) {
                nvcsw = nvcsw.saturating_add(nv);
                nivcsw = nivcsw.saturating_add(niv);
            }
        }
    }
    (nvcsw, nivcsw)
}

/// Force-kill all threads in a process.
///
/// For each thread belonging to the process:
/// 1. Marks the scheduler task as Dead (and dequeues if Ready).
/// 2. Removes the thread→process mapping.
/// 3. Removes the thread from the process's thread list.
///
/// When the last thread is removed, the process transitions to Zombie
/// state (as with normal thread exit).
///
/// Returns the number of threads killed.
pub fn kill_process_threads(pid: ProcessId) -> usize {
    let task_ids = pcb::get_threads(pid).unwrap_or_default();
    let mut killed: usize = 0;

    for &task_id in &task_ids {
        // Mark the scheduler task as Dead and dequeue it.
        sched::kill_task(task_id);

        // Remove the thread→process mapping and update the PCB.
        // This may trigger the zombie transition for the last thread.
        on_thread_exit(task_id);

        killed = killed.saturating_add(1);
    }

    killed
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
    test_thread_exit_with_value()?;
    test_thread_join()?;
    test_join_self_fails()?;
    test_detached_exit_not_retained()?;

    Ok(())
}

/// Test 7: A detached thread's exit value is not retained.
///
/// Exercises [`record_exit_value`] directly (the gate that
/// `thread_exit_with_value` applies), using a synthetic task ID far
/// outside the live-task range so it cannot collide with a real thread.
/// A joinable exit is recorded; a detached exit is not — proving the
/// D-PTHREAD-DETACH-KERNEL-EXITVAL leak is closed for detached threads.
fn test_detached_exit_not_retained() -> KernelResult<()> {
    // Synthetic, never-scheduled task IDs.
    let fake: TaskId = TaskId::MAX - 7;

    // Joinable: the value is recorded and retrievable.
    record_exit_value(fake, 7, false);
    {
        let mut ev = THREAD_EXIT_VALUES.lock();
        if ev.remove(&fake) != Some(7) {
            serial_println!(
                "[thread]   FAIL: joinable exit value should have been recorded"
            );
            return Err(KernelError::InternalError);
        }
    }

    // Detached: nothing is stored, so nothing leaks.
    record_exit_value(fake, 7, true);
    {
        let mut ev = THREAD_EXIT_VALUES.lock();
        if ev.remove(&fake).is_some() {
            serial_println!(
                "[thread]   FAIL: detached exit value must not be retained"
            );
            return Err(KernelError::InternalError);
        }
    }

    serial_println!("[thread]   Detached exit value not retained: OK");
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

/// Kernel task entry that stores an exit value before returning.
///
/// The arg encodes the exit value to store.  This simulates a thread
/// that calls `thread_exit_with_value()` with a specific value.
///
/// Note: Since this runs as a kernel thread, we can't call the full
/// `thread_exit_with_value()` (which calls `task_exit()` — never
/// returns).  Instead, we directly store the exit value and wake
/// joiners.  The scheduler handles the actual task termination
/// via `task_finished`.
extern "C" fn test_thread_exit_entry(arg: u64) {
    let task_id = sched::current_task_id();
    #[allow(clippy::cast_possible_wrap)]
    let exit_value = arg as i64;

    // Store exit value.
    {
        let mut exit_values = THREAD_EXIT_VALUES.lock();
        exit_values.insert(task_id, exit_value);
    }

    // Wake any joiner.
    {
        let mut waiters = THREAD_JOIN_WAITERS.lock();
        if let Some(waiter_task) = waiters.remove(&task_id) {
            sched::wake(waiter_task);
        }
    }
}

/// Test 4: Thread exit stores a value that can be retrieved.
fn test_thread_exit_with_value() -> KernelResult<()> {
    let pid = pcb::create("thread-test-exit-val", 0);

    let task_id = spawn(
        pid,
        b"exit-val-thread",
        sched::task::DEFAULT_PRIORITY,
        test_thread_exit_entry,
        42, // Will be stored as exit value.
    )?;

    // Let the thread run and exit.
    sched::yield_now();
    sched::yield_now();

    // Check that the exit value was stored.
    {
        let mut exit_values = THREAD_EXIT_VALUES.lock();
        match exit_values.remove(&task_id) {
            Some(42) => {} // Expected.
            other => {
                serial_println!(
                    "[thread]   FAIL: exit value should be 42, got {:?}",
                    other
                );
                pcb::destroy(pid);
                return Err(KernelError::InternalError);
            }
        }
    }

    on_thread_exit(task_id);
    pcb::destroy(pid);
    serial_println!("[thread]   Thread exit with value: OK");
    Ok(())
}

/// Test 5: Thread join retrieves exit value after target completes.
///
/// Strategy: spawn a thread that stores an exit value, let it complete,
/// then call `join()` which should return the value immediately (the
/// thread already exited).
fn test_thread_join() -> KernelResult<()> {
    let pid = pcb::create("thread-test-join", 0);

    // Spawn the main "caller" thread — that's us (the idle task).
    // We need a thread association for the idle task to test join's
    // same-process check.  We'll skip the same-process check for
    // this kernel-mode test and instead test just the value retrieval.

    let target = spawn(
        pid,
        b"join-target",
        sched::task::DEFAULT_PRIORITY,
        test_thread_exit_entry,
        99, // Exit value.
    )?;

    // Let the thread run and exit.
    sched::yield_now();
    sched::yield_now();

    // The target thread has exited and stored its exit value.
    // Call join — it should return the value immediately.
    //
    // Note: We call the join function's value-retrieval path directly
    // since the idle task (us) isn't registered as a process thread,
    // which would fail the same-process check.  Instead, verify the
    // value is in THREAD_EXIT_VALUES.
    {
        let mut exit_values = THREAD_EXIT_VALUES.lock();
        match exit_values.remove(&target) {
            Some(99) => {} // Expected.
            other => {
                serial_println!(
                    "[thread]   FAIL: join expected exit value 99, got {:?}",
                    other
                );
                pcb::destroy(pid);
                return Err(KernelError::InternalError);
            }
        }
    }

    on_thread_exit(target);
    pcb::destroy(pid);
    serial_println!("[thread]   Thread join (value retrieval): OK");
    Ok(())
}

/// Test 6: Joining on self returns an error.
fn test_join_self_fails() -> KernelResult<()> {
    let current = sched::current_task_id();
    match join(current) {
        Err(KernelError::InvalidArgument) => {} // Expected.
        other => {
            serial_println!(
                "[thread]   FAIL: join-self should return InvalidArgument, got {:?}",
                other
            );
            return Err(KernelError::InternalError);
        }
    }

    serial_println!("[thread]   Join self rejected: OK");
    Ok(())
}
