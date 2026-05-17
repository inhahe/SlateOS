//! `<linux/xattr.h>` — Extended attribute namespace constants.
//!
//! Extended attributes (xattrs) are name-value pairs associated with
//! files and directories. They are organized into namespaces that
//! control access: user (any process), system (kernel use), security
//! (LSM labels), and trusted (root only). xattrs store metadata like
//! SELinux labels, POSIX ACLs, file capabilities, and user-defined data.

// ---------------------------------------------------------------------------
// Namespace prefixes
// ---------------------------------------------------------------------------

/// User namespace prefix.
pub const XATTR_USER_PREFIX: &str = "user.";
/// System namespace prefix (ACLs, etc).
pub const XATTR_SYSTEM_PREFIX: &str = "system.";
/// Security namespace prefix (SELinux, Smack, AppArmor).
pub const XATTR_SECURITY_PREFIX: &str = "security.";
/// Trusted namespace prefix (root-only).
pub const XATTR_TRUSTED_PREFIX: &str = "trusted.";

// ---------------------------------------------------------------------------
// Well-known xattr names
// ---------------------------------------------------------------------------

/// SELinux security label.
pub const XATTR_NAME_SELINUX: &str = "security.selinux";
/// Smack label.
pub const XATTR_NAME_SMACK: &str = "security.SMACK64";
/// AppArmor profile.
pub const XATTR_NAME_APPARMOR: &str = "security.apparmor";
/// File capabilities.
pub const XATTR_NAME_CAPS: &str = "security.capability";
/// POSIX ACL (access).
pub const XATTR_NAME_POSIX_ACL_ACCESS: &str = "system.posix_acl_access";
/// POSIX ACL (default).
pub const XATTR_NAME_POSIX_ACL_DEFAULT: &str = "system.posix_acl_default";

// ---------------------------------------------------------------------------
// xattr size limits
// ---------------------------------------------------------------------------

/// Maximum xattr name length (including namespace prefix).
pub const XATTR_NAME_MAX: u32 = 255;
/// Maximum xattr value size.
pub const XATTR_SIZE_MAX: u32 = 65536;
/// Maximum total xattr list size for a file.
pub const XATTR_LIST_MAX: u32 = 65536;

// ---------------------------------------------------------------------------
// xattr flags (for setxattr)
// ---------------------------------------------------------------------------

/// Create: fail if attribute already exists.
pub const XATTR_CREATE: u32 = 0x1;
/// Replace: fail if attribute does not exist.
pub const XATTR_REPLACE: u32 = 0x2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prefixes_distinct() {
        let prefixes = [
            XATTR_USER_PREFIX, XATTR_SYSTEM_PREFIX,
            XATTR_SECURITY_PREFIX, XATTR_TRUSTED_PREFIX,
        ];
        for i in 0..prefixes.len() {
            for j in (i + 1)..prefixes.len() {
                assert_ne!(prefixes[i], prefixes[j]);
            }
        }
    }

    #[test]
    fn test_prefixes_end_with_dot() {
        assert!(XATTR_USER_PREFIX.ends_with('.'));
        assert!(XATTR_SYSTEM_PREFIX.ends_with('.'));
        assert!(XATTR_SECURITY_PREFIX.ends_with('.'));
        assert!(XATTR_TRUSTED_PREFIX.ends_with('.'));
    }

    #[test]
    fn test_well_known_names_have_namespace() {
        assert!(XATTR_NAME_SELINUX.starts_with(XATTR_SECURITY_PREFIX));
        assert!(XATTR_NAME_SMACK.starts_with(XATTR_SECURITY_PREFIX));
        assert!(XATTR_NAME_APPARMOR.starts_with(XATTR_SECURITY_PREFIX));
        assert!(XATTR_NAME_CAPS.starts_with(XATTR_SECURITY_PREFIX));
        assert!(XATTR_NAME_POSIX_ACL_ACCESS.starts_with(XATTR_SYSTEM_PREFIX));
        assert!(XATTR_NAME_POSIX_ACL_DEFAULT.starts_with(XATTR_SYSTEM_PREFIX));
    }

    #[test]
    fn test_size_limits_positive() {
        assert!(XATTR_NAME_MAX > 0);
        assert!(XATTR_SIZE_MAX > 0);
        assert!(XATTR_LIST_MAX > 0);
    }

    #[test]
    fn test_flags_distinct() {
        assert_ne!(XATTR_CREATE, XATTR_REPLACE);
        assert_eq!(XATTR_CREATE & XATTR_REPLACE, 0);
    }
}
