//! `<linux/cma.h>` — Contiguous Memory Allocator constants.
//!
//! CMA reserves physically-contiguous memory regions at boot time for
//! drivers that need large DMA buffers (camera ISPs, framebuffers,
//! video codecs). Each region is a "cma area" with a name, base, size,
//! and order_per_bit allocation granularity.

// ---------------------------------------------------------------------------
// Default CMA region sizes (bytes)
// ---------------------------------------------------------------------------

/// Default global CMA region size (16 MiB).
pub const CMA_GLOBAL_DEFAULT_SIZE: u64 = 16 * 1024 * 1024;

/// Minimum sensible CMA region (1 MiB).
pub const CMA_MIN_SIZE: u64 = 1024 * 1024;

/// Maximum number of CMA regions (kernel default).
pub const MAX_CMA_AREAS: usize = 19;

// ---------------------------------------------------------------------------
// CMA alignment shift (in pages)
// ---------------------------------------------------------------------------

/// Default allocation order — one page at a time.
pub const CMA_DEFAULT_ORDER_PER_BIT: u32 = 0;

/// Maximum alignment order (kernel page allocator limit, 2^10 pages = 4 MiB).
pub const CMA_MAX_ORDER: u32 = 10;

// ---------------------------------------------------------------------------
// /sys/kernel/debug/cma directory and per-area file names
// ---------------------------------------------------------------------------

pub const CMA_DEBUGFS_DIR: &str = "/sys/kernel/debug/cma";
pub const CMA_DEBUGFS_BASE: &str = "base_pfn";
pub const CMA_DEBUGFS_COUNT: &str = "count";
pub const CMA_DEBUGFS_ORDER: &str = "order_per_bit";
pub const CMA_DEBUGFS_USED: &str = "used";
pub const CMA_DEBUGFS_MAXCHUNK: &str = "maxchunk";
pub const CMA_DEBUGFS_BITMAP: &str = "bitmap";

// ---------------------------------------------------------------------------
// CMA area name maximum length (kernel uses CMA_MAX_NAME = 64)
// ---------------------------------------------------------------------------

pub const CMA_MAX_NAME: usize = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_size_is_16mib() {
        assert_eq!(CMA_GLOBAL_DEFAULT_SIZE, 16 * 1024 * 1024);
        assert!(CMA_GLOBAL_DEFAULT_SIZE.is_power_of_two());
    }

    #[test]
    fn test_min_size_1mib() {
        assert_eq!(CMA_MIN_SIZE, 1024 * 1024);
        assert!(CMA_MIN_SIZE < CMA_GLOBAL_DEFAULT_SIZE);
        assert_eq!(CMA_GLOBAL_DEFAULT_SIZE / CMA_MIN_SIZE, 16);
    }

    #[test]
    fn test_max_areas_is_19() {
        // Kernel default MAX_CMA_AREAS = 19.
        assert_eq!(MAX_CMA_AREAS, 19);
    }

    #[test]
    fn test_order_bounds() {
        assert_eq!(CMA_DEFAULT_ORDER_PER_BIT, 0);
        assert_eq!(CMA_MAX_ORDER, 10);
        // 2^10 pages = 1024 pages = 4 MiB at 4K pages.
        assert_eq!(1u64 << CMA_MAX_ORDER, 1024);
    }

    #[test]
    fn test_debugfs_files_distinct() {
        let f = [
            CMA_DEBUGFS_BASE,
            CMA_DEBUGFS_COUNT,
            CMA_DEBUGFS_ORDER,
            CMA_DEBUGFS_USED,
            CMA_DEBUGFS_MAXCHUNK,
            CMA_DEBUGFS_BITMAP,
        ];
        for (i, &x) in f.iter().enumerate() {
            for &y in &f[i + 1..] {
                assert_ne!(x, y);
            }
            // All files are lowercase ASCII / underscore.
            for c in x.chars() {
                assert!(c.is_ascii_lowercase() || c == '_');
            }
        }
    }

    #[test]
    fn test_debugfs_dir_under_sys() {
        assert!(CMA_DEBUGFS_DIR.starts_with("/sys/kernel/debug/"));
        assert_eq!(CMA_DEBUGFS_DIR, "/sys/kernel/debug/cma");
    }

    #[test]
    fn test_max_name_is_64() {
        assert_eq!(CMA_MAX_NAME, 64);
        assert!(CMA_MAX_NAME.is_power_of_two());
    }
}
