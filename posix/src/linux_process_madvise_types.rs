//! `<sys/mman.h>` — process_madvise() behavior constants.
//!
//! The `process_madvise()` syscall (Linux 5.10+) allows one process
//! to give memory usage hints about another process's address space.
//! This is used by memory management daemons and Android's
//! ActivityManagerService to hint about cold/unused pages in
//! background processes.

// ---------------------------------------------------------------------------
// process_madvise() behaviors (reuses MADV_* values)
// ---------------------------------------------------------------------------

/// Hint: pages will be needed soon (prefetch).
pub const MADV_WILLNEED: u32 = 3;
/// Hint: pages not needed soon (allow reclaim).
pub const MADV_DONTNEED: u32 = 4;
/// Hint: pages are cold (lazy free if clean).
pub const MADV_COLD: u32 = 20;
/// Hint: pages can be reclaimed immediately.
pub const MADV_PAGEOUT: u32 = 21;
/// Free pages lazily (reuse if not reclaimed).
pub const MADV_FREE: u32 = 8;
/// Collapse to huge pages.
pub const MADV_COLLAPSE: u32 = 25;
/// Poison pages (testing only).
pub const MADV_HWPOISON: u32 = 100;
/// Soft-offline pages (testing only).
pub const MADV_SOFT_OFFLINE: u32 = 101;

// ---------------------------------------------------------------------------
// process_madvise() flags
// ---------------------------------------------------------------------------

/// Apply to all threads in the process.
pub const PROCESS_MADVISE_FLAG_ALL: u32 = 0x01;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_madv_values_distinct() {
        let behaviors = [
            MADV_WILLNEED, MADV_DONTNEED, MADV_COLD,
            MADV_PAGEOUT, MADV_FREE, MADV_COLLAPSE,
            MADV_HWPOISON, MADV_SOFT_OFFLINE,
        ];
        for i in 0..behaviors.len() {
            for j in (i + 1)..behaviors.len() {
                assert_ne!(behaviors[i], behaviors[j]);
            }
        }
    }

    #[test]
    fn test_common_values() {
        assert_eq!(MADV_WILLNEED, 3);
        assert_eq!(MADV_DONTNEED, 4);
        assert_eq!(MADV_FREE, 8);
    }

    #[test]
    fn test_cold_and_pageout() {
        assert_eq!(MADV_COLD, 20);
        assert_eq!(MADV_PAGEOUT, 21);
    }

    #[test]
    fn test_hwpoison_values() {
        assert_eq!(MADV_HWPOISON, 100);
        assert_eq!(MADV_SOFT_OFFLINE, 101);
    }

    #[test]
    fn test_process_flag() {
        assert_eq!(PROCESS_MADVISE_FLAG_ALL, 0x01);
    }
}
