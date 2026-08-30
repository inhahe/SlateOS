//! `<linux/mman.h>` — madvise() advice value constants.
//!
//! madvise() tells the kernel about expected memory access patterns
//! for a range of pages. This allows the kernel to optimize paging,
//! readahead, and reclaim behavior. Some advice values are hints
//! (kernel may ignore), while others are commands (MADV_DONTNEED
//! immediately discards pages).

// ---------------------------------------------------------------------------
// Conventional madvise advice values
// ---------------------------------------------------------------------------

/// No special treatment (default).
pub const MADV_NORMAL: u32 = 0;
/// Expect sequential access (aggressive readahead).
pub const MADV_SEQUENTIAL: u32 = 2;
/// Expect random access (disable readahead).
pub const MADV_RANDOM: u32 = 1;
/// Will need this range soon (initiate readahead).
pub const MADV_WILLNEED: u32 = 3;
/// Don't need this range (free pages immediately).
pub const MADV_DONTNEED: u32 = 4;
/// Free pages (lazy: actual free on memory pressure).
pub const MADV_FREE: u32 = 8;

// ---------------------------------------------------------------------------
// Linux-specific advice values
// ---------------------------------------------------------------------------

/// Remove pages and swap space (like punch hole).
pub const MADV_REMOVE: u32 = 9;
/// Don't inherit on fork.
pub const MADV_DONTFORK: u32 = 10;
/// Do inherit on fork (undo DONTFORK).
pub const MADV_DOFORK: u32 = 11;
/// Mark pages as mergeable (KSM).
pub const MADV_MERGEABLE: u32 = 12;
/// Undo MERGEABLE.
pub const MADV_UNMERGEABLE: u32 = 13;
/// Pages are good for THP (Transparent Huge Pages).
pub const MADV_HUGEPAGE: u32 = 14;
/// Undo HUGEPAGE.
pub const MADV_NOHUGEPAGE: u32 = 15;
/// Don't dump these pages in core file.
pub const MADV_DONTDUMP: u32 = 16;
/// Dump these pages in core file (undo DONTDUMP).
pub const MADV_DODUMP: u32 = 17;
/// Poison pages (inject hardware memory error).
pub const MADV_HWPOISON: u32 = 100;
/// Soft offline pages (migrate away from questionable memory).
pub const MADV_SOFT_OFFLINE: u32 = 101;
/// Wipe on fork (security: zero pages in child).
pub const MADV_WIPEONFORK: u32 = 18;
/// Keep on fork (undo WIPEONFORK).
pub const MADV_KEEPONFORK: u32 = 19;
/// Mark pages as cold (likely to be reclaimed).
pub const MADV_COLD: u32 = 20;
/// Move pages to swap immediately.
pub const MADV_PAGEOUT: u32 = 21;
/// Like DONTNEED but applies to entire process.
pub const MADV_DONTNEED_LOCKED: u32 = 24;
/// Collapse range into THP where possible.
pub const MADV_COLLAPSE: u32 = 25;
/// Populate read fault pages.
pub const MADV_POPULATE_READ: u32 = 22;
/// Populate write fault pages.
pub const MADV_POPULATE_WRITE: u32 = 23;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conventional_values_distinct() {
        let vals = [
            MADV_NORMAL,
            MADV_RANDOM,
            MADV_SEQUENTIAL,
            MADV_WILLNEED,
            MADV_DONTNEED,
            MADV_FREE,
        ];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j]);
            }
        }
    }

    #[test]
    fn test_linux_specific_distinct() {
        let vals = [
            MADV_REMOVE,
            MADV_DONTFORK,
            MADV_DOFORK,
            MADV_MERGEABLE,
            MADV_UNMERGEABLE,
            MADV_HUGEPAGE,
            MADV_NOHUGEPAGE,
            MADV_DONTDUMP,
            MADV_DODUMP,
            MADV_WIPEONFORK,
            MADV_KEEPONFORK,
            MADV_COLD,
            MADV_PAGEOUT,
            MADV_POPULATE_READ,
            MADV_POPULATE_WRITE,
            MADV_DONTNEED_LOCKED,
            MADV_COLLAPSE,
            MADV_HWPOISON,
            MADV_SOFT_OFFLINE,
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
    fn test_fork_pairs() {
        // DONTFORK/DOFORK are adjacent
        assert_eq!(MADV_DOFORK, MADV_DONTFORK + 1);
        // WIPEONFORK/KEEPONFORK are adjacent
        assert_eq!(MADV_KEEPONFORK, MADV_WIPEONFORK + 1);
    }
}
