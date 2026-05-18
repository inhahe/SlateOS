//! `<linux/mman.h>` — Additional madvise constants.
//!
//! Supplementary madvise constants covering additional advice values,
//! process_madvise flags, and CRIU-related operations.

// ---------------------------------------------------------------------------
// madvise advice values (MADV_*)
// ---------------------------------------------------------------------------

/// Normal behavior (default).
pub const MADV_NORMAL: u32 = 0;
/// Random access expected.
pub const MADV_RANDOM: u32 = 1;
/// Sequential access expected.
pub const MADV_SEQUENTIAL: u32 = 2;
/// Will need in near future.
pub const MADV_WILLNEED: u32 = 3;
/// Don't need right now.
pub const MADV_DONTNEED: u32 = 4;
/// Free pages (lazy reclaim).
pub const MADV_FREE: u32 = 8;
/// Remove pages (like DONTNEED but for file mappings).
pub const MADV_REMOVE: u32 = 9;
/// Don't fork these pages.
pub const MADV_DONTFORK: u32 = 10;
/// Do fork these pages.
pub const MADV_DOFORK: u32 = 11;
/// Poison a page (for testing).
pub const MADV_HWPOISON: u32 = 100;
/// Enable soft-dirty tracking.
pub const MADV_SOFT_OFFLINE: u32 = 101;
/// Merge identical pages (KSM).
pub const MADV_MERGEABLE: u32 = 12;
/// Don't merge identical pages.
pub const MADV_UNMERGEABLE: u32 = 13;
/// Worth backing with huge pages.
pub const MADV_HUGEPAGE: u32 = 14;
/// Not worth backing with huge pages.
pub const MADV_NOHUGEPAGE: u32 = 15;
/// Don't include in core dump.
pub const MADV_DONTDUMP: u32 = 16;
/// Include in core dump.
pub const MADV_DODUMP: u32 = 17;
/// Wipe on fork.
pub const MADV_WIPEONFORK: u32 = 18;
/// Keep on fork.
pub const MADV_KEEPONFORK: u32 = 19;
/// Poison pages (guard pages).
pub const MADV_COLD: u32 = 20;
/// Pages are about to be reclaimed.
pub const MADV_PAGEOUT: u32 = 21;
/// Populate read faults.
pub const MADV_POPULATE_READ: u32 = 22;
/// Populate write faults.
pub const MADV_POPULATE_WRITE: u32 = 23;
/// Like DONTNEED but locked.
pub const MADV_DONTNEED_LOCKED: u32 = 24;
/// Collapse to huge page.
pub const MADV_COLLAPSE: u32 = 25;

// ---------------------------------------------------------------------------
// process_madvise flags
// ---------------------------------------------------------------------------

/// No special flags for process_madvise.
pub const PROCESS_MADVISE_FLAG_NONE: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_advice_values_distinct() {
        let vals = [
            MADV_NORMAL, MADV_RANDOM, MADV_SEQUENTIAL,
            MADV_WILLNEED, MADV_DONTNEED, MADV_FREE,
            MADV_REMOVE, MADV_DONTFORK, MADV_DOFORK,
            MADV_MERGEABLE, MADV_UNMERGEABLE,
            MADV_HUGEPAGE, MADV_NOHUGEPAGE,
            MADV_DONTDUMP, MADV_DODUMP,
            MADV_WIPEONFORK, MADV_KEEPONFORK,
            MADV_COLD, MADV_PAGEOUT,
            MADV_POPULATE_READ, MADV_POPULATE_WRITE,
            MADV_DONTNEED_LOCKED, MADV_COLLAPSE,
        ];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j]);
            }
        }
    }

    #[test]
    fn test_normal_is_zero() {
        assert_eq!(MADV_NORMAL, 0);
    }

    #[test]
    fn test_hwpoison_value() {
        assert_eq!(MADV_HWPOISON, 100);
    }

    #[test]
    fn test_fork_pairs() {
        // Fork-related pairs should be adjacent values
        assert_eq!(MADV_DONTFORK + 1, MADV_DOFORK);
        assert_eq!(MADV_WIPEONFORK + 1, MADV_KEEPONFORK);
    }

    #[test]
    fn test_dump_pairs() {
        assert_eq!(MADV_DONTDUMP + 1, MADV_DODUMP);
    }

    #[test]
    fn test_hugepage_pairs() {
        assert_eq!(MADV_HUGEPAGE + 1, MADV_NOHUGEPAGE);
    }

    #[test]
    fn test_populate_pair() {
        assert_eq!(MADV_POPULATE_READ + 1, MADV_POPULATE_WRITE);
    }
}
