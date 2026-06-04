//! `<linux/cgroupstats.h>` and cgroup-v1 user view.
//!
//! cgroup-v1 mounted each controller at its own hierarchy under
//! `/sys/fs/cgroup/<controller>`. This module covers the legacy
//! per-controller mount paths, the shared task-file names, and
//! the controller-bit flags used by the cgroup_subsys enum.

// ---------------------------------------------------------------------------
// Mount paths (cgroup-v1 layout)
// ---------------------------------------------------------------------------

pub const CGROUP1_MOUNT_ROOT: &str = "/sys/fs/cgroup";
pub const CGROUP1_FS_TYPE: &str = "cgroup";

// ---------------------------------------------------------------------------
// Subsystem index bits (`enum cgroup_subsys_id`)
// ---------------------------------------------------------------------------

pub const CGROUP_SUBSYS_CPUSET: u32 = 0;
pub const CGROUP_SUBSYS_CPU: u32 = 1;
pub const CGROUP_SUBSYS_CPUACCT: u32 = 2;
pub const CGROUP_SUBSYS_IO: u32 = 3;
pub const CGROUP_SUBSYS_MEMORY: u32 = 4;
pub const CGROUP_SUBSYS_DEVICES: u32 = 5;
pub const CGROUP_SUBSYS_FREEZER: u32 = 6;
pub const CGROUP_SUBSYS_NET_CLS: u32 = 7;
pub const CGROUP_SUBSYS_PERF_EVENT: u32 = 8;
pub const CGROUP_SUBSYS_NET_PRIO: u32 = 9;
pub const CGROUP_SUBSYS_HUGETLB: u32 = 10;
pub const CGROUP_SUBSYS_PIDS: u32 = 11;
pub const CGROUP_SUBSYS_RDMA: u32 = 12;
pub const CGROUP_SUBSYS_MISC: u32 = 13;

// ---------------------------------------------------------------------------
// Task / process files (legacy v1 names)
// ---------------------------------------------------------------------------

pub const CGROUP1_FILE_TASKS: &str = "tasks";
pub const CGROUP1_FILE_PROCS: &str = "cgroup.procs";
pub const CGROUP1_FILE_RELEASE_AGENT: &str = "release_agent";
pub const CGROUP1_FILE_NOTIFY_ON_RELEASE: &str = "notify_on_release";
pub const CGROUP1_FILE_CLONE_CHILDREN: &str = "cgroup.clone_children";

// ---------------------------------------------------------------------------
// Mount option strings
// ---------------------------------------------------------------------------

pub const CGROUP1_OPT_NOPREFIX: &str = "noprefix";
pub const CGROUP1_OPT_RELEASE_AGENT: &str = "release_agent";
pub const CGROUP1_OPT_NAME: &str = "name";
pub const CGROUP1_OPT_XATTR: &str = "xattr";
pub const CGROUP1_OPT_NSDELEGATE: &str = "nsdelegate";

// ---------------------------------------------------------------------------
// Maximum hierarchy depth
// ---------------------------------------------------------------------------

/// Maximum number of cgroup subsystems compiled into the kernel.
pub const CGROUP_SUBSYS_COUNT: u32 = 14;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mount_root_and_fs_type() {
        assert_eq!(CGROUP1_MOUNT_ROOT, "/sys/fs/cgroup");
        assert_eq!(CGROUP1_FS_TYPE, "cgroup");
    }

    #[test]
    fn test_subsys_ids_dense_0_to_13() {
        let s = [
            CGROUP_SUBSYS_CPUSET,
            CGROUP_SUBSYS_CPU,
            CGROUP_SUBSYS_CPUACCT,
            CGROUP_SUBSYS_IO,
            CGROUP_SUBSYS_MEMORY,
            CGROUP_SUBSYS_DEVICES,
            CGROUP_SUBSYS_FREEZER,
            CGROUP_SUBSYS_NET_CLS,
            CGROUP_SUBSYS_PERF_EVENT,
            CGROUP_SUBSYS_NET_PRIO,
            CGROUP_SUBSYS_HUGETLB,
            CGROUP_SUBSYS_PIDS,
            CGROUP_SUBSYS_RDMA,
            CGROUP_SUBSYS_MISC,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(CGROUP_SUBSYS_COUNT as usize, s.len());
    }

    #[test]
    fn test_legacy_files_distinct() {
        let f = [
            CGROUP1_FILE_TASKS,
            CGROUP1_FILE_PROCS,
            CGROUP1_FILE_RELEASE_AGENT,
            CGROUP1_FILE_NOTIFY_ON_RELEASE,
            CGROUP1_FILE_CLONE_CHILDREN,
        ];
        for (i, &x) in f.iter().enumerate() {
            for &y in &f[i + 1..] {
                assert_ne!(x, y);
            }
        }
        // tasks vs cgroup.procs differ: tasks contains TIDs, procs contains PIDs.
        assert_eq!(CGROUP1_FILE_TASKS, "tasks");
        assert_eq!(CGROUP1_FILE_PROCS, "cgroup.procs");
    }

    #[test]
    fn test_mount_options_unique_lowercase() {
        let o = [
            CGROUP1_OPT_NOPREFIX,
            CGROUP1_OPT_RELEASE_AGENT,
            CGROUP1_OPT_NAME,
            CGROUP1_OPT_XATTR,
            CGROUP1_OPT_NSDELEGATE,
        ];
        for (i, &x) in o.iter().enumerate() {
            for &y in &o[i + 1..] {
                assert_ne!(x, y);
            }
            for c in x.chars() {
                assert!(c.is_ascii_lowercase() || c == '_');
            }
        }
    }

    #[test]
    fn test_subsys_count_matches_enum() {
        assert_eq!(CGROUP_SUBSYS_COUNT, CGROUP_SUBSYS_MISC + 1);
    }

    #[test]
    fn test_net_subsystems_clustered_7_to_9() {
        // NET_CLS, PERF_EVENT, NET_PRIO sit 7..9.
        for v in [
            CGROUP_SUBSYS_NET_CLS,
            CGROUP_SUBSYS_PERF_EVENT,
            CGROUP_SUBSYS_NET_PRIO,
        ] {
            assert!((7..=9).contains(&v));
        }
    }
}
