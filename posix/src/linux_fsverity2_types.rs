//! `<linux/fsverity.h>` — Additional fs-verity constants.
//!
//! Supplementary fs-verity constants covering hash algorithms,
//! ioctl commands, and descriptor flags.

// ---------------------------------------------------------------------------
// fs-verity hash algorithms
// ---------------------------------------------------------------------------

/// SHA-256.
pub const FS_VERITY_HASH_ALG_SHA256: u32 = 1;
/// SHA-512.
pub const FS_VERITY_HASH_ALG_SHA512: u32 = 2;

// ---------------------------------------------------------------------------
// fs-verity ioctl commands
// ---------------------------------------------------------------------------

/// Enable verity.
pub const FS_IOC_ENABLE_VERITY: u32 = 0x40806685;
/// Measure verity.
pub const FS_IOC_MEASURE_VERITY: u32 = 0xC0046686;
/// Read verity metadata.
pub const FS_IOC_READ_VERITY_METADATA: u32 = 0xC0286687;

// ---------------------------------------------------------------------------
// fs-verity metadata types
// ---------------------------------------------------------------------------

/// Merkle tree.
pub const FS_VERITY_METADATA_TYPE_MERKLE_TREE: u64 = 1;
/// Descriptor.
pub const FS_VERITY_METADATA_TYPE_DESCRIPTOR: u64 = 2;
/// Signature.
pub const FS_VERITY_METADATA_TYPE_SIGNATURE: u64 = 3;

// ---------------------------------------------------------------------------
// fs-verity block sizes
// ---------------------------------------------------------------------------

/// Minimum block size (256 bytes).
pub const FS_VERITY_LOG_BLOCKSIZE_MIN: u32 = 8;
/// Maximum block size log2.
pub const FS_VERITY_LOG_BLOCKSIZE_MAX: u32 = 16;

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
    fn test_ioctl_cmds_distinct() {
        let cmds = [
            FS_IOC_ENABLE_VERITY, FS_IOC_MEASURE_VERITY,
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
    fn test_blocksize_range() {
        assert!(FS_VERITY_LOG_BLOCKSIZE_MIN < FS_VERITY_LOG_BLOCKSIZE_MAX);
    }
}
