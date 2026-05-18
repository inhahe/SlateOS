//! `<linux/fsverity.h>` — fs-verity file integrity constants.
//!
//! fs-verity provides transparent file integrity verification using
//! Merkle trees. Once enabled on a file, all reads are verified
//! against a pre-computed hash tree. Corruption is detected at
//! read time without needing to hash the entire file upfront.

// ---------------------------------------------------------------------------
// fs-verity hash algorithms
// ---------------------------------------------------------------------------

/// SHA-256 hash algorithm.
pub const FS_VERITY_HASH_ALG_SHA256: u32 = 1;
/// SHA-512 hash algorithm.
pub const FS_VERITY_HASH_ALG_SHA512: u32 = 2;

// ---------------------------------------------------------------------------
// fs-verity ioctl commands (as indices, actual ioctls differ by fs)
// ---------------------------------------------------------------------------

/// Enable fs-verity on a file.
pub const FS_VERITY_CMD_ENABLE: u32 = 1;
/// Measure a verity file (get digest).
pub const FS_VERITY_CMD_MEASURE: u32 = 2;
/// Read verity metadata (Merkle tree, descriptor, signature).
pub const FS_VERITY_CMD_READ_METADATA: u32 = 3;

// ---------------------------------------------------------------------------
// fs-verity metadata types (for read_metadata)
// ---------------------------------------------------------------------------

/// Read Merkle tree blocks.
pub const FS_VERITY_METADATA_TYPE_MERKLE_TREE: u32 = 1;
/// Read verity descriptor.
pub const FS_VERITY_METADATA_TYPE_DESCRIPTOR: u32 = 2;
/// Read verity signature.
pub const FS_VERITY_METADATA_TYPE_SIGNATURE: u32 = 3;

// ---------------------------------------------------------------------------
// fs-verity block sizes (log2)
// ---------------------------------------------------------------------------

/// Minimum block size (256 bytes, log2 = 8). Rarely used.
pub const FS_VERITY_LOG_BLOCKSIZE_MIN: u32 = 8;
/// Default/common block size (4096 bytes, log2 = 12).
pub const FS_VERITY_LOG_BLOCKSIZE_4K: u32 = 12;
/// Maximum block size (65536 bytes, log2 = 16).
pub const FS_VERITY_LOG_BLOCKSIZE_MAX: u32 = 16;

// ---------------------------------------------------------------------------
// fs-verity descriptor version
// ---------------------------------------------------------------------------

/// Descriptor format version 1.
pub const FS_VERITY_DESCRIPTOR_VERSION: u32 = 1;

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
    fn test_commands_distinct() {
        assert_ne!(FS_VERITY_CMD_ENABLE, FS_VERITY_CMD_MEASURE);
        assert_ne!(FS_VERITY_CMD_MEASURE, FS_VERITY_CMD_READ_METADATA);
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
    fn test_block_size_ordering() {
        assert!(FS_VERITY_LOG_BLOCKSIZE_MIN < FS_VERITY_LOG_BLOCKSIZE_4K);
        assert!(FS_VERITY_LOG_BLOCKSIZE_4K < FS_VERITY_LOG_BLOCKSIZE_MAX);
    }

    #[test]
    fn test_descriptor_version() {
        assert_eq!(FS_VERITY_DESCRIPTOR_VERSION, 1);
    }
}
