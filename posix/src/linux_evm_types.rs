//! `<linux/evm.h>` — Extended Verification Module (EVM) constants.
//!
//! EVM protects the integrity of file metadata (extended attributes)
//! by computing an HMAC or digital signature over security-relevant
//! xattrs (security.selinux, security.ima, security.capability, etc.).
//! This prevents offline tampering with security labels. EVM stores
//! its HMAC/signature in the `security.evm` xattr. When a file's
//! metadata is accessed, EVM verifies the HMAC/signature matches
//! the current xattr values.

// ---------------------------------------------------------------------------
// EVM HMAC/signature types (stored in security.evm xattr)
// ---------------------------------------------------------------------------

/// No EVM protection.
pub const EVM_XATTR_NONE: u32 = 0;
/// HMAC (symmetric key, stored in TPM/keyring).
pub const EVM_XATTR_HMAC: u32 = 1;
/// Portable digital signature (asymmetric key).
pub const EVM_XATTR_PORTABLE_DIGSIG: u32 = 2;
/// Digital signature (platform-specific, includes UUID).
pub const EVM_XATTR_DIGSIG: u32 = 3;

// ---------------------------------------------------------------------------
// EVM initialization states
// ---------------------------------------------------------------------------

/// EVM is not initialized.
pub const EVM_STATE_DISABLED: u32 = 0;
/// EVM HMAC key is loaded.
pub const EVM_STATE_KEY_LOADED: u32 = 1;
/// EVM is fully initialized and enforcing.
pub const EVM_STATE_INITIALIZED: u32 = 2;

// ---------------------------------------------------------------------------
// EVM setup mask (written to /sys/kernel/security/evm)
// ---------------------------------------------------------------------------

/// Enable EVM HMAC validation.
pub const EVM_SETUP_HMAC: u32 = 1 << 0;
/// Enable EVM signature validation.
pub const EVM_SETUP_DIGSIG: u32 = 1 << 1;
/// Enable EVM for new files (compute HMAC on metadata changes).
pub const EVM_SETUP_NEW_FILE: u32 = 1 << 2;
/// Make EVM immutable (cannot be disabled after this).
pub const EVM_SETUP_IMMUTABLE: u32 = 1 << 31;

// ---------------------------------------------------------------------------
// EVM status codes (returned from verification)
// ---------------------------------------------------------------------------

/// Verification passed.
pub const EVM_STATUS_PASS: u32 = 0;
/// Verification failed (HMAC/signature mismatch).
pub const EVM_STATUS_FAIL: u32 = 1;
/// No security.evm xattr present.
pub const EVM_STATUS_NO_XATTR: u32 = 2;
/// Unknown xattr type.
pub const EVM_STATUS_UNKNOWN_TYPE: u32 = 3;
/// Key not available for verification.
pub const EVM_STATUS_NO_KEY: u32 = 4;

// ---------------------------------------------------------------------------
// EVM protected xattrs (xattrs included in HMAC computation)
// ---------------------------------------------------------------------------

/// security.selinux included in HMAC.
pub const EVM_PROTECT_SELINUX: u32 = 1 << 0;
/// security.ima included in HMAC.
pub const EVM_PROTECT_IMA: u32 = 1 << 1;
/// security.capability included in HMAC.
pub const EVM_PROTECT_CAPABILITY: u32 = 1 << 2;
/// security.apparmor included in HMAC.
pub const EVM_PROTECT_APPARMOR: u32 = 1 << 3;
/// security.smack* included in HMAC.
pub const EVM_PROTECT_SMACK: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xattr_types_distinct() {
        let types = [
            EVM_XATTR_NONE,
            EVM_XATTR_HMAC,
            EVM_XATTR_PORTABLE_DIGSIG,
            EVM_XATTR_DIGSIG,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            EVM_STATE_DISABLED,
            EVM_STATE_KEY_LOADED,
            EVM_STATE_INITIALIZED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_setup_flags_selective_overlap() {
        // HMAC, DIGSIG, NEW_FILE are non-overlapping bit flags
        let flags = [EVM_SETUP_HMAC, EVM_SETUP_DIGSIG, EVM_SETUP_NEW_FILE];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
        // IMMUTABLE is separate high bit
        assert!(EVM_SETUP_IMMUTABLE.is_power_of_two());
        assert_eq!(EVM_SETUP_IMMUTABLE, 1 << 31);
    }

    #[test]
    fn test_status_codes_distinct() {
        let codes = [
            EVM_STATUS_PASS,
            EVM_STATUS_FAIL,
            EVM_STATUS_NO_XATTR,
            EVM_STATUS_UNKNOWN_TYPE,
            EVM_STATUS_NO_KEY,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_protect_flags_no_overlap() {
        let flags = [
            EVM_PROTECT_SELINUX,
            EVM_PROTECT_IMA,
            EVM_PROTECT_CAPABILITY,
            EVM_PROTECT_APPARMOR,
            EVM_PROTECT_SMACK,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
