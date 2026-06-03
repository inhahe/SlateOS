//! `<linux/perf_event.h>` — Performance event attribute type constants.
//!
//! The `perf_event_attr` structure configures a performance event.
//! The `type` field selects the event source (hardware, software,
//! tracepoint, etc.) and various flags control counting vs sampling,
//! inheritance, and exclusion.

// ---------------------------------------------------------------------------
// Event type (perf_event_attr.type)
// ---------------------------------------------------------------------------

/// Hardware performance counter.
pub const PERF_TYPE_HARDWARE: u32 = 0;
/// Software event (kernel-generated).
pub const PERF_TYPE_SOFTWARE: u32 = 1;
/// Kernel tracepoint.
pub const PERF_TYPE_TRACEPOINT: u32 = 2;
/// Hardware cache event.
pub const PERF_TYPE_HW_CACHE: u32 = 3;
/// Raw hardware event (arch-specific).
pub const PERF_TYPE_RAW: u32 = 4;
/// Hardware breakpoint.
pub const PERF_TYPE_BREAKPOINT: u32 = 5;

// ---------------------------------------------------------------------------
// perf_event_attr flags (bit fields)
// ---------------------------------------------------------------------------

/// Count events only when disabled=0.
pub const PERF_ATTR_FLAG_DISABLED: u64 = 1 << 0;
/// Children inherit this event.
pub const PERF_ATTR_FLAG_INHERIT: u64 = 1 << 1;
/// Event is pinned to a counter.
pub const PERF_ATTR_FLAG_PINNED: u64 = 1 << 2;
/// Event must always be on CPU.
pub const PERF_ATTR_FLAG_EXCLUSIVE: u64 = 1 << 3;
/// Don't count events in user space.
pub const PERF_ATTR_FLAG_EXCLUDE_USER: u64 = 1 << 4;
/// Don't count events in kernel space.
pub const PERF_ATTR_FLAG_EXCLUDE_KERNEL: u64 = 1 << 5;
/// Don't count events in hypervisor.
pub const PERF_ATTR_FLAG_EXCLUDE_HV: u64 = 1 << 6;
/// Don't count when idle.
pub const PERF_ATTR_FLAG_EXCLUDE_IDLE: u64 = 1 << 7;
/// Include mmap records.
pub const PERF_ATTR_FLAG_MMAP: u64 = 1 << 8;
/// Include comm (exec) records.
pub const PERF_ATTR_FLAG_COMM: u64 = 1 << 9;
/// Use frequency, not period.
pub const PERF_ATTR_FLAG_FREQ: u64 = 1 << 10;
/// Count after inherit on parent exit.
pub const PERF_ATTR_FLAG_INHERIT_STAT: u64 = 1 << 11;
/// Enable on exec.
pub const PERF_ATTR_FLAG_ENABLE_ON_EXEC: u64 = 1 << 12;
/// Include fork/exit records.
pub const PERF_ATTR_FLAG_TASK: u64 = 1 << 13;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_types_distinct() {
        let types = [
            PERF_TYPE_HARDWARE,
            PERF_TYPE_SOFTWARE,
            PERF_TYPE_TRACEPOINT,
            PERF_TYPE_HW_CACHE,
            PERF_TYPE_RAW,
            PERF_TYPE_BREAKPOINT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_hardware_is_zero() {
        assert_eq!(PERF_TYPE_HARDWARE, 0);
    }

    #[test]
    fn test_attr_flags_power_of_two() {
        let flags = [
            PERF_ATTR_FLAG_DISABLED,
            PERF_ATTR_FLAG_INHERIT,
            PERF_ATTR_FLAG_PINNED,
            PERF_ATTR_FLAG_EXCLUSIVE,
            PERF_ATTR_FLAG_EXCLUDE_USER,
            PERF_ATTR_FLAG_EXCLUDE_KERNEL,
            PERF_ATTR_FLAG_EXCLUDE_HV,
            PERF_ATTR_FLAG_EXCLUDE_IDLE,
            PERF_ATTR_FLAG_MMAP,
            PERF_ATTR_FLAG_COMM,
            PERF_ATTR_FLAG_FREQ,
            PERF_ATTR_FLAG_INHERIT_STAT,
            PERF_ATTR_FLAG_ENABLE_ON_EXEC,
            PERF_ATTR_FLAG_TASK,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_attr_flags_no_overlap() {
        let flags = [
            PERF_ATTR_FLAG_DISABLED,
            PERF_ATTR_FLAG_INHERIT,
            PERF_ATTR_FLAG_PINNED,
            PERF_ATTR_FLAG_EXCLUSIVE,
            PERF_ATTR_FLAG_EXCLUDE_USER,
            PERF_ATTR_FLAG_EXCLUDE_KERNEL,
            PERF_ATTR_FLAG_EXCLUDE_HV,
            PERF_ATTR_FLAG_EXCLUDE_IDLE,
            PERF_ATTR_FLAG_MMAP,
            PERF_ATTR_FLAG_COMM,
            PERF_ATTR_FLAG_FREQ,
            PERF_ATTR_FLAG_INHERIT_STAT,
            PERF_ATTR_FLAG_ENABLE_ON_EXEC,
            PERF_ATTR_FLAG_TASK,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
