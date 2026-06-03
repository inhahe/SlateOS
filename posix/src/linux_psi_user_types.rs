//! `/proc/pressure/*` — Pressure Stall Information ABI.
//!
//! PSI (Linux 4.20+) exports CPU/memory/IO contention as percent-time
//! windows. systemd-oomd, kubelet's eviction manager, and Android's
//! lmkd read these files to make pre-OOM eviction decisions.
//! Userspace can also `write(2)` a poll trigger into one of these
//! files and then `poll(2)` for threshold crossings.

// ---------------------------------------------------------------------------
// Pressure files
// ---------------------------------------------------------------------------

pub const PROC_PRESSURE_DIR: &str = "/proc/pressure";
pub const PROC_PRESSURE_CPU: &str = "/proc/pressure/cpu";
pub const PROC_PRESSURE_MEMORY: &str = "/proc/pressure/memory";
pub const PROC_PRESSURE_IO: &str = "/proc/pressure/io";
pub const PROC_PRESSURE_IRQ: &str = "/proc/pressure/irq";

// ---------------------------------------------------------------------------
// Cgroup v2 PSI files (per-cgroup view)
// ---------------------------------------------------------------------------

pub const CGROUP_PSI_CPU: &str = "cpu.pressure";
pub const CGROUP_PSI_MEMORY: &str = "memory.pressure";
pub const CGROUP_PSI_IO: &str = "io.pressure";
pub const CGROUP_PSI_IRQ: &str = "irq.pressure";

// ---------------------------------------------------------------------------
// Line prefixes in the pressure files
// ---------------------------------------------------------------------------

/// "some" line — at least one task was stalled.
pub const PSI_LINE_SOME: &str = "some";
/// "full" line — every runnable task was stalled (no useful CPU progress).
pub const PSI_LINE_FULL: &str = "full";

// ---------------------------------------------------------------------------
// Poll-trigger limits enforced by the kernel
// ---------------------------------------------------------------------------

/// Minimum window the trigger can monitor (500 ms).
pub const PSI_WINDOW_MIN_US: u64 = 500_000;
/// Maximum window the trigger can monitor (10 s).
pub const PSI_WINDOW_MAX_US: u64 = 10_000_000;

/// Threshold must be > 0 and ≤ window.
pub const PSI_THRESHOLD_MIN_US: u64 = 0;

/// Hard cap on concurrent poll triggers per file (kernel default).
pub const PSI_TRIG_PER_FILE_MAX: u32 = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proc_pressure_paths_under_proc() {
        let p = [
            PROC_PRESSURE_CPU,
            PROC_PRESSURE_MEMORY,
            PROC_PRESSURE_IO,
            PROC_PRESSURE_IRQ,
        ];
        for path in p {
            assert!(path.starts_with(PROC_PRESSURE_DIR));
        }
    }

    #[test]
    fn test_cgroup_pressure_filenames_distinct() {
        let c = [
            CGROUP_PSI_CPU,
            CGROUP_PSI_MEMORY,
            CGROUP_PSI_IO,
            CGROUP_PSI_IRQ,
        ];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
            // All four end with .pressure.
            assert!(c[i].ends_with(".pressure"));
        }
    }

    #[test]
    fn test_some_and_full_line_prefixes() {
        assert_eq!(PSI_LINE_SOME, "some");
        assert_eq!(PSI_LINE_FULL, "full");
        assert_ne!(PSI_LINE_SOME, PSI_LINE_FULL);
    }

    #[test]
    fn test_window_bounds_match_kernel() {
        // PSI poll triggers are clamped to a 500 ms..10 s window.
        assert_eq!(PSI_WINDOW_MIN_US, 500_000);
        assert_eq!(PSI_WINDOW_MAX_US, 10_000_000);
        assert!(PSI_WINDOW_MIN_US < PSI_WINDOW_MAX_US);
        // Threshold > 0; 0 is the "no threshold" sentinel.
        assert_eq!(PSI_THRESHOLD_MIN_US, 0);
    }

    #[test]
    fn test_trig_per_file_cap_power_of_two() {
        // Default cap is 16 — same as PSI_T_MAX in include/linux/psi.h.
        assert_eq!(PSI_TRIG_PER_FILE_MAX, 16);
        assert!(PSI_TRIG_PER_FILE_MAX.is_power_of_two());
    }
}
