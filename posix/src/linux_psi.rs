//! `<linux/psi.h>` — Pressure Stall Information constants.
//!
//! PSI tracks resource pressure (CPU, memory, I/O) across the
//! system and per cgroup. Applications can poll /proc/pressure/*
//! or use PSI triggers to get notifications when pressure exceeds
//! thresholds. Used by Android LMKD and systemd-oomd.

// ---------------------------------------------------------------------------
// PSI resource types
// ---------------------------------------------------------------------------

/// CPU pressure.
pub const PSI_CPU: u32 = 0;
/// Memory pressure.
pub const PSI_MEM: u32 = 1;
/// I/O pressure.
pub const PSI_IO: u32 = 2;
/// IRQ pressure.
pub const PSI_IRQ: u32 = 3;
/// Number of PSI resource types.
pub const NR_PSI_RESOURCES: u32 = 4;

// ---------------------------------------------------------------------------
// PSI states
// ---------------------------------------------------------------------------

/// Some tasks are stalled.
pub const PSI_SOME: u32 = 0;
/// All non-idle tasks are stalled (full pressure).
pub const PSI_FULL: u32 = 1;
/// Number of PSI states.
pub const NR_PSI_STATES: u32 = 2;

// ---------------------------------------------------------------------------
// PSI aggregation windows
// ---------------------------------------------------------------------------

/// 10-second average.
pub const PSI_AVG10: u32 = 0;
/// 60-second average.
pub const PSI_AVG60: u32 = 1;
/// 300-second average.
pub const PSI_AVG300: u32 = 2;
/// Number of averaging windows.
pub const NR_PSI_AVG: u32 = 3;

// ---------------------------------------------------------------------------
// PSI trigger thresholds (microseconds per window)
// ---------------------------------------------------------------------------

/// Minimum trigger threshold (1ms).
pub const PSI_TRIG_MIN_WIN_US: u64 = 500_000;
/// Maximum trigger threshold.
pub const PSI_TRIG_MAX_WIN_US: u64 = 10_000_000;

// ---------------------------------------------------------------------------
// PSI file names
// ---------------------------------------------------------------------------

/// CPU pressure file.
pub const PSI_FILE_CPU: &str = "/proc/pressure/cpu";
/// Memory pressure file.
pub const PSI_FILE_MEM: &str = "/proc/pressure/memory";
/// I/O pressure file.
pub const PSI_FILE_IO: &str = "/proc/pressure/io";
/// IRQ pressure file.
pub const PSI_FILE_IRQ: &str = "/proc/pressure/irq";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resources_distinct() {
        let res = [PSI_CPU, PSI_MEM, PSI_IO, PSI_IRQ];
        for i in 0..res.len() {
            for j in (i + 1)..res.len() {
                assert_ne!(res[i], res[j]);
            }
        }
    }

    #[test]
    fn test_resources_below_nr() {
        assert!(PSI_CPU < NR_PSI_RESOURCES);
        assert!(PSI_MEM < NR_PSI_RESOURCES);
        assert!(PSI_IO < NR_PSI_RESOURCES);
        assert!(PSI_IRQ < NR_PSI_RESOURCES);
    }

    #[test]
    fn test_states_distinct() {
        assert_ne!(PSI_SOME, PSI_FULL);
        assert!(PSI_SOME < NR_PSI_STATES);
        assert!(PSI_FULL < NR_PSI_STATES);
    }

    #[test]
    fn test_avg_windows_distinct() {
        let avgs = [PSI_AVG10, PSI_AVG60, PSI_AVG300];
        for i in 0..avgs.len() {
            for j in (i + 1)..avgs.len() {
                assert_ne!(avgs[i], avgs[j]);
            }
        }
    }

    #[test]
    fn test_trigger_range() {
        assert!(PSI_TRIG_MIN_WIN_US < PSI_TRIG_MAX_WIN_US);
    }

    #[test]
    fn test_file_names_distinct() {
        let files = [PSI_FILE_CPU, PSI_FILE_MEM, PSI_FILE_IO, PSI_FILE_IRQ];
        for i in 0..files.len() {
            for j in (i + 1)..files.len() {
                assert_ne!(files[i], files[j]);
            }
        }
    }
}
