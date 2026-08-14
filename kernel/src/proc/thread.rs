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
//! - `join()` supports a single waiter per target thread; a second
//!   concurrent joiner gets [`KernelError::WouldBlock`].

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

/// How a thread ended, from a joiner's point of view.
///
/// A joiner must be able to tell "ran to completion and produced this
/// value" apart from "was killed before it produced anything".  Reporting
/// the latter as a normal return with value 0 is a *silent wrong answer*:
/// the joiner folds a zero into its result and the program exits
/// successfully having quietly dropped the dead thread's work.  See
/// known-issues.md `B-PTHREAD-CHILD-JUMPS-TO-GARBAGE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ThreadOutcome {
    /// The thread ran to completion (or called `pthread_exit`) and
    /// produced this value.
    Exited(i64),
    /// The thread was killed before it could produce a value: an
    /// unhandled ring-3 fault, an explicit `kill`, or process teardown.
    Killed,
}

/// Stores the outcome of threads that have ended.
///
/// When a thread calls `thread_exit_with_value()`, its exit value is
/// stored here; when a thread is killed, [`ThreadOutcome::Killed`] is
/// stored instead.  The joining thread reads it from this map.  Entries
/// are removed when the join completes (or never, if no one joins).
///
/// This is independent of process exit codes — each thread has its
/// own exit value that another thread in the same process can retrieve.
static THREAD_OUTCOMES: Mutex<BTreeMap<TaskId, ThreadOutcome>> =
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
    // 0/0 = leave the TLS bases at the `Task` default (kernel tasks never
    // read %fs, and a userspace task that needs one seeds it explicitly).
    spawn_with_tls(pid, name, priority, entry, arg, 0, 0)
}

/// Spawn a thread, seeding its `%fs`/`%gs` bases **before** it can run.
///
/// Identical to [`spawn`] except that `fs_base`/`gs_base` are written onto
/// the new [`Task`](crate::sched::task::Task) while it is still suspended,
/// so the value is in place the first time the scheduler switches it in.
///
/// # Why this exists (B-PTHREAD-CHILD-JUMPS-TO-GARBAGE, defect 1)
///
/// `IA32_FS_BASE` is a global CPU register that is **not** part of the saved
/// GP `Context` and is **never read back on switch-out** — `Task::fs_base` is
/// its sole authority, and the switch-in path restores it unconditionally for
/// every user task. A caller that did:
///
/// ```ignore
/// let tid = thread::spawn(...)?;      // <-- task is ADMITTED here
/// sched::set_task_fs_base(tid, fs);   // <-- too late
/// ```
///
/// left a window in which the child was runnable with `fs_base == 0`. The
/// child's *first* run survived it (the clone trampoline installs the base
/// with a one-shot `WRMSR`), but if the child was **preempted** before the
/// parent reached `set_task_fs_base`, its next switch-in restored the still
/// zero `Task::fs_base` and clobbered the live MSR. glibc's `start_thread`
/// then read `pd->start_routine` through a null TLS base and jumped to
/// garbage — an intermittent (~1-in-10 boots) ring-3 fault whose `RIP`
/// equalled `CR2` with an otherwise intact stack.
///
/// Seeding before admission closes the window structurally: there is no
/// instant at which the task is schedulable with a wrong TLS base. This is
/// the same "register everything before admitting the task" discipline that
/// [`spawn`] already applies to `THREAD_OWNERS`/`pcb::add_thread` (which
/// closed B-PTHREAD-YIELDBUDGET).
///
/// # Errors
///
/// Same as [`spawn`].
pub fn spawn_with_tls(
    pid: ProcessId,
    name: &[u8],
    priority: u8,
    entry: extern "C" fn(u64),
    arg: u64,
    fs_base: u64,
    gs_base: u64,
) -> KernelResult<TaskId> {
    let task_id = spawn_suspended_with_tls(pid, name, priority, entry, arg, fs_base, gs_base)?;
    admit(pid, task_id)?;
    Ok(task_id)
}

