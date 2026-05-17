//! `<linux/idle_inject.h>` — CPU idle injection framework constants.
//!
//! Idle injection forces CPUs to enter idle states for thermal
//! throttling without reducing the operating frequency. The thermal
//! framework's power_allocator governor uses idle injection when
//! devices exceed their thermal budget. A configurable duty cycle
//! (idle_duration_us / run_duration_us) controls the effective
//! performance reduction. This approach maintains frequency (good
//! for latency-sensitive workloads) while reducing average power.

// ---------------------------------------------------------------------------
// Idle injection states
// ---------------------------------------------------------------------------

/// Idle injection is stopped (not active).
pub const IDLE_INJECT_STOPPED: u32 = 0;
/// Idle injection is running (actively throttling).
pub const IDLE_INJECT_RUNNING: u32 = 1;
/// Idle injection should stop (stop requested, winding down).
pub const IDLE_INJECT_STOP_PENDING: u32 = 2;

// ---------------------------------------------------------------------------
// Idle injection timing defaults (microseconds)
// ---------------------------------------------------------------------------

/// Default idle duration (microseconds) — time forced idle per cycle.
pub const IDLE_INJECT_DEFAULT_IDLE_US: u32 = 1000;
/// Default run duration (microseconds) — time allowed to run per cycle.
pub const IDLE_INJECT_DEFAULT_RUN_US: u32 = 1000;
/// Minimum idle duration (microseconds).
pub const IDLE_INJECT_MIN_IDLE_US: u32 = 100;
/// Maximum idle duration (microseconds).
pub const IDLE_INJECT_MAX_IDLE_US: u32 = 100_000;
/// Minimum run duration (microseconds).
pub const IDLE_INJECT_MIN_RUN_US: u32 = 100;
/// Maximum run duration (microseconds).
pub const IDLE_INJECT_MAX_RUN_US: u32 = 100_000;

// ---------------------------------------------------------------------------
// Idle injection duty cycle limits
// ---------------------------------------------------------------------------

/// Minimum duty cycle percentage (min throttling).
pub const IDLE_INJECT_MIN_DUTY_CYCLE: u32 = 0;
/// Maximum duty cycle percentage (max throttling).
pub const IDLE_INJECT_MAX_DUTY_CYCLE: u32 = 100;

// ---------------------------------------------------------------------------
// Idle injection flags
// ---------------------------------------------------------------------------

/// Align injection across CPUs (all idle together for power saving).
pub const IDLE_INJECT_FLAG_ALIGNED: u32 = 1 << 0;
/// Use deepest idle state available.
pub const IDLE_INJECT_FLAG_DEEP_IDLE: u32 = 1 << 1;
/// Inject on all CPUs in the cluster.
pub const IDLE_INJECT_FLAG_CLUSTER_WIDE: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_states_distinct() {
        let states = [
            IDLE_INJECT_STOPPED, IDLE_INJECT_RUNNING,
            IDLE_INJECT_STOP_PENDING,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_timing_defaults() {
        assert_eq!(IDLE_INJECT_DEFAULT_IDLE_US, IDLE_INJECT_DEFAULT_RUN_US);
        assert!(IDLE_INJECT_MIN_IDLE_US <= IDLE_INJECT_DEFAULT_IDLE_US);
        assert!(IDLE_INJECT_DEFAULT_IDLE_US <= IDLE_INJECT_MAX_IDLE_US);
        assert!(IDLE_INJECT_MIN_RUN_US <= IDLE_INJECT_DEFAULT_RUN_US);
        assert!(IDLE_INJECT_DEFAULT_RUN_US <= IDLE_INJECT_MAX_RUN_US);
    }

    #[test]
    fn test_duty_cycle_range() {
        assert_eq!(IDLE_INJECT_MIN_DUTY_CYCLE, 0);
        assert_eq!(IDLE_INJECT_MAX_DUTY_CYCLE, 100);
        assert!(IDLE_INJECT_MIN_DUTY_CYCLE < IDLE_INJECT_MAX_DUTY_CYCLE);
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            IDLE_INJECT_FLAG_ALIGNED,
            IDLE_INJECT_FLAG_DEEP_IDLE,
            IDLE_INJECT_FLAG_CLUSTER_WIDE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
