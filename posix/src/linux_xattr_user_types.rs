//! `<linux/xattr.h>` — extended-attribute namespace prefixes and limits.
//!
//! POSIX xattrs are key/value pairs stored alongside file inodes.
//! Namespace prefixes (`user.`, `trusted.`, `system.`, `security.`)
//! gate which UIDs can read/write the attribute. ACLs, SELinux
//! labels, and capability sets all live in well-known names.

// ---------------------------------------------------------------------------
// Size limits (struct getxattr / setxattr)
// ---------------------------------------------------------------------------

/// Maximum xattr name length (including the namespace prefix).
pub const XATTR_NAME_MAX: usize = 255;
/// Maximum xattr value size on most filesystems (ext4, xfs allow more).
pub const XATTR_SIZE_MAX: usize = 65_536;
/// Maximum total size of a `listxattr()` reply.
pub const XATTR_LIST_MAX: usize = 65_536;

// ---------------------------------------------------------------------------
// Namespace prefixes (string + length)
// ---------------------------------------------------------------------------

/// `user.` — userspace-arbitrary xattrs.
pub const XATTR_USER_PREFIX: &str = "user.";
/// Length of `user.` prefix.
pub const XATTR_USER_PREFIX_LEN: usize = 5;

/// `trusted.` — privileged userspace xattrs (CAP_SYS_ADMIN).
pub const XATTR_TRUSTED_PREFIX: &str = "trusted.";
/// Length of `trusted.` prefix.
pub const XATTR_TRUSTED_PREFIX_LEN: usize = 8;

/// `system.` — system xattrs (POSIX ACLs, etc).
pub const XATTR_SYSTEM_PREFIX: &str = "system.";
/// Length of `system.` prefix.
pub const XATTR_SYSTEM_PREFIX_LEN: usize = 7;

/// `security.` — LSM xattrs.
pub const XATTR_SECURITY_PREFIX: &str = "security.";
/// Length of `security.` prefix.
pub const XATTR_SECURITY_PREFIX_LEN: usize = 9;

// ---------------------------------------------------------------------------
// Well-known attribute names
// ---------------------------------------------------------------------------

/// `system.posix_acl_access`.
pub const XATTR_NAME_POSIX_ACL_ACCESS: &str = "system.posix_acl_access";
/// `system.posix_acl_default`.
pub const XATTR_NAME_POSIX_ACL_DEFAULT: &str = "system.posix_acl_default";
/// `security.capability` — POSIX file capabilities.
pub const XATTR_NAME_CAPS: &str = "security.capability";
/// `security.selinux` — SELinux label.
pub const XATTR_NAME_SELINUX: &str = "security.selinux";
/// `security.evm` — EVM/IMA HMAC.
pub const XATTR_NAME_EVM: &str = "security.evm";
/// `security.ima` — IMA hash.
pub const XATTR_NAME_IMA: &str = "security.ima";

// ---------------------------------------------------------------------------
// setxattr flags
// ---------------------------------------------------------------------------

/// `XATTR_CREATE` — fail if the attribute already exists (EEXIST).
pub const XATTR_CREATE: u32 = 0x1;
/// `XATTR_REPLACE` — fail if the attribute doesn't exist (ENODATA).
pub const XATTR_REPLACE: u32 = 0x2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_size_limits() {
        // 255 matches NAME_MAX — the inode-level filename limit.
        assert_eq!(XATTR_NAME_MAX, 255);
        // 64 KiB is the kernel default and matches every common fs.
        assert_eq!(XATTR_SIZE_MAX, 65_536);
        assert!(XATTR_SIZE_MAX.is_power_of_two());
        assert_eq!(XATTR_LIST_MAX, XATTR_SIZE_MAX);
    }

    #[test]
    fn test_prefix_lengths_match_strings() {
        // The *_LEN constants are widely used in kernel-style
        // strcmp(name, PFX, LEN) checks — keep them consistent.
        assert_eq!(XATTR_USER_PREFIX.len(), XATTR_USER_PREFIX_LEN);
        assert_eq!(XATTR_TRUSTED_PREFIX.len(), XATTR_TRUSTED_PREFIX_LEN);
        assert_eq!(XATTR_SYSTEM_PREFIX.len(), XATTR_SYSTEM_PREFIX_LEN);
        assert_eq!(XATTR_SECURITY_PREFIX.len(), XATTR_SECURITY_PREFIX_LEN);
    }

    #[test]
    fn test_prefixes_end_in_dot() {
        for p in [
            XATTR_USER_PREFIX,
            XATTR_TRUSTED_PREFIX,
            XATTR_SYSTEM_PREFIX,
            XATTR_SECURITY_PREFIX,
        ] {
            assert!(p.ends_with('.'));
        }
    }

    #[test]
    fn test_well_known_names_use_expected_prefix() {
        assert!(XATTR_NAME_POSIX_ACL_ACCESS.starts_with(XATTR_SYSTEM_PREFIX));
        assert!(XATTR_NAME_POSIX_ACL_DEFAULT.starts_with(XATTR_SYSTEM_PREFIX));
        assert!(XATTR_NAME_CAPS.starts_with(XATTR_SECURITY_PREFIX));
        assert!(XATTR_NAME_SELINUX.starts_with(XATTR_SECURITY_PREFIX));
        assert!(XATTR_NAME_EVM.starts_with(XATTR_SECURITY_PREFIX));
        assert!(XATTR_NAME_IMA.starts_with(XATTR_SECURITY_PREFIX));
    }

    #[test]
    fn test_setxattr_flags_pow2_distinct() {
        assert!(XATTR_CREATE.is_power_of_two());
        assert!(XATTR_REPLACE.is_power_of_two());
        assert_ne!(XATTR_CREATE, XATTR_REPLACE);
        // Passing both is invalid (EINVAL) — they're a 2-state enum.
    }
}
