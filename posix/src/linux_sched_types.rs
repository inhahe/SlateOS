//! `<linux/sched/types.h>` — scheduling parameter types.
//!
//! Re-exports scheduling policies from `sched` and `linux_sched`,
//! and provides the `SchedAttr` struct for `sched_setattr(2)`.

// ---------------------------------------------------------------------------
// Re-exports
// ---------------------------------------------------------------------------

pub use crate::sched::SCHED_BATCH;
pub use crate::sched::SCHED_DEADLINE;
pub use crate::sched::SCHED_FIFO;
pub use crate::sched::SCHED_IDLE;
pub use crate::sched::SCHED_OTHER;
pub use crate::sched::SCHED_RR;

// ---------------------------------------------------------------------------
// sched_attr struct (for sched_setattr / sched_getattr)
// ---------------------------------------------------------------------------

/// Extended scheduling attributes (56 bytes, v2).
///
/// Used with `sched_setattr(2)` and `sched_getattr(2)` for
/// SCHED_DEADLINE and other policy parameters.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SchedAttr {
    /// Size of this struct.
    pub size: u32,
    /// Scheduling policy.
    pub sched_policy: u32,
    /// Scheduling flags.
    pub sched_flags: u64,
    /// Nice value (SCHED_OTHER/BATCH).
    pub sched_nice: i32,
    /// Priority (SCHED_FIFO/RR).
    pub sched_priority: u32,
    /// SCHED_DEADLINE: runtime (ns).
    pub sched_runtime: u64,
    /// SCHED_DEADLINE: deadline (ns).
    pub sched_deadline: u64,
    /// SCHED_DEADLINE: period (ns).
    pub sched_period: u64,
}

impl SchedAttr {
    /// Create a zeroed scheduling attributes struct.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }

    /// Create a new SchedAttr with size pre-filled.
    pub fn new() -> Self {
        let mut attr = Self::zeroed();
        attr.size = core::mem::size_of::<Self>() as u32;
        attr
    }
}

impl Default for SchedAttr {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// sched_flags
// ---------------------------------------------------------------------------

/// Reset on fork.
pub const SCHED_FLAG_RESET_ON_FORK: u64 = 0x01;
/// Reclaim bandwidth (DEADLINE).
pub const SCHED_FLAG_RECLAIM: u64 = 0x02;
/// Latency-nice for DL_SERVER.
pub const SCHED_FLAG_DL_OVERRUN: u64 = 0x04;
/// Keep all scheduling parameters.
pub const SCHED_FLAG_KEEP_ALL: u64 = 0x08;
/// Keep scheduling params.
pub const SCHED_FLAG_KEEP_PARAMS: u64 = 0x10;
/// Utilization clamping.
pub const SCHED_FLAG_UTIL_CLAMP_MIN: u64 = 0x20;
/// Utilization clamping (max).
pub const SCHED_FLAG_UTIL_CLAMP_MAX: u64 = 0x40;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sched_attr_size() {
        assert_eq!(core::mem::size_of::<SchedAttr>(), 48);
    }

    #[test]
    fn test_sched_attr_new() {
        let attr = SchedAttr::new();
        assert_eq!(attr.size, 48);
        assert_eq!(attr.sched_policy, 0);
        assert_eq!(attr.sched_nice, 0);
        assert_eq!(attr.sched_runtime, 0);
    }

    #[test]
    fn test_policies() {
        assert_eq!(SCHED_OTHER, 0);
        assert_eq!(SCHED_FIFO, 1);
        assert_eq!(SCHED_RR, 2);
        assert_eq!(SCHED_BATCH, 3);
        assert_eq!(SCHED_IDLE, 5);
        assert_eq!(SCHED_DEADLINE, 6);
    }

    #[test]
    fn test_flags_are_powers_of_two() {
        let flags = [
            SCHED_FLAG_RESET_ON_FORK,
            SCHED_FLAG_RECLAIM,
            SCHED_FLAG_DL_OVERRUN,
            SCHED_FLAG_KEEP_ALL,
            SCHED_FLAG_KEEP_PARAMS,
            SCHED_FLAG_UTIL_CLAMP_MIN,
            SCHED_FLAG_UTIL_CLAMP_MAX,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "flag {f:#x} not a power of 2");
        }
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(SCHED_FIFO, crate::sched::SCHED_FIFO);
        assert_eq!(SCHED_DEADLINE, crate::sched::SCHED_DEADLINE);
    }
}
