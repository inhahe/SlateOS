//! `<linux/user_namespace.h>` — User namespace constants.
//!
//! User namespaces allow unprivileged users to have root-like
//! capabilities within a container while remaining unprivileged
//! on the host. They provide UID/GID mapping (container UID 0
//! maps to host UID 100000, etc.), capability isolation, and are
//! the foundation for rootless containers. A user namespace owns
//! other namespace types and controls what resources processes
//! within it can access.

// ---------------------------------------------------------------------------
// User namespace UID/GID map limits
// ---------------------------------------------------------------------------

/// Maximum number of UID mapping extents per user namespace.
pub const UID_MAP_MAX_EXTENTS: u32 = 340;
/// Maximum number of GID mapping extents per user namespace.
pub const GID_MAP_MAX_EXTENTS: u32 = 340;
/// Maximum length of a single mapping line in uid_map/gid_map.
pub const MAP_LINE_MAX: u32 = 4096;

// ---------------------------------------------------------------------------
// User namespace flags
// ---------------------------------------------------------------------------

/// User namespace has been fully set up (maps written).
pub const USERNS_SETGROUPS_ALLOWED: u32 = 0;
/// setgroups() is denied in this user namespace.
pub const USERNS_SETGROUPS_DENIED: u32 = 1;

// ---------------------------------------------------------------------------
// Subordinate ID (subuid/subgid) range limits
// ---------------------------------------------------------------------------

/// Default subordinate UID range size (65536 UIDs per user).
pub const SUBID_RANGE_DEFAULT: u32 = 65536;
/// Minimum valid subordinate ID start.
pub const SUBID_MIN: u32 = 100000;

// ---------------------------------------------------------------------------
// /proc/<pid>/ns/ namespace identifiers
// ---------------------------------------------------------------------------

/// User namespace type identifier.
pub const CLONE_NEWUSER_TYPE: u32 = 0x1000_0000;

// ---------------------------------------------------------------------------
// User namespace init state
// ---------------------------------------------------------------------------

/// Namespace just created (no maps set).
pub const USERNS_STATE_CREATED: u32 = 0;
/// UID map has been written.
pub const USERNS_STATE_UID_MAPPED: u32 = 1;
/// GID map has been written.
pub const USERNS_STATE_GID_MAPPED: u32 = 2;
/// Both maps written, namespace fully operational.
pub const USERNS_STATE_READY: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_extents_limits() {
        assert_eq!(UID_MAP_MAX_EXTENTS, 340);
        assert_eq!(GID_MAP_MAX_EXTENTS, 340);
        assert!(MAP_LINE_MAX > 0);
    }

    #[test]
    fn test_setgroups_values_distinct() {
        assert_ne!(USERNS_SETGROUPS_ALLOWED, USERNS_SETGROUPS_DENIED);
    }

    #[test]
    fn test_subid_range() {
        assert!(SUBID_RANGE_DEFAULT > 0);
        assert!(SUBID_MIN > 0);
        // Default range should fit in u32 when added to minimum
        assert!(SUBID_MIN.checked_add(SUBID_RANGE_DEFAULT).is_some());
    }

    #[test]
    fn test_namespace_states_distinct() {
        let states = [
            USERNS_STATE_CREATED,
            USERNS_STATE_UID_MAPPED,
            USERNS_STATE_GID_MAPPED,
            USERNS_STATE_READY,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}
