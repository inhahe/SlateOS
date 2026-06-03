//! Linux time namespace constants.
//!
//! Time namespaces (added in Linux 5.6) allow per-namespace
//! offsets for CLOCK_MONOTONIC and CLOCK_BOOTTIME. This enables
//! containers to have independent uptime values after checkpoint
//! and restore (CRIU).

// ---------------------------------------------------------------------------
// Clone flags
// ---------------------------------------------------------------------------

/// Create new time namespace (for children).
pub const CLONE_NEWTIME: u64 = 0x00000080;

// ---------------------------------------------------------------------------
// /proc interface
// ---------------------------------------------------------------------------

/// Time namespace proc link.
pub const PROC_NS_TIME: &str = "ns/time";
/// Time namespace for children.
pub const PROC_NS_TIME_FOR_CHILDREN: &str = "ns/time_for_children";
/// Time offsets file (writable before any process enters).
pub const PROC_TIMENS_OFFSETS: &str = "timens_offsets";

// ---------------------------------------------------------------------------
// Clocks affected by time namespace
// ---------------------------------------------------------------------------

/// CLOCK_MONOTONIC can be offset.
pub const TIMENS_CLOCK_MONOTONIC: u32 = 1;
/// CLOCK_BOOTTIME can be offset.
pub const TIMENS_CLOCK_BOOTTIME: u32 = 7;

// ---------------------------------------------------------------------------
// Offset format
// ---------------------------------------------------------------------------

/// Format string for timens_offsets entries: "clock_id seconds nanoseconds".
pub const TIMENS_OFFSET_FMT: &str = "%d %lld %lu";

/// Maximum nanoseconds value in offset.
pub const TIMENS_NSEC_MAX: u64 = 999_999_999;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Number of clocks that support time namespace offsets.
pub const TIMENS_NUM_CLOCKS: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clone_newtime() {
        assert_eq!(CLONE_NEWTIME, 0x00000080);
        assert!((CLONE_NEWTIME as u64).is_power_of_two());
    }

    #[test]
    fn test_clone_no_overlap() {
        let other_ns: &[u64] = &[
            0x10000000, // CLONE_NEWUSER
            0x20000000, // CLONE_NEWPID
            0x00020000, // CLONE_NEWNS
            0x40000000, // CLONE_NEWNET
            0x08000000, // CLONE_NEWIPC
            0x04000000, // CLONE_NEWUTS
            0x02000000, // CLONE_NEWCGROUP
        ];
        for flag in other_ns {
            assert_ne!(CLONE_NEWTIME, *flag);
        }
    }

    #[test]
    fn test_proc_paths_distinct() {
        let paths = [PROC_NS_TIME, PROC_NS_TIME_FOR_CHILDREN, PROC_TIMENS_OFFSETS];
        for i in 0..paths.len() {
            for j in (i + 1)..paths.len() {
                assert_ne!(paths[i], paths[j]);
            }
        }
    }

    #[test]
    fn test_clocks_distinct() {
        assert_ne!(TIMENS_CLOCK_MONOTONIC, TIMENS_CLOCK_BOOTTIME);
    }

    #[test]
    fn test_nsec_max() {
        assert_eq!(TIMENS_NSEC_MAX, 999_999_999);
    }

    #[test]
    fn test_num_clocks() {
        assert_eq!(TIMENS_NUM_CLOCKS, 2);
    }
}
