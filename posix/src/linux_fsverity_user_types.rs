//! `<linux/fsverity.h>` — read-only file authentication userspace ABI.
//!
//! fs-verity hashes a file's contents into a Merkle tree at enable
//! time and refuses to serve any read whose hash doesn't match. It is
//! used by Android (system_server APK verification), package managers,
//! and AOT-compiled JIT caches. ioctls below enable verity, read the
//! digest, and check status.

// ---------------------------------------------------------------------------
// Hash algorithms (struct fsverity_enable_arg.hash_algorithm)
// ---------------------------------------------------------------------------

/// SHA-256.
pub const FS_VERITY_HASH_ALG_SHA256: u32 = 1;
/// SHA-512.
pub const FS_VERITY_HASH_ALG_SHA512: u32 = 2;

// ---------------------------------------------------------------------------
// Digest sizes (matches the algorithms above)
// ---------------------------------------------------------------------------

/// SHA-256 digest length.
pub const FS_VERITY_SHA256_LEN: usize = 32;
/// SHA-512 digest length.
pub const FS_VERITY_SHA512_LEN: usize = 64;
/// Maximum hash digest size that fits in fsverity_digest.
pub const FS_VERITY_MAX_DIGEST_SIZE: usize = 64;
/// Maximum signature size accepted by FS_IOC_ENABLE_VERITY.
pub const FS_VERITY_MAX_SIGNATURE_SIZE: usize = 16128;

// ---------------------------------------------------------------------------
// ioctls (group letter 'f')
// ---------------------------------------------------------------------------

/// `FS_IOC_ENABLE_VERITY`.
pub const FS_IOC_ENABLE_VERITY: u32 = 0x4020_6685;
/// `FS_IOC_MEASURE_VERITY` — read root digest of an already-verity file.
pub const FS_IOC_MEASURE_VERITY: u32 = 0xC004_6686;
/// `FS_IOC_READ_VERITY_METADATA` — read Merkle tree blocks.
pub const FS_IOC_READ_VERITY_METADATA: u32 = 0xC028_6687;

// ---------------------------------------------------------------------------
// Read-metadata types
// ---------------------------------------------------------------------------

/// Read raw Merkle tree blocks.
pub const FS_VERITY_METADATA_TYPE_MERKLE_TREE: u32 = 1;
/// Read the fsverity_descriptor.
pub const FS_VERITY_METADATA_TYPE_DESCRIPTOR: u32 = 2;
/// Read the signature, if present.
pub const FS_VERITY_METADATA_TYPE_SIGNATURE: u32 = 3;

// ---------------------------------------------------------------------------
// Block sizes
// ---------------------------------------------------------------------------

/// Block size used by the Merkle tree (currently fixed at 4096).
pub const FS_VERITY_BLOCK_SIZE: u32 = 4096;
/// Log-2 of block size (for shift math).
pub const FS_VERITY_LOG_BLOCKSIZE: u32 = 12;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_algorithms_distinct() {
        assert_eq!(FS_VERITY_HASH_ALG_SHA256, 1);
        assert_eq!(FS_VERITY_HASH_ALG_SHA512, 2);
        assert_ne!(FS_VERITY_HASH_ALG_SHA256, FS_VERITY_HASH_ALG_SHA512);
    }

    #[test]
    fn test_digest_sizes() {
        // Digest lengths must match the algorithm bit-widths in bytes.
        assert_eq!(FS_VERITY_SHA256_LEN, 32);
        assert_eq!(FS_VERITY_SHA512_LEN, 64);
        // The descriptor's digest buffer must fit the largest hash.
        assert!(FS_VERITY_MAX_DIGEST_SIZE >= FS_VERITY_SHA512_LEN);
    }

    #[test]
    fn test_signature_limit() {
        // 16128 bytes = 16 KiB - 256 bytes header overhead.
        assert_eq!(FS_VERITY_MAX_SIGNATURE_SIZE, 16128);
    }

    #[test]
    fn test_ioctls_distinct_use_letter_f() {
        let ops = [
            FS_IOC_ENABLE_VERITY,
            FS_IOC_MEASURE_VERITY,
            FS_IOC_READ_VERITY_METADATA,
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
    fn test_metadata_types_dense_from_1() {
        let m = [
            FS_VERITY_METADATA_TYPE_MERKLE_TREE,
            FS_VERITY_METADATA_TYPE_DESCRIPTOR,
            FS_VERITY_METADATA_TYPE_SIGNATURE,
        ];
        for (i, &v) in m.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_block_sizes() {
        // Block size is power-of-two and the log matches.
        assert!(FS_VERITY_BLOCK_SIZE.is_power_of_two());
        assert_eq!(1u32 << FS_VERITY_LOG_BLOCKSIZE, FS_VERITY_BLOCK_SIZE);
    }
}
