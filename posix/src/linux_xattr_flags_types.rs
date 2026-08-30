//! `<sys/xattr.h>` — Extended attribute operation flag constants.
//!
//! Extended attributes (xattrs) are name-value pairs attached to
//! files and directories. These flags control the behavior of
//! `setxattr()`, `getxattr()`, `listxattr()`, and `removexattr()`
//! syscalls.

// ---------------------------------------------------------------------------
// setxattr() flags
// ---------------------------------------------------------------------------

/// Create the attribute; fail if it already exists.
pub const XATTR_CREATE: u32 = 0x1;
/// Replace the attribute; fail if it doesn't exist.
pub const XATTR_REPLACE: u32 = 0x2;

// ---------------------------------------------------------------------------
// Xattr namespace prefixes (string length limits)
// ---------------------------------------------------------------------------

/// Maximum length of an xattr name.
pub const XATTR_NAME_MAX: u32 = 255;
/// Maximum size of an xattr value.
pub const XATTR_SIZE_MAX: u32 = 65536;
/// Maximum total xattr list size per inode.
pub const XATTR_LIST_MAX: u32 = 65536;

// ---------------------------------------------------------------------------
// Well-known xattr namespace indices
// ---------------------------------------------------------------------------

/// User namespace index.
pub const XATTR_USER_PREFIX_INDEX: u32 = 1;
/// POSIX ACL access namespace index.
pub const XATTR_POSIX_ACL_ACCESS_INDEX: u32 = 2;
/// POSIX ACL default namespace index.
pub const XATTR_POSIX_ACL_DEFAULT_INDEX: u32 = 3;
/// Trusted namespace index.
pub const XATTR_TRUSTED_PREFIX_INDEX: u32 = 4;
/// Security namespace index (SELinux, etc.).
pub const XATTR_SECURITY_PREFIX_INDEX: u32 = 6;
/// System namespace index.
pub const XATTR_SYSTEM_PREFIX_INDEX: u32 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_replace_no_overlap() {
        assert_eq!(XATTR_CREATE & XATTR_REPLACE, 0);
    }

    #[test]
    fn test_create_replace_values() {
        assert_eq!(XATTR_CREATE, 0x1);
        assert_eq!(XATTR_REPLACE, 0x2);
    }

    #[test]
    fn test_name_size_limits() {
        assert_eq!(XATTR_NAME_MAX, 255);
        assert_eq!(XATTR_SIZE_MAX, 65536);
        assert_eq!(XATTR_LIST_MAX, 65536);
    }

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
}
