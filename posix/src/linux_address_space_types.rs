//! `<linux/fs.h>` (address_space subset) — Page cache address space constants.
//!
//! The address_space is the page cache for a single inode — it maps
//! file offsets to physical pages in memory. When you read a file,
//! the VFS checks the address_space first (page cache hit = fast path).
//! On a miss, it invokes the filesystem's readahead to fill pages from
//! disk. Dirty pages (modified in cache) are written back by the
//! writeback subsystem according to timing and memory pressure heuristics.

// ---------------------------------------------------------------------------
// Address space flags (gfp_mask / mapping flags)
// ---------------------------------------------------------------------------

/// Pages can be written back to disk.
pub const AS_EIO: u32 = 0;
/// Address space has outstanding errors.
pub const AS_ENOSPC: u32 = 1;
/// Memory-mapped pages exist.
pub const AS_MM_ALL_LOCKS: u32 = 2;
/// Unevictable pages (e.g., mlock'd).
pub const AS_UNEVICTABLE: u32 = 3;
/// Has exceptional entries (DAX, swap).
pub const AS_EXITING: u32 = 4;
/// Writeback in progress.
pub const AS_NO_WRITEBACK_TAGS: u32 = 5;
/// Large folios supported.
pub const AS_LARGE_FOLIO_SUPPORT: u32 = 6;
/// Address space supports stable writes.
pub const AS_STABLE_WRITES: u32 = 7;

// ---------------------------------------------------------------------------
// Page cache lookup flags
// ---------------------------------------------------------------------------

/// Don't create new page if not found.
pub const FGP_ACCESSED: u32 = 0x01;
/// Lock the page.
pub const FGP_LOCK: u32 = 0x02;
/// Create page if not found.
pub const FGP_CREAT: u32 = 0x04;
/// For write access (mark dirty).
pub const FGP_WRITE: u32 = 0x08;
/// Don't wait for writeback.
pub const FGP_NOWAIT: u32 = 0x10;
/// No OOM kill on allocation failure.
pub const FGP_NOFS: u32 = 0x20;
/// Allow huge/large folios.
pub const FGP_HUGE: u32 = 0x40;

// ---------------------------------------------------------------------------
// Writeback tags (radix tree tags)
// ---------------------------------------------------------------------------

/// Page is dirty (needs writeback).
pub const PAGECACHE_TAG_DIRTY: u32 = 0;
/// Page has writeback in progress.
pub const PAGECACHE_TAG_WRITEBACK: u32 = 1;
/// Page needs write (dirty + to-write).
pub const PAGECACHE_TAG_TOWRITE: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_as_flags_distinct() {
        let flags = [
            AS_EIO,
            AS_ENOSPC,
            AS_MM_ALL_LOCKS,
            AS_UNEVICTABLE,
            AS_EXITING,
            AS_NO_WRITEBACK_TAGS,
            AS_LARGE_FOLIO_SUPPORT,
            AS_STABLE_WRITES,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_fgp_flags_no_overlap() {
        let flags = [
            FGP_ACCESSED,
            FGP_LOCK,
            FGP_CREAT,
            FGP_WRITE,
            FGP_NOWAIT,
            FGP_NOFS,
            FGP_HUGE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_cache_tags_distinct() {
        let tags = [
            PAGECACHE_TAG_DIRTY,
            PAGECACHE_TAG_WRITEBACK,
            PAGECACHE_TAG_TOWRITE,
        ];
        for i in 0..tags.len() {
            for j in (i + 1)..tags.len() {
                assert_ne!(tags[i], tags[j]);
            }
        }
    }
}
