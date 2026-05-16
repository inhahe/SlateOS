//! Performance monitoring cgroup controller constants.
//!
//! The perf_event cgroup controller allows restricting and
//! filtering performance monitoring (PMU) events per cgroup.
//! This enables per-container performance accounting without
//! interference between tenants.

// ---------------------------------------------------------------------------
// Cgroup v2 interface
// ---------------------------------------------------------------------------

/// Perf events cgroup controller name.
pub const PERF_EVENT_CONTROLLER: &str = "perf_event";

// ---------------------------------------------------------------------------
// perf_event cgroup sysctl paths
// ---------------------------------------------------------------------------

/// Allow unprivileged users to use perf_event.
pub const SYSCTL_PERF_EVENT_PARANOID: &str = "kernel.perf_event_paranoid";
/// Maximum sample rate.
pub const SYSCTL_PERF_EVENT_MAX_SAMPLE_RATE: &str = "kernel.perf_event_max_sample_rate";
/// Maximum stack depth for perf.
pub const SYSCTL_PERF_EVENT_MAX_STACK: &str = "kernel.perf_event_max_stack";
/// Maximum number of perf events per context.
pub const SYSCTL_PERF_EVENT_MAX_CONTEXTS_PER_STACK: &str = "kernel.perf_event_max_contexts_per_stack";

// ---------------------------------------------------------------------------
// perf_event_paranoid levels
// ---------------------------------------------------------------------------

/// Disallow raw tracepoint access for unprivileged users.
pub const PERF_PARANOID_DISALLOW_RAW: i32 = 3;
/// Disallow CPU-wide events for unprivileged users.
pub const PERF_PARANOID_DISALLOW_CPU: i32 = 2;
/// Disallow kernel profiling for unprivileged users.
pub const PERF_PARANOID_DISALLOW_KERNEL: i32 = 1;
/// Allow everything.
pub const PERF_PARANOID_ALLOW_ALL: i32 = 0;
/// Allow even without being in perf_event cgroup (legacy compat).
pub const PERF_PARANOID_NO_RESTRICT: i32 = -1;

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

/// Default perf_event_paranoid level.
pub const PERF_PARANOID_DEFAULT: i32 = 2;

/// Default maximum sample rate (per second).
pub const PERF_MAX_SAMPLE_RATE_DEFAULT: u32 = 100_000;

/// Default maximum stack depth.
pub const PERF_MAX_STACK_DEFAULT: u32 = 127;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_controller_name() {
        assert_eq!(PERF_EVENT_CONTROLLER, "perf_event");
    }

    #[test]
    fn test_sysctl_paths_distinct() {
        let paths = [
            SYSCTL_PERF_EVENT_PARANOID,
            SYSCTL_PERF_EVENT_MAX_SAMPLE_RATE,
            SYSCTL_PERF_EVENT_MAX_STACK,
            SYSCTL_PERF_EVENT_MAX_CONTEXTS_PER_STACK,
        ];
        for i in 0..paths.len() {
            for j in (i + 1)..paths.len() {
                assert_ne!(paths[i], paths[j]);
            }
        }
    }

    #[test]
    fn test_paranoid_levels_distinct() {
        let levels = [
            PERF_PARANOID_DISALLOW_RAW,
            PERF_PARANOID_DISALLOW_CPU,
            PERF_PARANOID_DISALLOW_KERNEL,
            PERF_PARANOID_ALLOW_ALL,
            PERF_PARANOID_NO_RESTRICT,
        ];
        for i in 0..levels.len() {
            for j in (i + 1)..levels.len() {
                assert_ne!(levels[i], levels[j]);
            }
        }
    }

    #[test]
    fn test_paranoid_ordering() {
        assert!(PERF_PARANOID_NO_RESTRICT < PERF_PARANOID_ALLOW_ALL);
        assert!(PERF_PARANOID_ALLOW_ALL < PERF_PARANOID_DISALLOW_KERNEL);
        assert!(PERF_PARANOID_DISALLOW_KERNEL < PERF_PARANOID_DISALLOW_CPU);
        assert!(PERF_PARANOID_DISALLOW_CPU < PERF_PARANOID_DISALLOW_RAW);
    }

    #[test]
    fn test_defaults() {
        assert_eq!(PERF_PARANOID_DEFAULT, 2);
        assert!(PERF_MAX_SAMPLE_RATE_DEFAULT > 0);
        assert!(PERF_MAX_STACK_DEFAULT > 0);
    }
}
