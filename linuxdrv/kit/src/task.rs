//! The bookkeeping of a driver process's tasks: which may run, which wait,
//! which sleep until when, and whose turn it is.
//!
//! A driver process runs Linux's tasks -- its kernel threads, workqueue
//! workers, interrupt handlers -- as coroutines on its one thread
//! (design-decisions 1578 rule 5), each switched to only where Linux code
//! calls into the scheduler or returns. The stacks and the switching are
//! [`crate::sched`]'s; this is the part that can be tested on the host: a
//! table of slots, each free, runnable, blocked (until woken, or until a
//! deadline) or finished, picked round-robin.

/// Tasks a driver process may have at once.
pub const MAX_TASKS: usize = 64;

/// What a task's slot holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// No task.
    Free,
    /// May run.
    Runnable,
    /// Waits to be woken, or until its deadline (nanoseconds of the
    /// monotonic clock) if it has one.
    Blocked { until: Option<u64> },
    /// Its function returned; its stack is to be given back.
    Finished,
}

/// The table.
pub struct Table {
    states: [State; MAX_TASKS],
    /// Where the round-robin looks first.
    next: usize,
}

impl Default for Table {
    fn default() -> Self {
        Self::new()
    }
}

impl Table {
    /// No tasks.
    #[must_use]
    pub const fn new() -> Self {
        Table {
            states: [State::Free; MAX_TASKS],
            next: 0,
        }
    }

    /// The state of task `i`; `Free` for no such slot.
    #[must_use]
    pub fn state(&self, i: usize) -> State {
        self.states.get(i).copied().unwrap_or(State::Free)
    }

    fn set(&mut self, i: usize, state: State) {
        if let Some(s) = self.states.get_mut(i) {
            *s = state;
        }
    }

    /// A free slot, made runnable: its index; `None` when all are taken.
    pub fn add(&mut self) -> Option<usize> {
        let i = self.states.iter().position(|&s| s == State::Free)?;
        self.set(i, State::Runnable);
        Some(i)
    }

    /// The next runnable task, round-robin from the one after the last
    /// picked, so that no runnable task waits behind another twice.
    pub fn pick(&mut self) -> Option<usize> {
        let i = (0..MAX_TASKS)
            .map(|k| self.next.wrapping_add(k) % MAX_TASKS)
            .find(|&i| self.state(i) == State::Runnable)?;
        self.next = i.wrapping_add(1) % MAX_TASKS;
        Some(i)
    }

    /// Task `i` stops being runnable: until woken, or until `until`.
    pub fn block(&mut self, i: usize, until: Option<u64>) {
        if self.state(i) == State::Runnable {
            self.set(i, State::Blocked { until });
        }
    }

    /// Task `i`, if blocked, may run again (a runnable or finished task is
    /// left as it is).
    pub fn wake(&mut self, i: usize) {
        if matches!(self.state(i), State::Blocked { .. }) {
            self.set(i, State::Runnable);
        }
    }

    /// Every task whose deadline is at or before `now` wakes.
    pub fn wake_expired(&mut self, now: u64) {
        for s in &mut self.states {
            if let State::Blocked { until: Some(t) } = *s
                && t <= now
            {
                *s = State::Runnable;
            }
        }
    }

    /// The earliest deadline a blocked task has, if any has one.
    #[must_use]
    pub fn next_deadline(&self) -> Option<u64> {
        self.states
            .iter()
            .filter_map(|s| match s {
                State::Blocked { until } => *until,
                _ => None,
            })
            .min()
    }

    /// Task `i`'s function returned.
    pub fn finish(&mut self, i: usize) {
        if self.state(i) != State::Free {
            self.set(i, State::Finished);
        }
    }

    /// Task `i`'s slot is free again (its stack given back).
    pub fn free(&mut self, i: usize) {
        self.set(i, State::Free);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn runnable_tasks_take_turns() {
        let mut t = Table::new();
        let (a, b, c) = (t.add().unwrap(), t.add().unwrap(), t.add().unwrap());
        let order: Vec<usize> = (0..6).map(|_| t.pick().unwrap()).collect();
        assert_eq!(order, [a, b, c, a, b, c]);
        t.block(b, None);
        let order: Vec<usize> = (0..4).map(|_| t.pick().unwrap()).collect();
        assert_eq!(order, [a, c, a, c]);
    }

    #[test]
    fn a_blocked_task_runs_again_only_when_woken_or_its_time_comes() {
        let mut t = Table::new();
        let a = t.add().unwrap();
        let b = t.add().unwrap();
        t.block(a, None);
        t.block(b, Some(500));
        assert_eq!(t.pick(), None);
        assert_eq!(t.next_deadline(), Some(500));
        t.wake_expired(499);
        assert_eq!(t.state(b), State::Blocked { until: Some(500) });
        t.wake_expired(500);
        assert_eq!(t.state(b), State::Runnable);
        assert_eq!(t.state(a), State::Blocked { until: None }, "no deadline");
        assert_eq!(t.pick(), Some(b));
        assert_eq!(t.next_deadline(), None, "a's wait has no deadline");
        t.wake(a);
        assert_eq!(t.state(a), State::Runnable);
        t.block(a, Some(9));
        t.wake(a);
        assert_eq!(t.state(a), State::Runnable, "woken before its deadline");
    }

    #[test]
    fn a_finished_task_is_not_woken_and_its_slot_comes_back() {
        let mut t = Table::new();
        let a = t.add().unwrap();
        t.finish(a);
        t.wake(a);
        t.block(a, None);
        assert_eq!(t.state(a), State::Finished);
        assert_eq!(t.pick(), None);
        t.free(a);
        assert_eq!(t.add(), Some(a));
    }

    #[test]
    fn the_slots_run_out_and_say_so() {
        let mut t = Table::new();
        for _ in 0..MAX_TASKS {
            assert!(t.add().is_some());
        }
        assert_eq!(t.add(), None);
        assert_eq!(t.state(MAX_TASKS), State::Free, "past the table");
    }
}
