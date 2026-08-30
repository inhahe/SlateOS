//! `<linux/xattr.h>` — Additional extended attribute constants.
//!
//! Supplementary xattr constants covering namespace prefixes,
//! flags, and size limits.

// ---------------------------------------------------------------------------
// Xattr namespace indices
// ---------------------------------------------------------------------------

/// User namespace.
pub const XATTR_USER_PREFIX_INDEX: u32 = 1;
/// POSIX ACL access.
pub const XATTR_POSIX_ACL_ACCESS_INDEX: u32 = 2;
/// POSIX ACL default.
pub const XATTR_POSIX_ACL_DEFAULT_INDEX: u32 = 3;
/// Trusted namespace.
pub const XATTR_TRUSTED_PREFIX_INDEX: u32 = 4;
/// Security namespace.
pub const XATTR_SECURITY_PREFIX_INDEX: u32 = 6;
/// System namespace.
pub const XATTR_SYSTEM_PREFIX_INDEX: u32 = 7;

// ---------------------------------------------------------------------------
// Xattr flags
// ---------------------------------------------------------------------------

/// Create only (fail if exists).
pub const XATTR_CREATE: u32 = 0x1;
/// Replace only (fail if not exists).
pub const XATTR_REPLACE: u32 = 0x2;

// ---------------------------------------------------------------------------
// Size limits
// ---------------------------------------------------------------------------

/// Maximum xattr name length.
pub const XATTR_NAME_MAX: u32 = 255;
/// Maximum xattr value size.
pub const XATTR_SIZE_MAX: u32 = 65536;
/// Maximum xattr list size.
pub const XATTR_LIST_MAX: u32 = 65536;

// ---------------------------------------------------------------------------
// Namespace prefix strings (as byte lengths)
// ---------------------------------------------------------------------------

/// "security." prefix length.
pub const XATTR_SECURITY_PREFIX_LEN: u32 = 9;
/// "system." prefix length.
pub const XATTR_SYSTEM_PREFIX_LEN: u32 = 7;
/// "trusted." prefix length.
pub const XATTR_TRUSTED_PREFIX_LEN: u32 = 8;
/// "user." prefix length.
pub const XATTR_USER_PREFIX_LEN: u32 = 5;

// ---------------------------------------------------------------------------
// Common xattr names (length of well-known names)
// ---------------------------------------------------------------------------

/// SELinux label.
pub const XATTR_SELINUX_LEN: u32 = 17;
/// SMACK label.
pub const XATTR_SMACK_LEN: u32 = 12;
/// Capability xattr.
pub const XATTR_CAPS_LEN: u32 = 20;
/// POSIX ACL access name length.
pub const XATTR_POSIX_ACL_ACCESS_LEN: u32 = 23;
/// POSIX ACL default name length.
pub const XATTR_POSIX_ACL_DEFAULT_LEN: u32 = 24;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_namespace_indices_distinct() {
        let indices = [
            XATTR_USER_PREFIX_INDEX,
            XATTR_POSIX_ACL_ACCESS_INDEX,
            XATTR_POSIX_ACL_DEFAULT_INDEX,
            XATTR_TRUSTED_PREFIX_INDEX,
            XATTR_SECURITY_PREFIX_INDEX,
            XATTR_SYSTEM_PREFIX_INDEX,
        ];
        for i in 0..indices.len() {
            for j in (i + 1)..indices.len() {
                assert_ne!(indices[i], indices[j]);
            }
        }
    }

    #[test]
    fn test_flags() {
        assert_eq!(XATTR_CREATE, 0x1);
        assert_eq!(XATTR_REPLACE, 0x2);
        assert_eq!(XATTR_CREATE & XATTR_REPLACE, 0);
    }

    #[test]
    fn test_size_limits() {
        assert_eq!(XATTR_NAME_MAX, 255);
        assert_eq!(XATTR_SIZE_MAX, 65536);
        assert_eq!(XATTR_LIST_MAX, 65536);
    }

    #[test]
    fn test_prefix_lengths() {
        assert_eq!(XATTR_USER_PREFIX_LEN, 5);
        assert_eq!(XATTR_SYSTEM_PREFIX_LEN, 7);
        assert_eq!(XATTR_TRUSTED_PREFIX_LEN, 8);
        assert_eq!(XATTR_SECURITY_PREFIX_LEN, 9);
    }

    #[test]
    fn test_prefix_lengths_ordering() {
        assert!(XATTR_USER_PREFIX_LEN < XATTR_SYSTEM_PREFIX_LEN);
        assert!(XATTR_SYSTEM_PREFIX_LEN < XATTR_TRUSTED_PREFIX_LEN);
        assert!(XATTR_TRUSTED_PREFIX_LEN < XATTR_SECURITY_PREFIX_LEN);
    }
}
