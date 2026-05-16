//! `<linux/posix_acl.h>` — POSIX Access Control List constants.
//!
//! POSIX ACLs extend the traditional Unix permission model with
//! fine-grained per-user/per-group entries. Each inode can have
//! an access ACL (controlling access) and a default ACL
//! (inherited by new files in a directory).

// ---------------------------------------------------------------------------
// ACL tag types
// ---------------------------------------------------------------------------

/// Undefined tag.
pub const ACL_UNDEFINED_TAG: u16 = 0x00;
/// User object (file owner).
pub const ACL_USER_OBJ: u16 = 0x01;
/// Named user.
pub const ACL_USER: u16 = 0x02;
/// Group object (file group).
pub const ACL_GROUP_OBJ: u16 = 0x04;
/// Named group.
pub const ACL_GROUP: u16 = 0x08;
/// Mask entry (limits named user/group permissions).
pub const ACL_MASK: u16 = 0x10;
/// Other entry (everyone else).
pub const ACL_OTHER: u16 = 0x20;

// ---------------------------------------------------------------------------
// ACL permission bits
// ---------------------------------------------------------------------------

/// Read permission.
pub const ACL_READ: u16 = 0x04;
/// Write permission.
pub const ACL_WRITE: u16 = 0x02;
/// Execute permission.
pub const ACL_EXECUTE: u16 = 0x01;

// ---------------------------------------------------------------------------
// ACL extended attribute names
// ---------------------------------------------------------------------------

/// Access ACL xattr name.
pub const ACL_XATTR_ACCESS: &str = "system.posix_acl_access";
/// Default ACL xattr name.
pub const ACL_XATTR_DEFAULT: &str = "system.posix_acl_default";

// ---------------------------------------------------------------------------
// ACL on-disk format
// ---------------------------------------------------------------------------

/// ACL version for on-disk format (v2).
pub const ACL_EA_VERSION: u32 = 0x0002;

/// Size of one ACL entry in xattr (tag + perm + id = 8 bytes).
pub const ACL_EA_ENTRY_SIZE: usize = 8;

/// Header size (version field).
pub const ACL_EA_HEADER_SIZE: usize = 4;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum ACL entries (matches ext4/XFS limit).
pub const ACL_MAX_ENTRIES: u32 = 32;

// ---------------------------------------------------------------------------
// Undefined ID
// ---------------------------------------------------------------------------

/// Undefined UID/GID in ACL entry (for USER_OBJ, GROUP_OBJ, MASK, OTHER).
pub const ACL_UNDEFINED_ID: u32 = u32::MAX;

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
    fn test_permission_bits_distinct() {
        let perms = [ACL_READ, ACL_WRITE, ACL_EXECUTE];
        for i in 0..perms.len() {
            for j in (i + 1)..perms.len() {
                assert_ne!(perms[i], perms[j]);
            }
        }
    }

    #[test]
    fn test_permission_bits_no_overlap() {
        assert_eq!(ACL_READ & ACL_WRITE, 0);
        assert_eq!(ACL_READ & ACL_EXECUTE, 0);
        assert_eq!(ACL_WRITE & ACL_EXECUTE, 0);
    }

    #[test]
    fn test_xattr_names_distinct() {
        assert_ne!(ACL_XATTR_ACCESS, ACL_XATTR_DEFAULT);
    }

    #[test]
    fn test_xattr_names_system_prefix() {
        assert!(ACL_XATTR_ACCESS.starts_with("system."));
        assert!(ACL_XATTR_DEFAULT.starts_with("system."));
    }

    #[test]
    fn test_ea_version() {
        assert_eq!(ACL_EA_VERSION, 2);
    }

    #[test]
    fn test_ea_entry_size() {
        assert_eq!(ACL_EA_ENTRY_SIZE, 8);
    }

    #[test]
    fn test_undefined_id() {
        assert_eq!(ACL_UNDEFINED_ID, u32::MAX);
    }

    #[test]
    fn test_max_entries() {
        assert!(ACL_MAX_ENTRIES > 0);
    }
}
