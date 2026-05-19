//! `<linux/cachestat.h>` — Cachestat syscall constants.
//!
//! Constants for the cachestat(2) system call covering
//! cache status flags, range specification, and result fields.

// ---------------------------------------------------------------------------
// Cachestat flags
// ---------------------------------------------------------------------------

/// No special flags.
pub const CACHESTAT_FLAG_NONE: u32 = 0;

// ---------------------------------------------------------------------------
// Cachestat result field indices
// ---------------------------------------------------------------------------

/// Number of cached pages.
pub const CACHESTAT_NR_CACHE: u32 = 0;
/// Number of dirty pages.
pub const CACHESTAT_NR_DIRTY: u32 = 1;
/// Number of pages under writeback.
pub const CACHESTAT_NR_WRITEBACK: u32 = 2;
/// Number of evicted pages.
pub const CACHESTAT_NR_EVICTED: u32 = 3;
/// Number of recently evicted pages.
pub const CACHESTAT_NR_RECENTLY_EVICTED: u32 = 4;

// ---------------------------------------------------------------------------
// Cachestat range off/len magic values
// ---------------------------------------------------------------------------

/// Entire file (offset = 0, len = 0 means whole file).
pub const CACHESTAT_WHOLE_FILE_OFF: u64 = 0;
/// Entire file length sentinel.
pub const CACHESTAT_WHOLE_FILE_LEN: u64 = 0;

// ---------------------------------------------------------------------------
// Page cache states
// ---------------------------------------------------------------------------

/// Page is clean (unmodified).
pub const PAGE_CACHE_CLEAN: u32 = 0;
/// Page is dirty (modified, not written back).
pub const PAGE_CACHE_DIRTY: u32 = 1;
/// Page is under writeback.
pub const PAGE_CACHE_WRITEBACK: u32 = 2;
/// Page is locked.
pub const PAGE_CACHE_LOCKED: u32 = 3;
/// Page is uptodate.
pub const PAGE_CACHE_UPTODATE: u32 = 4;
/// Page has been referenced.
pub const PAGE_CACHE_REFERENCED: u32 = 5;
/// Page is active (hot).
pub const PAGE_CACHE_ACTIVE: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_result_fields_distinct() {
        let fields = [
            CACHESTAT_NR_CACHE, CACHESTAT_NR_DIRTY,
            CACHESTAT_NR_WRITEBACK, CACHESTAT_NR_EVICTED,
            CACHESTAT_NR_RECENTLY_EVICTED,
        ];
        for i in 0..fields.len() {
            for j in (i + 1)..fields.len() {
                assert_ne!(fields[i], fields[j]);
            }
        }
    }

    #[test]
    fn test_whole_file_sentinel() {
        assert_eq!(CACHESTAT_WHOLE_FILE_OFF, 0);
        assert_eq!(CACHESTAT_WHOLE_FILE_LEN, 0);
    }

    #[test]
    fn test_page_cache_states_distinct() {
        let states = [
            PAGE_CACHE_CLEAN, PAGE_CACHE_DIRTY,
            PAGE_CACHE_WRITEBACK, PAGE_CACHE_LOCKED,
            PAGE_CACHE_UPTODATE, PAGE_CACHE_REFERENCED,
            PAGE_CACHE_ACTIVE,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_page_cache_clean_is_zero() {
        assert_eq!(PAGE_CACHE_CLEAN, 0);
    }

    #[test]
    fn test_result_fields_sequential() {
        assert_eq!(CACHESTAT_NR_CACHE, 0);
        assert_eq!(CACHESTAT_NR_RECENTLY_EVICTED, 4);
    }
}
