//! `<linux/sched.h>` — Additional scheduler constants.
//!
//! Supplementary scheduler constants covering scheduling
//! policies, flags, and CPU affinity parameters.

// ---------------------------------------------------------------------------
// Scheduling policies (SCHED_*)
// ---------------------------------------------------------------------------

/// Normal (CFS/EEVDF).
pub const SCHED_NORMAL: u32 = 0;
/// FIFO (real-time).
pub const SCHED_FIFO: u32 = 1;
/// Round-robin (real-time).
pub const SCHED_RR: u32 = 2;
/// Batch.
pub const SCHED_BATCH: u32 = 3;
/// Idle.
pub const SCHED_IDLE: u32 = 5;
/// Deadline.
pub const SCHED_DEADLINE: u32 = 6;
/// Extension.
pub const SCHED_EXT: u32 = 7;

// ---------------------------------------------------------------------------
// Scheduling flags (SCHED_FLAG_*)
// ---------------------------------------------------------------------------

/// Reset on fork.
pub const SCHED_FLAG_RESET_ON_FORK: u64 = 0x01;
/// Reclaim.
pub const SCHED_FLAG_RECLAIM: u64 = 0x02;
/// DL overrun.
pub const SCHED_FLAG_DL_OVERRUN: u64 = 0x04;
/// Keep policy.
pub const SCHED_FLAG_KEEP_POLICY: u64 = 0x08;
/// Keep params.
pub const SCHED_FLAG_KEEP_PARAMS: u64 = 0x10;
/// Util clamp min.
pub const SCHED_FLAG_UTIL_CLAMP_MIN: u64 = 0x20;
/// Util clamp max.
pub const SCHED_FLAG_UTIL_CLAMP_MAX: u64 = 0x40;
/// Util clamp (both min and max).
pub const SCHED_FLAG_UTIL_CLAMP: u64 = 0x60;
/// All flags.
pub const SCHED_FLAG_ALL: u64 = 0x7F;

// ---------------------------------------------------------------------------
// Scheduling priority ranges
// ---------------------------------------------------------------------------

/// Minimum RT priority.
pub const SCHED_PRIORITY_MIN: u32 = 1;
/// Maximum RT priority.
pub const SCHED_PRIORITY_MAX: u32 = 99;
/// Nice range: min.
pub const NICE_MIN: i32 = -20;
/// Nice range: max.
pub const NICE_MAX: i32 = 19;

// ---------------------------------------------------------------------------
// CPU affinity
// ---------------------------------------------------------------------------

/// Maximum CPUs supported.
pub const CPU_SETSIZE: u32 = 1024;

// ---------------------------------------------------------------------------
// Clone3 flags
// ---------------------------------------------------------------------------

/// Clear child TID.
pub const CLONE_CLEAR_SIGHAND: u64 = 0x100000000;
/// Into cgroup.
pub const CLONE_INTO_CGROUP: u64 = 0x200000000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policies_distinct() {
        let policies = [
            SCHED_NORMAL, SCHED_FIFO, SCHED_RR, SCHED_BATCH,
            SCHED_IDLE, SCHED_DEADLINE, SCHED_EXT,
        ];
        for i in 0..policies.len() {
            for j in (i + 1)..policies.len() {
                assert_ne!(policies[i], policies[j]);
            }
        }
    }

    #[test]
    fn test_flags_power_of_two() {
        let flags = [
            SCHED_FLAG_RESET_ON_FORK, SCHED_FLAG_RECLAIM,
            SCHED_FLAG_DL_OVERRUN, SCHED_FLAG_KEEP_POLICY,
            SCHED_FLAG_KEEP_PARAMS, SCHED_FLAG_UTIL_CLAMP_MIN,
            SCHED_FLAG_UTIL_CLAMP_MAX,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:02x} not power of two", f);
        }
    }

    #[test]
    fn test_util_clamp() {
        assert_eq!(
            SCHED_FLAG_UTIL_CLAMP,
            SCHED_FLAG_UTIL_CLAMP_MIN | SCHED_FLAG_UTIL_CLAMP_MAX
        );
    }

    #[test]
    fn test_priority_range() {
        assert!(SCHED_PRIORITY_MIN < SCHED_PRIORITY_MAX);
        assert_eq!(SCHED_PRIORITY_MIN, 1);
        assert_eq!(SCHED_PRIORITY_MAX, 99);
    }

    #[test]
    fn test_nice_range() {
        assert!(NICE_MIN < NICE_MAX);
        assert_eq!(NICE_MIN, -20);
        assert_eq!(NICE_MAX, 19);
    }

    #[test]
    fn test_cpu_setsize() {
        assert_eq!(CPU_SETSIZE, 1024);
    }
}
