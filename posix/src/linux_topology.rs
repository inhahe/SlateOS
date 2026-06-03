//! `<linux/topology.h>` — CPU topology constants.
//!
//! CPU topology describes the physical structure of the processor:
//! packages (sockets), clusters, cores, and hardware threads (SMT).
//! The scheduler uses topology for load balancing, cache locality,
//! and power management decisions.

// ---------------------------------------------------------------------------
// Topology levels
// ---------------------------------------------------------------------------

/// SMT (Simultaneous Multi-Threading / hyper-thread) level.
pub const TOPOLOGY_SMT_LEVEL: u32 = 0;
/// Core level (within a cluster or package).
pub const TOPOLOGY_CORE_LEVEL: u32 = 1;
/// Cluster level (L2-sharing group).
pub const TOPOLOGY_CLUSTER_LEVEL: u32 = 2;
/// Package/die level.
pub const TOPOLOGY_PKG_LEVEL: u32 = 3;
/// NUMA node level.
pub const TOPOLOGY_NUMA_LEVEL: u32 = 4;
/// System-wide (all CPUs).
pub const TOPOLOGY_SYSTEM_LEVEL: u32 = 5;

// ---------------------------------------------------------------------------
// Scheduler domain flags (SD_*)
// ---------------------------------------------------------------------------

/// Load balance at this level.
pub const SD_LOAD_BALANCE: u32 = 1 << 0;
/// Balance on exec.
pub const SD_BALANCE_EXEC: u32 = 1 << 1;
/// Balance on fork.
pub const SD_BALANCE_FORK: u32 = 1 << 2;
/// Balance on wake.
pub const SD_BALANCE_WAKE: u32 = 1 << 3;
/// Wake to idle sibling.
pub const SD_WAKE_AFFINE: u32 = 1 << 4;
/// Asym packing (prefer faster cores).
pub const SD_ASYM_PACKING: u32 = 1 << 5;
/// Prefer sibling for tasks.
pub const SD_PREFER_SIBLING: u32 = 1 << 6;
/// Overlap scheduling domains.
pub const SD_OVERLAP: u32 = 1 << 7;
/// NUMA domain.
pub const SD_NUMA: u32 = 1 << 8;
/// Asym CPU capacity (big.LITTLE).
pub const SD_ASYM_CPUCAPACITY: u32 = 1 << 9;
/// Asym CPU capacity at full load.
pub const SD_ASYM_CPUCAPACITY_FULL: u32 = 1 << 10;
/// Share CPU power.
pub const SD_SHARE_CPUCAPACITY: u32 = 1 << 11;
/// Share power domain.
pub const SD_SHARE_POWERDOMAIN: u32 = 1 << 12;
/// Share L1 cache.
pub const SD_SHARE_L1_CACHE: u32 = 1 << 13;
/// Share L2 cache.
pub const SD_SHARE_L2_CACHE: u32 = 1 << 14;
/// Share L3 cache.
pub const SD_SHARE_L3_CACHE: u32 = 1 << 15;
/// Serialize load balancing.
pub const SD_SERIALIZE: u32 = 1 << 16;

// ---------------------------------------------------------------------------
// Cache levels
// ---------------------------------------------------------------------------

/// L1 data cache.
pub const CACHE_L1_DATA: u32 = 1;
/// L1 instruction cache.
pub const CACHE_L1_INST: u32 = 2;
/// L2 unified cache.
pub const CACHE_L2: u32 = 3;
/// L3 unified cache.
pub const CACHE_L3: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topology_levels_distinct() {
        let levels = [
            TOPOLOGY_SMT_LEVEL,
            TOPOLOGY_CORE_LEVEL,
            TOPOLOGY_CLUSTER_LEVEL,
            TOPOLOGY_PKG_LEVEL,
            TOPOLOGY_NUMA_LEVEL,
            TOPOLOGY_SYSTEM_LEVEL,
        ];
        for i in 0..levels.len() {
            for j in (i + 1)..levels.len() {
                assert_ne!(levels[i], levels[j]);
            }
        }
    }

    #[test]
    fn test_topology_levels_ordered() {
        assert!(TOPOLOGY_SMT_LEVEL < TOPOLOGY_CORE_LEVEL);
        assert!(TOPOLOGY_CORE_LEVEL < TOPOLOGY_CLUSTER_LEVEL);
        assert!(TOPOLOGY_CLUSTER_LEVEL < TOPOLOGY_PKG_LEVEL);
        assert!(TOPOLOGY_PKG_LEVEL < TOPOLOGY_NUMA_LEVEL);
        assert!(TOPOLOGY_NUMA_LEVEL < TOPOLOGY_SYSTEM_LEVEL);
    }

    #[test]
    fn test_sd_flags_powers_of_two() {
        let flags = [
            SD_LOAD_BALANCE,
            SD_BALANCE_EXEC,
            SD_BALANCE_FORK,
            SD_BALANCE_WAKE,
            SD_WAKE_AFFINE,
            SD_ASYM_PACKING,
            SD_PREFER_SIBLING,
            SD_OVERLAP,
            SD_NUMA,
            SD_ASYM_CPUCAPACITY,
            SD_ASYM_CPUCAPACITY_FULL,
            SD_SHARE_CPUCAPACITY,
            SD_SHARE_POWERDOMAIN,
            SD_SHARE_L1_CACHE,
            SD_SHARE_L2_CACHE,
            SD_SHARE_L3_CACHE,
            SD_SERIALIZE,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_sd_flags_no_overlap() {
        let flags = [
            SD_LOAD_BALANCE,
            SD_BALANCE_EXEC,
            SD_BALANCE_FORK,
            SD_BALANCE_WAKE,
            SD_WAKE_AFFINE,
            SD_ASYM_PACKING,
            SD_PREFER_SIBLING,
            SD_OVERLAP,
            SD_NUMA,
            SD_ASYM_CPUCAPACITY,
            SD_ASYM_CPUCAPACITY_FULL,
            SD_SHARE_CPUCAPACITY,
            SD_SHARE_POWERDOMAIN,
            SD_SHARE_L1_CACHE,
            SD_SHARE_L2_CACHE,
            SD_SHARE_L3_CACHE,
            SD_SERIALIZE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_cache_levels_distinct() {
        let levels = [CACHE_L1_DATA, CACHE_L1_INST, CACHE_L2, CACHE_L3];
        for i in 0..levels.len() {
            for j in (i + 1)..levels.len() {
                assert_ne!(levels[i], levels[j]);
            }
        }
    }
}
