//! `<linux/zram_drv.h>` — zram (compressed RAM block device) constants.
//!
//! zram creates compressed block devices in RAM, primarily used as
//! swap backends. Data written to zram is compressed in memory using
//! configurable algorithms (lzo, lz4, zstd). This provides effective
//! "memory compression" — a system can hold more data in RAM at the
//! cost of CPU cycles for compression/decompression.

// ---------------------------------------------------------------------------
// Compression algorithms (string names as used in sysfs)
// ---------------------------------------------------------------------------

/// LZO compression algorithm name.
pub const ZRAM_COMP_LZO: &str = "lzo";
/// LZO-RLE compression algorithm name.
pub const ZRAM_COMP_LZO_RLE: &str = "lzo-rle";
/// LZ4 compression algorithm name.
pub const ZRAM_COMP_LZ4: &str = "lz4";
/// LZ4HC compression algorithm name.
pub const ZRAM_COMP_LZ4HC: &str = "lz4hc";
/// Zstd compression algorithm name.
pub const ZRAM_COMP_ZSTD: &str = "zstd";
/// Deflate compression algorithm name.
pub const ZRAM_COMP_DEFLATE: &str = "deflate";
/// 842 compression algorithm name.
pub const ZRAM_COMP_842: &str = "842";

// ---------------------------------------------------------------------------
// Page/sector sizes
// ---------------------------------------------------------------------------

/// Default zram page size (bytes).
pub const ZRAM_PAGE_SIZE: u32 = 4096;
/// Sector size for zram block device.
pub const ZRAM_SECTOR_SIZE: u32 = 512;
/// Sectors per page.
pub const ZRAM_SECTORS_PER_PAGE: u32 = ZRAM_PAGE_SIZE / ZRAM_SECTOR_SIZE;

// ---------------------------------------------------------------------------
// Sysfs attribute limits
// ---------------------------------------------------------------------------

/// Maximum number of compression streams (per-CPU default).
pub const ZRAM_MAX_COMP_STREAMS: u32 = 64;
/// Maximum disk size (in bytes) that can be set.
pub const ZRAM_MAX_DISKSIZE: u64 = u64::MAX;

// ---------------------------------------------------------------------------
// Memory tracking flags
// ---------------------------------------------------------------------------

/// Page is same-filled (single repeated value).
pub const ZRAM_SAME: u32 = 1 << 0;
/// Page is stored in write-back storage.
pub const ZRAM_WB: u32 = 1 << 1;
/// Page is under write-back (in flight).
pub const ZRAM_UNDER_WB: u32 = 1 << 2;
/// Page is huge (incompressible, stored uncompressed).
pub const ZRAM_HUGE: u32 = 1 << 3;
/// Page is idle (candidate for write-back).
pub const ZRAM_IDLE: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_comp_names_distinct() {
        let names = [
            ZRAM_COMP_LZO, ZRAM_COMP_LZO_RLE, ZRAM_COMP_LZ4,
            ZRAM_COMP_LZ4HC, ZRAM_COMP_ZSTD, ZRAM_COMP_DEFLATE,
            ZRAM_COMP_842,
        ];
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                assert_ne!(names[i], names[j]);
            }
        }
    }

    #[test]
    fn test_page_sector_relationship() {
        assert_eq!(ZRAM_PAGE_SIZE / ZRAM_SECTOR_SIZE, ZRAM_SECTORS_PER_PAGE);
        assert_eq!(ZRAM_SECTORS_PER_PAGE, 8);
    }

    #[test]
    fn test_tracking_flags_no_overlap() {
        let flags = [ZRAM_SAME, ZRAM_WB, ZRAM_UNDER_WB, ZRAM_HUGE, ZRAM_IDLE];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_page_size() {
        assert_eq!(ZRAM_PAGE_SIZE, 4096);
        assert!(ZRAM_PAGE_SIZE.is_power_of_two());
    }

    #[test]
    fn test_max_streams() {
        assert!(ZRAM_MAX_COMP_STREAMS > 0);
        assert!(ZRAM_MAX_COMP_STREAMS.is_power_of_two());
    }
}
