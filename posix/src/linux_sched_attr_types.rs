//! `<linux/sched/types.h>` — sched_attr structure constants.
//!
//! The sched_attr structure (used by sched_setattr/sched_getattr)
//! is a versioned, extensible structure for setting all scheduling
//! parameters at once. It supersedes the older sched_setscheduler()
//! and sched_setparam() interfaces. The structure includes policy,
//! nice value, priority, deadline parameters, and utilization clamp
//! values — everything needed to fully describe a task's scheduling
//! requirements in one atomic operation.

// ---------------------------------------------------------------------------
// sched_attr structure sizes (for versioning)
// ---------------------------------------------------------------------------

/// Size of sched_attr v1 (original, policy + priority).
pub const SCHED_ATTR_SIZE_V1: u32 = 48;
/// Size of sched_attr v2 (added util clamp fields).
pub const SCHED_ATTR_SIZE_V2: u32 = 56;

// ---------------------------------------------------------------------------
// Utilization clamp values (SCHED_FLAG_UTIL_CLAMP_*)
// ---------------------------------------------------------------------------

/// Minimum utilization clamp value (0%).
pub const SCHED_UTIL_CLAMP_MIN: u32 = 0;
/// Maximum utilization clamp value (100% = 1024).
pub const SCHED_UTIL_CLAMP_MAX: u32 = 1024;
/// Bucket count for utilization clamp (internal).
pub const SCHED_UTIL_CLAMP_BUCKET_COUNT: u32 = 20;

// ---------------------------------------------------------------------------
// RT priority range
// ---------------------------------------------------------------------------

/// Minimum real-time priority.
pub const SCHED_RT_PRIO_MIN: u32 = 1;
/// Maximum real-time priority.
pub const SCHED_RT_PRIO_MAX: u32 = 99;

// ---------------------------------------------------------------------------
// sched_attr flags field values
// ---------------------------------------------------------------------------

/// No special flags.
pub const SCHED_ATTR_FLAGS_NONE: u32 = 0;

// ---------------------------------------------------------------------------
// Scheduling latency hints
// ---------------------------------------------------------------------------

/// No latency hint.
pub const SCHED_LATENCY_NONE: u32 = 0;
/// Low latency (prefer responsiveness over throughput).
pub const SCHED_LATENCY_LOW: u32 = 1;
/// Normal latency (balanced).
pub const SCHED_LATENCY_NORMAL: u32 = 2;
/// High latency tolerance (prefer throughput).
pub const SCHED_LATENCY_HIGH: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attr_sizes_increasing() {
        assert!(SCHED_ATTR_SIZE_V1 < SCHED_ATTR_SIZE_V2);
    }

    #[test]
    fn test_util_clamp_range() {
        assert!(SCHED_UTIL_CLAMP_MIN < SCHED_UTIL_CLAMP_MAX);
        assert_eq!(SCHED_UTIL_CLAMP_MAX, 1024);
    }

    #[test]
    fn test_rt_priority_range() {
        assert!(SCHED_RT_PRIO_MIN < SCHED_RT_PRIO_MAX);
        assert_eq!(SCHED_RT_PRIO_MIN, 1);
        assert_eq!(SCHED_RT_PRIO_MAX, 99);
    }

    #[test]
    fn test_latency_hints_distinct() {
        let hints = [
            SCHED_LATENCY_NONE, SCHED_LATENCY_LOW,
            SCHED_LATENCY_NORMAL, SCHED_LATENCY_HIGH,
        ];
        for i in 0..hints.len() {
            for j in (i + 1)..hints.len() {
                assert_ne!(hints[i], hints[j]);
            }
        }
    }
}