/// Phase 1 of the two-phase spawn: create a thread that is fully registered
/// with its process but **not yet runnable**.
///
/// The returned task is `Blocked` and enqueued nowhere, so it cannot run — and
/// therefore cannot *exit* — until [`admit`] is called. That gives the caller a
/// quiescent window in which to perform any registration that
/// [`on_thread_exit_hook`](super::thread_clone::on_thread_exit_hook) will later
/// need to find.
///
/// # Why the two phases are separate (B-PTHREAD-JOIN-LOST-CTID)
///
/// The set of per-thread state that must be in place *before* the child can run
/// keeps growing — `THREAD_OWNERS`, `pcb::add_thread`, the `%fs`/`%gs` bases,
/// and the `CLONE_CHILD_CLEARTID` ctid registration. Threading every one of
/// them through a widening parameter list does not scale, and each new one that
/// gets registered *after* the spawn call re-opens the same race.
///
/// The concrete failure this closed: `clone_thread` admitted the child and only
/// then called `register_clear_child_tid`. A child that ran to completion inside
/// that window exited with no ctid registered, so `on_thread_exit_hook` took its
/// `None => return` path and performed **neither** the zero-write to `*ctid`
/// **nor** the `futex_wake` on it. glibc's `pthread_join` then blocked forever
/// on a tid word that would never be cleared, the process never zombified, and
/// the Path-Z pthread self-test timed out (~1-in-10 boots). The late
/// registration also leaked a permanent `CLEAR_CHILD_TID` entry for a dead task.
///
/// Callers that need no extra registration should use [`spawn`] /
/// [`spawn_with_tls`], which are just phase 1 followed immediately by phase 2.
///
/// # Errors
///
/// Same as [`spawn`].
pub fn spawn_suspended_with_tls(
    pid: ProcessId,
    name: &[u8],
    priority: u8,
    entry: extern "C" fn(u64),
    arg: u64,
    fs_base: u64,
    gs_base: u64,
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

    // Seed the TLS bases while the task is still suspended.  This MUST happen
    // before `admit`: the switch-in path restores `Task::fs_base` into
    // IA32_FS_BASE unconditionally and never saves it back on switch-out, so a
    // task admitted with a stale 0 here would have its trampoline-installed
    // base clobbered the first time it was preempted and resumed.  See this
    // function's doc comment (B-PTHREAD-CHILD-JUMPS-TO-GARBAGE defect 1).
    if fs_base != 0 {
        sched::set_task_fs_base(task_id, fs_base);
    }
    if gs_base != 0 {
        sched::set_task_gs_base(task_id, gs_base);
    }

    Ok(task_id)
}

/// Phase 2 of the two-phase spawn: make a thread created by
/// [`spawn_suspended_with_tls`] runnable.
///
/// Must be called exactly once per successful `spawn_suspended_with_tls`. Only
/// after this returns can the child run — and exit — so every registration the
/// exit path depends on must already be installed. On failure the thread's
/// registration is unwound and the task destroyed, so the caller must not
/// reference `task_id` again.
///
/// # Errors
///
/// - [`KernelError::InternalError`] if the task was concurrently killed and so
///   could not be admitted.
pub fn admit(pid: ProcessId, task_id: TaskId) -> KernelResult<()> {
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

    Ok(())
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
/// value would leak the `THREAD_OUTCOMES` map entry until the owning
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
    let mut outcomes = THREAD_OUTCOMES.lock();
    outcomes.insert(task_id, ThreadOutcome::Exited(exit_value));
}

