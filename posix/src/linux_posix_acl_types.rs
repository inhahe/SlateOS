//! `<linux/posix_acl.h>` — POSIX Access Control List constants.
//!
//! POSIX ACLs extend traditional Unix permission bits with
//! fine-grained per-user and per-group access entries. These
//! constants define ACL tag types, permission bits, and
//! on-disk format version numbers.

// ---------------------------------------------------------------------------
// ACL entry tag types
// ---------------------------------------------------------------------------

/// ACL entry applies to the file owner.
pub const ACL_USER_OBJ: u16 = 0x01;
/// ACL entry applies to a specific user.
pub const ACL_USER: u16 = 0x02;
/// ACL entry applies to the file group.
pub const ACL_GROUP_OBJ: u16 = 0x04;
/// ACL entry applies to a specific group.
pub const ACL_GROUP: u16 = 0x08;
/// ACL mask entry (limits effective permissions).
pub const ACL_MASK: u16 = 0x10;
/// ACL entry applies to other (everyone else).
pub const ACL_OTHER: u16 = 0x20;

// ---------------------------------------------------------------------------
// ACL permission bits
// ---------------------------------------------------------------------------

/// Execute permission.
pub const ACL_EXECUTE: u16 = 0x01;
/// Write permission.
pub const ACL_WRITE: u16 = 0x02;
/// Read permission.
pub const ACL_READ: u16 = 0x04;

// ---------------------------------------------------------------------------
// ACL type identifiers (for get/setxattr)
// ---------------------------------------------------------------------------

/// Access ACL type (controls access to the object).
pub const ACL_TYPE_ACCESS: u32 = 0x8000;
/// Default ACL type (inherited by new files in directory).
pub const ACL_TYPE_DEFAULT: u32 = 0x4000;

// ---------------------------------------------------------------------------
// On-disk ACL format version
// ---------------------------------------------------------------------------

/// POSIX ACL on-disk format version 2.
pub const ACL_EA_VERSION: u32 = 0x0002;
/// Size of a single ACL entry on disk (tag + perm + id = 8 bytes).
pub const ACL_EA_ENTRY_SIZE: u32 = 8;
/// Size of the ACL header on disk (version = 4 bytes).
pub const ACL_EA_HEADER_SIZE: u32 = 4;

// ---------------------------------------------------------------------------
// Special qualifier values
// ---------------------------------------------------------------------------

/// Undefined qualifier (for USER_OBJ, GROUP_OBJ, MASK, OTHER).
pub const ACL_UNDEFINED_ID: u32 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tag_types_distinct() {
        let tags = [
            ACL_USER_OBJ, ACL_USER, ACL_GROUP_OBJ,
            ACL_GROUP, ACL_MASK, ACL_OTHER,
        ];
        for i in 0..tags.len() {
            for j in (i + 1)..tags.len() {
                assert_ne!(tags[i], tags[j]);
            }
        }
    }

    #[test]
    fn test_permission_bits_no_overlap() {
        let perms = [ACL_EXECUTE, ACL_WRITE, ACL_READ];
        for i in 0..perms.len() {
            for j in (i + 1)..perms.len() {
                assert_eq!(perms[i] & perms[j], 0);
            }
        }
    }

    #[test]
    fn test_permission_bits_power_of_two() {
        assert!(ACL_EXECUTE.is_power_of_two());
        assert!(ACL_WRITE.is_power_of_two());
        assert!(ACL_READ.is_power_of_two());
    }

    #[test]
    fn test_acl_types() {
        assert_ne!(ACL_TYPE_ACCESS, ACL_TYPE_DEFAULT);
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
}
