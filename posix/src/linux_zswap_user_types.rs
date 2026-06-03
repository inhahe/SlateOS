//! `/sys/module/zswap/parameters/*` — compressed swap cache.
//!
//! zswap intercepts pages on the way out to swap, compresses them, and
//! holds them in memory. Pages that can't be compressed (or that get
//! evicted from the in-memory pool) flow on to the backing swap device.
//! All configuration is via module parameters, *not* sysctl — zswap
//! predates the modern mm/ sysfs conventions.

// ---------------------------------------------------------------------------
// Parameter directory and individual files
// ---------------------------------------------------------------------------

pub const ZSWAP_PARAM_DIR: &str = "/sys/module/zswap/parameters";

pub const ZSWAP_PARAM_ENABLED: &str = "/sys/module/zswap/parameters/enabled";
pub const ZSWAP_PARAM_COMPRESSOR: &str = "/sys/module/zswap/parameters/compressor";
pub const ZSWAP_PARAM_ZPOOL: &str = "/sys/module/zswap/parameters/zpool";
pub const ZSWAP_PARAM_MAX_POOL_PERCENT: &str = "/sys/module/zswap/parameters/max_pool_percent";
pub const ZSWAP_PARAM_ACCEPT_THRESHOLD_PERCENT: &str =
    "/sys/module/zswap/parameters/accept_threshold_percent";
pub const ZSWAP_PARAM_SAME_FILLED_PAGES_ENABLED: &str =
    "/sys/module/zswap/parameters/same_filled_pages_enabled";
pub const ZSWAP_PARAM_NON_SAME_FILLED_PAGES_ENABLED: &str =
    "/sys/module/zswap/parameters/non_same_filled_pages_enabled";
pub const ZSWAP_PARAM_EXCLUSIVE_LOADS: &str = "/sys/module/zswap/parameters/exclusive_loads";
pub const ZSWAP_PARAM_SHRINKER_ENABLED: &str = "/sys/module/zswap/parameters/shrinker_enabled";

// ---------------------------------------------------------------------------
// Default values (`mm/zswap.c`)
// ---------------------------------------------------------------------------

/// 20 % of total memory may be used by the compressed pool.
pub const ZSWAP_MAX_POOL_PERCENT_DEFAULT: u32 = 20;

/// New pages are accepted only when usage drops below this percent of
/// the pool limit (hysteresis to avoid thrash).
pub const ZSWAP_ACCEPT_THRESHOLD_PERCENT_DEFAULT: u32 = 90;

// ---------------------------------------------------------------------------
// Compressor names (must be a registered `crypto_comp`/`crypto_acomp`)
// ---------------------------------------------------------------------------

pub const ZSWAP_COMP_LZO: &str = "lzo";
pub const ZSWAP_COMP_LZO_RLE: &str = "lzo-rle";
pub const ZSWAP_COMP_LZ4: &str = "lz4";
pub const ZSWAP_COMP_LZ4HC: &str = "lz4hc";
pub const ZSWAP_COMP_DEFLATE: &str = "deflate";
pub const ZSWAP_COMP_842: &str = "842";
pub const ZSWAP_COMP_ZSTD: &str = "zstd";

/// `lzo` is the build-time default in `mm/Kconfig` for historical reasons.
pub const ZSWAP_DEFAULT_COMP: &str = "lzo";

// ---------------------------------------------------------------------------
// zpool allocator names (one of `zbud`, `z3fold`, `zsmalloc`)
// ---------------------------------------------------------------------------

pub const ZSWAP_ZPOOL_ZBUD: &str = "zbud";
pub const ZSWAP_ZPOOL_Z3FOLD: &str = "z3fold";
pub const ZSWAP_ZPOOL_ZSMALLOC: &str = "zsmalloc";

/// `zsmalloc` is the most space-efficient and is the modern default.
pub const ZSWAP_DEFAULT_ZPOOL: &str = "zsmalloc";

// ---------------------------------------------------------------------------
// debugfs counters under `/sys/kernel/debug/zswap/`
// ---------------------------------------------------------------------------

