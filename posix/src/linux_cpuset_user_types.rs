//! `<linux/cpuset.h>` — cpuset cgroup controller sysfs interface.
//!
//! cpuset constrains a cgroup's tasks to a subset of CPUs and memory
//! nodes. Each cgroup directory has `cpuset.cpus`, `cpuset.mems`, and
//! a handful of policy switches.

// ---------------------------------------------------------------------------
// Sysfs locations (cgroup v1 and v2 conventions)
// ---------------------------------------------------------------------------

pub const CPUSET_V1_MOUNT: &str = "/sys/fs/cgroup/cpuset";
pub const CPUSET_V2_MOUNT: &str = "/sys/fs/cgroup";

// ---------------------------------------------------------------------------
// Control files
// ---------------------------------------------------------------------------

pub const CPUSET_FILE_CPUS: &str = "cpuset.cpus";
pub const CPUSET_FILE_MEMS: &str = "cpuset.mems";
pub const CPUSET_FILE_EFFECTIVE_CPUS: &str = "cpuset.cpus.effective";
pub const CPUSET_FILE_EFFECTIVE_MEMS: &str = "cpuset.mems.effective";
pub const CPUSET_FILE_CPU_EXCLUSIVE: &str = "cpuset.cpu_exclusive";
pub const CPUSET_FILE_MEM_EXCLUSIVE: &str = "cpuset.mem_exclusive";
pub const CPUSET_FILE_MEM_HARDWALL: &str = "cpuset.mem_hardwall";
pub const CPUSET_FILE_MEM_MIGRATE: &str = "cpuset.memory_migrate";
pub const CPUSET_FILE_MEM_PRESSURE: &str = "cpuset.memory_pressure";
pub const CPUSET_FILE_MEM_SPREAD_PAGE: &str = "cpuset.memory_spread_page";
pub const CPUSET_FILE_MEM_SPREAD_SLAB: &str = "cpuset.memory_spread_slab";
pub const CPUSET_FILE_SCHED_LOAD_BALANCE: &str = "cpuset.sched_load_balance";
pub const CPUSET_FILE_SCHED_RELAX_DOMAIN: &str = "cpuset.sched_relax_domain_level";

// ---------------------------------------------------------------------------
// cpuset.cpus.partition values (v2 only)
// ---------------------------------------------------------------------------

pub const CPUSET_PARTITION_MEMBER: &str = "member";
pub const CPUSET_PARTITION_ROOT: &str = "root";
pub const CPUSET_PARTITION_ISOLATED: &str = "isolated";

pub const CPUSET_FILE_PARTITION: &str = "cpuset.cpus.partition";

// ---------------------------------------------------------------------------
// Boolean flag values (writing to *.exclusive etc.)
// ---------------------------------------------------------------------------

pub const CPUSET_FLAG_OFF: u8 = 0;
pub const CPUSET_FLAG_ON: u8 = 1;

// ---------------------------------------------------------------------------
// sched_relax_domain_level range (-1 .. 5)
// ---------------------------------------------------------------------------

pub const CPUSET_RELAX_NONE: i32 = -1;
pub const CPUSET_RELAX_SIBLING_THREAD: i32 = 0;
pub const CPUSET_RELAX_SIBLING_CORE: i32 = 1;
pub const CPUSET_RELAX_PACKAGE: i32 = 2;
pub const CPUSET_RELAX_NODE: i32 = 3;
pub const CPUSET_RELAX_ALL_NODES: i32 = 4;
pub const CPUSET_RELAX_DEFAULT: i32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mount_paths_under_cgroup() {
        assert!(CPUSET_V1_MOUNT.starts_with("/sys/fs/cgroup"));
        assert!(CPUSET_V2_MOUNT.starts_with("/sys/fs/cgroup"));
    }

    #[test]
    fn test_control_files_distinct_have_cpuset_prefix() {
        let f = [
            CPUSET_FILE_CPUS,
            CPUSET_FILE_MEMS,
            CPUSET_FILE_EFFECTIVE_CPUS,
            CPUSET_FILE_EFFECTIVE_MEMS,
            CPUSET_FILE_CPU_EXCLUSIVE,
            CPUSET_FILE_MEM_EXCLUSIVE,
            CPUSET_FILE_MEM_HARDWALL,
            CPUSET_FILE_MEM_MIGRATE,
            CPUSET_FILE_MEM_PRESSURE,
            CPUSET_FILE_MEM_SPREAD_PAGE,
            CPUSET_FILE_MEM_SPREAD_SLAB,
            CPUSET_FILE_SCHED_LOAD_BALANCE,
            CPUSET_FILE_SCHED_RELAX_DOMAIN,
        ];
        for (i, &x) in f.iter().enumerate() {
            assert!(x.starts_with("cpuset."));
            for &y in &f[i + 1..] {
                assert_ne!(x, y);
            }
        }
    }

    #[test]
    fn test_partition_values_distinct() {
        let p = [
            CPUSET_PARTITION_MEMBER,
            CPUSET_PARTITION_ROOT,
            CPUSET_PARTITION_ISOLATED,
        ];
        for (i, &x) in p.iter().enumerate() {
            for &y in &p[i + 1..] {
                assert_ne!(x, y);
            }
        }
        assert!(CPUSET_FILE_PARTITION.starts_with("cpuset."));
    }

    #[test]
    fn test_flag_values_binary() {
        assert_eq!(CPUSET_FLAG_OFF, 0);
        assert_eq!(CPUSET_FLAG_ON, 1);
    }

    #[test]
    fn test_relax_levels_dense_neg1_to_5() {
        let r = [
            CPUSET_RELAX_NONE,
            CPUSET_RELAX_SIBLING_THREAD,
            CPUSET_RELAX_SIBLING_CORE,
            CPUSET_RELAX_PACKAGE,
            CPUSET_RELAX_NODE,
            CPUSET_RELAX_ALL_NODES,
            CPUSET_RELAX_DEFAULT,
        ];
        for (i, &v) in r.iter().enumerate() {
            assert_eq!(v, (i as i32) - 1);
        }
    }
}
