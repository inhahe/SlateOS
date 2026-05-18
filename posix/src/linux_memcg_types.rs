//! `<linux/memcontrol.h>` — Memory cgroup v2 constants.
//!
//! Memory cgroups (memcg) limit and account memory usage of
//! process groups. These constants define event types, stat
//! fields, and control knobs for cgroupv2 memory controller.

// ---------------------------------------------------------------------------
// Memory cgroup stat fields
// ---------------------------------------------------------------------------

/// Current memory usage (bytes).
pub const MEMCG_STAT_CURRENT: u32 = 0;
/// Minimum memory guarantee (bytes).
pub const MEMCG_STAT_MIN: u32 = 1;
/// Low memory threshold (soft limit).
pub const MEMCG_STAT_LOW: u32 = 2;
/// High memory threshold (throttle reclaim).
pub const MEMCG_STAT_HIGH: u32 = 3;
/// Maximum memory limit (hard limit).
pub const MEMCG_STAT_MAX: u32 = 4;

// ---------------------------------------------------------------------------
// Memory cgroup event types
// ---------------------------------------------------------------------------

/// OOM kill event.
pub const MEMCG_OOM_KILL: u32 = 0;
/// Max usage reached event.
pub const MEMCG_MAX: u32 = 1;
/// High threshold crossed event.
pub const MEMCG_HIGH: u32 = 2;
/// Low threshold crossed event.
pub const MEMCG_LOW: u32 = 3;
/// Swap max reached event.
pub const MEMCG_SWAP_MAX: u32 = 4;
/// Swap high threshold event.
pub const MEMCG_SWAP_HIGH: u32 = 5;

// ---------------------------------------------------------------------------
// Memory cgroup OOM control
// ---------------------------------------------------------------------------

/// OOM killer is active.
pub const MEMCG_OOM_CONTROL_KILL: u32 = 0;
/// OOM killer paused (tasks suspended).
pub const MEMCG_OOM_CONTROL_PAUSE: u32 = 1;

// ---------------------------------------------------------------------------
// Memory cgroup pressure levels
// ---------------------------------------------------------------------------

/// Low memory pressure.
pub const MEMCG_PRESSURE_LOW: u32 = 0;
/// Medium memory pressure.
pub const MEMCG_PRESSURE_MEDIUM: u32 = 1;
/// Critical memory pressure.
pub const MEMCG_PRESSURE_CRITICAL: u32 = 2;

// ---------------------------------------------------------------------------
// Memory cgroup charge types
// ---------------------------------------------------------------------------

/// Anonymous memory charge.
pub const MEMCG_CHARGE_ANON: u32 = 0;
/// File cache charge.
pub const MEMCG_CHARGE_FILE: u32 = 1;
/// Slab cache charge.
pub const MEMCG_CHARGE_SLAB: u32 = 2;
/// Stack charge.
pub const MEMCG_CHARGE_STACK: u32 = 3;
/// Page table charge.
pub const MEMCG_CHARGE_PAGETABLE: u32 = 4;
/// Kernel misc charge.
pub const MEMCG_CHARGE_KMEM_MISC: u32 = 5;
/// Socket buffer charge.
pub const MEMCG_CHARGE_SOCK: u32 = 6;

// ---------------------------------------------------------------------------
// Default limit values
// ---------------------------------------------------------------------------

/// No limit set (effectively unlimited).
pub const MEMCG_LIMIT_MAX: u64 = u64::MAX;
/// Default high watermark (no throttle).
pub const MEMCG_HIGH_DEFAULT: u64 = u64::MAX;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stat_fields_distinct() {
        let fields = [
            MEMCG_STAT_CURRENT, MEMCG_STAT_MIN,
            MEMCG_STAT_LOW, MEMCG_STAT_HIGH, MEMCG_STAT_MAX,
        ];
        for i in 0..fields.len() {
            for j in (i + 1)..fields.len() {
                assert_ne!(fields[i], fields[j]);
            }
        }
    }

    #[test]
    fn test_events_distinct() {
        let events = [
            MEMCG_OOM_KILL, MEMCG_MAX, MEMCG_HIGH,
            MEMCG_LOW, MEMCG_SWAP_MAX, MEMCG_SWAP_HIGH,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_pressure_levels_distinct() {
        let levels = [
            MEMCG_PRESSURE_LOW,
            MEMCG_PRESSURE_MEDIUM,
            MEMCG_PRESSURE_CRITICAL,
        ];
        for i in 0..levels.len() {
            for j in (i + 1)..levels.len() {
                assert_ne!(levels[i], levels[j]);
            }
        }
    }

    #[test]
    fn test_charge_types_distinct() {
        let types = [
            MEMCG_CHARGE_ANON, MEMCG_CHARGE_FILE,
            MEMCG_CHARGE_SLAB, MEMCG_CHARGE_STACK,
            MEMCG_CHARGE_PAGETABLE, MEMCG_CHARGE_KMEM_MISC,
            MEMCG_CHARGE_SOCK,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_limit_max() {
        assert_eq!(MEMCG_LIMIT_MAX, u64::MAX);
    }

    #[test]
    fn test_high_default() {
        assert_eq!(MEMCG_HIGH_DEFAULT, u64::MAX);
    }
}
