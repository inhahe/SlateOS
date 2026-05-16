//! Linux mount namespace constants.
//!
//! Mount namespaces isolate the set of filesystem mount points
//! seen by a group of processes. Each mount namespace has an
//! independent mount tree, enabling per-process filesystem views
//! (containers, chroot replacements, etc.).

// ---------------------------------------------------------------------------
// Clone flags
// ---------------------------------------------------------------------------

/// Create new mount namespace.
pub const CLONE_NEWNS: u64 = 0x00020000;

// ---------------------------------------------------------------------------
// Mount propagation types
// ---------------------------------------------------------------------------

/// Shared mount — events propagate bidirectionally.
pub const MS_SHARED: u32 = 1 << 20;
/// Slave mount — receives events from master, doesn't send.
pub const MS_SLAVE: u32 = 1 << 19;
/// Private mount — no propagation.
pub const MS_PRIVATE: u32 = 1 << 18;
/// Unbindable mount — cannot be bind-mounted.
pub const MS_UNBINDABLE: u32 = 1 << 17;

// ---------------------------------------------------------------------------
// Mount propagation flags (for mount(2) / mount_setattr)
// ---------------------------------------------------------------------------

/// Recursive propagation change.
pub const MS_REC: u32 = 1 << 14;

// ---------------------------------------------------------------------------
// /proc interface
// ---------------------------------------------------------------------------

/// Mount namespace proc link.
pub const PROC_NS_MNT: &str = "ns/mnt";
/// Mount info file.
pub const PROC_MOUNTINFO: &str = "mountinfo";
/// Mounts file.
pub const PROC_MOUNTS: &str = "mounts";
/// Mount stats.
pub const PROC_MOUNTSTATS: &str = "mountstats";

// ---------------------------------------------------------------------------
// Propagation type names
// ---------------------------------------------------------------------------

/// "shared" propagation string.
pub const PROP_SHARED: &str = "shared";
/// "slave" propagation string.
pub const PROP_SLAVE: &str = "slave";
/// "private" propagation string.
pub const PROP_PRIVATE: &str = "private";
/// "unbindable" propagation string.
pub const PROP_UNBINDABLE: &str = "unbindable";

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum number of mounts per namespace (default sysctl).
pub const MNT_NS_MAX_MOUNTS: u32 = 100_000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clone_newns() {
        assert_eq!(CLONE_NEWNS, 0x00020000);
        assert!((CLONE_NEWNS as u64).is_power_of_two());
    }

    #[test]
    fn test_propagation_types_powers_of_two() {
        let types = [MS_SHARED, MS_SLAVE, MS_PRIVATE, MS_UNBINDABLE];
        for t in &types {
            assert!(t.is_power_of_two(), "0x{:x}", t);
        }
    }

    #[test]
    fn test_propagation_types_no_overlap() {
        let types = [MS_SHARED, MS_SLAVE, MS_PRIVATE, MS_UNBINDABLE, MS_REC];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_eq!(types[i] & types[j], 0);
            }
        }
    }

    #[test]
    fn test_proc_files_distinct() {
        let files = [PROC_NS_MNT, PROC_MOUNTINFO, PROC_MOUNTS, PROC_MOUNTSTATS];
        for i in 0..files.len() {
            for j in (i + 1)..files.len() {
                assert_ne!(files[i], files[j]);
            }
        }
    }

    #[test]
    fn test_prop_names_distinct() {
        let names = [PROP_SHARED, PROP_SLAVE, PROP_PRIVATE, PROP_UNBINDABLE];
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                assert_ne!(names[i], names[j]);
            }
        }
    }

    #[test]
    fn test_max_mounts() {
        assert_eq!(MNT_NS_MAX_MOUNTS, 100_000);
    }

    #[test]
    fn test_ms_rec_is_power_of_two() {
        assert!(MS_REC.is_power_of_two());
    }
}
