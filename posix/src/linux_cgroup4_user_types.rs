//! `<linux/cgroup.h>` (part 4) — cgroup-v2 memory controller files.
//!
//! The memory controller exposes per-cgroup memory accounting and
//! limits: `memory.max`, `memory.high`, `memory.low`, plus a `.peak`
//! sibling for high-water marks and `.events` for OOM accounting.

// ---------------------------------------------------------------------------
// File names
// ---------------------------------------------------------------------------

pub const CGROUP_MEM_CURRENT: &str = "memory.current";
pub const CGROUP_MEM_PEAK: &str = "memory.peak";
pub const CGROUP_MEM_MIN: &str = "memory.min";
pub const CGROUP_MEM_LOW: &str = "memory.low";
pub const CGROUP_MEM_HIGH: &str = "memory.high";
pub const CGROUP_MEM_MAX: &str = "memory.max";
pub const CGROUP_MEM_OOM_GROUP: &str = "memory.oom.group";
pub const CGROUP_MEM_EVENTS: &str = "memory.events";
pub const CGROUP_MEM_EVENTS_LOCAL: &str = "memory.events.local";
pub const CGROUP_MEM_STAT: &str = "memory.stat";
pub const CGROUP_MEM_PRESSURE: &str = "memory.pressure";
pub const CGROUP_MEM_SWAP_CURRENT: &str = "memory.swap.current";
pub const CGROUP_MEM_SWAP_MAX: &str = "memory.swap.max";
pub const CGROUP_MEM_SWAP_HIGH: &str = "memory.swap.high";
pub const CGROUP_MEM_SWAP_EVENTS: &str = "memory.swap.events";
pub const CGROUP_MEM_ZSWAP_CURRENT: &str = "memory.zswap.current";
pub const CGROUP_MEM_ZSWAP_MAX: &str = "memory.zswap.max";

// ---------------------------------------------------------------------------
// memory.events keys
// ---------------------------------------------------------------------------

pub const CGROUP_MEM_EVT_LOW: &str = "low";
pub const CGROUP_MEM_EVT_HIGH: &str = "high";
pub const CGROUP_MEM_EVT_MAX: &str = "max";
pub const CGROUP_MEM_EVT_OOM: &str = "oom";
pub const CGROUP_MEM_EVT_OOM_KILL: &str = "oom_kill";
pub const CGROUP_MEM_EVT_OOM_GROUP_KILL: &str = "oom_group_kill";

// ---------------------------------------------------------------------------
// "max" sentinel (the literal text in memory.max / memory.high)
// ---------------------------------------------------------------------------

/// Sentinel string written to disable the limit ("max").
pub const CGROUP_MEM_MAX_LITERAL: &str = "max";

// ---------------------------------------------------------------------------
// Page-sizes and conversion constants
// ---------------------------------------------------------------------------

/// Default kernel page size on x86-64 (4 KiB).
pub const CGROUP_MEM_PAGE_SIZE_DEFAULT: usize = 4_096;

/// Number of bytes in a kibibyte (used for sysfs parsing).
pub const KIB: u64 = 1_024;

/// Number of bytes in a mebibyte.
pub const MIB: u64 = 1_024 * 1_024;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_files_prefix() {
        for f in [
            CGROUP_MEM_CURRENT,
            CGROUP_MEM_PEAK,
            CGROUP_MEM_MIN,
            CGROUP_MEM_LOW,
            CGROUP_MEM_HIGH,
            CGROUP_MEM_MAX,
            CGROUP_MEM_OOM_GROUP,
            CGROUP_MEM_EVENTS,
            CGROUP_MEM_EVENTS_LOCAL,
            CGROUP_MEM_STAT,
            CGROUP_MEM_PRESSURE,
            CGROUP_MEM_SWAP_CURRENT,
            CGROUP_MEM_SWAP_MAX,
            CGROUP_MEM_SWAP_HIGH,
            CGROUP_MEM_SWAP_EVENTS,
            CGROUP_MEM_ZSWAP_CURRENT,
            CGROUP_MEM_ZSWAP_MAX,
        ] {
            assert!(f.starts_with("memory."));
        }
    }

    #[test]
    fn test_swap_files_have_swap_subprefix() {
        for f in [
            CGROUP_MEM_SWAP_CURRENT,
            CGROUP_MEM_SWAP_MAX,
            CGROUP_MEM_SWAP_HIGH,
            CGROUP_MEM_SWAP_EVENTS,
        ] {
            assert!(f.starts_with("memory.swap."));
        }
    }

    #[test]
    fn test_zswap_files_have_zswap_subprefix() {
        for f in [CGROUP_MEM_ZSWAP_CURRENT, CGROUP_MEM_ZSWAP_MAX] {
            assert!(f.starts_with("memory.zswap."));
        }
    }

    #[test]
    fn test_events_keys_distinct_lowercase() {
        let e = [
            CGROUP_MEM_EVT_LOW,
            CGROUP_MEM_EVT_HIGH,
            CGROUP_MEM_EVT_MAX,
            CGROUP_MEM_EVT_OOM,
            CGROUP_MEM_EVT_OOM_KILL,
            CGROUP_MEM_EVT_OOM_GROUP_KILL,
        ];
        for (i, &x) in e.iter().enumerate() {
            for &y in &e[i + 1..] {
                assert_ne!(x, y);
            }
            for c in x.chars() {
                assert!(c.is_ascii_lowercase() || c == '_');
            }
        }
    }

    #[test]
    fn test_max_sentinel_literal() {
        assert_eq!(CGROUP_MEM_MAX_LITERAL, "max");
        assert!(CGROUP_MEM_MAX.ends_with(CGROUP_MEM_MAX_LITERAL));
    }

    #[test]
    fn test_size_constants_consistent() {
        assert_eq!(CGROUP_MEM_PAGE_SIZE_DEFAULT, 4_096);
        assert!(CGROUP_MEM_PAGE_SIZE_DEFAULT.is_power_of_two());
        // MiB = 1024 * KiB.
        assert_eq!(MIB / KIB, 1_024);
        // Page size is 4 KiB.
        assert_eq!(CGROUP_MEM_PAGE_SIZE_DEFAULT as u64, 4 * KIB);
    }
}
