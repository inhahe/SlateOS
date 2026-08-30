//! `<linux/fscrypt.h>` — Filesystem encryption (fscrypt) constants.
//!
//! fscrypt provides transparent encryption of files and directories
//! at the filesystem layer. It's used by ext4, F2FS, and UBIFS for
//! per-file encryption with per-directory policies. Each file gets
//! a unique derived key; filenames in encrypted directories are also
//! encrypted.

// ---------------------------------------------------------------------------
// Encryption modes (contents encryption)
// ---------------------------------------------------------------------------

/// AES-256-XTS (default for file contents).
pub const FSCRYPT_MODE_AES_256_XTS: u8 = 1;
/// AES-256-CTS (for filenames).
pub const FSCRYPT_MODE_AES_256_CTS: u8 = 4;
/// AES-128-CBC (legacy, contents).
pub const FSCRYPT_MODE_AES_128_CBC: u8 = 5;
/// AES-128-CTS (legacy, filenames).
pub const FSCRYPT_MODE_AES_128_CTS: u8 = 6;
/// Adiantum (for low-end hardware without AES).
pub const FSCRYPT_MODE_ADIANTUM: u8 = 9;
/// AES-256-HCTR2.
pub const FSCRYPT_MODE_AES_256_HCTR2: u8 = 10;

// ---------------------------------------------------------------------------
// Policy version
// ---------------------------------------------------------------------------

/// v1 encryption policy.
pub const FSCRYPT_POLICY_V1: u8 = 0;
/// v2 encryption policy (recommended).
pub const FSCRYPT_POLICY_V2: u8 = 2;

// ---------------------------------------------------------------------------
// Policy flags
// ---------------------------------------------------------------------------

/// Use direct key derivation.
pub const FSCRYPT_POLICY_FLAG_DIRECT_KEY: u8 = 1 << 2;
/// Use IV_INO_LBLK_64 for IV generation.
pub const FSCRYPT_POLICY_FLAG_IV_INO_LBLK_64: u8 = 1 << 3;
/// Use IV_INO_LBLK_32 for IV generation.
pub const FSCRYPT_POLICY_FLAG_IV_INO_LBLK_32: u8 = 1 << 4;

// ---------------------------------------------------------------------------
// Key descriptor/identifier sizes
// ---------------------------------------------------------------------------

/// v1 key descriptor size (bytes).
pub const FSCRYPT_KEY_DESCRIPTOR_SIZE: u8 = 8;
/// v2 key identifier size (bytes).
pub const FSCRYPT_KEY_IDENTIFIER_SIZE: u8 = 16;
/// Maximum key size (bytes).
pub const FSCRYPT_MAX_KEY_SIZE: u8 = 64;

// ---------------------------------------------------------------------------
// Key specifier types
// ---------------------------------------------------------------------------

/// Key specified by descriptor (v1).
pub const FSCRYPT_KEY_SPEC_TYPE_DESCRIPTOR: u8 = 1;
/// Key specified by identifier (v2).
pub const FSCRYPT_KEY_SPEC_TYPE_IDENTIFIER: u8 = 2;

// ---------------------------------------------------------------------------
// Key status
// ---------------------------------------------------------------------------

/// Key is absent (not loaded).
pub const FSCRYPT_KEY_STATUS_ABSENT: u32 = 1;
/// Key is present.
pub const FSCRYPT_KEY_STATUS_PRESENT: u32 = 2;
/// Key incompletely removed (in-use files remain).
pub const FSCRYPT_KEY_STATUS_INCOMPLETELY_REMOVED: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_distinct() {
        let modes = [
            FSCRYPT_MODE_AES_256_XTS,
            FSCRYPT_MODE_AES_256_CTS,
            FSCRYPT_MODE_AES_128_CBC,
            FSCRYPT_MODE_AES_128_CTS,
            FSCRYPT_MODE_ADIANTUM,
            FSCRYPT_MODE_AES_256_HCTR2,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_policy_versions_distinct() {
        assert_ne!(FSCRYPT_POLICY_V1, FSCRYPT_POLICY_V2);
    }

    #[test]
    fn test_policy_flags_no_overlap() {
        let flags = [
            FSCRYPT_POLICY_FLAG_DIRECT_KEY,
            FSCRYPT_POLICY_FLAG_IV_INO_LBLK_64,
            FSCRYPT_POLICY_FLAG_IV_INO_LBLK_32,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_key_spec_types_distinct() {
        assert_ne!(
            FSCRYPT_KEY_SPEC_TYPE_DESCRIPTOR,
            FSCRYPT_KEY_SPEC_TYPE_IDENTIFIER
        );
    }

    #[test]
    fn test_key_status_distinct() {
        let statuses = [
            FSCRYPT_KEY_STATUS_ABSENT,
            FSCRYPT_KEY_STATUS_PRESENT,
            FSCRYPT_KEY_STATUS_INCOMPLETELY_REMOVED,
        ];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }

    #[test]
    fn test_key_sizes() {
        assert!(FSCRYPT_KEY_DESCRIPTOR_SIZE < FSCRYPT_KEY_IDENTIFIER_SIZE);
        assert!(FSCRYPT_KEY_IDENTIFIER_SIZE < FSCRYPT_MAX_KEY_SIZE);
    }
}
