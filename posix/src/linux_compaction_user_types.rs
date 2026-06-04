//! `<linux/compaction.h>` — Memory compaction action constants.
//!
//! Memory compaction defragments physical memory by moving movable
//! pages to free contiguous high-order page blocks. Userspace can
//! trigger compaction via /proc/sys/vm/compact_memory and observe
//! results via /proc/vmstat counters.

// ---------------------------------------------------------------------------
// Compaction result codes (kcompactd return values)
// ---------------------------------------------------------------------------

pub const COMPACT_SKIPPED: u32 = 0;
pub const COMPACT_DEFERRED: u32 = 1;
pub const COMPACT_NO_SUITABLE_PAGE: u32 = 2;
pub const COMPACT_CONTINUE: u32 = 3;
pub const COMPACT_COMPLETE: u32 = 4;
pub const COMPACT_PARTIAL_SKIPPED: u32 = 5;
pub const COMPACT_CONTENDED: u32 = 6;
pub const COMPACT_SUCCESS: u32 = 7;

// ---------------------------------------------------------------------------
// Compaction priority levels
// ---------------------------------------------------------------------------

pub const COMPACT_PRIO_SYNC_FULL: u32 = 0;
pub const COMPACT_PRIO_SYNC_LIGHT: u32 = 1;
pub const COMPACT_PRIO_ASYNC: u32 = 2;

// ---------------------------------------------------------------------------
// /proc/sys/vm files
// ---------------------------------------------------------------------------

pub const PROC_VM_COMPACT_MEMORY: &str = "/proc/sys/vm/compact_memory";
pub const PROC_VM_EXTFRAG_THRESHOLD: &str = "/proc/sys/vm/extfrag_threshold";
pub const PROC_VM_COMPACTION_PROACTIVENESS: &str = "/proc/sys/vm/compaction_proactiveness";
pub const PROC_VM_COMPACT_UNEVICTABLE_ALLOWED: &str =
    "/proc/sys/vm/compact_unevictable_allowed";

// ---------------------------------------------------------------------------
// Proactiveness default value (0..100, 20 is kernel default)
// ---------------------------------------------------------------------------

pub const COMPACTION_PROACTIVENESS_MIN: u32 = 0;
pub const COMPACTION_PROACTIVENESS_DEFAULT: u32 = 20;
pub const COMPACTION_PROACTIVENESS_MAX: u32 = 100;

// ---------------------------------------------------------------------------
// Extfrag threshold (0..1000)
// ---------------------------------------------------------------------------

pub const EXTFRAG_THRESHOLD_MIN: u32 = 0;
pub const EXTFRAG_THRESHOLD_DEFAULT: u32 = 500;
pub const EXTFRAG_THRESHOLD_MAX: u32 = 1000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_results_dense_0_to_7() {
        let r = [
            COMPACT_SKIPPED,
            COMPACT_DEFERRED,
            COMPACT_NO_SUITABLE_PAGE,
            COMPACT_CONTINUE,
            COMPACT_COMPLETE,
            COMPACT_PARTIAL_SKIPPED,
            COMPACT_CONTENDED,
            COMPACT_SUCCESS,
        ];
        for (i, &v) in r.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_priorities_dense_0_to_2() {
        let p = [
            COMPACT_PRIO_SYNC_FULL,
            COMPACT_PRIO_SYNC_LIGHT,
            COMPACT_PRIO_ASYNC,
        ];
        for (i, &v) in p.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // Lower number = more aggressive (sync full < async).
        assert!(COMPACT_PRIO_SYNC_FULL < COMPACT_PRIO_ASYNC);
    }

    #[test]
    fn test_proc_paths_under_vm() {
        for f in [
            PROC_VM_COMPACT_MEMORY,
            PROC_VM_EXTFRAG_THRESHOLD,
            PROC_VM_COMPACTION_PROACTIVENESS,
            PROC_VM_COMPACT_UNEVICTABLE_ALLOWED,
        ] {
            assert!(f.starts_with("/proc/sys/vm/"));
        }
    }

    #[test]
    fn test_proactiveness_range() {
        assert_eq!(COMPACTION_PROACTIVENESS_MIN, 0);
        assert_eq!(COMPACTION_PROACTIVENESS_DEFAULT, 20);
        assert_eq!(COMPACTION_PROACTIVENESS_MAX, 100);
        assert!(COMPACTION_PROACTIVENESS_DEFAULT < COMPACTION_PROACTIVENESS_MAX);
        assert!(COMPACTION_PROACTIVENESS_DEFAULT > COMPACTION_PROACTIVENESS_MIN);
    }

    #[test]
    fn test_extfrag_range_0_to_1000() {
        assert_eq!(EXTFRAG_THRESHOLD_MIN, 0);
        assert_eq!(EXTFRAG_THRESHOLD_DEFAULT, 500);
        assert_eq!(EXTFRAG_THRESHOLD_MAX, 1000);
        // 500 is exactly the midpoint.
        assert_eq!(EXTFRAG_THRESHOLD_DEFAULT * 2, EXTFRAG_THRESHOLD_MAX);
    }
}
