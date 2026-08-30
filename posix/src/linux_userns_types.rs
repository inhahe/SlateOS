//! `<linux/user_namespace.h>` — User namespace UID/GID mapping constants.
//!
//! User namespaces provide UID/GID isolation. Inside a user namespace,
//! a process can have UID 0 (root) while being mapped to an unprivileged
//! UID outside. Mappings are written to /proc/[pid]/uid_map and
//! /proc/[pid]/gid_map. Each line specifies: inside_id outside_id count.

// ---------------------------------------------------------------------------
// UID/GID map limits
// ---------------------------------------------------------------------------

/// Maximum number of lines in uid_map/gid_map.
pub const UID_GID_MAP_MAX_EXTENTS: u32 = 340;
/// Maximum base IDs (historical limit, now 340 extents).
pub const UID_GID_MAP_MAX_BASE_EXTENTS: u32 = 5;

// ---------------------------------------------------------------------------
// Special UID/GID values
// ---------------------------------------------------------------------------

/// Overflow UID (nobody) used when no mapping exists.
pub const OVERFLOW_UID: u32 = 65534;
/// Overflow GID (nogroup) used when no mapping exists.
pub const OVERFLOW_GID: u32 = 65534;
/// Invalid UID (unmapped).
pub const INVALID_UID: u32 = 0xFFFF_FFFF;
/// Invalid GID (unmapped).
pub const INVALID_GID: u32 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// /proc/[pid]/setgroups control
// ---------------------------------------------------------------------------

/// setgroups is allowed.
pub const USERNS_SETGROUPS_ALLOW: u32 = 0;
/// setgroups is denied (required before writing gid_map in some cases).
pub const USERNS_SETGROUPS_DENY: u32 = 1;

// ---------------------------------------------------------------------------
// User namespace flags
// ---------------------------------------------------------------------------

/// Unprivileged user namespace creation allowed.
pub const USERNS_UNPRIVILEGED_ALLOWED: u32 = 1;
/// Unprivileged user namespace creation denied (sysctl).
pub const USERNS_UNPRIVILEGED_DENIED: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_limits() {
        assert!(UID_GID_MAP_MAX_BASE_EXTENTS < UID_GID_MAP_MAX_EXTENTS);
        assert!(UID_GID_MAP_MAX_EXTENTS > 0);
    }

    #[test]
    fn test_overflow_ids() {
        assert_eq!(OVERFLOW_UID, 65534);
        assert_eq!(OVERFLOW_GID, 65534);
    }

    #[test]
    fn test_invalid_ids() {
        assert_eq!(INVALID_UID, u32::MAX);
        assert_eq!(INVALID_GID, u32::MAX);
    }

    #[test]
    fn test_setgroups_distinct() {
        assert_ne!(USERNS_SETGROUPS_ALLOW, USERNS_SETGROUPS_DENY);
    }

    #[test]
    fn test_unprivileged_distinct() {
        assert_ne!(USERNS_UNPRIVILEGED_ALLOWED, USERNS_UNPRIVILEGED_DENIED);
    }
}
