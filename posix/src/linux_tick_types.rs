//! `<linux/tick.h>` — kernel tick and NO_HZ mode constants.
//!
//! The kernel tick is the periodic timer interrupt that drives
//! scheduling, timekeeping, and accounting. Modern kernels support
//! NO_HZ (tickless) modes where the tick is suppressed when the
//! CPU is idle or running a single task, saving power and reducing
//! overhead for latency-sensitive workloads.

// ---------------------------------------------------------------------------
// Tick modes (nohz_mode)
// ---------------------------------------------------------------------------

/// Tick is always active (legacy periodic mode).
pub const NOHZ_MODE_INACTIVE: u32 = 0;
/// Low-resolution NO_HZ: tick stops in idle only.
pub const NOHZ_MODE_LOWRES: u32 = 1;
/// High-resolution NO_HZ: tick stops in idle with hrtimer.
pub const NOHZ_MODE_HIGHRES: u32 = 2;

// ---------------------------------------------------------------------------
// Tick dependency types
// ---------------------------------------------------------------------------

/// Dependency from a specific task.
pub const TICK_DEP_BIT_POSIX_TIMER: u32 = 0;
/// Dependency from perf events.
pub const TICK_DEP_BIT_PERF_EVENTS: u32 = 1;
/// Dependency from scheduler.
pub const TICK_DEP_BIT_SCHED: u32 = 2;
/// Dependency from clocksource watchdog.
pub const TICK_DEP_BIT_CLOCK_UNSTABLE: u32 = 3;
/// Dependency from RCU.
pub const TICK_DEP_BIT_RCU: u32 = 4;
/// Dependency from RCU expedited.
pub const TICK_DEP_BIT_RCU_EXP: u32 = 5;

// ---------------------------------------------------------------------------
// Tick dependency masks
// ---------------------------------------------------------------------------

/// POSIX timer tick dependency.
pub const TICK_DEP_MASK_POSIX_TIMER: u32 = 1 << TICK_DEP_BIT_POSIX_TIMER;
/// Perf events tick dependency.
pub const TICK_DEP_MASK_PERF_EVENTS: u32 = 1 << TICK_DEP_BIT_PERF_EVENTS;
/// Scheduler tick dependency.
pub const TICK_DEP_MASK_SCHED: u32 = 1 << TICK_DEP_BIT_SCHED;
/// Clocksource watchdog tick dependency.
pub const TICK_DEP_MASK_CLOCK_UNSTABLE: u32 = 1 << TICK_DEP_BIT_CLOCK_UNSTABLE;
/// RCU tick dependency.
pub const TICK_DEP_MASK_RCU: u32 = 1 << TICK_DEP_BIT_RCU;
/// RCU expedited tick dependency.
pub const TICK_DEP_MASK_RCU_EXP: u32 = 1 << TICK_DEP_BIT_RCU_EXP;

// ---------------------------------------------------------------------------
// Common tick rates
// ---------------------------------------------------------------------------

/// Default CONFIG_HZ value (desktop / server).
pub const HZ_1000: u32 = 1000;
/// Common embedded CONFIG_HZ value.
pub const HZ_250: u32 = 250;
/// Low-latency CONFIG_HZ value.
pub const HZ_300: u32 = 300;
/// Minimal CONFIG_HZ value.
pub const HZ_100: u32 = 100;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nohz_modes_distinct() {
        assert_ne!(NOHZ_MODE_INACTIVE, NOHZ_MODE_LOWRES);
        assert_ne!(NOHZ_MODE_LOWRES, NOHZ_MODE_HIGHRES);
        assert_ne!(NOHZ_MODE_INACTIVE, NOHZ_MODE_HIGHRES);
    }

    #[test]
    fn test_dep_masks_from_bits() {
        assert_eq!(TICK_DEP_MASK_POSIX_TIMER, 1 << 0);
        assert_eq!(TICK_DEP_MASK_PERF_EVENTS, 1 << 1);
        assert_eq!(TICK_DEP_MASK_SCHED, 1 << 2);
        assert_eq!(TICK_DEP_MASK_CLOCK_UNSTABLE, 1 << 3);
        assert_eq!(TICK_DEP_MASK_RCU, 1 << 4);
        assert_eq!(TICK_DEP_MASK_RCU_EXP, 1 << 5);
    }

    #[test]
    fn test_dep_masks_no_overlap() {
        let masks = [
            TICK_DEP_MASK_POSIX_TIMER, TICK_DEP_MASK_PERF_EVENTS,
            TICK_DEP_MASK_SCHED, TICK_DEP_MASK_CLOCK_UNSTABLE,
            TICK_DEP_MASK_RCU, TICK_DEP_MASK_RCU_EXP,
        ];
        for i in 0..masks.len() {
            assert!(masks[i].is_power_of_two());
            for j in (i + 1)..masks.len() {
                assert_eq!(masks[i] & masks[j], 0);
            }
        }
    }

    #[test]
    fn test_hz_values() {
        assert!(HZ_100 < HZ_250);
        assert!(HZ_250 < HZ_300);
        assert!(HZ_300 < HZ_1000);
    }
}
