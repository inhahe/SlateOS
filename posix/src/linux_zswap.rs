//! `<linux/zswap.h>` — Compressed swap cache constants.
//!
//! zswap is a compressed write-back cache for swap pages. Instead
//! of writing pages directly to the swap device, zswap compresses
//! them in RAM. This reduces swap I/O significantly for many
//! workloads, especially those with compressible data.

// ---------------------------------------------------------------------------
// zswap compressors
// ---------------------------------------------------------------------------

/// LZO compressor.
pub const ZSWAP_COMPRESSOR_LZO: &str = "lzo";
/// LZO-RLE compressor.
pub const ZSWAP_COMPRESSOR_LZO_RLE: &str = "lzo-rle";
/// LZ4 compressor.
pub const ZSWAP_COMPRESSOR_LZ4: &str = "lz4";
/// LZ4HC compressor.
pub const ZSWAP_COMPRESSOR_LZ4HC: &str = "lz4hc";
/// Zstd compressor.
pub const ZSWAP_COMPRESSOR_ZSTD: &str = "zstd";
/// Deflate compressor.
pub const ZSWAP_COMPRESSOR_DEFLATE: &str = "deflate";
/// 842 compressor.
pub const ZSWAP_COMPRESSOR_842: &str = "842";

/// Default compressor.
pub const ZSWAP_COMPRESSOR_DEFAULT: &str = "lzo-rle";

// ---------------------------------------------------------------------------
// zswap pool allocators (zpools)
// ---------------------------------------------------------------------------

/// zbud (buddy allocator for compressed pages, 2 pages per entry).
pub const ZSWAP_ZPOOL_ZBUD: &str = "zbud";
/// z3fold (3 compressed pages per physical page).
pub const ZSWAP_ZPOOL_Z3FOLD: &str = "z3fold";
/// zsmalloc (compressed page allocator).
pub const ZSWAP_ZPOOL_ZSMALLOC: &str = "zsmalloc";

/// Default zpool.
pub const ZSWAP_ZPOOL_DEFAULT: &str = "zsmalloc";

// ---------------------------------------------------------------------------
// zswap enable state
// ---------------------------------------------------------------------------

/// zswap disabled.
pub const ZSWAP_DISABLED: u32 = 0;
/// zswap enabled.
pub const ZSWAP_ENABLED: u32 = 1;

// ---------------------------------------------------------------------------
// zswap limits
// ---------------------------------------------------------------------------

/// Default max pool percentage of RAM.
pub const ZSWAP_MAX_POOL_PERCENT_DEFAULT: u32 = 20;
/// Minimum max pool percentage.
pub const ZSWAP_MAX_POOL_PERCENT_MIN: u32 = 1;
/// Maximum max pool percentage.
pub const ZSWAP_MAX_POOL_PERCENT_MAX: u32 = 90;

// ---------------------------------------------------------------------------
// zswap accept threshold
// ---------------------------------------------------------------------------

/// Default accept threshold percentage.
pub const ZSWAP_ACCEPT_THRESHOLD_DEFAULT: u32 = 90;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compressors_distinct() {
        let comps = [
            ZSWAP_COMPRESSOR_LZO,
            ZSWAP_COMPRESSOR_LZO_RLE,
            ZSWAP_COMPRESSOR_LZ4,
            ZSWAP_COMPRESSOR_LZ4HC,
            ZSWAP_COMPRESSOR_ZSTD,
            ZSWAP_COMPRESSOR_DEFLATE,
            ZSWAP_COMPRESSOR_842,
        ];
        for i in 0..comps.len() {
            for j in (i + 1)..comps.len() {
                assert_ne!(comps[i], comps[j]);
            }
        }
    }

    #[test]
    fn test_zpools_distinct() {
        let pools = [ZSWAP_ZPOOL_ZBUD, ZSWAP_ZPOOL_Z3FOLD, ZSWAP_ZPOOL_ZSMALLOC];
        for i in 0..pools.len() {
            for j in (i + 1)..pools.len() {
                assert_ne!(pools[i], pools[j]);
            }
        }
    }

    #[test]
    fn test_enable_states() {
        assert_ne!(ZSWAP_DISABLED, ZSWAP_ENABLED);
    }

    #[test]
    fn test_pool_percent_range() {
        assert!(ZSWAP_MAX_POOL_PERCENT_MIN < ZSWAP_MAX_POOL_PERCENT_DEFAULT);
        assert!(ZSWAP_MAX_POOL_PERCENT_DEFAULT < ZSWAP_MAX_POOL_PERCENT_MAX);
    }

    #[test]
    fn test_default_compressor() {
        assert_eq!(ZSWAP_COMPRESSOR_DEFAULT, "lzo-rle");
    }

    #[test]
    fn test_default_zpool() {
        assert_eq!(ZSWAP_ZPOOL_DEFAULT, "zsmalloc");
    }
}
