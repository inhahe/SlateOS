//! `<linux/posix_acl.h>` — POSIX Access Control List constants.
//!
//! POSIX ACLs extend the traditional Unix permission model (user/group/other)
//! with fine-grained per-user and per-group permissions. ACL entries
//! specify permissions for specific users or groups beyond the owning
//! user/group. They are stored as extended attributes (system.posix_acl_access
//! and system.posix_acl_default).

// ---------------------------------------------------------------------------
// ACL tag types (e_tag field)
// ---------------------------------------------------------------------------

/// Undefined/invalid tag.
pub const ACL_UNDEFINED_TAG: u16 = 0x00;
/// Permissions for the file owner.
pub const ACL_USER_OBJ: u16 = 0x01;
/// Permissions for a specific user.
pub const ACL_USER: u16 = 0x02;
/// Permissions for the owning group.
pub const ACL_GROUP_OBJ: u16 = 0x04;
/// Permissions for a specific group.
pub const ACL_GROUP: u16 = 0x08;
/// Maximum effective permissions mask.
pub const ACL_MASK: u16 = 0x10;
/// Permissions for others.
pub const ACL_OTHER: u16 = 0x20;

// ---------------------------------------------------------------------------
// ACL permission bits (e_perm field)
// ---------------------------------------------------------------------------

/// Execute permission.
pub const ACL_EXECUTE: u16 = 0x01;
/// Write permission.
pub const ACL_WRITE: u16 = 0x02;
/// Read permission.
pub const ACL_READ: u16 = 0x04;

// ---------------------------------------------------------------------------
// ACL extended attribute names
// ---------------------------------------------------------------------------

/// Access ACL xattr name.
pub const POSIX_ACL_XATTR_ACCESS: &str = "system.posix_acl_access";
/// Default ACL xattr name (directories only).
pub const POSIX_ACL_XATTR_DEFAULT: &str = "system.posix_acl_default";

// ---------------------------------------------------------------------------
// ACL version
// ---------------------------------------------------------------------------

/// POSIX ACL on-disk format version.
pub const POSIX_ACL_XATTR_VERSION: u32 = 0x0002;

// ---------------------------------------------------------------------------
// ACL limits
// ---------------------------------------------------------------------------

/// Maximum number of ACL entries (no hard kernel limit, but practical).
pub const ACL_MAX_ENTRIES: u32 = 32;
/// Minimum ACL entries (owner, group, other).
pub const ACL_MIN_ENTRIES: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tag_types_distinct() {
        let tags = [
            ACL_UNDEFINED_TAG, ACL_USER_OBJ, ACL_USER,
            ACL_GROUP_OBJ, ACL_GROUP, ACL_MASK, ACL_OTHER,
        ];
        for i in 0..tags.len() {
            for j in (i + 1)..tags.len() {
                assert_ne!(tags[i], tags[j]);
            }
        }
    }

    #[test]
    fn test_perm_bits_no_overlap() {
        let perms = [ACL_EXECUTE, ACL_WRITE, ACL_READ];
        for i in 0..perms.len() {
            assert!(perms[i].is_power_of_two());
            for j in (i + 1)..perms.len() {
                assert_eq!(perms[i] & perms[j], 0);
            }
        }
    }

    #[test]
    fn test_xattr_names() {
        assert!(POSIX_ACL_XATTR_ACCESS.starts_with("system."));
        assert!(POSIX_ACL_XATTR_DEFAULT.starts_with("system."));
        assert_ne!(POSIX_ACL_XATTR_ACCESS, POSIX_ACL_XATTR_DEFAULT);
    }

    #[test]
    fn test_version() {
        assert_eq!(POSIX_ACL_XATTR_VERSION, 2);
    }

    #[test]
    fn test_entry_limits() {
        assert!(ACL_MIN_ENTRIES < ACL_MAX_ENTRIES);
    }
}
