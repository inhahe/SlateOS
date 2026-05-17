//! `<linux/fsverity.h>` — fs-verity (file integrity) constants.
//!
//! fs-verity provides transparent integrity verification of file
//! contents using a Merkle tree. Once enabled on a file, the kernel
//! verifies each data block against the tree on every read. Used for
//! APK verification (Android), IMA appraisal, and integrity-protected
//! OS images. Supported by ext4, F2FS, and btrfs.

// ---------------------------------------------------------------------------
// Hash algorithms
// ---------------------------------------------------------------------------

/// SHA-256 hash algorithm.
pub const FS_VERITY_HASH_ALG_SHA256: u32 = 1;
/// SHA-512 hash algorithm.
pub const FS_VERITY_HASH_ALG_SHA512: u32 = 2;

// ---------------------------------------------------------------------------
// Block sizes (log2)
// ---------------------------------------------------------------------------

/// Default block size for Merkle tree (4096 bytes = 2^12).
pub const FS_VERITY_LOG_BLOCKSIZE_4096: u32 = 12;

// ---------------------------------------------------------------------------
// ioctl commands
// ---------------------------------------------------------------------------

/// Enable fs-verity on a file.
pub const FS_IOC_ENABLE_VERITY: u32 = 0x8040_6685;
/// Measure (get digest of) a verity file.
pub const FS_IOC_MEASURE_VERITY: u32 = 0xC040_6686;
/// Read Merkle tree metadata.
pub const FS_IOC_READ_VERITY_METADATA: u32 = 0xC020_6687;

// ---------------------------------------------------------------------------
// Metadata type (for FS_IOC_READ_VERITY_METADATA)
// ---------------------------------------------------------------------------

/// Read the Merkle tree pages.
pub const FS_VERITY_METADATA_TYPE_MERKLE_TREE: u32 = 1;
/// Read the verity descriptor.
pub const FS_VERITY_METADATA_TYPE_DESCRIPTOR: u32 = 2;
/// Read the signature blob.
pub const FS_VERITY_METADATA_TYPE_SIGNATURE: u32 = 3;

// ---------------------------------------------------------------------------
// Digest sizes (bytes)
// ---------------------------------------------------------------------------

/// SHA-256 digest size.
pub const FS_VERITY_SHA256_DIGEST_SIZE: u32 = 32;
/// SHA-512 digest size.
pub const FS_VERITY_SHA512_DIGEST_SIZE: u32 = 64;
/// Maximum digest size supported.
pub const FS_VERITY_MAX_DIGEST_SIZE: u32 = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_algs_distinct() {
        assert_ne!(FS_VERITY_HASH_ALG_SHA256, FS_VERITY_HASH_ALG_SHA512);
    }

    #[test]
    fn test_block_size() {
        assert_eq!(1u32 << FS_VERITY_LOG_BLOCKSIZE_4096, 4096);
    }

    #[test]
    fn test_ioctls_distinct() {
        let cmds = [
            FS_IOC_ENABLE_VERITY,
            FS_IOC_MEASURE_VERITY,
            FS_IOC_READ_VERITY_METADATA,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
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
        assert!(FS_VERITY_MAX_DIGEST_SIZE >= FS_VERITY_SHA512_DIGEST_SIZE);
    }
}
