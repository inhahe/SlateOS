//! `<linux/zram.h>` (sysfs interface) — compressed RAM-block device.
//!
//! `zram` exposes a block device whose backing store is compressed
//! memory. Distros enable it as the primary swap device on memory-
//! constrained systems (laptops, Raspberry Pi, Android). It is
//! configured entirely through sysfs — no ioctls, no character device.

// ---------------------------------------------------------------------------
// Device-node prefix
// ---------------------------------------------------------------------------

pub const DEV_ZRAM_PREFIX: &str = "/dev/zram";

// ---------------------------------------------------------------------------
// sysfs control points
// ---------------------------------------------------------------------------

pub const SYS_ZRAM_HOT_ADD: &str = "/sys/class/zram-control/hot_add";
pub const SYS_ZRAM_HOT_REMOVE: &str = "/sys/class/zram-control/hot_remove";

/// Per-device sysfs attributes (relative to `/sys/block/zramN/`).
pub const ZRAM_ATTR_DISKSIZE: &str = "disksize";
pub const ZRAM_ATTR_RESET: &str = "reset";
pub const ZRAM_ATTR_COMP_ALGORITHM: &str = "comp_algorithm";
pub const ZRAM_ATTR_INITSTATE: &str = "initstate";
pub const ZRAM_ATTR_MEM_LIMIT: &str = "mem_limit";
pub const ZRAM_ATTR_MEM_USED_MAX: &str = "mem_used_max";
pub const ZRAM_ATTR_MAX_COMP_STREAMS: &str = "max_comp_streams";
pub const ZRAM_ATTR_BACKING_DEV: &str = "backing_dev";
pub const ZRAM_ATTR_IDLE: &str = "idle";
pub const ZRAM_ATTR_WRITEBACK: &str = "writeback";
pub const ZRAM_ATTR_RECOMP_ALGORITHM: &str = "recomp_algorithm";

// ---------------------------------------------------------------------------
// mm_stat / io_stat / bd_stat are space-separated rows — known fields
// ---------------------------------------------------------------------------

pub const ZRAM_STAT_FILE_MM: &str = "mm_stat";
pub const ZRAM_STAT_FILE_IO: &str = "io_stat";
pub const ZRAM_STAT_FILE_BD: &str = "bd_stat";

// ---------------------------------------------------------------------------
// Compression algorithm names (must be a kernel-supported `crypto_comp`)
// ---------------------------------------------------------------------------

pub const ZRAM_COMP_LZO: &str = "lzo";
pub const ZRAM_COMP_LZO_RLE: &str = "lzo-rle";
pub const ZRAM_COMP_LZ4: &str = "lz4";
pub const ZRAM_COMP_LZ4HC: &str = "lz4hc";
pub const ZRAM_COMP_DEFLATE: &str = "deflate";
pub const ZRAM_COMP_842: &str = "842";
pub const ZRAM_COMP_ZSTD: &str = "zstd";

/// Default since Linux 5.1.
pub const ZRAM_DEFAULT_COMP: &str = "lzo-rle";

// ---------------------------------------------------------------------------
// `initstate` values
// ---------------------------------------------------------------------------

pub const ZRAM_INITSTATE_UNINIT: u32 = 0;
pub const ZRAM_INITSTATE_INIT: u32 = 1;

// ---------------------------------------------------------------------------
// Writeback control keywords
// ---------------------------------------------------------------------------

pub const ZRAM_WRITEBACK_HUGE: &str = "huge";
pub const ZRAM_WRITEBACK_IDLE: &str = "idle";
pub const ZRAM_WRITEBACK_HUGE_IDLE: &str = "huge_idle";
pub const ZRAM_WRITEBACK_INCOMPRESSIBLE: &str = "incompressible";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dev_prefix_and_hot_paths() {
        assert_eq!(DEV_ZRAM_PREFIX, "/dev/zram");
        // hot_add / hot_remove are siblings in the same control directory.
        assert!(SYS_ZRAM_HOT_ADD.starts_with("/sys/class/zram-control/"));
        assert!(SYS_ZRAM_HOT_REMOVE.starts_with("/sys/class/zram-control/"));
    }

    #[test]
    fn test_per_device_attrs_distinct() {
        let a = [
            ZRAM_ATTR_DISKSIZE,
            ZRAM_ATTR_RESET,
            ZRAM_ATTR_COMP_ALGORITHM,
            ZRAM_ATTR_INITSTATE,
            ZRAM_ATTR_MEM_LIMIT,
            ZRAM_ATTR_MEM_USED_MAX,
            ZRAM_ATTR_MAX_COMP_STREAMS,
            ZRAM_ATTR_BACKING_DEV,
            ZRAM_ATTR_IDLE,
            ZRAM_ATTR_WRITEBACK,
            ZRAM_ATTR_RECOMP_ALGORITHM,
        ];
        for i in 0..a.len() {
            for j in (i + 1)..a.len() {
                assert_ne!(a[i], a[j]);
            }
        }
    }

    #[test]
    fn test_stat_file_names_end_with_stat() {
        for n in [ZRAM_STAT_FILE_MM, ZRAM_STAT_FILE_IO, ZRAM_STAT_FILE_BD] {
            assert!(n.ends_with("_stat"));
        }
    }

    #[test]
    fn test_comp_names_distinct() {
        let c = [
            ZRAM_COMP_LZO,
            ZRAM_COMP_LZO_RLE,
            ZRAM_COMP_LZ4,
            ZRAM_COMP_LZ4HC,
            ZRAM_COMP_DEFLATE,
            ZRAM_COMP_842,
            ZRAM_COMP_ZSTD,
        ];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
        }
        // Default must be one of the supported algorithms.
        assert_eq!(ZRAM_DEFAULT_COMP, ZRAM_COMP_LZO_RLE);
    }

    #[test]
    fn test_initstate_dense_0_1() {
        assert_eq!(ZRAM_INITSTATE_UNINIT, 0);
        assert_eq!(ZRAM_INITSTATE_INIT, 1);
    }

    #[test]
    fn test_writeback_keywords_distinct() {
        let w = [
            ZRAM_WRITEBACK_HUGE,
            ZRAM_WRITEBACK_IDLE,
            ZRAM_WRITEBACK_HUGE_IDLE,
            ZRAM_WRITEBACK_INCOMPRESSIBLE,
        ];
        for i in 0..w.len() {
            for j in (i + 1)..w.len() {
                assert_ne!(w[i], w[j]);
            }
        }
    }
}
