//! `<linux/fscrypt.h>` — filesystem encryption userspace ABI.
//!
//! fscrypt provides per-directory encryption for ext4, F2FS, UBIFS,
//! and recently CephFS. Android (FBE) and ChromeOS depend heavily on
//! it. Userspace daemons (vold, cryptsetup-fscrypt) call the FS_IOC_*
//! ioctls below to add keys, lock directories, and check status.

// ---------------------------------------------------------------------------
// Policy version
// ---------------------------------------------------------------------------

/// fscrypt v1 (legacy).
pub const FSCRYPT_POLICY_V1: u8 = 0;
/// fscrypt v2 (current default; preferred).
pub const FSCRYPT_POLICY_V2: u8 = 2;

// ---------------------------------------------------------------------------
// Contents/filenames encryption modes
// ---------------------------------------------------------------------------

/// Invalid mode sentinel.
pub const FSCRYPT_MODE_INVALID: u8 = 0;
/// AES-256-XTS (data).
pub const FSCRYPT_MODE_AES_256_XTS: u8 = 1;
/// AES-256-CBC-CTS (filenames, legacy).
pub const FSCRYPT_MODE_AES_256_CTS: u8 = 4;
/// AES-128-CBC (data).
pub const FSCRYPT_MODE_AES_128_CBC: u8 = 5;
/// AES-128-CTS (filenames).
pub const FSCRYPT_MODE_AES_128_CTS: u8 = 6;
/// Adiantum (low-power, no AES hardware).
pub const FSCRYPT_MODE_ADIANTUM: u8 = 9;
/// AES-256-HCTR2 (filenames, v2 only).
pub const FSCRYPT_MODE_AES_256_HCTR2: u8 = 10;

// ---------------------------------------------------------------------------
// Policy flags
// ---------------------------------------------------------------------------

/// Pad filenames to 4 bytes.
pub const FSCRYPT_POLICY_FLAGS_PAD_4: u32 = 0x00;
/// Pad filenames to 8 bytes.
pub const FSCRYPT_POLICY_FLAGS_PAD_8: u32 = 0x01;
/// Pad filenames to 16 bytes.
pub const FSCRYPT_POLICY_FLAGS_PAD_16: u32 = 0x02;
/// Pad filenames to 32 bytes.
pub const FSCRYPT_POLICY_FLAGS_PAD_32: u32 = 0x03;
/// Mask covering the pad bits.
pub const FSCRYPT_POLICY_FLAGS_PAD_MASK: u32 = 0x03;
/// Direct-key (Adiantum).
pub const FSCRYPT_POLICY_FLAG_DIRECT_KEY: u32 = 0x04;
/// IV inode number (per-file unique IVs).
pub const FSCRYPT_POLICY_FLAG_IV_INO_LBLK_64: u32 = 0x08;
/// IV inode number 32-bit (FBE).
pub const FSCRYPT_POLICY_FLAG_IV_INO_LBLK_32: u32 = 0x10;

// ---------------------------------------------------------------------------
// Key identifier / descriptor sizes
// ---------------------------------------------------------------------------

/// v1 master-key descriptor (8 bytes).
pub const FSCRYPT_KEY_DESCRIPTOR_SIZE: usize = 8;
/// v2 master-key identifier (16 bytes, HKDF-derived).
pub const FSCRYPT_KEY_IDENTIFIER_SIZE: usize = 16;
/// Largest master key the kernel accepts.
pub const FSCRYPT_MAX_KEY_SIZE: usize = 64;

// ---------------------------------------------------------------------------
// ioctls (group letter 'f')
// ---------------------------------------------------------------------------

/// `FS_IOC_SET_ENCRYPTION_POLICY`.
pub const FS_IOC_SET_ENCRYPTION_POLICY: u32 = 0x800C_6613;
/// `FS_IOC_GET_ENCRYPTION_PWSALT` (legacy v1 only).
pub const FS_IOC_GET_ENCRYPTION_PWSALT: u32 = 0x4014_6614;
/// `FS_IOC_GET_ENCRYPTION_POLICY` (legacy).
pub const FS_IOC_GET_ENCRYPTION_POLICY: u32 = 0x400C_6615;
/// `FS_IOC_GET_ENCRYPTION_POLICY_EX`.
pub const FS_IOC_GET_ENCRYPTION_POLICY_EX: u32 = 0xC048_6616;
/// `FS_IOC_ADD_ENCRYPTION_KEY`.
pub const FS_IOC_ADD_ENCRYPTION_KEY: u32 = 0xC050_6617;
/// `FS_IOC_REMOVE_ENCRYPTION_KEY`.
pub const FS_IOC_REMOVE_ENCRYPTION_KEY: u32 = 0xC040_6618;
/// `FS_IOC_REMOVE_ENCRYPTION_KEY_ALL_USERS`.
pub const FS_IOC_REMOVE_ENCRYPTION_KEY_ALL_USERS: u32 = 0xC040_6619;
/// `FS_IOC_GET_ENCRYPTION_KEY_STATUS`.
pub const FS_IOC_GET_ENCRYPTION_KEY_STATUS: u32 = 0xC050_661A;
/// `FS_IOC_GET_ENCRYPTION_NONCE`.
pub const FS_IOC_GET_ENCRYPTION_NONCE: u32 = 0x4010_661B;

