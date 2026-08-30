//! `<linux/fsverity.h>` — fs-verity file integrity constants.
//!
//! fs-verity provides transparent integrity verification of file
//! contents. Once enabled on a file, a Merkle tree is built over
//! the file's data blocks. Each time a page is read from disk, its
//! hash is verified against the Merkle tree. Corruption is detected
//! immediately as EIO. The file becomes immutable (no writes allowed).
//! Used for APK verification (Android), dm-verity replacement for
//! individual files, and IMA integration.

// ---------------------------------------------------------------------------
// fs-verity IOCTLs
// ---------------------------------------------------------------------------

/// Enable fs-verity on a file.
pub const FS_IOC_ENABLE_VERITY: u32 = 0x40806685;
/// Measure (get digest of) a verity file.
pub const FS_IOC_MEASURE_VERITY: u32 = 0xC0046686;
/// Read fs-verity metadata (Merkle tree, descriptor).
pub const FS_IOC_READ_VERITY_METADATA: u32 = 0xC0286687;

// ---------------------------------------------------------------------------
// fs-verity hash algorithms
// ---------------------------------------------------------------------------

/// SHA-256 hash algorithm.
pub const FS_VERITY_HASH_ALG_SHA256: u32 = 1;
/// SHA-512 hash algorithm.
pub const FS_VERITY_HASH_ALG_SHA512: u32 = 2;

// ---------------------------------------------------------------------------
// fs-verity block sizes
// ---------------------------------------------------------------------------

/// 4 KiB Merkle tree block size (default on most filesystems).
pub const FS_VERITY_BLOCK_SIZE_4K: u32 = 4096;
/// 1 KiB Merkle tree block size (for small files).
pub const FS_VERITY_BLOCK_SIZE_1K: u32 = 1024;

// ---------------------------------------------------------------------------
// fs-verity metadata types (for FS_IOC_READ_VERITY_METADATA)
// ---------------------------------------------------------------------------

/// Read Merkle tree pages.
pub const FS_VERITY_METADATA_TYPE_MERKLE_TREE: u32 = 1;
/// Read verity descriptor (hash algorithm, salt, root hash).
pub const FS_VERITY_METADATA_TYPE_DESCRIPTOR: u32 = 2;
/// Read signature (if file was signed).
pub const FS_VERITY_METADATA_TYPE_SIGNATURE: u32 = 3;

// ---------------------------------------------------------------------------
// fs-verity descriptor version
// ---------------------------------------------------------------------------

/// Descriptor version 1.
pub const FS_VERITY_DESCRIPTOR_V1: u32 = 1;

// ---------------------------------------------------------------------------
// fs-verity builtin signature verification
// ---------------------------------------------------------------------------

/// No signature required.
pub const FS_VERITY_SIG_NONE: u32 = 0;
/// PKCS#7 signature.
pub const FS_VERITY_SIG_PKCS7: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_hash_algos_distinct() {
        assert_ne!(FS_VERITY_HASH_ALG_SHA256, FS_VERITY_HASH_ALG_SHA512);
    }

    #[test]
    fn test_block_sizes_powers_of_two() {
        assert!(FS_VERITY_BLOCK_SIZE_4K.is_power_of_two());
        assert!(FS_VERITY_BLOCK_SIZE_1K.is_power_of_two());
        assert_ne!(FS_VERITY_BLOCK_SIZE_4K, FS_VERITY_BLOCK_SIZE_1K);
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
    fn test_sig_types_distinct() {
        assert_ne!(FS_VERITY_SIG_NONE, FS_VERITY_SIG_PKCS7);
    }
}
