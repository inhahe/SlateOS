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
// Workload profiles
// ---------------------------------------------------------------------------

/// Predefined workload profiles that tune scheduler time slices.
///
/// From the design spec (§ "Workload profiles"):
/// > Workload profiles would just be named presets of these runtime
/// > parameters.  The user selects a profile, the OS applies the preset
/// > values, no recompile needed.
///
/// Each profile tunes the time slice formula `base + level * increment`
/// for the 32 priority levels.  At 100 Hz timer tick rate, 1 tick = 10 ms.
///
/// The numeric encoding matches the syscall argument (0–3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WorkloadProfile {
    /// Balanced for general desktop use.  Moderate time slices across
    /// all priorities; interactive tasks (detected via burst tracking)
    /// get a priority boost.  Good for mixed workloads: browsing,
    /// document editing, background builds.
    ///
    /// base=2 (20 ms), increment=1 → level 0: 20 ms, level 31: 330 ms.
    Desktop = 0,

    /// Database / server workloads.  Longer time slices to reduce
    /// context switch overhead and maximize throughput.  Tasks run
    /// longer before preemption, reducing scheduling jitter for
    /// sustained CPU-bound work.
    ///
    /// base=4 (40 ms), increment=2 → level 0: 40 ms, level 31: 660 ms.
    Server = 1,

    /// Software development workloads.  Many short-lived processes
    /// (compiler invocations, test runners, build scripts) benefit
    /// from quick scheduling.  Short base slices keep context-switch
    /// latency low for parallel `make -j` or `cargo build` runs.
    ///
    /// base=1 (10 ms), increment=1 → level 0: 10 ms, level 31: 320 ms.
    Development = 2,

    /// Gaming and real-time workloads.  Very short slices at high
    /// priorities for minimal input-to-frame latency.  Low-priority
    /// background tasks get generous slices to avoid starving them
    /// entirely, but the foreground game (high priority) preempts
    /// quickly.
    ///
    /// base=1 (10 ms), increment=2 → level 0: 10 ms, level 31: 630 ms.
    Gaming = 3,
}

impl WorkloadProfile {
    /// Try to convert a raw u8 to a profile.
    #[must_use]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Desktop),
            1 => Some(Self::Server),
            2 => Some(Self::Development),
            3 => Some(Self::Gaming),
            _ => None,
        }
    }

    /// Get the time slice base for this profile (in timer ticks).
    #[must_use]
    pub const fn base(self) -> u32 {
        match self {
            Self::Desktop     => 2,
            Self::Server      => 4,
            Self::Development => 1,
            Self::Gaming      => 1,
        }
    }

    /// Get the time slice increment per priority level.
    #[must_use]
    pub const fn increment(self) -> u32 {
        match self {
            Self::Desktop     => 1,
            Self::Server      => 2,
            Self::Development => 1,
            Self::Gaming      => 2,
        }
    }

    /// Human-readable name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Desktop     => "Desktop",
            Self::Server      => "Server",
            Self::Development => "Development",
            Self::Gaming      => "Gaming",
        }
    }
}

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

    /// Set the time slice for a specific priority level.
    ///
    /// `level` must be in `0..NUM_PRIORITIES` and `ticks` must be at
    /// least 1 (a zero-tick time slice would starve the task).
    ///
    /// Returns `true` on success, `false` if the level is out of range
    /// or ticks is 0.
    pub fn set_time_slice(&mut self, level: usize, ticks: u32) -> bool {
        if level >= NUM_PRIORITIES || ticks == 0 {
            return false;
        }
        self.time_slices[level] = ticks;
        true
    }

    /// Get the current time slice for a priority level (in timer ticks).
    ///
    /// Returns `None` if the level is out of range.
    #[must_use]
    pub fn time_slice(&self, level: usize) -> Option<u32> {
        self.time_slices.get(level).copied()
    }

    /// Reconfigure all time slices with a new base and increment.
    ///
    /// Formula: `time_slice[level] = base + level * increment`.
    /// Both `base` and `increment` must be >= 1.
    ///
    /// Returns `true` on success.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    pub fn reconfigure_slices(&mut self, base: u32, increment: u32) -> bool {
        if base == 0 {
            return false;
        }
        for (i, slot) in self.time_slices.iter_mut().enumerate() {
            *slot = base.saturating_add((i as u32).saturating_mul(increment));
        }
        true
    }

    /// Apply a workload profile preset.
    ///
    /// Reconfigures all time slices to the profile's base and increment.
    /// The currently-running task's remaining slice is NOT changed — the
    /// new configuration takes effect on the next `pick_next()`.
    pub fn apply_profile(&mut self, profile: WorkloadProfile) {
        // apply_profile delegates to reconfigure_slices, which always
        // succeeds because profile bases are all >= 1.
        let ok = self.reconfigure_slices(profile.base(), profile.increment());
        debug_assert!(ok, "WorkloadProfile base must be >= 1");
    }
}
