//! `<linux/topology.h>` — CPU topology sysfs interface.
//!
//! Each logical CPU exposes its place in the package/die/core/thread
//! hierarchy under /sys/devices/system/cpu/cpuN/topology/, plus a
//! sibling-list bitmap for each shared resource.

// ---------------------------------------------------------------------------
// Sysfs root and per-cpu subdir
// ---------------------------------------------------------------------------

pub const TOPOLOGY_SYSFS_ROOT: &str = "/sys/devices/system/cpu";
pub const TOPOLOGY_SYSFS_CPU_PREFIX: &str = "cpu";
pub const TOPOLOGY_SYSFS_SUBDIR: &str = "topology";

// ---------------------------------------------------------------------------
// Identifier files
// ---------------------------------------------------------------------------

pub const TOPOLOGY_FILE_PHYSICAL_PACKAGE_ID: &str = "physical_package_id";
pub const TOPOLOGY_FILE_DIE_ID: &str = "die_id";
pub const TOPOLOGY_FILE_CLUSTER_ID: &str = "cluster_id";
pub const TOPOLOGY_FILE_CORE_ID: &str = "core_id";
pub const TOPOLOGY_FILE_BOOK_ID: &str = "book_id";
pub const TOPOLOGY_FILE_DRAWER_ID: &str = "drawer_id";

// ---------------------------------------------------------------------------
// Sibling-list files (bitmap + comma list)
// ---------------------------------------------------------------------------

pub const TOPOLOGY_FILE_THREAD_SIBLINGS: &str = "thread_siblings";
pub const TOPOLOGY_FILE_THREAD_SIBLINGS_LIST: &str = "thread_siblings_list";
pub const TOPOLOGY_FILE_CORE_SIBLINGS: &str = "core_siblings";
pub const TOPOLOGY_FILE_CORE_SIBLINGS_LIST: &str = "core_siblings_list";
pub const TOPOLOGY_FILE_PACKAGE_CPUS: &str = "package_cpus";
pub const TOPOLOGY_FILE_PACKAGE_CPUS_LIST: &str = "package_cpus_list";
pub const TOPOLOGY_FILE_DIE_CPUS: &str = "die_cpus";
pub const TOPOLOGY_FILE_DIE_CPUS_LIST: &str = "die_cpus_list";
pub const TOPOLOGY_FILE_CLUSTER_CPUS: &str = "cluster_cpus";
pub const TOPOLOGY_FILE_CLUSTER_CPUS_LIST: &str = "cluster_cpus_list";
pub const TOPOLOGY_FILE_CORE_CPUS: &str = "core_cpus";
pub const TOPOLOGY_FILE_CORE_CPUS_LIST: &str = "core_cpus_list";

// ---------------------------------------------------------------------------
// NUMA node sysfs
// ---------------------------------------------------------------------------

pub const TOPOLOGY_NODE_SYSFS_ROOT: &str = "/sys/devices/system/node";
pub const TOPOLOGY_NODE_PREFIX: &str = "node";
pub const TOPOLOGY_NODE_FILE_CPULIST: &str = "cpulist";
pub const TOPOLOGY_NODE_FILE_CPUMAP: &str = "cpumap";
pub const TOPOLOGY_NODE_FILE_DISTANCE: &str = "distance";
pub const TOPOLOGY_NODE_FILE_MEMINFO: &str = "meminfo";

// ---------------------------------------------------------------------------
// Limits and constants
// ---------------------------------------------------------------------------

/// Maximum NUMA distance (per ACPI SLIT — 0xFF is "unreachable").
pub const NUMA_DISTANCE_MAX: u32 = 0xFF;
/// Self-distance is conventionally 10.
pub const NUMA_LOCAL_DISTANCE: u32 = 10;
/// Default cross-node distance when unspecified.
pub const NUMA_REMOTE_DISTANCE: u32 = 20;

/// Upper bound on NUMA nodes (MAX_NUMNODES on x86_64).
pub const NUMA_NODES_MAX: u32 = 1024;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sysfs_roots_well_formed() {
        assert!(TOPOLOGY_SYSFS_ROOT.starts_with("/sys/devices/system/"));
        assert!(TOPOLOGY_NODE_SYSFS_ROOT.starts_with("/sys/devices/system/"));
    }

    #[test]
    fn test_id_files_distinct() {
        let f = [
            TOPOLOGY_FILE_PHYSICAL_PACKAGE_ID,
            TOPOLOGY_FILE_DIE_ID,
            TOPOLOGY_FILE_CLUSTER_ID,
            TOPOLOGY_FILE_CORE_ID,
            TOPOLOGY_FILE_BOOK_ID,
            TOPOLOGY_FILE_DRAWER_ID,
        ];
        for (i, &x) in f.iter().enumerate() {
            for &y in &f[i + 1..] {
                assert_ne!(x, y);
            }
            assert!(x.ends_with("_id"));
        }
    }

    #[test]
    fn test_sibling_list_pairs_have_list_suffix() {
        let pairs = [
            (TOPOLOGY_FILE_THREAD_SIBLINGS, TOPOLOGY_FILE_THREAD_SIBLINGS_LIST),
            (TOPOLOGY_FILE_CORE_SIBLINGS, TOPOLOGY_FILE_CORE_SIBLINGS_LIST),
            (TOPOLOGY_FILE_PACKAGE_CPUS, TOPOLOGY_FILE_PACKAGE_CPUS_LIST),
            (TOPOLOGY_FILE_DIE_CPUS, TOPOLOGY_FILE_DIE_CPUS_LIST),
            (TOPOLOGY_FILE_CLUSTER_CPUS, TOPOLOGY_FILE_CLUSTER_CPUS_LIST),
            (TOPOLOGY_FILE_CORE_CPUS, TOPOLOGY_FILE_CORE_CPUS_LIST),
        ];
        for (base, list) in pairs {
            assert!(list.starts_with(base));
            assert!(list.ends_with("_list"));
        }
    }

    #[test]
    fn test_numa_distance_constants() {
        assert_eq!(NUMA_LOCAL_DISTANCE, 10);
        assert_eq!(NUMA_REMOTE_DISTANCE, 20);
        assert!(NUMA_LOCAL_DISTANCE < NUMA_REMOTE_DISTANCE);
        assert!(NUMA_REMOTE_DISTANCE < NUMA_DISTANCE_MAX);
        assert_eq!(NUMA_DISTANCE_MAX, 0xFF);
    }

    #[test]
    fn test_numa_nodes_max_power_of_two() {
        assert_eq!(NUMA_NODES_MAX, 1024);
        assert!(NUMA_NODES_MAX.is_power_of_two());
    }

    #[test]
    fn test_node_files_distinct() {
        let f = [
            TOPOLOGY_NODE_FILE_CPULIST,
            TOPOLOGY_NODE_FILE_CPUMAP,
            TOPOLOGY_NODE_FILE_DISTANCE,
            TOPOLOGY_NODE_FILE_MEMINFO,
        ];
        for (i, &x) in f.iter().enumerate() {
            for &y in &f[i + 1..] {
                assert_ne!(x, y);
            }
        }
    }
}