// ---------------------------------------------------------------------------
// Add-key spec types
// ---------------------------------------------------------------------------

/// Master-key descriptor (v1 wire form).
pub const FSCRYPT_KEY_SPEC_TYPE_DESCRIPTOR: u32 = 1;
/// Master-key identifier (v2 wire form).
pub const FSCRYPT_KEY_SPEC_TYPE_IDENTIFIER: u32 = 2;

// ---------------------------------------------------------------------------
// Key status (FS_IOC_GET_ENCRYPTION_KEY_STATUS result)
// ---------------------------------------------------------------------------

/// Key is absent (no users have added it).
pub const FSCRYPT_KEY_STATUS_ABSENT: u32 = 1;
/// Key is present in the filesystem.
pub const FSCRYPT_KEY_STATUS_PRESENT: u32 = 2;
/// Key is incompletely removed.
pub const FSCRYPT_KEY_STATUS_INCOMPLETELY_REMOVED: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policy_versions_distinct() {
        // v1 == 0 (historical); v2 == 2; 1 is reserved and never used.
        assert_eq!(FSCRYPT_POLICY_V1, 0);
        assert_eq!(FSCRYPT_POLICY_V2, 2);
    }

    #[test]
    fn test_modes_distinct() {
        let m = [
            FSCRYPT_MODE_INVALID,
            FSCRYPT_MODE_AES_256_XTS,
            FSCRYPT_MODE_AES_256_CTS,
            FSCRYPT_MODE_AES_128_CBC,
            FSCRYPT_MODE_AES_128_CTS,
            FSCRYPT_MODE_ADIANTUM,
            FSCRYPT_MODE_AES_256_HCTR2,
        ];
        for i in 0..m.len() {
            for j in (i + 1)..m.len() {
                assert_ne!(m[i], m[j]);
            }
        }
    }

    #[test]
    fn test_policy_pad_flags() {
        // PAD_* take values 0..3 (the low two bits of the flags field).
        assert_eq!(FSCRYPT_POLICY_FLAGS_PAD_4, 0);
        assert_eq!(FSCRYPT_POLICY_FLAGS_PAD_8, 1);
        assert_eq!(FSCRYPT_POLICY_FLAGS_PAD_16, 2);
        assert_eq!(FSCRYPT_POLICY_FLAGS_PAD_32, 3);
        assert_eq!(FSCRYPT_POLICY_FLAGS_PAD_MASK, 0x3);
        for &b in &[
            FSCRYPT_POLICY_FLAG_DIRECT_KEY,
            FSCRYPT_POLICY_FLAG_IV_INO_LBLK_64,
            FSCRYPT_POLICY_FLAG_IV_INO_LBLK_32,
        ] {
            assert!(b.is_power_of_two());
        }
    }

    #[test]
    fn test_key_size_constants() {
        assert_eq!(FSCRYPT_KEY_DESCRIPTOR_SIZE, 8);
        assert_eq!(FSCRYPT_KEY_IDENTIFIER_SIZE, 16);
        // The kernel hard-caps master keys at 64 bytes (AES-256-XTS).
        assert_eq!(FSCRYPT_MAX_KEY_SIZE, 64);
    }

    #[test]
    fn test_ioctls_distinct_use_letter_f() {
        let ops = [
            FS_IOC_SET_ENCRYPTION_POLICY,
            FS_IOC_GET_ENCRYPTION_PWSALT,
            FS_IOC_GET_ENCRYPTION_POLICY,
            FS_IOC_GET_ENCRYPTION_POLICY_EX,
            FS_IOC_ADD_ENCRYPTION_KEY,
            FS_IOC_REMOVE_ENCRYPTION_KEY,
            FS_IOC_REMOVE_ENCRYPTION_KEY_ALL_USERS,
            FS_IOC_GET_ENCRYPTION_KEY_STATUS,
            FS_IOC_GET_ENCRYPTION_NONCE,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
            // 'f' (0x66) is the magic byte.
            assert_eq!((ops[i] >> 8) & 0xff, b'f' as u32);
        }
    }

    #[test]
    fn test_key_spec_types_dense_from_1() {
        assert_eq!(FSCRYPT_KEY_SPEC_TYPE_DESCRIPTOR, 1);
        assert_eq!(FSCRYPT_KEY_SPEC_TYPE_IDENTIFIER, 2);
    }

    #[test]
    fn test_key_status_dense_from_1() {
        let s = [
            FSCRYPT_KEY_STATUS_ABSENT,
            FSCRYPT_KEY_STATUS_PRESENT,
            FSCRYPT_KEY_STATUS_INCOMPLETELY_REMOVED,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }
}
