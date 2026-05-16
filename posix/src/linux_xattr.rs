//! `<linux/xattr.h>` — extended attribute constants and prefixes.
//!
//! Re-exports `XATTR_CREATE`/`XATTR_REPLACE` and adds the standard
//! namespace prefix strings.

pub use crate::xattr::XATTR_CREATE;
pub use crate::xattr::XATTR_REPLACE;

// ---------------------------------------------------------------------------
// Namespace prefixes
// ---------------------------------------------------------------------------

/// Security namespace prefix.
pub const XATTR_SECURITY_PREFIX: &str = "security.";
/// System namespace prefix.
pub const XATTR_SYSTEM_PREFIX: &str = "system.";
/// Trusted namespace prefix.
pub const XATTR_TRUSTED_PREFIX: &str = "trusted.";
/// User namespace prefix.
pub const XATTR_USER_PREFIX: &str = "user.";

/// Security prefix length (including the dot).
pub const XATTR_SECURITY_PREFIX_LEN: usize = 9;
/// System prefix length.
pub const XATTR_SYSTEM_PREFIX_LEN: usize = 7;
/// Trusted prefix length.
pub const XATTR_TRUSTED_PREFIX_LEN: usize = 8;
/// User prefix length.
pub const XATTR_USER_PREFIX_LEN: usize = 5;

// ---------------------------------------------------------------------------
// Well-known xattr names
// ---------------------------------------------------------------------------

/// POSIX ACL (access).
pub const XATTR_NAME_POSIX_ACL_ACCESS: &str = "system.posix_acl_access";
/// POSIX ACL (default).
pub const XATTR_NAME_POSIX_ACL_DEFAULT: &str = "system.posix_acl_default";
/// Security capability.
pub const XATTR_NAME_CAPS: &str = "security.capability";
/// SELinux label.
pub const XATTR_NAME_SELINUX: &str = "security.selinux";
/// Smack label.
pub const XATTR_NAME_SMACK: &str = "security.SMACK64";

// ---------------------------------------------------------------------------
// Limits (re-exports from linux_limits if available, or define here)
// ---------------------------------------------------------------------------

/// Maximum xattr name length.
pub const XATTR_NAME_MAX: usize = 255;
/// Maximum xattr value size.
pub const XATTR_SIZE_MAX: usize = 65536;
/// Maximum xattr list size.
pub const XATTR_LIST_MAX: usize = 65536;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_replace_flags() {
        assert_ne!(XATTR_CREATE, XATTR_REPLACE);
    }

    #[test]
    fn test_prefix_lengths() {
        assert_eq!(XATTR_SECURITY_PREFIX.len(), XATTR_SECURITY_PREFIX_LEN);
        assert_eq!(XATTR_SYSTEM_PREFIX.len(), XATTR_SYSTEM_PREFIX_LEN);
        assert_eq!(XATTR_TRUSTED_PREFIX.len(), XATTR_TRUSTED_PREFIX_LEN);
        assert_eq!(XATTR_USER_PREFIX.len(), XATTR_USER_PREFIX_LEN);
    }

    #[test]
    fn test_well_known_names() {
        assert!(XATTR_NAME_CAPS.starts_with(XATTR_SECURITY_PREFIX));
        assert!(XATTR_NAME_SELINUX.starts_with(XATTR_SECURITY_PREFIX));
        assert!(XATTR_NAME_POSIX_ACL_ACCESS.starts_with(XATTR_SYSTEM_PREFIX));
    }

    #[test]
    fn test_limits() {
        assert_eq!(XATTR_NAME_MAX, 255);
        assert_eq!(XATTR_SIZE_MAX, 65536);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(XATTR_CREATE, crate::xattr::XATTR_CREATE);
    }
}
