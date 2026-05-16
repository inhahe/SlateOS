//! Linux cgroup namespace constants.
//!
//! Cgroup namespaces virtualize the view of the cgroup hierarchy.
//! A process in a cgroup namespace sees its own cgroup as the
//! root, enabling containers to have isolated cgroup views
//! without seeing the host's full hierarchy.

// ---------------------------------------------------------------------------
// Clone flags
// ---------------------------------------------------------------------------

/// Create new cgroup namespace.
pub const CLONE_NEWCGROUP: u64 = 0x02000000;

// ---------------------------------------------------------------------------
// /proc interface
// ---------------------------------------------------------------------------

/// Cgroup namespace proc link.
pub const PROC_NS_CGROUP: &str = "ns/cgroup";
/// Process cgroup membership file.
pub const PROC_CGROUP: &str = "cgroup";

// ---------------------------------------------------------------------------
// Cgroup filesystem
// ---------------------------------------------------------------------------

/// Cgroup v2 unified filesystem type.
pub const CGROUPFS_TYPE: &str = "cgroup2";
/// Cgroup v1 filesystem type.
pub const CGROUPFS_V1_TYPE: &str = "cgroup";
/// Default cgroup v2 mount point.
pub const CGROUPFS_MOUNT: &str = "/sys/fs/cgroup";

// ---------------------------------------------------------------------------
// Root cgroup paths
// ---------------------------------------------------------------------------

/// Root cgroup path (within namespace).
pub const CGROUP_ROOT_PATH: &str = "/";
/// Init cgroup (systemd default for PID 1).
pub const CGROUP_INIT_SCOPE: &str = "/init.scope";

// ---------------------------------------------------------------------------
// Cgroup delegation files
// ---------------------------------------------------------------------------

/// Controllers available for delegation.
pub const CGROUP_CONTROLLERS: &str = "cgroup.controllers";
/// Subtree control (enable/disable controllers for children).
pub const CGROUP_SUBTREE_CONTROL: &str = "cgroup.subtree_control";
/// Cgroup events.
pub const CGROUP_EVENTS: &str = "cgroup.events";
/// Process list.
pub const CGROUP_PROCS: &str = "cgroup.procs";
/// Thread list.
pub const CGROUP_THREADS: &str = "cgroup.threads";
/// Cgroup type (domain vs threaded).
pub const CGROUP_TYPE: &str = "cgroup.type";

// ---------------------------------------------------------------------------
// Cgroup type values
// ---------------------------------------------------------------------------

/// Domain cgroup (resource domain).
pub const CGROUP_TYPE_DOMAIN: &str = "domain";
/// Threaded cgroup (thread granularity).
pub const CGROUP_TYPE_THREADED: &str = "threaded";
/// Domain invalid (parent is threaded).
pub const CGROUP_TYPE_DOMAIN_INVALID: &str = "domain invalid";
/// Domain threaded (contains threaded children).
pub const CGROUP_TYPE_DOMAIN_THREADED: &str = "domain threaded";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clone_newcgroup() {
        assert_eq!(CLONE_NEWCGROUP, 0x02000000);
        assert!((CLONE_NEWCGROUP as u64).is_power_of_two());
    }

    #[test]
    fn test_clone_no_overlap() {
        let other_ns: &[u64] = &[
            0x10000000, // CLONE_NEWUSER
            0x20000000, // CLONE_NEWPID
            0x00020000, // CLONE_NEWNS
            0x40000000, // CLONE_NEWNET
            0x08000000, // CLONE_NEWIPC
            0x04000000, // CLONE_NEWUTS
        ];
        for flag in other_ns {
            assert_ne!(CLONE_NEWCGROUP, *flag);
        }
    }

    #[test]
    fn test_proc_paths_distinct() {
        assert_ne!(PROC_NS_CGROUP, PROC_CGROUP);
    }

    #[test]
    fn test_fs_types_distinct() {
        assert_ne!(CGROUPFS_TYPE, CGROUPFS_V1_TYPE);
    }

    #[test]
    fn test_delegation_files_distinct() {
        let files = [
            CGROUP_CONTROLLERS, CGROUP_SUBTREE_CONTROL,
            CGROUP_EVENTS, CGROUP_PROCS,
            CGROUP_THREADS, CGROUP_TYPE,
        ];
        for i in 0..files.len() {
            for j in (i + 1)..files.len() {
                assert_ne!(files[i], files[j]);
            }
        }
    }

    #[test]
    fn test_delegation_files_have_prefix() {
        let files = [
            CGROUP_CONTROLLERS, CGROUP_SUBTREE_CONTROL,
            CGROUP_EVENTS, CGROUP_PROCS,
            CGROUP_THREADS, CGROUP_TYPE,
        ];
        for file in &files {
            assert!(file.starts_with("cgroup."), "{}", file);
        }
    }

    #[test]
    fn test_cgroup_types_distinct() {
        let types = [
            CGROUP_TYPE_DOMAIN, CGROUP_TYPE_THREADED,
            CGROUP_TYPE_DOMAIN_INVALID, CGROUP_TYPE_DOMAIN_THREADED,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
