//! `vm.dirty_*` sysctls and `BDI_*` sysfs — page-cache writeback.
//!
//! When dirty pages exceed a threshold the kernel flushes them back
//! to the backing store. The trigger ratios (`dirty_ratio`,
//! `dirty_background_ratio`) and the writeback period
//! (`dirty_writeback_centisecs`) live under `/proc/sys/vm/`. Per-bdi
//! tuning sits under `/sys/class/bdi/`.

// ---------------------------------------------------------------------------
// /proc/sys/vm writeback knobs
// ---------------------------------------------------------------------------

pub const SYSCTL_DIRTY_RATIO: &str = "/proc/sys/vm/dirty_ratio";
pub const SYSCTL_DIRTY_BACKGROUND_RATIO: &str = "/proc/sys/vm/dirty_background_ratio";
pub const SYSCTL_DIRTY_BYTES: &str = "/proc/sys/vm/dirty_bytes";
pub const SYSCTL_DIRTY_BACKGROUND_BYTES: &str = "/proc/sys/vm/dirty_background_bytes";
pub const SYSCTL_DIRTY_WRITEBACK_CS: &str = "/proc/sys/vm/dirty_writeback_centisecs";
pub const SYSCTL_DIRTY_EXPIRE_CS: &str = "/proc/sys/vm/dirty_expire_centisecs";
pub const SYSCTL_DIRTYTIME_EXPIRE_S: &str = "/proc/sys/vm/dirtytime_expire_seconds";

// ---------------------------------------------------------------------------
// Default values from `mm/page-writeback.c`
// ---------------------------------------------------------------------------

/// Foreground threshold — application threads start writing back when
/// dirty pages reach this percent of available memory.
pub const VM_DIRTY_RATIO_DEFAULT: u32 = 20;

/// Background threshold — flusher threads kick in at this percent.
pub const VM_DIRTY_BACKGROUND_RATIO_DEFAULT: u32 = 10;

/// 5 s — interval at which `kworker/u*` wakes up to flush.
pub const VM_DIRTY_WRITEBACK_CS_DEFAULT: u32 = 500;

/// 30 s — page must be dirty this long before being written.
pub const VM_DIRTY_EXPIRE_CS_DEFAULT: u32 = 3000;

/// 12 hours — `lazytime` mtime/atime flush window.
pub const VM_DIRTYTIME_EXPIRE_S_DEFAULT: u32 = 43_200;

// ---------------------------------------------------------------------------
// Per-BDI sysfs paths
// ---------------------------------------------------------------------------

pub const SYS_CLASS_BDI: &str = "/sys/class/bdi";

pub const BDI_ATTR_MIN_RATIO: &str = "min_ratio";
pub const BDI_ATTR_MAX_RATIO: &str = "max_ratio";
pub const BDI_ATTR_READ_AHEAD_KB: &str = "read_ahead_kb";
pub const BDI_ATTR_STABLE_PAGES_REQUIRED: &str = "stable_pages_required";
pub const BDI_ATTR_STRICT_LIMIT: &str = "strict_limit";

// ---------------------------------------------------------------------------
// Writeback reasons (kernel-internal but exposed via tracepoints)
// ---------------------------------------------------------------------------

pub const WB_REASON_BACKGROUND: u32 = 0;
pub const WB_REASON_VMSCAN: u32 = 1;
pub const WB_REASON_SYNC: u32 = 2;
pub const WB_REASON_PERIODIC: u32 = 3;
pub const WB_REASON_LAPTOP_TIMER: u32 = 4;
pub const WB_REASON_FS_FREE_SPACE: u32 = 5;
pub const WB_REASON_FORKER_THREAD: u32 = 6;
pub const WB_REASON_FOREIGN_FLUSH: u32 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sysctl_paths_under_proc_sys_vm() {
        let p = [
            SYSCTL_DIRTY_RATIO,
            SYSCTL_DIRTY_BACKGROUND_RATIO,
            SYSCTL_DIRTY_BYTES,
            SYSCTL_DIRTY_BACKGROUND_BYTES,
            SYSCTL_DIRTY_WRITEBACK_CS,
            SYSCTL_DIRTY_EXPIRE_CS,
            SYSCTL_DIRTYTIME_EXPIRE_S,
        ];
        for path in p {
            assert!(path.starts_with("/proc/sys/vm/"));
        }
    }

    #[test]
    fn test_defaults_match_mm_page_writeback() {
        // 20 / 10 / 500 / 3000 are the canonical defaults from
        // mm/page-writeback.c.
        assert_eq!(VM_DIRTY_RATIO_DEFAULT, 20);
        assert_eq!(VM_DIRTY_BACKGROUND_RATIO_DEFAULT, 10);
        // Background threshold must be below foreground (otherwise the
        // flushers wouldn't start before app threads block).
        assert!(VM_DIRTY_BACKGROUND_RATIO_DEFAULT < VM_DIRTY_RATIO_DEFAULT);
        assert_eq!(VM_DIRTY_WRITEBACK_CS_DEFAULT, 500);
        assert_eq!(VM_DIRTY_EXPIRE_CS_DEFAULT, 3000);
        // 12 hours in seconds.
        assert_eq!(VM_DIRTYTIME_EXPIRE_S_DEFAULT, 12 * 60 * 60);
    }

    #[test]
    fn test_bdi_attrs_distinct() {
        let a = [
            BDI_ATTR_MIN_RATIO,
            BDI_ATTR_MAX_RATIO,
            BDI_ATTR_READ_AHEAD_KB,
            BDI_ATTR_STABLE_PAGES_REQUIRED,
            BDI_ATTR_STRICT_LIMIT,
        ];
        for i in 0..a.len() {
            for j in (i + 1)..a.len() {
                assert_ne!(a[i], a[j]);
            }
        }
        assert_eq!(SYS_CLASS_BDI, "/sys/class/bdi");
    }

    #[test]
    fn test_wb_reasons_dense_0_to_7() {
        let r = [
            WB_REASON_BACKGROUND,
            WB_REASON_VMSCAN,
            WB_REASON_SYNC,
            WB_REASON_PERIODIC,
            WB_REASON_LAPTOP_TIMER,
            WB_REASON_FS_FREE_SPACE,
            WB_REASON_FORKER_THREAD,
            WB_REASON_FOREIGN_FLUSH,
        ];
        for (i, &v) in r.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_centiseconds_unit_consistent() {
        // _CS suffix means 1/100 second units, so 500 == 5 s.
        assert_eq!(VM_DIRTY_WRITEBACK_CS_DEFAULT, 5 * 100);
        // 3000 == 30 s.
        assert_eq!(VM_DIRTY_EXPIRE_CS_DEFAULT, 30 * 100);
    }
}
