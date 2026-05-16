//! `<linux/cgroup/pids.h>` — PIDs cgroup controller constants.
//!
//! The PIDs controller limits the number of processes (tasks)
//! that can be created within a cgroup hierarchy. This prevents
//! fork bombs and runaway process creation from consuming all
//! available PID space.

// ---------------------------------------------------------------------------
// Cgroup v2 interface files
// ---------------------------------------------------------------------------

/// Maximum number of PIDs allowed.
pub const PIDS_MAX: &str = "pids.max";
/// Current number of PIDs.
pub const PIDS_CURRENT: &str = "pids.current";
/// Peak PID usage.
pub const PIDS_PEAK: &str = "pids.peak";
/// PID events (hit max).
pub const PIDS_EVENTS: &str = "pids.events";

// ---------------------------------------------------------------------------
// Special values
// ---------------------------------------------------------------------------

/// Unlimited PIDs (written as "max" to pids.max).
pub const PIDS_MAX_STR: &str = "max";

/// Sentinel value for unlimited PIDs in kernel.
pub const PIDS_UNLIMITED: i64 = -1;

// ---------------------------------------------------------------------------
// Event names
// ---------------------------------------------------------------------------

/// Event: PID allocation was rejected due to limit.
pub const PIDS_EVENT_MAX: &str = "max";

// ---------------------------------------------------------------------------
// Default limits
// ---------------------------------------------------------------------------

/// Default PID maximum (matches kernel default /proc/sys/kernel/pid_max).
pub const PID_MAX_DEFAULT: u32 = 32768;
/// Maximum configurable PID limit.
pub const PID_MAX_LIMIT: u32 = 4_194_304;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interface_files_distinct() {
        let files = [PIDS_MAX, PIDS_CURRENT, PIDS_PEAK, PIDS_EVENTS];
        for i in 0..files.len() {
            for j in (i + 1)..files.len() {
                assert_ne!(files[i], files[j]);
            }
        }
    }

    #[test]
    fn test_interface_files_have_prefix() {
        let files = [PIDS_MAX, PIDS_CURRENT, PIDS_PEAK, PIDS_EVENTS];
        for file in &files {
            assert!(file.starts_with("pids."), "{}", file);
        }
    }

    #[test]
    fn test_unlimited_sentinel() {
        assert!(PIDS_UNLIMITED < 0);
    }

    #[test]
    fn test_pid_max_defaults() {
        assert_eq!(PID_MAX_DEFAULT, 32768);
        assert!(PID_MAX_DEFAULT < PID_MAX_LIMIT);
    }

    #[test]
    fn test_pid_max_limit() {
        assert_eq!(PID_MAX_LIMIT, 4_194_304);
    }
}
