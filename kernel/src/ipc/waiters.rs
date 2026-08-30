//! Shared waiter bookkeeping for blocking IPC objects.
//!
//! Every blocking IPC object in this module tree (pipes, eventfds, stream
//! sockets, timerfds) needs the same thing: remember which tasks are parked on
//! a given end of the object, and wake them when the object's state changes.
//!
//! Each of them originally spelled that as a **single** `Option<TaskId>` slot
//! per end.  That representation cannot hold more than one waiter, so when a
//! second task parked on the same end its assignment silently overwrote the
//! first — and the overwritten task was then never woken by anyone, parking
//! forever (`BUG-PIPE-SINGLE-WAITER-SLOT` in `known-issues.md`).  Several
//! waiters per end is not exotic: `dup()` and process spawn hand the same
//! object to multiple processes, `socketpair` endpoints are routinely shared,
//! and an eventfd is a many-waiter primitive by construction.
//!
//! [`WaiterSet`] is the one representation that cannot lose a waiter, and
//! factoring it out here keeps the four objects from drifting apart in their
//! wake semantics.
//!
//! # Usage contract
//!
//! Each object owns its own lock (the pipe table, the eventfd table, …) and
//! that lock is always taken *before* the scheduler lock.  The set is
//! therefore designed to be mutated entirely inside the object's critical
//! section, with the actual wake happening after the lock is dropped:
//!
//! ```ignore
//! let waiters = obj.readers.take_all();
//! drop(table);          // release the object lock first
//! wake_all(waiters);    // ... then take the scheduler lock
//! ```
//!
//! Park loops must call [`WaiterSet::remove`] at the *top* of every iteration,
//! inside the lock, before deciding what to do.  Deregistering only on the
//! signal path (as the original code did) leaves a stale entry behind on the
//! timeout path, and a stale entry names a task that is no longer parked —
//! once task IDs are recycled, that entry wakes an unrelated task.

use alloc::vec::Vec;

use crate::sched::{self, task::TaskId};

/// The set of tasks parked on one end of a blocking IPC object.
///
/// Wake semantics are deliberately **wake-all**: every state change wakes
/// every waiter on the affected end, matching Linux (`fs/pipe.c` parks on a
/// non-exclusive wait queue, so `wake_up_interruptible_sync_poll()` wakes
/// every sleeper).  Waking one would be enough when the wake means "one unit
/// of work became available", but it is not enough for the permanent
/// broadcast conditions — EOF, EPIPE, peer shutdown — and it goes wrong even
/// for data if the single woken task leaves without consuming (a signal, or a
/// timeout expiring in the same instant).  Losers simply re-check under the
/// lock and park again.
pub struct WaiterSet {
    /// Parked task IDs.  Order is registration order; duplicates are rejected
    /// by [`insert`](Self::insert) so a task that re-parks after a spurious
    /// wake does not occupy two entries.
    tasks: Vec<TaskId>,
}

impl WaiterSet {
    /// Create an empty waiter set.
    pub const fn new() -> Self {
        Self { tasks: Vec::new() }
    }

    /// Register `task` as parked on this end.  Idempotent.
    pub fn insert(&mut self, task: TaskId) {
        if !self.tasks.contains(&task) {
            self.tasks.push(task);
        }
    }

    /// Deregister `task`.  No-op if it is not registered.
    ///
    /// Callers must do this on *every* path out of a park loop, not just the
    /// signal path — see the module docs.
    pub fn remove(&mut self, task: TaskId) {
        self.tasks.retain(|t| *t != task);
    }

    /// Take every waiter, leaving the set empty.
    ///
    /// The caller must drop the owning object's lock before waking them.
    pub fn take_all(&mut self) -> Vec<TaskId> {
        core::mem::take(&mut self.tasks)
    }
}

impl Default for WaiterSet {
    fn default() -> Self {
        Self::new()
    }
}

/// Wake every task in `tasks`.
///
/// Split out so call sites read as "drop the lock, then wake", which is the
/// ordering the IPC lock hierarchy requires.
pub fn wake_all(tasks: Vec<TaskId>) {
    for task_id in tasks {
        sched::wake(task_id);
    }
}

// ---------------------------------------------------------------------------
// Signal-interruptible parking
// ---------------------------------------------------------------------------
//
// Every one of these objects is *slow* — it has no inherent timeout, so a task
// blocked on one could wait forever. A blocking operation interrupted by a
// deliverable signal must therefore wake and return, so the signal's handler
// can run (the syscall layer maps the resulting `Interrupted` to the
// ERESTARTSYS sentinel). Without that, a process blocked on an empty pipe, a
// full eventfd or a quiet socket could never be killed.
//
// The preamble that achieves it is three lines long and race-sensitive, and
// five modules (pipe, eventfd, stream_socket, timerfd, futex) each grew their
// own byte-identical copy. Five copies of a race-sensitive idiom is five
// chances for one to be "fixed" alone and drift — which is exactly how
// `BUG-PIPE-SINGLE-WAITER-SLOT` came to exist in four places at once. They live
// here now, next to the waiter set they are always used with.

/// The owning user process id of the current task, or `0` for a kernel task.
///
/// Blocking IPC waits are interruptible by signals only for user processes;
/// kernel tasks (`pid == 0`) have no signal state and park uninterruptibly.
#[must_use]
pub fn current_user_pid() -> u64 {
    crate::proc::thread::owner_process(sched::current_task_id()).unwrap_or(0)
}

/// `true` if a deliverable (unblocked) signal is pending for `pid`.
///
/// Always `false` for `pid == 0` (kernel task — no signal context).
#[must_use]
pub fn deliverable_signal_pending(pid: u64) -> bool {
    pid != 0 && crate::proc::signal::has_pending_in_mask(pid, !crate::proc::signal::blocked(pid))
}

/// Park the current task on a blocking IPC object, interruptibly for user
/// processes.
///
/// For a user process this registers a signal-waiter (so `set_pending` wakes
/// the park when a deliverable signal arrives) using the register-then-recheck
/// idiom that closes the post-before-park race, blocks, then deregisters.
/// Kernel tasks park uninterruptibly.
///
/// The caller's surrounding loop must, after this returns, re-acquire the
/// object's lock and re-evaluate **both** the object state and
/// [`deliverable_signal_pending`] — a signal wake is reported by the latter,
/// not by this function's return.
pub fn park_interruptible(pid: u64, task: TaskId) {
    if pid == 0 {
        sched::block_current();
        return;
    }
    let deliverable = !crate::proc::signal::blocked(pid);
    crate::proc::signal::register_signalfd_waiter(pid, task, deliverable);
    if crate::proc::signal::has_pending_in_mask(pid, deliverable) {
        // A signal arrived between enqueue and registration — don't block; the
        // caller's loop will observe the pending signal and return Interrupted.
        crate::proc::signal::deregister_signalfd_waiter(pid, task);
        return;
    }
    sched::block_current();
    crate::proc::signal::deregister_signalfd_waiter(pid, task);
}