/// Record that a thread was **killed** rather than exiting on its own.
///
/// Must be called *before* [`on_thread_exit`] on every involuntary death
/// path, because `on_thread_exit` is what releases a parked joiner: once
/// it has run, the joiner can wake and read the map at any moment, so a
/// marker written afterwards may arrive too late.
///
/// Uses `or_insert` rather than an unconditional insert so that a thread
/// which already recorded a real exit value and is only *then* swept up
/// by process teardown keeps the value it produced.
fn record_killed(task_id: TaskId) {
    let mut outcomes = THREAD_OUTCOMES.lock();
    outcomes.entry(task_id).or_insert(ThreadOutcome::Killed);
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

    // Notify the thread/process system (may zombie the process if
    // this was the last thread).  This also wakes any thread joining on
    // us — the wake lives in `on_thread_exit` so that *every* death path
    // releases the joiner, not just this one.  The exit value is already
    // recorded above, so the joiner is guaranteed to find it.
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

    // Check if the target has already ended.
    if let Some(outcome) = take_outcome(target_task) {
        return outcome_to_result(target_task, outcome);
    }

    // Verify the target belongs to the same process as the caller.
    {
        let owners = THREAD_OWNERS.lock();
        let caller_pid = owners.get(&caller_task).copied();
        let target_pid = owners.get(&target_task).copied();

        match (caller_pid, target_pid) {
            (Some(cp), Some(tp)) if cp == tp => {} // Same process — OK.
            (_, None) => {
                // Target not registered — may have already ended and been
                // cleaned up.  Check the outcome map one more time.
                drop(owners);
                if let Some(outcome) = take_outcome(target_task) {
                    return outcome_to_result(target_task, outcome);
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

    // Register-then-recheck: the liveness check above released the
    // `THREAD_OWNERS` lock before we took the `THREAD_JOIN_WAITERS` lock, so
    // the target may have exited in between.  `on_thread_exit` drains the
    // waiter map *before* removing itself from `THREAD_OWNERS`, so a target
    // that is gone from `THREAD_OWNERS` now can no longer arrive to wake us —
    // our entry would sit in the map forever and the park loop below, whose
    // only exit condition is that entry being removed, would never terminate.
    //
    // Re-checking after publishing our registration closes the window in the
    // one direction that matters: either the exit ran first (we observe it
    // here and unwind), or it runs after our insert (it finds our entry and
    // wakes us).  This is the same idiom the futex wait path uses for the
    // signal-arrival race.  Ordering note: we drop the waiters lock before
    // taking the owners lock, matching the acquisition order used above.
    let target_gone = {
        let owners = THREAD_OWNERS.lock();
        !owners.contains_key(&target_task)
    };
    if target_gone {
        // Withdraw our registration — but only if it is still ours.  A racing
        // `on_thread_exit` may have already removed it and woken us, in which
        // case the wake is real and we must fall through to collect the
        // outcome rather than reporting the thread as missing.
        let withdrawn = {
            let mut waiters = THREAD_JOIN_WAITERS.lock();
            if waiters.get(&target_task) == Some(&caller_task) {
                waiters.remove(&target_task);
                true
            } else {
                false
            }
        };
        if withdrawn {
            // The exit completed before we registered, so no wake is coming.
            // Its outcome (if any) is already recorded.
            if let Some(outcome) = take_outcome(target_task) {
                return outcome_to_result(target_task, outcome);
            }
            return Err(KernelError::NoSuchProcess);
        }
    }

    // Park until the target thread actually exits.
    //
    // `block_current()` can return **spuriously**: a `sched::wake` that
    // lands while this task is still running leaves a `pending_wake`
    // token which the next park consumes without any event having
    // occurred (see known-issues.md
    // `BUG-TRYWAKE-FALSE-CONFLATES-CONTENTION`).  Returning on such a
    // wake would hand the caller a bogus exit value for a thread that is
    // still alive, so the loop re-checks the real condition: our own
    // registration.  `on_thread_exit` removes it under the
    // `THREAD_JOIN_WAITERS` lock immediately before waking us, so while
    // the entry is still ours nothing has happened and we park again.
    loop {
        sched::block_current();
        let released = {
            let waiters = THREAD_JOIN_WAITERS.lock();
            waiters.get(&target_task) != Some(&caller_task)
        };
        if released {
            break;
        }
    }

    // Woken up — retrieve the outcome.
    if let Some(outcome) = take_outcome(target_task) {
        return outcome_to_result(target_task, outcome);
    }

    // The target ended without recording any outcome at all.  That is
    // expected only for a **detached** thread: `record_exit_value`
    // deliberately stores nothing for one, because by contract nobody may
    // join it.  Every involuntary death path records
    // [`ThreadOutcome::Killed`], so reaching here means the caller joined
    // a thread it had no right to join.
    serial_println!(
        "[thread] join: task {} ended with no recorded outcome (detached?) — reporting EINVAL",
        target_task
    );
    Err(KernelError::InvalidArgument)
}

/// Remove and return a thread's recorded outcome, if any.
fn take_outcome(task_id: TaskId) -> Option<ThreadOutcome> {
    let mut outcomes = THREAD_OUTCOMES.lock();
    outcomes.remove(&task_id)
}

/// Translate a recorded outcome into `join()`'s result.
///
/// A killed thread is reported as [`KernelError::Cancelled`], never as
/// `Ok(0)` — see [`ThreadOutcome`].  The POSIX layer turns this into the
/// `PTHREAD_CANCELED` return value, which is precisely the slot POSIX
/// reserves for "this thread did not finish normally".
fn outcome_to_result(task_id: TaskId, outcome: ThreadOutcome) -> KernelResult<i64> {
    match outcome {
        ThreadOutcome::Exited(v) => Ok(v),
        ThreadOutcome::Killed => {
            serial_println!(
                "[thread] join: task {} was killed — reporting Cancelled, not a normal return",
                task_id
            );
            Err(KernelError::Cancelled)
        }
    }
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

    // Release a thread parked in `join(task_id)`.
    //
    // This lives here — the *universal* thread-death hook — rather than
    // in `thread_exit_with_value`, because a thread can die by several
    // other paths: an unhandled exception (`idt.rs`), `exit_group`, or
    // process teardown.  Those all reach `on_thread_exit` but never
    // `thread_exit_with_value`, so waking only there left the joiner
    // blocked forever.  It must also run *before* the `THREAD_OWNERS`
    // lookup below, which returns early for tasks that were never
    // registered as process threads — a joiner has to be released
    // regardless of registration state.
    {
        let mut waiters = THREAD_JOIN_WAITERS.lock();
        if let Some(waiter_task) = waiters.remove(&task_id) {
            sched::wake(waiter_task);
        }
    }

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

/// Map a POSIX **nice** value to a scheduler **priority** level.
///
/// Nice ranges `-20..=19` (lower = more favourable / higher priority);
/// our scheduler priority ranges `0..=31` (0 = highest, 31 = lowest, see
/// [`crate::sched::task::NUM_PRIORITIES`]). The mapping is linear and
/// monotonic (higher nice ⇒ higher priority number ⇒ lower scheduling
/// priority), pinned so nice `0` lands on the default priority level:
///
/// `priority = round((nice + 20) * 31 / 39)`
///
/// which yields `nice -20 → 0`, `nice 0 → 16` (== [`task::DEFAULT_PRIORITY`]),
/// and `nice 19 → 31`. Inputs are clamped to the valid nice range first.
#[must_use]
pub fn nice_to_priority(nice: i32) -> u8 {
    // Clamp to the POSIX nice range, then bias to 0..=39 so the scaling is a
    // non-negative integer computation.
    let biased = nice.clamp(-20, 19) + 20; // 0..=39
    // round(biased * 31 / 39): add half the denominator before the floor.
    // biased*31 <= 39*31 = 1209, +19 = 1228, well within i32 — no overflow.
    #[allow(clippy::arithmetic_side_effects)]
    let prio = (biased * 31 + 19) / 39; // 0..=31
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        prio.clamp(0, 31) as u8
    }
}

/// Record a process's **nice** value and apply it to the scheduler.
///
/// This is the authoritative "make nice real" primitive shared by both the
/// native `SYS_PROCESS_SET_NICE` path and the Linux-ABI `setpriority` /
/// `sched_setattr` paths. It:
/// 1. clamps `nice` to `-20..=19`,
/// 2. stores it in the process's PCB (via [`pcb::set_nice`]), and
/// 3. maps it to a priority level and re-prioritises **every** task the
///    process currently owns via [`sched::set_priority`].
///
/// Returns the *previous* nice value, or `None` if `pid` is unknown.
///
/// Lock ordering: [`pcb::set_nice`] and [`pcb::get_threads`] each take and
/// release the process-table lock; [`sched::set_priority`] takes the
/// scheduler lock. We deliberately finish all PCB access before touching the
/// scheduler so the two locks are never held simultaneously.
pub fn set_process_nice(pid: ProcessId, nice: i32) -> Option<i32> {
    let clamped = nice.clamp(-20, 19);
    let old = pcb::set_nice(pid, clamped)?;
    let prio = nice_to_priority(clamped);
    if let Some(task_ids) = pcb::get_threads(pid) {
        for tid in task_ids {
            // Ignore a missing task: a thread may have exited between the
            // get_threads snapshot and here; its priority is moot.
            let _ = sched::set_priority(tid, prio);
        }
    }
    Some(old)
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
        // Record the involuntary death *before* `on_thread_exit`, which
        // releases any parked joiner — see `record_killed`.
        record_killed(task_id);

        // Mark the scheduler task as Dead and dequeue it.
        sched::kill_task(task_id);

        // Remove the thread→process mapping and update the PCB.
        // This may trigger the zombie transition for the last thread.
        on_thread_exit(task_id);

        killed = killed.saturating_add(1);
    }

    killed
}

/// Kill a single thread, leaving the rest of its process running.
///
/// This is the only correct way to take out one thread from outside it:
/// it records the involuntary death so a `join()` on the victim reports
/// [`KernelError::Cancelled`] instead of a bogus normal return, kills the
/// scheduler task, *and* runs the universal death hook so the
/// thread→process mapping, IRQ registrations and parked joiners are all
/// cleaned up.  Calling `sched::kill_task` on its own — which is what the
/// shell's `kill` command used to do — skips every one of those and
/// leaves the thread registered forever with its joiner parked.
///
/// Returns `true` if the scheduler accepted the kill.  It refuses the
/// *current* task (that is `task_exit`'s job) and tasks that are already
/// dead.
pub fn kill_thread(task_id: TaskId) -> bool {
    record_killed(task_id);
    let accepted = sched::kill_task(task_id);
    if accepted {
        on_thread_exit(task_id);
    } else {
        // Nothing died, so do not leave a phantom `Killed` marker behind
        // that a later legitimate join would trip over.
        let mut outcomes = THREAD_OUTCOMES.lock();
        if outcomes.get(&task_id) == Some(&ThreadOutcome::Killed) {
            outcomes.remove(&task_id);
        }
    }
    accepted
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
    test_blocking_join()?;
    test_join_self_fails()?;
    test_detached_exit_not_retained()?;
    test_killed_thread_does_not_join_normally()?;

    Ok(())
}

/// Test 8: a **killed** thread does not join as a normal return.
///
/// Regression test for `B-PTHREAD-CHILD-JUMPS-TO-GARBAGE` defect 2.
/// `join()` used to report `Ok(0)` for any thread that ended without
/// recording an exit value, which is exactly what a killed thread looks
/// like.  A worker's contribution then silently vanished from the
/// caller's total and the program exited 0 — a wrong answer with no
/// error reported anywhere.  A kill must surface as `Cancelled`.
///
/// Exercises the outcome map and `join()`'s already-ended fast path
/// directly with synthetic task IDs, so no real thread has to be killed
/// and the test cannot race the scheduler.
fn test_killed_thread_does_not_join_normally() -> KernelResult<()> {
    // Synthetic, never-scheduled task IDs, distinct from test 7's.
    let killed: TaskId = TaskId::MAX - 11;
    let exited: TaskId = TaskId::MAX - 12;

    // A killed thread reports Cancelled, not Ok(0).
    record_killed(killed);
    match join(killed) {
        Err(KernelError::Cancelled) => {}
        other => {
            serial_println!(
                "[thread]   FAIL: join on a killed thread should be Cancelled, got {:?}",
                other
            );
            let mut outcomes = THREAD_OUTCOMES.lock();
            outcomes.remove(&killed);
            return Err(KernelError::InternalError);
        }
    }

    // ...and the outcome is consumed, so a second join finds nothing.
    // (The synthetic ID is not registered in `THREAD_OWNERS`, so `join`
    // takes its "target not registered" path and reports NoSuchProcess.)
    if join(killed) != Err(KernelError::NoSuchProcess) {
        serial_println!(
            "[thread]   FAIL: a killed thread's outcome should be consumed by join"
        );
        return Err(KernelError::InternalError);
    }

    // A thread that really exited keeps its value even if process
    // teardown later sweeps it: `record_killed` must not clobber it.
    record_exit_value(exited, -1, false);
    record_killed(exited);
    match join(exited) {
        // -1 is `PTHREAD_CANCELED` at the C level: a perfectly legal
        // exit value that the old value-in-rax join ABI could not tell
        // apart from an error code.
        Ok(-1) => {}
        other => {
            serial_println!(
                "[thread]   FAIL: a real exit value must survive a later kill sweep, got {:?}",
                other
            );
            let mut outcomes = THREAD_OUTCOMES.lock();
            outcomes.remove(&exited);
            return Err(KernelError::InternalError);
        }
    }

    serial_println!("[thread]   Killed thread joins as Cancelled, not 0: OK");
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
        let mut ev = THREAD_OUTCOMES.lock();
        if ev.remove(&fake) != Some(ThreadOutcome::Exited(7)) {
            serial_println!(
                "[thread]   FAIL: joinable exit value should have been recorded"
            );
            return Err(KernelError::InternalError);
        }
    }

    // Detached: nothing is stored, so nothing leaks.
    record_exit_value(fake, 7, true);
    {
        let mut ev = THREAD_OUTCOMES.lock();
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
        let mut outcomes = THREAD_OUTCOMES.lock();
        outcomes.insert(task_id, ThreadOutcome::Exited(exit_value));
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
        let mut outcomes = THREAD_OUTCOMES.lock();
        match outcomes.remove(&task_id) {
            Some(ThreadOutcome::Exited(42)) => {} // Expected.
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
    // value is in THREAD_OUTCOMES.
    {
        let mut outcomes = THREAD_OUTCOMES.lock();
        match outcomes.remove(&target) {
            Some(ThreadOutcome::Exited(99)) => {} // Expected.
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

// ---------------------------------------------------------------------------
// Test 5b: the *blocking* join path
// ---------------------------------------------------------------------------

/// Set to 1 by the joiner task once `join()` has returned.
static BJ_DONE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
/// The value `join()` returned (`i64::MIN` if it returned an error).
static BJ_RESULT: core::sync::atomic::AtomicI64 = core::sync::atomic::AtomicI64::new(0);
/// Non-zero if the *target* task detected a protocol violation:
/// 1 = the joiner never registered, 2 = `join()` returned while the
/// target was still alive.
static BJ_FAIL: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
/// The error `join()` returned, as its `#[repr(i32)]` discriminant, or 0
/// if it succeeded (no `KernelError` variant is 0).
static BJ_ERR: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(0);

/// Join target: waits until the joiner has parked on it, then dies.
///
/// The handshake is deliberately **between the two spawned tasks**, not
/// with the task running the self-test: both spawned tasks run at
/// `DEFAULT_PRIORITY`, so while either is runnable the (lower-priority)
/// boot task is never scheduled.  Gating the target on a flag set by the
/// boot task would therefore livelock.
///
/// `arg != 0` records an exit value first (the `thread_exit_with_value`
/// path); `arg == 0` records a *kill* instead (the crash / `exit_group`
/// path, where the thread never passes through `thread_exit_with_value`
/// and the exception or teardown code stamps `record_killed` on it).
/// Either way the death is signalled only through `on_thread_exit`,
/// which is where the join wake lives — neither `record_exit_value` nor
/// `record_killed` wakes anybody, so the wake is still being tested.
extern "C" fn bj_target_entry(arg: u64) {
    use core::sync::atomic::Ordering::SeqCst;
    let task_id = sched::current_task_id();

    // Wait for the joiner to register itself on us — that is the exact
    // moment it is about to park.
    let mut spins = 0u32;
    while bj_waiter_of(task_id).is_none() && spins < 100_000 {
        sched::yield_now();
        spins = spins.saturating_add(1);
    }
    if bj_waiter_of(task_id).is_none() {
        BJ_FAIL.store(1, SeqCst);
        return;
    }

    // We are still alive, so a correct `join()` must still be parked.
    // Give it many scheduling opportunities to prove it stays parked
    // (this is what a spurious wake would break).
    for _ in 0..256 {
        sched::yield_now();
    }
    if BJ_DONE.load(SeqCst) != 0 {
        BJ_FAIL.store(2, SeqCst);
        return;
    }

    if arg != 0 {
        record_exit_value(task_id, 77, false);
    } else {
        record_killed(task_id);
    }
    on_thread_exit(task_id);
}

/// Joiner: blocks in `join(target)` and publishes the outcome.
///
/// `join()` returns a `Result`, and both halves matter to this test, so
/// the two are published separately: `BJ_ERR` is 0 on success and the
/// error's stable ABI discriminant otherwise, with `BJ_RESULT` only
/// meaningful when `BJ_ERR` is 0.  (Folding an error into the value —
/// the old `unwrap_or(i64::MIN)` — is exactly the ambiguity that made
/// the value-in-rax join ABI unfixable; see design-decisions.md §127.)
extern "C" fn bj_joiner_entry(target: u64) {
    use core::sync::atomic::Ordering::SeqCst;
    match join(target) {
        Ok(value) => {
            BJ_RESULT.store(value, SeqCst);
            BJ_ERR.store(0, SeqCst);
        }
        Err(e) => {
            BJ_RESULT.store(0, SeqCst);
            BJ_ERR.store(e as i32, SeqCst);
        }
    }
    BJ_DONE.store(1, SeqCst);
}

/// Which task, if any, is currently registered as joining on `target`?
fn bj_waiter_of(target: TaskId) -> Option<TaskId> {
    let waiters = THREAD_JOIN_WAITERS.lock();
    waiters.get(&target).copied()
}

/// Test 5b: `join()` blocks until the target *really* exits.
///
/// Regression test for two defects:
///
/// 1. `join()` used to park with a single bare `block_current()`, so a
///    spurious wake (a stale `pending_wake` token — see known-issues.md
///    `BUG-TRYWAKE-FALSE-CONFLATES-CONTENTION`) could make it return a
///    bogus exit value while the target was still running.  The target
///    itself asserts the joiner is still parked (256 yields after the
///    registration appears, `BJ_DONE` must still be 0).
/// 2. The join wake used to live in `thread_exit_with_value`, so a
///    thread that died by any other route (unhandled exception,
///    `exit_group`, process teardown) never released its joiner.  Both
///    phases kill the target through `on_thread_exit` alone; phase 2
///    additionally records no exit value, as a crash would not.
fn test_blocking_join() -> KernelResult<()> {
    run_blocking_join_phase(true, Ok(77))?;
    // A crashed thread is *killed*, so it must not join as a normal
    // return — that conflation is `B-PTHREAD-CHILD-JUMPS-TO-GARBAGE`
    // defect 2, where a lost worker's contribution silently vanished
    // from the caller's total and the program still exited 0.
    run_blocking_join_phase(false, Err(KernelError::Cancelled))
}

/// Tear down a failed phase's fixture and fail.
///
/// The target releases the joiner on *every* exit path it takes, so by
/// the time the boot task runs again neither task can still be parked;
/// this only reaps the bookkeeping.
fn bj_fail(pid: ProcessId, target: TaskId, joiner: TaskId) -> KernelResult<()> {
    // Release a joiner still parked on a target that bailed out early
    // (BJ_FAIL paths return without calling `on_thread_exit`).
    on_thread_exit(target);
    for _ in 0..256 {
        sched::yield_now();
    }
    on_thread_exit(joiner);
    pcb::destroy(pid);
    Err(KernelError::InternalError)
}

fn run_blocking_join_phase(record_value: bool, expected: KernelResult<i64>) -> KernelResult<()> {
    use core::sync::atomic::Ordering::SeqCst;

    BJ_DONE.store(0, SeqCst);
    BJ_FAIL.store(0, SeqCst);
    BJ_RESULT.store(i64::MIN, SeqCst);
    BJ_ERR.store(i32::MIN, SeqCst);

    let pid = pcb::create("thread-test-blocking-join", 0);
    let target = spawn(
        pid,
        b"bj-target",
        sched::task::DEFAULT_PRIORITY,
        bj_target_entry,
        u64::from(record_value),
    )?;
    let joiner = spawn(
        pid,
        b"bj-joiner",
        sched::task::DEFAULT_PRIORITY,
        bj_joiner_entry,
        target,
    )?;

    // Both spawned tasks outrank the boot task, so this loop only makes
    // progress once they are done (or the joiner is stuck parked with
    // the target gone, which the bound below catches).
    let mut spins = 0u32;
    while BJ_DONE.load(SeqCst) == 0 && BJ_FAIL.load(SeqCst) == 0 && spins < 100_000 {
        sched::yield_now();
        spins = spins.saturating_add(1);
    }

    match BJ_FAIL.load(SeqCst) {
        0 => {}
        1 => {
            serial_println!(
                "[thread]   FAIL: joiner {} never registered on target {}",
                joiner, target
            );
            return bj_fail(pid, target, joiner);
        }
        code => {
            serial_println!(
                "[thread]   FAIL: join() returned while the target was still alive (code {})",
                code
            );
            return bj_fail(pid, target, joiner);
        }
    }

    if BJ_DONE.load(SeqCst) == 0 {
        serial_println!(
            "[thread]   FAIL: joiner still blocked {} yields after the target exited",
            spins
        );
        return bj_fail(pid, target, joiner);
    }

    // Compare on the wire form (value + discriminant) rather than
    // rebuilding a `KernelError`, so an unexpected code prints as itself
    // instead of being forced into the nearest known variant.
    let err = BJ_ERR.load(SeqCst);
    let value = BJ_RESULT.load(SeqCst);
    let matched = match expected {
        Ok(want) => err == 0 && value == want,
        Err(want) => err == want as i32,
    };
    if !matched {
        serial_println!(
            "[thread]   FAIL: join() returned (value {}, err {}), expected {:?} (record_value={})",
            value, err, expected, record_value
        );
        return bj_fail(pid, target, joiner);
    }

    on_thread_exit(target);
    on_thread_exit(joiner);
    pcb::destroy(pid);
    serial_println!(
        "[thread]   Blocking join (exit value recorded: {}): OK",
        record_value
    );
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
