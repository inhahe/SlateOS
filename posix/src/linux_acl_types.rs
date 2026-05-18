//! `<sys/acl.h>` — POSIX.1e Access Control List constants.
//!
//! POSIX ACLs extend the traditional Unix permission model with
//! fine-grained access control entries.  These constants define
//! tag types, permission bits, and ACL types.

// ---------------------------------------------------------------------------
// ACL tag types (acl_tag_t)
// ---------------------------------------------------------------------------

/// Undefined tag (invalid).
pub const ACL_UNDEFINED_TAG: u32 = 0x00;
/// Entry for the file owner.
pub const ACL_USER_OBJ: u32 = 0x01;
/// Entry for a specific user.
pub const ACL_USER: u32 = 0x02;
/// Entry for the owning group.
pub const ACL_GROUP_OBJ: u32 = 0x04;
/// Entry for a specific group.
pub const ACL_GROUP: u32 = 0x08;
/// Mask entry (limits effective permissions of USER/GROUP entries).
pub const ACL_MASK: u32 = 0x10;
/// Entry for other (everyone else).
pub const ACL_OTHER: u32 = 0x20;

// ---------------------------------------------------------------------------
// ACL permission bits (acl_perm_t)
// ---------------------------------------------------------------------------

/// Read permission.
pub const ACL_READ: u32 = 0x04;
/// Write permission.
pub const ACL_WRITE: u32 = 0x02;
/// Execute permission.
pub const ACL_EXECUTE: u32 = 0x01;

// ---------------------------------------------------------------------------
// ACL types (for acl_get_file / acl_set_file)
// ---------------------------------------------------------------------------

/// Access ACL (standard file permissions).
pub const ACL_TYPE_ACCESS: u32 = 0x8000;
/// Default ACL (inherited by new files in directory).
pub const ACL_TYPE_DEFAULT: u32 = 0x4000;

// ---------------------------------------------------------------------------
// ACL entry count limits
// ---------------------------------------------------------------------------

/// Minimum number of ACL entries (owner, group, other).
pub const ACL_MIN_ENTRIES: u32 = 3;
/// Maximum number of ACL entries on ext4.
pub const ACL_MAX_ENTRIES_EXT4: u32 = 32;

// ---------------------------------------------------------------------------
// On-disk ACL format constants (ext2/ext3/ext4)
// ---------------------------------------------------------------------------

/// On-disk ACL version.
pub const ACL_EA_VERSION: u32 = 0x0002;
/// Size of one on-disk ACL entry (bytes).
pub const ACL_EA_ENTRY_SIZE: u32 = 8;
/// Size of ACL header on disk (bytes).
pub const ACL_EA_HEADER_SIZE: u32 = 4;

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
    fn test_undefined_is_zero() {
        assert_eq!(ACL_UNDEFINED_TAG, 0);
    }

    #[test]
    fn test_permission_bits_no_overlap() {
        assert_eq!(ACL_READ & ACL_WRITE, 0);
        assert_eq!(ACL_READ & ACL_EXECUTE, 0);
        assert_eq!(ACL_WRITE & ACL_EXECUTE, 0);
    }

    #[test]
    fn test_permission_values() {
        assert_eq!(ACL_READ, 4);
        assert_eq!(ACL_WRITE, 2);
        assert_eq!(ACL_EXECUTE, 1);
    }

    #[test]
    fn test_acl_types_distinct() {
        assert_ne!(ACL_TYPE_ACCESS, ACL_TYPE_DEFAULT);
    }

    #[test]
    fn test_min_entries() {
        assert_eq!(ACL_MIN_ENTRIES, 3);
    }

    #[test]
    fn test_max_entries_ext4() {
        assert!(ACL_MAX_ENTRIES_EXT4 >= ACL_MIN_ENTRIES);
    }

    #[test]
    fn test_ea_version() {
        assert_eq!(ACL_EA_VERSION, 2);
    }

    #[test]
    fn test_ea_entry_size() {
        assert_eq!(ACL_EA_ENTRY_SIZE, 8);
    }
}
