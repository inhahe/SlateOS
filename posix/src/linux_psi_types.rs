//! `<linux/psi.h>` — Pressure Stall Information (PSI) constants.
//!
//! PSI tracks resource pressure (CPU, memory, I/O) at the cgroup and
//! system level. "Some" pressure means at least one task is stalled;
//! "full" means all tasks are stalled. Userspace (systemd, Android
//! LMKD) uses PSI triggers to detect resource contention and react
//! (e.g., kill low-priority processes before OOM).

// ---------------------------------------------------------------------------
// PSI resource types
// ---------------------------------------------------------------------------

/// CPU pressure (tasks waiting for CPU time).
pub const PSI_CPU: u32 = 0;
/// Memory pressure (tasks stalled on memory allocation/reclaim).
pub const PSI_MEM: u32 = 1;
/// I/O pressure (tasks stalled on block I/O).
pub const PSI_IO: u32 = 2;
/// IRQ pressure (softirq/hardirq saturation).
pub const PSI_IRQ: u32 = 3;
/// Number of PSI resource types.
pub const PSI_NUM_RESOURCES: u32 = 4;

// ---------------------------------------------------------------------------
// PSI states (for each resource)
// ---------------------------------------------------------------------------

/// "some" — at least one task is stalled.
pub const PSI_SOME: u32 = 0;
/// "full" — all non-idle tasks are stalled.
pub const PSI_FULL: u32 = 1;
/// Number of PSI states per resource.
pub const PSI_NUM_STATES: u32 = 2;

// ---------------------------------------------------------------------------
// PSI trigger window sizes (microseconds)
// ---------------------------------------------------------------------------

/// Minimum trigger window (500ms).
pub const PSI_TRIG_MIN_WIN_US: u64 = 500_000;
/// Maximum trigger window (10 seconds).
pub const PSI_TRIG_MAX_WIN_US: u64 = 10_000_000;

// ---------------------------------------------------------------------------
// PSI poll constants
// ---------------------------------------------------------------------------

/// PSI trigger file is pollable (EPOLLPRI events).
pub const PSI_POLL_EVENTS: u32 = 0x0002;

// ---------------------------------------------------------------------------
// PSI averaging windows (in seconds, for /proc/pressure/* output)
// ---------------------------------------------------------------------------

/// 10-second average window.
pub const PSI_AVG10: u32 = 10;
/// 60-second average window.
pub const PSI_AVG60: u32 = 60;
/// 300-second average window.
pub const PSI_AVG300: u32 = 300;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_types_sequential() {
        assert_eq!(PSI_CPU, 0);
        assert_eq!(PSI_MEM, 1);
        assert_eq!(PSI_IO, 2);
        assert_eq!(PSI_IRQ, 3);
        assert_eq!(PSI_NUM_RESOURCES, 4);
    }

    #[test]
    fn test_resource_types_distinct() {
        let resources = [PSI_CPU, PSI_MEM, PSI_IO, PSI_IRQ];
        for i in 0..resources.len() {
            for j in (i + 1)..resources.len() {
                assert_ne!(resources[i], resources[j]);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        assert_ne!(PSI_SOME, PSI_FULL);
        assert_eq!(PSI_NUM_STATES, 2);
    }

    #[test]
    fn test_trigger_window_bounds() {
        assert!(PSI_TRIG_MIN_WIN_US < PSI_TRIG_MAX_WIN_US);
        assert_eq!(PSI_TRIG_MIN_WIN_US, 500_000);
        assert_eq!(PSI_TRIG_MAX_WIN_US, 10_000_000);
    }

    #[test]
    fn test_avg_windows_ordered() {
        assert!(PSI_AVG10 < PSI_AVG60);
        assert!(PSI_AVG60 < PSI_AVG300);
    }

    #[test]
    fn test_all_resources_within_count() {
        assert!(PSI_CPU < PSI_NUM_RESOURCES);
        assert!(PSI_MEM < PSI_NUM_RESOURCES);
        assert!(PSI_IO < PSI_NUM_RESOURCES);
        assert!(PSI_IRQ < PSI_NUM_RESOURCES);
    }
}
