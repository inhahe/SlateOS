//! `<linux/blk-crypto.h>` — block-layer inline encryption (BLK-Crypto).
//!
//! BLK-Crypto offloads per-bio encryption to either dedicated UFS/eMMC
//! crypto engines or to a software fallback. Userspace mounters
//! (`f2fs`/`ext4` fscrypt, kernel keyrings) name the algorithms by
//! these stable enum tags.

// ---------------------------------------------------------------------------
// Supported encryption modes (`enum blk_crypto_mode_num`)
// ---------------------------------------------------------------------------

pub const BLK_ENCRYPTION_MODE_INVALID: u32 = 0;
pub const BLK_ENCRYPTION_MODE_AES_256_XTS: u32 = 1;
pub const BLK_ENCRYPTION_MODE_AES_128_CBC_ESSIV: u32 = 2;
pub const BLK_ENCRYPTION_MODE_ADIANTUM: u32 = 3;
pub const BLK_ENCRYPTION_MODE_SM4_XTS: u32 = 4;

pub const BLK_ENCRYPTION_MODE_MAX: u32 = 5;

// ---------------------------------------------------------------------------
// Key and IV size limits
// ---------------------------------------------------------------------------

/// Maximum raw-key size (in bytes) any supported mode might require.
pub const BLK_CRYPTO_MAX_RAW_KEY_SIZE: usize = 64;

/// Maximum hardware-wrapped key size (for inline-crypto engines that
/// wrap their own keys).
pub const BLK_CRYPTO_MAX_HW_WRAPPED_KEY_SIZE: usize = 128;

/// Per-bio IV is 16 bytes (block-cipher IV size).
pub const BLK_CRYPTO_DUN_IV_BYTES: usize = 16;

/// Data-Unit Number is at most 4 × u64 (16 bytes).
pub const BLK_CRYPTO_MAX_IV_SIZE: usize = 32;

// ---------------------------------------------------------------------------
// Cipher-engine capability flags
// ---------------------------------------------------------------------------

pub const BLK_CRYPTO_KEY_TYPE_STANDARD: u32 = 1 << 0;
pub const BLK_CRYPTO_KEY_TYPE_HW_WRAPPED: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Data unit size constraints
// ---------------------------------------------------------------------------

/// Smallest data-unit size driver may advertise (one sector).
pub const BLK_CRYPTO_MIN_DUN_SIZE: u32 = 512;
/// Largest data-unit size (matches our 16 KiB page).
pub const BLK_CRYPTO_MAX_DUN_SIZE: u32 = 65_536;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_dense_with_invalid_at_zero() {
        let m = [
            BLK_ENCRYPTION_MODE_INVALID,
            BLK_ENCRYPTION_MODE_AES_256_XTS,
            BLK_ENCRYPTION_MODE_AES_128_CBC_ESSIV,
            BLK_ENCRYPTION_MODE_ADIANTUM,
            BLK_ENCRYPTION_MODE_SM4_XTS,
        ];
        for (i, &v) in m.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // _MAX is one past the last valid mode (exclusive upper bound).
        assert_eq!(BLK_ENCRYPTION_MODE_MAX, m.len() as u32);
    }

    #[test]
    fn test_key_size_bounds() {
        // Raw keys top out at 64 B (AES-256-XTS uses two 32 B halves).
        assert_eq!(BLK_CRYPTO_MAX_RAW_KEY_SIZE, 64);
        // HW-wrapped keys can be twice as large (driver opaque wrapping).
        assert_eq!(BLK_CRYPTO_MAX_HW_WRAPPED_KEY_SIZE, 128);
        assert!(BLK_CRYPTO_MAX_HW_WRAPPED_KEY_SIZE > BLK_CRYPTO_MAX_RAW_KEY_SIZE);
        // Both sizes are powers of two for SIMD alignment.
        assert!(BLK_CRYPTO_MAX_RAW_KEY_SIZE.is_power_of_two());
        assert!(BLK_CRYPTO_MAX_HW_WRAPPED_KEY_SIZE.is_power_of_two());
    }

    #[test]
    fn test_iv_geometry() {
        assert_eq!(BLK_CRYPTO_DUN_IV_BYTES, 16);
        assert_eq!(BLK_CRYPTO_MAX_IV_SIZE, 32);
        // Standard mode IV (16 B) fits in the max IV envelope (32 B).
        assert!(BLK_CRYPTO_DUN_IV_BYTES <= BLK_CRYPTO_MAX_IV_SIZE);
        assert!(BLK_CRYPTO_DUN_IV_BYTES.is_power_of_two());
        assert!(BLK_CRYPTO_MAX_IV_SIZE.is_power_of_two());
    }

    #[test]
    fn test_key_type_flags_each_single_bit() {
        assert_eq!(BLK_CRYPTO_KEY_TYPE_STANDARD, 1);
        assert_eq!(BLK_CRYPTO_KEY_TYPE_HW_WRAPPED, 2);
        assert!(BLK_CRYPTO_KEY_TYPE_STANDARD.is_power_of_two());
        assert!(BLK_CRYPTO_KEY_TYPE_HW_WRAPPED.is_power_of_two());
        assert_eq!(
            BLK_CRYPTO_KEY_TYPE_STANDARD & BLK_CRYPTO_KEY_TYPE_HW_WRAPPED,
            0
        );
    }

    #[test]
    fn test_dun_size_bounds() {
        assert_eq!(BLK_CRYPTO_MIN_DUN_SIZE, 512);
        assert_eq!(BLK_CRYPTO_MAX_DUN_SIZE, 65_536);
        assert!(BLK_CRYPTO_MIN_DUN_SIZE.is_power_of_two());
        assert!(BLK_CRYPTO_MAX_DUN_SIZE.is_power_of_two());
        // 128× span from one sector to 64 KiB.
        assert_eq!(BLK_CRYPTO_MAX_DUN_SIZE / BLK_CRYPTO_MIN_DUN_SIZE, 128);
    }
}
