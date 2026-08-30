//! `<linux/topology.h>` — CPU topology level and identifier constants.
//!
//! The kernel's topology subsystem describes the hierarchical
//! structure of CPUs: threads within cores, cores within packages,
//! packages within NUMA nodes. These constants define topology
//! levels and identification indices.

// ---------------------------------------------------------------------------
// CPU topology levels
// ---------------------------------------------------------------------------

/// SMT (Simultaneous Multi-Threading) / hyperthread level.
pub const TOPOLOGY_SMT_LEVEL: u32 = 0;
/// Physical core level.
pub const TOPOLOGY_CORE_LEVEL: u32 = 1;
/// Module level (Intel hybrid: P-core/E-core cluster).
pub const TOPOLOGY_MODULE_LEVEL: u32 = 2;
/// Tile level (multi-die within package).
pub const TOPOLOGY_TILE_LEVEL: u32 = 3;
/// Die level.
pub const TOPOLOGY_DIE_LEVEL: u32 = 4;
/// Package (socket) level.
pub const TOPOLOGY_PKG_LEVEL: u32 = 5;

// ---------------------------------------------------------------------------
// CPU topology identification constants
// ---------------------------------------------------------------------------

/// Invalid/unassigned CPU ID.
pub const CPU_ID_INVALID: u32 = u32::MAX;
/// Maximum CPUs supported (kernel compile-time default).
pub const NR_CPUS_DEFAULT: u32 = 8192;

// ---------------------------------------------------------------------------
// Cache topology levels
// ---------------------------------------------------------------------------

/// L1 data cache.
pub const CACHE_LEVEL_L1D: u32 = 1;
/// L1 instruction cache.
pub const CACHE_LEVEL_L1I: u32 = 2;
/// L2 unified cache.
pub const CACHE_LEVEL_L2: u32 = 3;
/// L3 unified cache (last-level cache).
pub const CACHE_LEVEL_L3: u32 = 4;

// ---------------------------------------------------------------------------
// NUMA distance constants
// ---------------------------------------------------------------------------

/// Distance to self (local node).
pub const NUMA_DISTANCE_LOCAL: u32 = 10;
/// Remote NUMA node distance (typical).
pub const NUMA_DISTANCE_REMOTE: u32 = 20;
/// No connection between nodes.
pub const NUMA_DISTANCE_UNREACHABLE: u32 = 255;

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
            TOPOLOGY_MODULE_LEVEL,
            TOPOLOGY_TILE_LEVEL,
            TOPOLOGY_DIE_LEVEL,
            TOPOLOGY_PKG_LEVEL,
        ];
        for i in 0..levels.len() {
            for j in (i + 1)..levels.len() {
                assert_ne!(levels[i], levels[j]);
            }
        }
    }

    #[test]
    fn test_topology_ordering() {
        assert!(TOPOLOGY_SMT_LEVEL < TOPOLOGY_CORE_LEVEL);
        assert!(TOPOLOGY_CORE_LEVEL < TOPOLOGY_PKG_LEVEL);
    }

    #[test]
    fn test_cpu_id_invalid() {
        assert_eq!(CPU_ID_INVALID, u32::MAX);
    }

    #[test]
    fn test_cache_levels_distinct() {
        let levels = [
            CACHE_LEVEL_L1D,
            CACHE_LEVEL_L1I,
            CACHE_LEVEL_L2,
            CACHE_LEVEL_L3,
        ];
        for i in 0..levels.len() {
            for j in (i + 1)..levels.len() {
                assert_ne!(levels[i], levels[j]);
            }
        }
    }

    #[test]
    fn test_numa_distances() {
        assert!(NUMA_DISTANCE_LOCAL < NUMA_DISTANCE_REMOTE);
        assert!(NUMA_DISTANCE_REMOTE < NUMA_DISTANCE_UNREACHABLE);
    }

    #[test]
    fn test_local_distance() {
        assert_eq!(NUMA_DISTANCE_LOCAL, 10);
    }
}
