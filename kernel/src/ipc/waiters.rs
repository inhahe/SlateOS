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
