//! `<linux/fscrypt.h>` — Filesystem encryption (fscrypt) constants.
//!
//! fscrypt provides transparent file-level encryption for ext4, F2FS,
//! and UBIFS. Each file/directory can have its own encryption policy
//! specifying the key identifier, encryption mode, and filename
//! padding. Keys are managed through the kernel keyring.

// ---------------------------------------------------------------------------
// Encryption modes
// ---------------------------------------------------------------------------

/// AES-256-XTS for file contents.
pub const FSCRYPT_MODE_AES_256_XTS: u32 = 1;
/// AES-256-CTS-CBC for filenames.
pub const FSCRYPT_MODE_AES_256_CTS: u32 = 4;
/// AES-128-CBC-ESSIV for contents (low-end hardware).
pub const FSCRYPT_MODE_AES_128_CBC: u32 = 5;
/// AES-128-CTS-CBC for filenames (low-end hardware).
pub const FSCRYPT_MODE_AES_128_CTS: u32 = 6;
/// Adiantum (ChaCha20 + Poly1305) for contents/filenames.
pub const FSCRYPT_MODE_ADIANTUM: u32 = 9;
/// AES-256-HCTR2 for filenames (hardware AES w/o XTS).
pub const FSCRYPT_MODE_AES_256_HCTR2: u32 = 10;

// ---------------------------------------------------------------------------
// Policy flags
// ---------------------------------------------------------------------------

/// Encrypt filenames with the same key derivation as contents.
pub const FSCRYPT_POLICY_FLAGS_PAD_4: u32 = 0x00;
/// Pad filenames to 8-byte boundary.
pub const FSCRYPT_POLICY_FLAGS_PAD_8: u32 = 0x01;
/// Pad filenames to 16-byte boundary.
pub const FSCRYPT_POLICY_FLAGS_PAD_16: u32 = 0x02;
/// Pad filenames to 32-byte boundary.
pub const FSCRYPT_POLICY_FLAGS_PAD_32: u32 = 0x03;
/// Padding size mask.
pub const FSCRYPT_POLICY_FLAGS_PAD_MASK: u32 = 0x03;
/// Use direct key derivation (no per-file nonce).
pub const FSCRYPT_POLICY_FLAG_DIRECT_KEY: u32 = 0x04;
/// Use inline encryption hardware (blk-crypto).
pub const FSCRYPT_POLICY_FLAG_IV_INO_LBLK_64: u32 = 0x08;
/// Large inode number in IV.
pub const FSCRYPT_POLICY_FLAG_IV_INO_LBLK_32: u32 = 0x10;

// ---------------------------------------------------------------------------
// Policy versions
// ---------------------------------------------------------------------------

/// Original encryption policy (v1).
pub const FSCRYPT_POLICY_V1: u32 = 0;
/// Updated encryption policy (v2, supports key identifiers).
pub const FSCRYPT_POLICY_V2: u32 = 2;

// ---------------------------------------------------------------------------
// Key sizes
// ---------------------------------------------------------------------------

/// Key descriptor size (v1 policy).
pub const FSCRYPT_KEY_DESCRIPTOR_SIZE: u32 = 8;
/// Key identifier size (v2 policy).
pub const FSCRYPT_KEY_IDENTIFIER_SIZE: u32 = 16;
/// Maximum key size in bytes.
pub const FSCRYPT_MAX_KEY_SIZE: u32 = 64;

// ---------------------------------------------------------------------------
// ioctl commands
// ---------------------------------------------------------------------------

/// Set encryption policy on a directory.
pub const FS_IOC_SET_ENCRYPTION_POLICY: u32 = 0x800C_6613;
/// Get encryption policy of a file/directory.
pub const FS_IOC_GET_ENCRYPTION_POLICY: u32 = 0x400C_6615;
/// Get encryption policy (v2, extended info).
pub const FS_IOC_GET_ENCRYPTION_POLICY_EX: u32 = 0xC014_6616;
/// Add encryption key to filesystem.
pub const FS_IOC_ADD_ENCRYPTION_KEY: u32 = 0xC044_6617;
/// Remove encryption key from filesystem.
pub const FS_IOC_REMOVE_ENCRYPTION_KEY: u32 = 0xC040_6618;
/// Get status of an encryption key.
pub const FS_IOC_GET_ENCRYPTION_KEY_STATUS: u32 = 0xC080_661A;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_distinct() {
        let modes = [
            FSCRYPT_MODE_AES_256_XTS, FSCRYPT_MODE_AES_256_CTS,
            FSCRYPT_MODE_AES_128_CBC, FSCRYPT_MODE_AES_128_CTS,
            FSCRYPT_MODE_ADIANTUM, FSCRYPT_MODE_AES_256_HCTR2,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_padding_flags() {
        assert_eq!(FSCRYPT_POLICY_FLAGS_PAD_4 & FSCRYPT_POLICY_FLAGS_PAD_MASK, 0);
        assert_eq!(FSCRYPT_POLICY_FLAGS_PAD_32 & FSCRYPT_POLICY_FLAGS_PAD_MASK, 3);
    }

    #[test]
    fn test_policy_versions_distinct() {
        assert_ne!(FSCRYPT_POLICY_V1, FSCRYPT_POLICY_V2);
    }

    #[test]
    fn test_key_sizes() {
        assert!(FSCRYPT_KEY_DESCRIPTOR_SIZE < FSCRYPT_KEY_IDENTIFIER_SIZE);
        assert!(FSCRYPT_KEY_IDENTIFIER_SIZE < FSCRYPT_MAX_KEY_SIZE);
    }

    #[test]
    fn test_ioctls_distinct() {
        let cmds = [
            FS_IOC_SET_ENCRYPTION_POLICY, FS_IOC_GET_ENCRYPTION_POLICY,
            FS_IOC_GET_ENCRYPTION_POLICY_EX, FS_IOC_ADD_ENCRYPTION_KEY,
            FS_IOC_REMOVE_ENCRYPTION_KEY, FS_IOC_GET_ENCRYPTION_KEY_STATUS,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }
}
