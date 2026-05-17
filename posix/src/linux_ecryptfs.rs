//! `<linux/ecryptfs.h>` — eCryptfs stacked filesystem encryption constants.
//!
//! eCryptfs is a POSIX-compliant stacked cryptographic filesystem for
//! Linux. It stores encrypted files on an underlying filesystem
//! (no separate partition needed). Each file has its own randomly-
//! generated File Encryption Key (FEK) wrapped with a user's key.

// ---------------------------------------------------------------------------
// Cipher modes
// ---------------------------------------------------------------------------

/// AES cipher.
pub const ECRYPTFS_CIPHER_AES: u8 = 0;
/// Blowfish cipher.
pub const ECRYPTFS_CIPHER_BLOWFISH: u8 = 1;
/// DES3-EDE cipher.
pub const ECRYPTFS_CIPHER_DES3_EDE: u8 = 2;
/// Twofish cipher.
pub const ECRYPTFS_CIPHER_TWOFISH: u8 = 3;
/// Cast6 cipher.
pub const ECRYPTFS_CIPHER_CAST6: u8 = 4;
/// Cast5 cipher.
pub const ECRYPTFS_CIPHER_CAST5: u8 = 5;

// ---------------------------------------------------------------------------
// Key sizes (in bytes)
// ---------------------------------------------------------------------------

/// AES-128.
pub const ECRYPTFS_AES_KEY_SIZE_128: u8 = 16;
/// AES-192.
pub const ECRYPTFS_AES_KEY_SIZE_192: u8 = 24;
/// AES-256.
pub const ECRYPTFS_AES_KEY_SIZE_256: u8 = 32;
/// Default key size (AES-128).
pub const ECRYPTFS_DEFAULT_KEY_SIZE: u8 = 16;

// ---------------------------------------------------------------------------
// Magic and markers
// ---------------------------------------------------------------------------

/// eCryptfs file header magic bytes.
pub const ECRYPTFS_MAGIC: u32 = 0x3c81_b7f5;
/// Minimum header extent size (8192 bytes).
pub const ECRYPTFS_MINIMUM_HEADER_EXTENT_SIZE: u32 = 8192;
/// Maximum filename length encrypted.
pub const ECRYPTFS_MAX_FILENAME_SIZE: u32 = 255;

// ---------------------------------------------------------------------------
// File flags (in metadata header)
// ---------------------------------------------------------------------------

/// File contents are encrypted.
pub const ECRYPTFS_FILE_ENCRYPTED: u32 = 1 << 0;
/// Filename is encrypted.
pub const ECRYPTFS_FILENAME_ENCRYPTED: u32 = 1 << 1;
/// Metadata in xattr (not header).
pub const ECRYPTFS_METADATA_IN_XATTR: u32 = 1 << 2;
/// File has signatures.
pub const ECRYPTFS_FILE_HAS_SIGNATURES: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Authentication token types
// ---------------------------------------------------------------------------

/// Password-based token.
pub const ECRYPTFS_TOKEN_PASSWORD: u8 = 0;
/// Private key token.
pub const ECRYPTFS_TOKEN_PRIVATE_KEY: u8 = 1;

// ---------------------------------------------------------------------------
// Key wrapping
// ---------------------------------------------------------------------------

/// Maximum wrapped key size.
pub const ECRYPTFS_MAX_WRAPPED_KEY_SIZE: u32 = 512;
/// Salt size (bytes).
pub const ECRYPTFS_SALT_SIZE: u8 = 8;
/// Signature size (hex string bytes).
pub const ECRYPTFS_SIG_SIZE: u8 = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ciphers_distinct() {
        let ciphers = [
            ECRYPTFS_CIPHER_AES, ECRYPTFS_CIPHER_BLOWFISH,
            ECRYPTFS_CIPHER_DES3_EDE, ECRYPTFS_CIPHER_TWOFISH,
            ECRYPTFS_CIPHER_CAST6, ECRYPTFS_CIPHER_CAST5,
        ];
        for i in 0..ciphers.len() {
            for j in (i + 1)..ciphers.len() {
                assert_ne!(ciphers[i], ciphers[j]);
            }
        }
    }

    #[test]
    fn test_key_sizes_distinct() {
        let sizes = [
            ECRYPTFS_AES_KEY_SIZE_128, ECRYPTFS_AES_KEY_SIZE_192,
            ECRYPTFS_AES_KEY_SIZE_256,
        ];
        for i in 0..sizes.len() {
            for j in (i + 1)..sizes.len() {
                assert_ne!(sizes[i], sizes[j]);
            }
        }
    }

    #[test]
    fn test_magic() {
        assert_eq!(ECRYPTFS_MAGIC, 0x3c81_b7f5);
    }

    #[test]
    fn test_file_flags_no_overlap() {
        let flags = [
            ECRYPTFS_FILE_ENCRYPTED, ECRYPTFS_FILENAME_ENCRYPTED,
            ECRYPTFS_METADATA_IN_XATTR, ECRYPTFS_FILE_HAS_SIGNATURES,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_token_types_distinct() {
        assert_ne!(ECRYPTFS_TOKEN_PASSWORD, ECRYPTFS_TOKEN_PRIVATE_KEY);
    }

    #[test]
    fn test_sizes() {
        assert_eq!(ECRYPTFS_SALT_SIZE, 8);
        assert_eq!(ECRYPTFS_SIG_SIZE, 16);
        assert_eq!(ECRYPTFS_DEFAULT_KEY_SIZE, ECRYPTFS_AES_KEY_SIZE_128);
    }
}
