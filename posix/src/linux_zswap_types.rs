//! `<linux/zswap.h>` — Compressed swap cache constants.
//!
//! zswap is a compressed write-back cache for swap pages. Instead of
//! writing pages directly to the swap device (slow I/O), zswap
//! compresses pages in RAM and stores them in a dynamically-allocated
//! pool. When the pool is full, zswap evicts the oldest/coldest pages
//! to the actual swap device. This reduces swap I/O significantly,
//! improving performance for memory-pressured workloads while using
//! less RAM than the uncompressed pages would require.

// ---------------------------------------------------------------------------
// zswap compressor algorithms
// ---------------------------------------------------------------------------

/// LZO compression (fast, moderate ratio).
pub const ZSWAP_COMPRESSOR_LZO: u32 = 0;
/// LZ4 compression (fastest, lower ratio).
pub const ZSWAP_COMPRESSOR_LZ4: u32 = 1;
/// ZSTD compression (slower, best ratio).
pub const ZSWAP_COMPRESSOR_ZSTD: u32 = 2;
/// LZO-RLE compression (LZO variant optimized for runs).
pub const ZSWAP_COMPRESSOR_LZO_RLE: u32 = 3;
/// Deflate (zlib) compression.
pub const ZSWAP_COMPRESSOR_DEFLATE: u32 = 4;
/// 842 compression (hardware-accelerated on POWER).
pub const ZSWAP_COMPRESSOR_842: u32 = 5;

// ---------------------------------------------------------------------------
// zswap pool allocators (zpool backends)
// ---------------------------------------------------------------------------

/// zbud allocator (2 compressed pages per physical page).
pub const ZSWAP_ZPOOL_ZBUD: u32 = 0;
/// zsmalloc allocator (high density, variable-size slots).
pub const ZSWAP_ZPOOL_ZSMALLOC: u32 = 1;
/// z3fold allocator (3 compressed pages per physical page).
pub const ZSWAP_ZPOOL_Z3FOLD: u32 = 2;

// ---------------------------------------------------------------------------
// zswap accept policy
// ---------------------------------------------------------------------------

/// Accept all swapped pages into zswap.
pub const ZSWAP_ACCEPT_ALL: u32 = 0;
/// Only accept pages that compress well (ratio threshold).
pub const ZSWAP_ACCEPT_THRESHOLD: u32 = 1;

// ---------------------------------------------------------------------------
// zswap writeback modes
// ---------------------------------------------------------------------------

/// Writeback enabled (evict compressed pages to swap when pool is full).
pub const ZSWAP_WRITEBACK_ENABLED: u32 = 0;
/// Writeback disabled (reject new pages when pool is full).
pub const ZSWAP_WRITEBACK_DISABLED: u32 = 1;

// ---------------------------------------------------------------------------
// zswap limits
// ---------------------------------------------------------------------------

/// Default maximum pool size (percentage of total RAM).
pub const ZSWAP_MAX_POOL_PERCENT: u32 = 20;
/// Minimum pool size percentage.
pub const ZSWAP_MIN_POOL_PERCENT: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compressors_distinct() {
        let compressors = [
            ZSWAP_COMPRESSOR_LZO,
            ZSWAP_COMPRESSOR_LZ4,
            ZSWAP_COMPRESSOR_ZSTD,
            ZSWAP_COMPRESSOR_LZO_RLE,
            ZSWAP_COMPRESSOR_DEFLATE,
            ZSWAP_COMPRESSOR_842,
        ];
        for i in 0..compressors.len() {
            for j in (i + 1)..compressors.len() {
                assert_ne!(compressors[i], compressors[j]);
            }
        }
    }

    #[test]
    fn test_zpools_distinct() {
        let pools = [ZSWAP_ZPOOL_ZBUD, ZSWAP_ZPOOL_ZSMALLOC, ZSWAP_ZPOOL_Z3FOLD];
        for i in 0..pools.len() {
            for j in (i + 1)..pools.len() {
                assert_ne!(pools[i], pools[j]);
            }
        }
    }

    #[test]
    fn test_writeback_modes_distinct() {
        assert_ne!(ZSWAP_WRITEBACK_ENABLED, ZSWAP_WRITEBACK_DISABLED);
    }

    #[test]
    fn test_pool_limits() {
        assert!(ZSWAP_MIN_POOL_PERCENT < ZSWAP_MAX_POOL_PERCENT);
        assert!(ZSWAP_MAX_POOL_PERCENT <= 100);
    }
}
