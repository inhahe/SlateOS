//! `<linux/xattr.h>` — Extended attribute namespace prefix constants.
//!
//! Extended attributes are name-value pairs attached to inodes.
//! The attribute name must begin with a namespace prefix that
//! determines access control semantics and visibility.

// ---------------------------------------------------------------------------
// Namespace prefix strings (as byte arrays for no_std compatibility)
// ---------------------------------------------------------------------------

/// "user." namespace prefix.
pub const XATTR_USER_PREFIX: &[u8] = b"user.";
/// "trusted." namespace prefix (root only).
pub const XATTR_TRUSTED_PREFIX: &[u8] = b"trusted.";
/// "security." namespace prefix (LSM labels).
pub const XATTR_SECURITY_PREFIX: &[u8] = b"security.";
/// "system." namespace prefix (ACLs, capabilities).
pub const XATTR_SYSTEM_PREFIX: &[u8] = b"system.";

// ---------------------------------------------------------------------------
// Namespace prefix lengths
// ---------------------------------------------------------------------------

/// Length of "user." prefix.
pub const XATTR_USER_PREFIX_LEN: u32 = 5;
/// Length of "trusted." prefix.
pub const XATTR_TRUSTED_PREFIX_LEN: u32 = 8;
/// Length of "security." prefix.
pub const XATTR_SECURITY_PREFIX_LEN: u32 = 9;
/// Length of "system." prefix.
pub const XATTR_SYSTEM_PREFIX_LEN: u32 = 7;

// ---------------------------------------------------------------------------
// Well-known attribute names
// ---------------------------------------------------------------------------

/// POSIX ACL access attribute name.
pub const XATTR_NAME_POSIX_ACL_ACCESS: &[u8] = b"system.posix_acl_access";
/// POSIX ACL default attribute name.
pub const XATTR_NAME_POSIX_ACL_DEFAULT: &[u8] = b"system.posix_acl_default";
/// SELinux security label attribute.
pub const XATTR_NAME_SELINUX: &[u8] = b"security.selinux";
/// Smack security label attribute.
pub const XATTR_NAME_SMACK: &[u8] = b"security.SMACK64";
/// Capability attribute name.
pub const XATTR_NAME_CAPS: &[u8] = b"security.capability";
/// IMA measurement attribute.
pub const XATTR_NAME_IMA: &[u8] = b"security.ima";
/// EVM signature attribute.
pub const XATTR_NAME_EVM: &[u8] = b"security.evm";

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum xattr name length (including null terminator).
pub const XATTR_NAME_MAX: u32 = 255;
/// Maximum xattr value size.
pub const XATTR_SIZE_MAX: u32 = 65536;
/// Maximum total xattr list size.
pub const XATTR_LIST_MAX: u32 = 65536;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prefix_lengths_match() {
        assert_eq!(XATTR_USER_PREFIX.len() as u32, XATTR_USER_PREFIX_LEN);
        assert_eq!(XATTR_TRUSTED_PREFIX.len() as u32, XATTR_TRUSTED_PREFIX_LEN);
        assert_eq!(
            XATTR_SECURITY_PREFIX.len() as u32,
            XATTR_SECURITY_PREFIX_LEN
        );
        assert_eq!(XATTR_SYSTEM_PREFIX.len() as u32, XATTR_SYSTEM_PREFIX_LEN);
    }

    #[test]
    fn test_prefixes_end_with_dot() {
        assert_eq!(XATTR_USER_PREFIX[XATTR_USER_PREFIX.len() - 1], b'.');
        assert_eq!(XATTR_TRUSTED_PREFIX[XATTR_TRUSTED_PREFIX.len() - 1], b'.');
        assert_eq!(XATTR_SECURITY_PREFIX[XATTR_SECURITY_PREFIX.len() - 1], b'.');
        assert_eq!(XATTR_SYSTEM_PREFIX[XATTR_SYSTEM_PREFIX.len() - 1], b'.');
    }

    #[test]
    fn test_well_known_names_start_with_prefix() {
        assert!(XATTR_NAME_POSIX_ACL_ACCESS.starts_with(XATTR_SYSTEM_PREFIX));
        assert!(XATTR_NAME_POSIX_ACL_DEFAULT.starts_with(XATTR_SYSTEM_PREFIX));
        assert!(XATTR_NAME_SELINUX.starts_with(XATTR_SECURITY_PREFIX));
        assert!(XATTR_NAME_CAPS.starts_with(XATTR_SECURITY_PREFIX));
    }

    #[test]
    fn test_name_max() {
        assert_eq!(XATTR_NAME_MAX, 255);
    }

    #[test]
    fn test_size_max() {
        assert_eq!(XATTR_SIZE_MAX, 65536);
    }

    #[test]
    fn test_list_max() {
        assert_eq!(XATTR_LIST_MAX, 65536);
    }
}
