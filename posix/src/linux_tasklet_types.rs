//! `<linux/interrupt.h>` (tasklet subset) — Tasklet constants.
//!
//! Tasklets are a simpler alternative to softirqs for deferred
//! interrupt handling. Unlike softirqs (which can run concurrently
//! on multiple CPUs), a tasklet is guaranteed to run on only one
//! CPU at a time and won't be re-entered. This simplifies locking.
//! Tasklets are dynamically allocated and scheduled via
//! tasklet_schedule(). They're being gradually replaced by threaded
//! IRQs and workqueues in modern drivers.

// ---------------------------------------------------------------------------
// Tasklet state flags
// ---------------------------------------------------------------------------

/// Tasklet is scheduled (will run soon).
pub const TASKLET_STATE_SCHED: u32 = 0;
/// Tasklet is currently running.
pub const TASKLET_STATE_RUN: u32 = 1;

// ---------------------------------------------------------------------------
// Tasklet priority levels
// ---------------------------------------------------------------------------

/// Normal priority tasklet (TASKLET_SOFTIRQ vector).
pub const TASKLET_NORMAL: u32 = 0;
/// High priority tasklet (HI_SOFTIRQ vector).
pub const TASKLET_HI: u32 = 1;

// ---------------------------------------------------------------------------
// Tasklet control flags
// ---------------------------------------------------------------------------

/// Tasklet is enabled (will execute when scheduled).
pub const TASKLET_ENABLED: u32 = 0;
/// Tasklet is disabled (schedule but don't execute).
pub const TASKLET_DISABLED: u32 = 1;

// ---------------------------------------------------------------------------
// Tasklet disable nesting depth
// ---------------------------------------------------------------------------

/// Maximum disable nesting depth.
pub const TASKLET_DISABLE_MAX: u32 = 65535;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_states_distinct() {
        assert_ne!(TASKLET_STATE_SCHED, TASKLET_STATE_RUN);
    }

    #[test]
    fn test_priorities_distinct() {
        assert_ne!(TASKLET_NORMAL, TASKLET_HI);
    }

    #[test]
    fn test_enable_disable_distinct() {
        assert_ne!(TASKLET_ENABLED, TASKLET_DISABLED);
    }

    #[test]
    fn test_max_nesting() {
        assert!(TASKLET_DISABLE_MAX > 0);
    }
}
