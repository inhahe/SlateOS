//! `<linux/workqueue.h>` — Workqueue constants.
//!
//! Workqueues provide a mechanism for deferring work to kernel
//! threads. They are the primary deferred-work mechanism in the
//! kernel, replacing tasklets for most use cases. Work items can
//! be queued, delayed, and cancelled. Different workqueue types
//! provide different concurrency and ordering guarantees.

// ---------------------------------------------------------------------------
// Workqueue flags (WQ_*)
// ---------------------------------------------------------------------------

/// Unbound workqueue — not bound to any CPU.
pub const WQ_UNBOUND: u32 = 1 << 1;
/// Freezable — suspend during system freeze.
pub const WQ_FREEZABLE: u32 = 1 << 2;
/// Memory reclaim safe.
pub const WQ_MEM_RECLAIM: u32 = 1 << 3;
/// High priority.
pub const WQ_HIGHPRI: u32 = 1 << 4;
/// CPU intensive (don't contribute to concurrency level).
pub const WQ_CPU_INTENSIVE: u32 = 1 << 5;
/// Sysfs entry (export to sysfs).
pub const WQ_SYSFS: u32 = 1 << 6;
/// Power efficient (allow system to batch).
pub const WQ_POWER_EFFICIENT: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Work item flags (WORK_*)
// ---------------------------------------------------------------------------

/// Pending work.
pub const WORK_STRUCT_PENDING_BIT: u32 = 0;
/// Delayed work.
pub const WORK_STRUCT_DELAYED_BIT: u32 = 1;
/// PWQ work.
pub const WORK_STRUCT_PWQ_BIT: u32 = 2;
/// Linked work.
pub const WORK_STRUCT_LINKED_BIT: u32 = 3;
/// Color mask shift.
pub const WORK_STRUCT_COLOR_SHIFT: u32 = 4;
/// Color mask bits.
pub const WORK_STRUCT_COLOR_BITS: u32 = 4;

// ---------------------------------------------------------------------------
// Default workqueue names
// ---------------------------------------------------------------------------

/// System workqueue (default, per-CPU, ordered).
pub const WQ_SYSTEM: &str = "events";
/// High priority workqueue.
pub const WQ_SYSTEM_HIGHPRI: &str = "events_highpri";
/// Long-running workqueue.
pub const WQ_SYSTEM_LONG: &str = "events_long";
/// Unbound workqueue.
pub const WQ_SYSTEM_UNBOUND: &str = "events_unbound";
/// Freezable workqueue.
pub const WQ_SYSTEM_FREEZABLE: &str = "events_freezable";
/// Power-efficient workqueue.
pub const WQ_SYSTEM_POWER_EFFICIENT: &str = "events_power_efficient";
/// Freezable + power-efficient.
pub const WQ_SYSTEM_FREEZABLE_POWER_EFFICIENT: &str = "events_freezable_power_eff";

// ---------------------------------------------------------------------------
// Concurrency limits
// ---------------------------------------------------------------------------

/// Max active work items (0 = use default).
pub const WQ_MAX_ACTIVE: u32 = 512;
/// Default max active (unbound).
pub const WQ_UNBOUND_MAX_ACTIVE: u32 = 512;
/// Default max active (per CPU).
pub const WQ_DFL_ACTIVE: u32 = 256;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wq_flags_powers_of_two() {
        let flags = [
            WQ_UNBOUND,
            WQ_FREEZABLE,
            WQ_MEM_RECLAIM,
            WQ_HIGHPRI,
            WQ_CPU_INTENSIVE,
            WQ_SYSFS,
            WQ_POWER_EFFICIENT,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_wq_flags_no_overlap() {
        let flags = [
            WQ_UNBOUND,
            WQ_FREEZABLE,
            WQ_MEM_RECLAIM,
            WQ_HIGHPRI,
            WQ_CPU_INTENSIVE,
            WQ_SYSFS,
            WQ_POWER_EFFICIENT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_work_struct_bits_distinct() {
        let bits = [
            WORK_STRUCT_PENDING_BIT,
            WORK_STRUCT_DELAYED_BIT,
            WORK_STRUCT_PWQ_BIT,
            WORK_STRUCT_LINKED_BIT,
        ];
        for i in 0..bits.len() {
            for j in (i + 1)..bits.len() {
                assert_ne!(bits[i], bits[j]);
            }
        }
    }

    #[test]
    fn test_system_wq_names_distinct() {
        let names = [
            WQ_SYSTEM,
            WQ_SYSTEM_HIGHPRI,
            WQ_SYSTEM_LONG,
            WQ_SYSTEM_UNBOUND,
            WQ_SYSTEM_FREEZABLE,
            WQ_SYSTEM_POWER_EFFICIENT,
            WQ_SYSTEM_FREEZABLE_POWER_EFFICIENT,
        ];
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                assert_ne!(names[i], names[j]);
            }
        }
    }

    #[test]
    fn test_concurrency_limits() {
        assert_eq!(WQ_MAX_ACTIVE, 512);
        assert_eq!(WQ_UNBOUND_MAX_ACTIVE, 512);
        assert_eq!(WQ_DFL_ACTIVE, 256);
        assert!(WQ_DFL_ACTIVE <= WQ_MAX_ACTIVE);
    }

    #[test]
    fn test_color_shift() {
        assert_eq!(WORK_STRUCT_COLOR_SHIFT, 4);
        assert_eq!(WORK_STRUCT_COLOR_BITS, 4);
    }
}
