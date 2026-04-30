//! Priority round-robin scheduler.
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
//! ## Per-CPU Queues (NOT YET IMPLEMENTED)
//!
//! Currently uses a single global set of queues (adequate for single-
//! CPU boot).  Per-CPU queues with work stealing will be added when
//! SMP support is implemented.
//!
//! ## Time Slices
//!
//! Each priority level has a configurable time slice (in timer ticks).
//! Higher priorities get shorter slices for lower latency; lower
//! priorities get longer slices for better throughput.  Time slices
//! are not enforced until the timer interrupt is wired up.

use alloc::collections::VecDeque;
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
    pub fn has_ready(&self) -> bool {
        self.bitmap != 0
    }
}
