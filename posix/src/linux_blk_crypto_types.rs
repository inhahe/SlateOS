//! `<linux/blk-crypto.h>` — block layer inline encryption constants.
//!
//! Inline encryption offloads data-at-rest encryption to the storage
//! controller hardware (UFS, NVMe, eMMC), avoiding the CPU overhead
//! of software encryption (dm-crypt). The block layer manages crypto
//! keys and selects the right algorithm/mode per bio.

// ---------------------------------------------------------------------------
// Crypto modes (blk_crypto_mode_num)
// ---------------------------------------------------------------------------

/// AES-256-XTS (default for full-disk encryption).
pub const BLK_ENCRYPTION_MODE_AES_256_XTS: u32 = 0;
/// AES-128-CBC-ESSIV (legacy Android).
pub const BLK_ENCRYPTION_MODE_AES_128_CBC_ESSIV: u32 = 1;
/// Adiantum (fast on ARM without AES hardware).
pub const BLK_ENCRYPTION_MODE_ADIANTUM: u32 = 2;
/// SM4-XTS (Chinese national standard).
pub const BLK_ENCRYPTION_MODE_SM4_XTS: u32 = 3;
/// Number of encryption modes.
pub const BLK_ENCRYPTION_MODE_MAX: u32 = 4;

// ---------------------------------------------------------------------------
// Key sizes (bytes)
// ---------------------------------------------------------------------------

/// AES-256-XTS key size (two 256-bit keys = 64 bytes).
pub const BLK_CRYPTO_KEY_SIZE_AES_256_XTS: u32 = 64;
/// AES-128-CBC-ESSIV key size (16 bytes).
pub const BLK_CRYPTO_KEY_SIZE_AES_128_CBC: u32 = 16;
/// Adiantum key size (32 bytes).
pub const BLK_CRYPTO_KEY_SIZE_ADIANTUM: u32 = 32;
/// SM4-XTS key size (two 128-bit keys = 32 bytes).
pub const BLK_CRYPTO_KEY_SIZE_SM4_XTS: u32 = 32;
/// Maximum key size supported.
pub const BLK_CRYPTO_MAX_KEY_SIZE: u32 = 64;

// ---------------------------------------------------------------------------
// Data unit sizes (bytes, must be power of 2)
// ---------------------------------------------------------------------------

/// 512-byte data unit (sector).
pub const BLK_CRYPTO_DUN_SIZE_512: u32 = 512;
/// 4096-byte data unit (filesystem block).
pub const BLK_CRYPTO_DUN_SIZE_4096: u32 = 4096;

// ---------------------------------------------------------------------------
// Flags
// ---------------------------------------------------------------------------

/// Key is for a hardware keyslot (inline crypto).
pub const BLK_CRYPTO_KEY_TYPE_HW_WRAPPED: u32 = 1 << 0;
/// Key is a standard (raw) key.
pub const BLK_CRYPTO_KEY_TYPE_STANDARD: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_distinct() {
        let modes = [
            BLK_ENCRYPTION_MODE_AES_256_XTS,
            BLK_ENCRYPTION_MODE_AES_128_CBC_ESSIV,
            BLK_ENCRYPTION_MODE_ADIANTUM,
            BLK_ENCRYPTION_MODE_SM4_XTS,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_mode_max() {
        assert_eq!(BLK_ENCRYPTION_MODE_MAX, 4);
    }

    #[test]
    fn test_key_sizes() {
        assert!(BLK_CRYPTO_KEY_SIZE_AES_128_CBC <= BLK_CRYPTO_MAX_KEY_SIZE);
        assert!(BLK_CRYPTO_KEY_SIZE_ADIANTUM <= BLK_CRYPTO_MAX_KEY_SIZE);
        assert!(BLK_CRYPTO_KEY_SIZE_SM4_XTS <= BLK_CRYPTO_MAX_KEY_SIZE);
        assert!(BLK_CRYPTO_KEY_SIZE_AES_256_XTS <= BLK_CRYPTO_MAX_KEY_SIZE);
    }

    #[test]
    fn test_dun_sizes_power_of_two() {
        assert!(BLK_CRYPTO_DUN_SIZE_512.is_power_of_two());
        assert!(BLK_CRYPTO_DUN_SIZE_4096.is_power_of_two());
    }

    #[test]
    fn test_key_types() {
        assert_ne!(BLK_CRYPTO_KEY_TYPE_STANDARD, BLK_CRYPTO_KEY_TYPE_HW_WRAPPED);
    }
}
