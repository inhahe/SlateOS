//! `<sys/acl.h>` — POSIX 1003.1e draft 17 ACL ABI.
//!
//! Linux stores POSIX ACLs as extended attributes
//! (`system.posix_acl_access`, `system.posix_acl_default`). `getfacl`,
//! `setfacl`, Samba, and NFSv4-ACL translators all encode and decode
//! the on-disk binary form using the tag types and permission bits
//! defined here.

// ---------------------------------------------------------------------------
// Permission bits (mirror `r/w/x` from `mode_t`)
// ---------------------------------------------------------------------------

pub const ACL_READ: u16 = 0x04;
pub const ACL_WRITE: u16 = 0x02;
pub const ACL_EXECUTE: u16 = 0x01;
pub const ACL_PERM_MASK: u16 = ACL_READ | ACL_WRITE | ACL_EXECUTE;

// ---------------------------------------------------------------------------
// ACL entry tag types (`enum acl_tag`)
// ---------------------------------------------------------------------------

pub const ACL_UNDEFINED_TAG: u16 = 0x00;
pub const ACL_USER_OBJ: u16 = 0x01;
pub const ACL_USER: u16 = 0x02;
pub const ACL_GROUP_OBJ: u16 = 0x04;
pub const ACL_GROUP: u16 = 0x08;
pub const ACL_MASK: u16 = 0x10;
pub const ACL_OTHER: u16 = 0x20;

// ---------------------------------------------------------------------------
// ACL types passed to `acl_get_file`/`acl_set_file`
// ---------------------------------------------------------------------------

pub const ACL_TYPE_ACCESS: u32 = 0x8000;
pub const ACL_TYPE_DEFAULT: u32 = 0x4000;

// ---------------------------------------------------------------------------
// xattr names that carry the binary ACL payload
// ---------------------------------------------------------------------------

pub const XATTR_NAME_POSIX_ACL_ACCESS: &str = "system.posix_acl_access";
pub const XATTR_NAME_POSIX_ACL_DEFAULT: &str = "system.posix_acl_default";

// ---------------------------------------------------------------------------
// On-disk header constants (`POSIX_ACL_XATTR_VERSION`)
// ---------------------------------------------------------------------------

pub const POSIX_ACL_XATTR_VERSION: u32 = 0x0002;
/// `ACL_UNDEFINED_ID` — sentinel id used in mask/other entries.
pub const ACL_UNDEFINED_ID: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_perm_bits_single_bit_low_3() {
        let p = [ACL_EXECUTE, ACL_WRITE, ACL_READ];
        for v in p {
            assert!(v.is_power_of_two());
        }
        assert_eq!(ACL_PERM_MASK, 0x7);
        // Same bit layout as `S_IRWX*`.
        assert_eq!(ACL_READ, 0x04);
        assert_eq!(ACL_WRITE, 0x02);
        assert_eq!(ACL_EXECUTE, 0x01);
    }

    #[test]
    fn test_tag_types_single_bit_distinct() {
        let t = [
            ACL_USER_OBJ,
            ACL_USER,
            ACL_GROUP_OBJ,
            ACL_GROUP,
            ACL_MASK,
            ACL_OTHER,
        ];
        for v in t {
            assert!(v.is_power_of_two());
        }
        // The six tag bits cover 0x01, 0x02, 0x04, 0x08, 0x10, 0x20.
        let or = t.iter().fold(0u16, |a, &v| a | v);
        assert_eq!(or, 0x3F);
        assert_eq!(ACL_UNDEFINED_TAG, 0);
    }

    #[test]
    fn test_acl_types_distinct_high_bits() {
        // ACCESS / DEFAULT pick different high bits (0x8000 vs 0x4000).
        assert!(ACL_TYPE_ACCESS.is_power_of_two());
        assert!(ACL_TYPE_DEFAULT.is_power_of_two());
        assert_ne!(ACL_TYPE_ACCESS, ACL_TYPE_DEFAULT);
        assert_eq!(ACL_TYPE_ACCESS, 0x8000);
        assert_eq!(ACL_TYPE_DEFAULT, 0x4000);
    }

    #[test]
    fn test_xattr_names_under_system_prefix() {
        assert!(XATTR_NAME_POSIX_ACL_ACCESS.starts_with("system.posix_acl_"));
        assert!(XATTR_NAME_POSIX_ACL_DEFAULT.starts_with("system.posix_acl_"));
        assert_ne!(XATTR_NAME_POSIX_ACL_ACCESS, XATTR_NAME_POSIX_ACL_DEFAULT);
    }

    #[test]
    fn test_xattr_version_and_undefined_id() {
        // The on-disk header version field is 2.
        assert_eq!(POSIX_ACL_XATTR_VERSION, 0x0002);
        // The "no id" sentinel is all-ones (uid_t/gid_t max).
        assert_eq!(ACL_UNDEFINED_ID, u32::MAX);
    }
}
