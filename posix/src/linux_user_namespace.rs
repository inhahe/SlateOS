//! Linux user namespace constants.
//!
//! User namespaces isolate UIDs and GIDs so that a process can
//! appear as root inside the namespace while being unprivileged
//! outside. They also own other namespace types and control
//! capability scope.

// ---------------------------------------------------------------------------
// Clone flags for namespace creation
// ---------------------------------------------------------------------------

/// Create new user namespace.
pub const CLONE_NEWUSER: u64 = 0x10000000;

// ---------------------------------------------------------------------------
// /proc files
// ---------------------------------------------------------------------------

/// UID mapping file.
pub const PROC_UID_MAP: &str = "uid_map";
/// GID mapping file.
pub const PROC_GID_MAP: &str = "gid_map";
/// Deny setgroups file.
pub const PROC_SETGROUPS: &str = "setgroups";

// ---------------------------------------------------------------------------
// setgroups control values
// ---------------------------------------------------------------------------

/// Allow setgroups(2).
pub const SETGROUPS_ALLOW: &str = "allow";
/// Deny setgroups(2).
pub const SETGROUPS_DENY: &str = "deny";

// ---------------------------------------------------------------------------
// Mapping limits
// ---------------------------------------------------------------------------

/// Maximum number of UID/GID mapping extents (kernel limit).
pub const USERNS_MAP_MAX_EXTENTS: u32 = 340;

/// Maximum UID/GID value (2^32 - 2, excluding overflow ID).
pub const USERNS_MAX_ID: u32 = 0xFFFF_FFFE;

// ---------------------------------------------------------------------------
// Overflow IDs (unmapped IDs appear as these)
// ---------------------------------------------------------------------------

/// Overflow UID (nobody).
pub const OVERFLOW_UID: u32 = 65534;
/// Overflow GID (nogroup).
pub const OVERFLOW_GID: u32 = 65534;

// ---------------------------------------------------------------------------
// ID inside namespace (relative)
// ---------------------------------------------------------------------------

/// Root UID inside user namespace.
pub const USERNS_ROOT_UID: u32 = 0;
/// Root GID inside user namespace.
pub const USERNS_ROOT_GID: u32 = 0;

// ---------------------------------------------------------------------------
// Sysctl limits
// ---------------------------------------------------------------------------

/// Maximum nested user namespaces.
pub const USERNS_MAX_NESTING: u32 = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clone_newuser() {
        assert_eq!(CLONE_NEWUSER, 0x10000000);
        assert!((CLONE_NEWUSER as u64).is_power_of_two());
    }

    #[test]
    fn test_proc_files_distinct() {
        let files = [PROC_UID_MAP, PROC_GID_MAP, PROC_SETGROUPS];
        for i in 0..files.len() {
            for j in (i + 1)..files.len() {
                assert_ne!(files[i], files[j]);
            }
        }
    }

    #[test]
    fn test_setgroups_values_distinct() {
        assert_ne!(SETGROUPS_ALLOW, SETGROUPS_DENY);
    }

    #[test]
    fn test_map_max_extents() {
        assert_eq!(USERNS_MAP_MAX_EXTENTS, 340);
    }

    #[test]
    fn test_overflow_ids() {
        assert_eq!(OVERFLOW_UID, 65534);
        assert_eq!(OVERFLOW_GID, 65534);
        // Overflow IDs should be less than max ID
        assert!(OVERFLOW_UID < USERNS_MAX_ID);
    }

    #[test]
    fn test_max_id() {
        assert_eq!(USERNS_MAX_ID, 0xFFFF_FFFE);
    }

    #[test]
    fn test_root_ids() {
        assert_eq!(USERNS_ROOT_UID, 0);
        assert_eq!(USERNS_ROOT_GID, 0);
    }

    #[test]
    fn test_max_nesting() {
        assert!(USERNS_MAX_NESTING > 0);
    }
}