pub const ZSWAP_DEBUGFS_DIR: &str = "/sys/kernel/debug/zswap";
pub const ZSWAP_DEBUGFS_POOL_TOTAL_SIZE: &str = "pool_total_size";
pub const ZSWAP_DEBUGFS_STORED_PAGES: &str = "stored_pages";
pub const ZSWAP_DEBUGFS_SAME_FILLED_PAGES: &str = "same_filled_pages";
pub const ZSWAP_DEBUGFS_REJECT_KMEMCACHE_FAIL: &str = "reject_kmemcache_fail";
pub const ZSWAP_DEBUGFS_REJECT_ALLOC_FAIL: &str = "reject_alloc_fail";
pub const ZSWAP_DEBUGFS_REJECT_COMPRESS_POOR: &str = "reject_compress_poor";
pub const ZSWAP_DEBUGFS_WRITTEN_BACK_PAGES: &str = "written_back_pages";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_params_under_module_zswap() {
        let p = [
            ZSWAP_PARAM_ENABLED,
            ZSWAP_PARAM_COMPRESSOR,
            ZSWAP_PARAM_ZPOOL,
            ZSWAP_PARAM_MAX_POOL_PERCENT,
            ZSWAP_PARAM_ACCEPT_THRESHOLD_PERCENT,
            ZSWAP_PARAM_SAME_FILLED_PAGES_ENABLED,
            ZSWAP_PARAM_NON_SAME_FILLED_PAGES_ENABLED,
            ZSWAP_PARAM_EXCLUSIVE_LOADS,
            ZSWAP_PARAM_SHRINKER_ENABLED,
        ];
        for path in p {
            assert!(path.starts_with("/sys/module/zswap/parameters/"));
        }
        assert_eq!(ZSWAP_PARAM_DIR, "/sys/module/zswap/parameters");
    }

    #[test]
    fn test_defaults_in_sane_bounds() {
        // Max-pool percent must leave room for everything else.
        assert!(ZSWAP_MAX_POOL_PERCENT_DEFAULT > 0);
        assert!(ZSWAP_MAX_POOL_PERCENT_DEFAULT < 100);
        assert_eq!(ZSWAP_MAX_POOL_PERCENT_DEFAULT, 20);
        // Accept threshold is also a percent.
        assert!(ZSWAP_ACCEPT_THRESHOLD_PERCENT_DEFAULT > 0);
        assert!(ZSWAP_ACCEPT_THRESHOLD_PERCENT_DEFAULT <= 100);
        assert_eq!(ZSWAP_ACCEPT_THRESHOLD_PERCENT_DEFAULT, 90);
        // Hysteresis sanity: accept threshold > 50 so the cache can't
        // get stuck refusing pages forever once it's near full.
        assert!(ZSWAP_ACCEPT_THRESHOLD_PERCENT_DEFAULT > 50);
    }

    #[test]
    fn test_comp_names_distinct_and_default_in_set() {
        let c = [
            ZSWAP_COMP_LZO,
            ZSWAP_COMP_LZO_RLE,
            ZSWAP_COMP_LZ4,
            ZSWAP_COMP_LZ4HC,
            ZSWAP_COMP_DEFLATE,
            ZSWAP_COMP_842,
            ZSWAP_COMP_ZSTD,
        ];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
        }
        assert!(c.iter().any(|&v| v == ZSWAP_DEFAULT_COMP));
    }

    #[test]
    fn test_zpool_names_distinct_and_default_zsmalloc() {
        let z = [
            ZSWAP_ZPOOL_ZBUD,
            ZSWAP_ZPOOL_Z3FOLD,
            ZSWAP_ZPOOL_ZSMALLOC,
        ];
        for i in 0..z.len() {
            for j in (i + 1)..z.len() {
                assert_ne!(z[i], z[j]);
            }
        }
        assert_eq!(ZSWAP_DEFAULT_ZPOOL, ZSWAP_ZPOOL_ZSMALLOC);
    }

    #[test]
    fn test_debugfs_paths_distinct() {
        let d = [
            ZSWAP_DEBUGFS_POOL_TOTAL_SIZE,
            ZSWAP_DEBUGFS_STORED_PAGES,
            ZSWAP_DEBUGFS_SAME_FILLED_PAGES,
            ZSWAP_DEBUGFS_REJECT_KMEMCACHE_FAIL,
            ZSWAP_DEBUGFS_REJECT_ALLOC_FAIL,
            ZSWAP_DEBUGFS_REJECT_COMPRESS_POOR,
            ZSWAP_DEBUGFS_WRITTEN_BACK_PAGES,
        ];
        for i in 0..d.len() {
            for j in (i + 1)..d.len() {
                assert_ne!(d[i], d[j]);
            }
        }
        assert_eq!(ZSWAP_DEBUGFS_DIR, "/sys/kernel/debug/zswap");
    }
}
