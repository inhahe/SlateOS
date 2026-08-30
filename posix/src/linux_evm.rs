//! `<linux/evm.h>` — Extended Verification Module constants.
//!
//! EVM protects file metadata (xattrs) integrity using HMAC or
//! digital signatures. It ensures that security-relevant attributes
//! (security.ima, security.selinux, etc.) cannot be tampered with
//! offline, complementing IMA's file content integrity.

// ---------------------------------------------------------------------------
// EVM status flags
// ---------------------------------------------------------------------------

/// EVM initialized.
pub const EVM_INIT: u32 = 1 << 0;
/// EVM HMAC key loaded.
pub const EVM_SETUP: u32 = 1 << 1;
/// EVM is protecting metadata.
pub const EVM_ACTIVE: u32 = 1 << 2;
/// EVM allows unprotected xattr updates.
pub const EVM_ALLOW_METADATA_WRITES: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// EVM signature types
// ---------------------------------------------------------------------------

/// HMAC-based (symmetric key in kernel keyring).
pub const EVM_XATTR_HMAC: u8 = 1;
/// Digital signature (public key verification).
pub const EVM_XATTR_PORTABLE_DIGSIG: u8 = 2;
/// IMA signature (combined IMA+EVM).
pub const EVM_IMA_XATTR_DIGSIG: u8 = 3;

// ---------------------------------------------------------------------------
// EVM integrity status
// ---------------------------------------------------------------------------

/// Integrity verified OK.
pub const INTEGRITY_PASS: i8 = 0;
/// Integrity check failed.
pub const INTEGRITY_FAIL: i8 = -1;
/// No xattr present (not protected).
pub const INTEGRITY_NOLABEL: i8 = -2;
/// xattr present but no key to verify.
pub const INTEGRITY_NOXATTR: i8 = -3;
/// Integrity unknown (not yet checked).
pub const INTEGRITY_UNKNOWN: i8 = -4;

// ---------------------------------------------------------------------------
// Protected xattr names (that EVM guards)
// ---------------------------------------------------------------------------

/// IMA measurement xattr.
pub const EVM_XATTR_IMA: &str = "security.ima";
/// SELinux context xattr.
pub const EVM_XATTR_SELINUX: &str = "security.selinux";
/// SMACK label xattr.
pub const EVM_XATTR_SMACK: &str = "security.SMACK64";
/// AppArmor profile xattr.
pub const EVM_XATTR_APPARMOR: &str = "security.apparmor";
/// Capabilities xattr.
pub const EVM_XATTR_CAPS: &str = "security.capability";
/// EVM signature xattr.
pub const EVM_XATTR_EVM: &str = "security.evm";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_flags_no_overlap() {
        let flags = [EVM_INIT, EVM_SETUP, EVM_ACTIVE, EVM_ALLOW_METADATA_WRITES];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_sig_types_distinct() {
        let types = [
            EVM_XATTR_HMAC,
            EVM_XATTR_PORTABLE_DIGSIG,
            EVM_IMA_XATTR_DIGSIG,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_integrity_status_distinct() {
        let statuses = [
            INTEGRITY_PASS,
            INTEGRITY_FAIL,
            INTEGRITY_NOLABEL,
            INTEGRITY_NOXATTR,
            INTEGRITY_UNKNOWN,
        ];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }

    #[test]
    fn test_xattr_names_distinct() {
        let names = [
            EVM_XATTR_IMA,
            EVM_XATTR_SELINUX,
            EVM_XATTR_SMACK,
            EVM_XATTR_APPARMOR,
            EVM_XATTR_CAPS,
            EVM_XATTR_EVM,
        ];
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                assert_ne!(names[i], names[j]);
            }
        }
    }
}
