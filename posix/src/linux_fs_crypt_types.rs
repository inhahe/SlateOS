//! `<linux/fscrypt.h>` — Filesystem encryption (fscrypt) constants.
//!
//! fscrypt provides transparent file-level encryption.
//! These constants define encryption modes, policy versions,
//! key specifier types, and IOCTL commands.

// ---------------------------------------------------------------------------
// Encryption modes (FSCRYPT_MODE_*)
// ---------------------------------------------------------------------------

/// AES-256-XTS for file contents.
pub const FSCRYPT_MODE_AES_256_XTS: u32 = 1;
/// AES-256-CTS-CBC for filenames.
pub const FSCRYPT_MODE_AES_256_CTS: u32 = 4;
/// AES-128-CBC-ESSIV for contents (low-power).
pub const FSCRYPT_MODE_AES_128_CBC: u32 = 5;
/// AES-128-CTS-CBC for filenames (low-power).
pub const FSCRYPT_MODE_AES_128_CTS: u32 = 6;
/// Adiantum for both contents and filenames.
pub const FSCRYPT_MODE_ADIANTUM: u32 = 9;
/// AES-256-HCTR2 for contents.
pub const FSCRYPT_MODE_AES_256_HCTR2: u32 = 10;
/// SM4-XTS for contents.
pub const FSCRYPT_MODE_SM4_XTS: u32 = 11;
/// SM4-CTS-CBC for filenames.
pub const FSCRYPT_MODE_SM4_CTS: u32 = 12;

// ---------------------------------------------------------------------------
// Encryption policy versions
// ---------------------------------------------------------------------------

/// Policy version 1 (legacy).
pub const FSCRYPT_POLICY_V1: u32 = 0;
/// Policy version 2 (current).
pub const FSCRYPT_POLICY_V2: u32 = 2;

// ---------------------------------------------------------------------------
// Encryption policy flags
// ---------------------------------------------------------------------------

/// Allow padding of filenames to 4 bytes.
pub const FSCRYPT_POLICY_FLAGS_PAD_4: u32 = 0x00;
/// Pad filenames to 8 bytes.
pub const FSCRYPT_POLICY_FLAGS_PAD_8: u32 = 0x01;
/// Pad filenames to 16 bytes.
pub const FSCRYPT_POLICY_FLAGS_PAD_16: u32 = 0x02;
/// Pad filenames to 32 bytes.
pub const FSCRYPT_POLICY_FLAGS_PAD_32: u32 = 0x03;
/// Padding mask.
pub const FSCRYPT_POLICY_FLAGS_PAD_MASK: u32 = 0x03;
/// Direct key derivation.
pub const FSCRYPT_POLICY_FLAG_DIRECT_KEY: u32 = 0x04;
/// IV from inode number.
pub const FSCRYPT_POLICY_FLAG_IV_INO_LBLK_64: u32 = 0x08;
/// IV from inode + logical block (32-bit).
pub const FSCRYPT_POLICY_FLAG_IV_INO_LBLK_32: u32 = 0x10;

// ---------------------------------------------------------------------------
// Key specifier types
// ---------------------------------------------------------------------------

/// Key by descriptor (v1).
pub const FSCRYPT_KEY_SPEC_TYPE_DESCRIPTOR: u32 = 1;
/// Key by identifier (v2).
pub const FSCRYPT_KEY_SPEC_TYPE_IDENTIFIER: u32 = 2;

// ---------------------------------------------------------------------------
// Key descriptor/identifier sizes
// ---------------------------------------------------------------------------

/// v1 key descriptor size.
pub const FSCRYPT_KEY_DESCRIPTOR_SIZE: u32 = 8;
/// v2 key identifier size.
pub const FSCRYPT_KEY_IDENTIFIER_SIZE: u32 = 16;
/// Maximum key size.
pub const FSCRYPT_MAX_KEY_SIZE: u32 = 64;

// ---------------------------------------------------------------------------
// fscrypt IOCTL commands
// ---------------------------------------------------------------------------

/// Set encryption policy.
pub const FS_IOC_SET_ENCRYPTION_POLICY: u32 = 0x800C6613;
/// Get encryption policy.
pub const FS_IOC_GET_ENCRYPTION_POLICY: u32 = 0x400C6615;
/// Get encryption policy (v2).
pub const FS_IOC_GET_ENCRYPTION_POLICY_EX: u32 = 0xC0096616;
/// Add encryption key.
pub const FS_IOC_ADD_ENCRYPTION_KEY: u32 = 0xC0506617;
/// Remove encryption key.
pub const FS_IOC_REMOVE_ENCRYPTION_KEY: u32 = 0xC0406618;
/// Remove encryption key (all users).
pub const FS_IOC_REMOVE_ENCRYPTION_KEY_ALL_USERS: u32 = 0xC0406619;
/// Get encryption key status.
pub const FS_IOC_GET_ENCRYPTION_KEY_STATUS: u32 = 0xC080661A;
/// Get nonce.
pub const FS_IOC_GET_ENCRYPTION_NONCE: u32 = 0x8010661B;

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
            FSCRYPT_MODE_SM4_XTS,
            FSCRYPT_MODE_SM4_CTS,
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
    fn test_key_spec_types_distinct() {
        assert_ne!(
            FSCRYPT_KEY_SPEC_TYPE_DESCRIPTOR,
            FSCRYPT_KEY_SPEC_TYPE_IDENTIFIER
        );
    }

    #[test]
    fn test_ioctls_distinct() {
        let cmds = [
            FS_IOC_SET_ENCRYPTION_POLICY,
            FS_IOC_GET_ENCRYPTION_POLICY,
            FS_IOC_GET_ENCRYPTION_POLICY_EX,
            FS_IOC_ADD_ENCRYPTION_KEY,
            FS_IOC_REMOVE_ENCRYPTION_KEY,
            FS_IOC_REMOVE_ENCRYPTION_KEY_ALL_USERS,
            FS_IOC_GET_ENCRYPTION_KEY_STATUS,
            FS_IOC_GET_ENCRYPTION_NONCE,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_descriptor_size() {
        assert_eq!(FSCRYPT_KEY_DESCRIPTOR_SIZE, 8);
    }

    #[test]
    fn test_identifier_size() {
        assert_eq!(FSCRYPT_KEY_IDENTIFIER_SIZE, 16);
    }

    #[test]
    fn test_max_key_size() {
        assert_eq!(FSCRYPT_MAX_KEY_SIZE, 64);
    }
}
