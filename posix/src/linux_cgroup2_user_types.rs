//! `<linux/cgroup.h>` (part 2) — cgroup-v2 unified-hierarchy controllers.
//!
//! cgroup-v2 unifies all controllers into a single hierarchy and
//! exposes them through a handful of well-known files
//! (`cgroup.controllers`, `cgroup.subtree_control`, `cgroup.events`).
//! This module captures the controller-name strings, the unified
//! event flags, and the well-known sysfs paths.

// ---------------------------------------------------------------------------
// Mount point and root file
// ---------------------------------------------------------------------------

pub const CGROUP2_MOUNT_DEFAULT: &str = "/sys/fs/cgroup";
pub const CGROUP2_FS_TYPE: &str = "cgroup2";

// ---------------------------------------------------------------------------
// Controller names (`cgroup.controllers` tokens)
// ---------------------------------------------------------------------------

pub const CGROUP2_CTRL_CPU: &str = "cpu";
pub const CGROUP2_CTRL_CPUSET: &str = "cpuset";
pub const CGROUP2_CTRL_IO: &str = "io";
pub const CGROUP2_CTRL_MEMORY: &str = "memory";
pub const CGROUP2_CTRL_PIDS: &str = "pids";
pub const CGROUP2_CTRL_HUGETLB: &str = "hugetlb";
pub const CGROUP2_CTRL_RDMA: &str = "rdma";
pub const CGROUP2_CTRL_MISC: &str = "misc";

// ---------------------------------------------------------------------------
// cgroup.events flag names
// ---------------------------------------------------------------------------

pub const CGROUP2_EVT_POPULATED: &str = "populated";
pub const CGROUP2_EVT_FROZEN: &str = "frozen";

// ---------------------------------------------------------------------------
// Special files written at cgroup creation
// ---------------------------------------------------------------------------

pub const CGROUP2_FILE_PROCS: &str = "cgroup.procs";
pub const CGROUP2_FILE_THREADS: &str = "cgroup.threads";
pub const CGROUP2_FILE_TYPE: &str = "cgroup.type";
pub const CGROUP2_FILE_CONTROLLERS: &str = "cgroup.controllers";
pub const CGROUP2_FILE_SUBTREE_CONTROL: &str = "cgroup.subtree_control";
pub const CGROUP2_FILE_EVENTS: &str = "cgroup.events";
pub const CGROUP2_FILE_FREEZE: &str = "cgroup.freeze";
pub const CGROUP2_FILE_MAX_DESCENDANTS: &str = "cgroup.max.descendants";
pub const CGROUP2_FILE_MAX_DEPTH: &str = "cgroup.max.depth";

// ---------------------------------------------------------------------------
// cgroup.type values
// ---------------------------------------------------------------------------

pub const CGROUP2_TYPE_DOMAIN: &str = "domain";
pub const CGROUP2_TYPE_DOMAIN_THREADED: &str = "domain threaded";
pub const CGROUP2_TYPE_DOMAIN_INVALID: &str = "domain invalid";
pub const CGROUP2_TYPE_THREADED: &str = "threaded";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mount_path_and_fs_type() {
        assert_eq!(CGROUP2_MOUNT_DEFAULT, "/sys/fs/cgroup");
        assert_eq!(CGROUP2_FS_TYPE, "cgroup2");
        assert!(CGROUP2_MOUNT_DEFAULT.starts_with("/sys/fs/"));
    }

    #[test]
    fn test_controllers_distinct_lowercase() {
        let c = [
            CGROUP2_CTRL_CPU,
            CGROUP2_CTRL_CPUSET,
            CGROUP2_CTRL_IO,
            CGROUP2_CTRL_MEMORY,
            CGROUP2_CTRL_PIDS,
            CGROUP2_CTRL_HUGETLB,
            CGROUP2_CTRL_RDMA,
            CGROUP2_CTRL_MISC,
        ];
        for (i, &x) in c.iter().enumerate() {
            for &y in &c[i + 1..] {
                assert_ne!(x, y);
            }
            for ch in x.chars() {
                assert!(ch.is_ascii_lowercase());
            }
        }
    }

    #[test]
    fn test_event_names_short() {
        for n in [CGROUP2_EVT_POPULATED, CGROUP2_EVT_FROZEN] {
            assert!(!n.is_empty());
            assert!(n.len() <= 16);
        }
    }

    #[test]
    fn test_special_files_prefixed_with_cgroup() {
        for f in [
            CGROUP2_FILE_PROCS,
            CGROUP2_FILE_THREADS,
            CGROUP2_FILE_TYPE,
            CGROUP2_FILE_CONTROLLERS,
            CGROUP2_FILE_SUBTREE_CONTROL,
            CGROUP2_FILE_EVENTS,
            CGROUP2_FILE_FREEZE,
            CGROUP2_FILE_MAX_DESCENDANTS,
            CGROUP2_FILE_MAX_DEPTH,
        ] {
            // All special files share the "cgroup." prefix.
            assert!(f.starts_with("cgroup."));
        }
    }

    #[test]
    fn test_type_strings_distinct() {
        let t = [
            CGROUP2_TYPE_DOMAIN,
            CGROUP2_TYPE_DOMAIN_THREADED,
            CGROUP2_TYPE_DOMAIN_INVALID,
            CGROUP2_TYPE_THREADED,
        ];
        for (i, &x) in t.iter().enumerate() {
            for &y in &t[i + 1..] {
                assert_ne!(x, y);
            }
        }
        // Three of four start with "domain"; one is bare "threaded".
        assert!(CGROUP2_TYPE_DOMAIN.starts_with("domain"));
        assert!(CGROUP2_TYPE_DOMAIN_THREADED.starts_with("domain"));
        assert!(CGROUP2_TYPE_DOMAIN_INVALID.starts_with("domain"));
        assert!(!CGROUP2_TYPE_THREADED.starts_with("domain"));
    }

    #[test]
    fn test_max_descendants_and_depth_paired() {
        // Both knobs live under "cgroup.max.*".
        assert!(CGROUP2_FILE_MAX_DESCENDANTS.starts_with("cgroup.max."));
        assert!(CGROUP2_FILE_MAX_DEPTH.starts_with("cgroup.max."));
    }
}
