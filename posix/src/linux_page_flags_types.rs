//! `<linux/page-flags.h>` — Page frame flag constants.
//!
//! Every physical page frame (struct page) has a flags field that
//! tracks the page's current state: whether it's locked, dirty,
//! referenced, in writeback, part of slab, compound, etc. These
//! flags drive the page reclaim, writeback, and allocation decisions.
//! Flags are manipulated atomically since multiple CPUs and contexts
//! can operate on pages concurrently.

// ---------------------------------------------------------------------------
// Page flags (bit positions in page->flags)
// ---------------------------------------------------------------------------

/// Page is locked (I/O in progress, don't touch).
pub const PG_LOCKED: u32 = 0;
/// Page has been referenced recently (LRU aging).
pub const PG_REFERENCED: u32 = 1;
/// Page is up-to-date (contents match disk/backing store).
pub const PG_UPTODATE: u32 = 2;
/// Page has been modified (needs writeback).
pub const PG_DIRTY: u32 = 3;
/// Page is on an LRU list.
pub const PG_LRU: u32 = 4;
/// Page is on the active LRU list.
pub const PG_ACTIVE: u32 = 5;
/// Page is in the slab allocator.
pub const PG_SLAB: u32 = 6;
/// Page owner (kernel).
pub const PG_OWNER_PRIV_1: u32 = 7;
/// Page is part of the architecture-specific state.
pub const PG_ARCH_1: u32 = 8;
/// Page must not be freed (reserved by kernel).
pub const PG_RESERVED: u32 = 9;
/// Page is a private page (file/swap cache).
pub const PG_PRIVATE: u32 = 10;
/// Page has private_2 semantics.
pub const PG_PRIVATE_2: u32 = 11;
/// Page is being written back to disk.
pub const PG_WRITEBACK: u32 = 12;
/// Page is a compound head page (huge page).
pub const PG_HEAD: u32 = 13;
/// Page is mapped into userspace page tables.
pub const PG_MAPPEDTODISK: u32 = 14;
/// Page is in a reclaim context.
pub const PG_RECLAIM: u32 = 15;
/// Page is in swap cache.
pub const PG_SWAPBACKED: u32 = 16;
/// Page is not reclaimable (pinned).
pub const PG_UNEVICTABLE: u32 = 17;
/// Page is mlocked (cannot be swapped).
pub const PG_MLOCKED: u32 = 18;

// ---------------------------------------------------------------------------
// Page flag compound masks
// ---------------------------------------------------------------------------

/// All flags that prevent page from being freed.
pub const PG_NONFREE_MASK: u32 = (1 << PG_LOCKED)
    | (1 << PG_RESERVED)
    | (1 << PG_SLAB)
    | (1 << PG_MLOCKED);

// ---------------------------------------------------------------------------
// Page zone IDs (stored in upper bits of page->flags)
// ---------------------------------------------------------------------------

/// DMA zone (low 16 MiB for legacy ISA DMA).
pub const ZONE_DMA: u32 = 0;
/// DMA32 zone (low 4 GiB for 32-bit devices).
pub const ZONE_DMA32: u32 = 1;
/// Normal zone (all regular memory).
pub const ZONE_NORMAL: u32 = 2;
/// Movable zone (pages that can be migrated for compaction).
pub const ZONE_MOVABLE: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_flags_distinct() {
        let flags = [
            PG_LOCKED, PG_REFERENCED, PG_UPTODATE, PG_DIRTY,
            PG_LRU, PG_ACTIVE, PG_SLAB, PG_OWNER_PRIV_1,
            PG_ARCH_1, PG_RESERVED, PG_PRIVATE, PG_PRIVATE_2,
            PG_WRITEBACK, PG_HEAD, PG_MAPPEDTODISK, PG_RECLAIM,
            PG_SWAPBACKED, PG_UNEVICTABLE, PG_MLOCKED,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_nonfree_mask_includes_expected() {
        assert_ne!(PG_NONFREE_MASK & (1 << PG_LOCKED), 0);
        assert_ne!(PG_NONFREE_MASK & (1 << PG_RESERVED), 0);
        assert_ne!(PG_NONFREE_MASK & (1 << PG_SLAB), 0);
        assert_ne!(PG_NONFREE_MASK & (1 << PG_MLOCKED), 0);
    }

    #[test]
    fn test_zones_distinct() {
        let zones = [ZONE_DMA, ZONE_DMA32, ZONE_NORMAL, ZONE_MOVABLE];
        for i in 0..zones.len() {
            for j in (i + 1)..zones.len() {
                assert_ne!(zones[i], zones[j]);
            }
        }
    }
}
