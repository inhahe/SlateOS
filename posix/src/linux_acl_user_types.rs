//! `<sys/acl.h>` / `<linux/posix_acl.h>` — POSIX.1e ACLs.
//!
//! Linux stores POSIX ACLs as the extended attributes
//! `system.posix_acl_access` and `system.posix_acl_default`. The
//! userspace library `libacl` (`getfacl`, `setfacl`) parses and emits
//! the on-disk format defined by these constants.

// ---------------------------------------------------------------------------
// Extended-attribute names
// ---------------------------------------------------------------------------

pub const XATTR_NAME_POSIX_ACL_ACCESS: &str = "system.posix_acl_access";
pub const XATTR_NAME_POSIX_ACL_DEFAULT: &str = "system.posix_acl_default";

// ---------------------------------------------------------------------------
// On-disk header (`struct posix_acl_xattr_header`)
// ---------------------------------------------------------------------------

/// 0x0002_0000 little-endian on disk — magic number for v2 ACL xattrs.
pub const POSIX_ACL_XATTR_VERSION: u32 = 0x0002_0000;

/// Sentinel for "no owner/group id" in a `posix_acl_xattr_entry`.
pub const ACL_UNDEFINED_ID: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// ACL entry tag types (`e_tag` in `posix_acl_xattr_entry`)
// ---------------------------------------------------------------------------

pub const ACL_UNDEFINED_TAG: u16 = 0x00;
pub const ACL_USER_OBJ: u16 = 0x01;
pub const ACL_USER: u16 = 0x02;
pub const ACL_GROUP_OBJ: u16 = 0x04;
pub const ACL_GROUP: u16 = 0x08;
pub const ACL_MASK: u16 = 0x10;
pub const ACL_OTHER: u16 = 0x20;

// ---------------------------------------------------------------------------
// Permission bits (`e_perm`) — same numeric values as the low mode bits
// ---------------------------------------------------------------------------

pub const ACL_READ: u16 = 0x04;
pub const ACL_WRITE: u16 = 0x02;
pub const ACL_EXECUTE: u16 = 0x01;

// ---------------------------------------------------------------------------
// Entry-size constants
// ---------------------------------------------------------------------------

/// Each entry on-disk is `tag(2) + perm(2) + id(4) = 8` bytes.
pub const POSIX_ACL_XATTR_ENTRY_SIZE: usize = 8;

/// Header is the version field alone (4 bytes).
pub const POSIX_ACL_XATTR_HEADER_SIZE: usize = 4;

/// ACL count from byte length: `(len - header) / entry`.
#[must_use]
pub const fn acl_entry_count_from_len(len: usize) -> Option<usize> {
    if len < POSIX_ACL_XATTR_HEADER_SIZE {
        return None;
    }
    let body = len - POSIX_ACL_XATTR_HEADER_SIZE;
    if body % POSIX_ACL_XATTR_ENTRY_SIZE != 0 {
        return None;
    }
    Some(body / POSIX_ACL_XATTR_ENTRY_SIZE)
}

// ---------------------------------------------------------------------------
// Library return values used by `acl_*` functions
// ---------------------------------------------------------------------------

pub const ACL_TYPE_ACCESS: u32 = 0x8000;
pub const ACL_TYPE_DEFAULT: u32 = 0x4000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xattr_names_in_system_namespace() {
        assert!(XATTR_NAME_POSIX_ACL_ACCESS.starts_with("system."));
        assert!(XATTR_NAME_POSIX_ACL_DEFAULT.starts_with("system."));
        assert_ne!(XATTR_NAME_POSIX_ACL_ACCESS, XATTR_NAME_POSIX_ACL_DEFAULT);
    }

    #[test]
    fn test_version_and_undefined_sentinels() {
        assert_eq!(POSIX_ACL_XATTR_VERSION, 0x0002_0000);
        // ID -1 (cast to u32) is the "no id" sentinel for USER_OBJ etc.
        assert_eq!(ACL_UNDEFINED_ID, 0xFFFF_FFFF);
    }

    #[test]
    fn test_tag_bits_single_or_zero() {
        // Real tags are single bits in low 6 positions; UNDEFINED is 0.
        let t = [ACL_USER_OBJ, ACL_USER, ACL_GROUP_OBJ, ACL_GROUP, ACL_MASK, ACL_OTHER];
        let mut or = 0u16;
        for v in t {
            assert!(v.is_power_of_two());
            or |= v;
        }
        assert_eq!(or, 0x3F);
        assert_eq!(ACL_UNDEFINED_TAG, 0);
    }

    #[test]
    fn test_perm_bits_match_low_mode() {
        // ACL perms = stat mode low octet.
        assert_eq!(ACL_READ, 0o4);
        assert_eq!(ACL_WRITE, 0o2);
        assert_eq!(ACL_EXECUTE, 0o1);
        assert_eq!(ACL_READ | ACL_WRITE | ACL_EXECUTE, 0o7);
    }

    #[test]
    fn test_entry_count_from_len() {
        // Header only — zero entries.
        assert_eq!(acl_entry_count_from_len(4), Some(0));
        // Header + 1 entry = 12 bytes.
        assert_eq!(acl_entry_count_from_len(12), Some(1));
        // Header + 6 entries (minimum minimal ACL is 4: u_obj/g_obj/other/mask).
        assert_eq!(acl_entry_count_from_len(4 + 6 * 8), Some(6));
        // Misaligned — not a valid xattr blob.
        assert_eq!(acl_entry_count_from_len(13), None);
        // Truncated header.
        assert_eq!(acl_entry_count_from_len(3), None);
    }

    #[test]
    fn test_type_access_default_top_bits() {
        // Both ACL_TYPE_* are single bits but in the high half.
        assert!(ACL_TYPE_ACCESS.is_power_of_two());
        assert!(ACL_TYPE_DEFAULT.is_power_of_two());
        assert_ne!(ACL_TYPE_ACCESS, ACL_TYPE_DEFAULT);
    }
}
