//! `<linux/fsverity.h>` — fs-verity (file integrity) constants.
//!
//! fs-verity provides transparent integrity verification of file
//! contents using a Merkle tree built over the file data. Once
//! enabled on a file, any read that doesn't match the expected hash
//! returns -EIO. Used by Android APK verification, Chrome OS dm-verity
//! for individual files, and package managers.

// ---------------------------------------------------------------------------
// Hash algorithms
// ---------------------------------------------------------------------------

/// SHA-256.
pub const FS_VERITY_HASH_ALG_SHA256: u8 = 1;
/// SHA-512.
pub const FS_VERITY_HASH_ALG_SHA512: u8 = 2;

// ---------------------------------------------------------------------------
// Block sizes (for Merkle tree)
// ---------------------------------------------------------------------------

/// Minimum block size for verity (1024 bytes).
pub const FS_VERITY_MIN_BLOCK_SIZE: u32 = 1024;
/// Maximum block size for verity (64 KiB).
pub const FS_VERITY_MAX_BLOCK_SIZE: u32 = 65536;
/// Default block size (4096 bytes, matches page size).
pub const FS_VERITY_DEFAULT_BLOCK_SIZE: u32 = 4096;

// ---------------------------------------------------------------------------
// Descriptor version
// ---------------------------------------------------------------------------

/// fs-verity descriptor version 1.
pub const FS_VERITY_DESCRIPTOR_VERSION: u8 = 1;

// ---------------------------------------------------------------------------
// ioctl command numbers (as constants, not full ioctl encoding)
// ---------------------------------------------------------------------------

/// Enable verity on a file.
pub const FS_IOC_ENABLE_VERITY: u32 = 0x40806685;
/// Measure verity (get root hash).
pub const FS_IOC_MEASURE_VERITY: u32 = 0xC0046686;
/// Read verity metadata.
pub const FS_IOC_READ_VERITY_METADATA: u32 = 0xC0286687;

// ---------------------------------------------------------------------------
// Metadata types (for FS_IOC_READ_VERITY_METADATA)
// ---------------------------------------------------------------------------

/// Read the Merkle tree.
pub const FS_VERITY_METADATA_TYPE_MERKLE_TREE: u64 = 1;
/// Read the descriptor.
pub const FS_VERITY_METADATA_TYPE_DESCRIPTOR: u64 = 2;
/// Read the signature.
pub const FS_VERITY_METADATA_TYPE_SIGNATURE: u64 = 3;

// ---------------------------------------------------------------------------
// Digest sizes
// ---------------------------------------------------------------------------

/// SHA-256 digest size (bytes).
pub const FS_VERITY_SHA256_DIGEST_SIZE: u8 = 32;
/// SHA-512 digest size (bytes).
pub const FS_VERITY_SHA512_DIGEST_SIZE: u8 = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_algorithms_distinct() {
        assert_ne!(FS_VERITY_HASH_ALG_SHA256, FS_VERITY_HASH_ALG_SHA512);
    }

    #[test]
    fn test_block_size_ordering() {
        assert!(FS_VERITY_MIN_BLOCK_SIZE < FS_VERITY_DEFAULT_BLOCK_SIZE);
        assert!(FS_VERITY_DEFAULT_BLOCK_SIZE < FS_VERITY_MAX_BLOCK_SIZE);
        assert!(FS_VERITY_MIN_BLOCK_SIZE.is_power_of_two());
        assert!(FS_VERITY_DEFAULT_BLOCK_SIZE.is_power_of_two());
        assert!(FS_VERITY_MAX_BLOCK_SIZE.is_power_of_two());
    }

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            FS_IOC_ENABLE_VERITY,
            FS_IOC_MEASURE_VERITY,
            FS_IOC_READ_VERITY_METADATA,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_metadata_types_distinct() {
        let types = [
            FS_VERITY_METADATA_TYPE_MERKLE_TREE,
            FS_VERITY_METADATA_TYPE_DESCRIPTOR,
            FS_VERITY_METADATA_TYPE_SIGNATURE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_digest_sizes() {
        assert_eq!(FS_VERITY_SHA256_DIGEST_SIZE, 32);
        assert_eq!(FS_VERITY_SHA512_DIGEST_SIZE, 64);
        assert!(FS_VERITY_SHA256_DIGEST_SIZE < FS_VERITY_SHA512_DIGEST_SIZE);
    }
}
