//! `<sys/types.h>` — User ID and group ID constants.
//!
//! These constants define special UID/GID values used by the kernel
//! for process credentials, file ownership, and access control.

// ---------------------------------------------------------------------------
// Special UID values
// ---------------------------------------------------------------------------

/// Root user ID.
pub const ROOT_UID: u32 = 0;
/// Nobody user (used for NFS squashing and unmapped IDs).
pub const NOBODY_UID: u32 = 65534;
/// Invalid/unset UID (sentinel).
pub const INVALID_UID: u32 = u32::MAX;
/// Keep current UID (for setresuid -1).
pub const UID_UNCHANGED: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// Special GID values
// ---------------------------------------------------------------------------

/// Root group ID.
pub const ROOT_GID: u32 = 0;
/// Nobody group.
pub const NOBODY_GID: u32 = 65534;
/// Invalid/unset GID (sentinel).
pub const INVALID_GID: u32 = u32::MAX;
/// Keep current GID (for setresgid -1).
pub const GID_UNCHANGED: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// UID/GID mapping limits (user namespaces)
// ---------------------------------------------------------------------------

/// Maximum entries in uid_map/gid_map.
pub const UID_GID_MAP_MAX_ENTRIES: u32 = 340;
/// Maximum base ID in a mapping.
pub const UID_GID_MAP_MAX_BASE_ID: u32 = u32::MAX - 1;

// ---------------------------------------------------------------------------
// Supplementary group limits
// ---------------------------------------------------------------------------

/// Maximum supplementary groups per process (NGROUPS_MAX).
pub const NGROUPS_MAX: u32 = 65536;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_root_ids() {
        assert_eq!(ROOT_UID, 0);
        assert_eq!(ROOT_GID, 0);
    }

    #[test]
    fn test_nobody_ids() {
        assert_eq!(NOBODY_UID, 65534);
        assert_eq!(NOBODY_GID, 65534);
    }

    #[test]
    fn test_invalid_ids() {
        assert_eq!(INVALID_UID, u32::MAX);
        assert_eq!(INVALID_GID, u32::MAX);
    }

    #[test]
    fn test_uid_gid_map_max() {
        assert_eq!(UID_GID_MAP_MAX_ENTRIES, 340);
    }

    #[test]
    fn test_ngroups_max() {
        assert_eq!(NGROUPS_MAX, 65536);
    }

    #[test]
    fn test_special_values_distinct_from_root() {
        assert_ne!(ROOT_UID, NOBODY_UID);
        assert_ne!(ROOT_UID, INVALID_UID);
        assert_ne!(NOBODY_UID, INVALID_UID);
    }
}
