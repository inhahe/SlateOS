//! `<linux/swap.h>` — Swap subsystem constants.
//!
//! Swap provides overflow storage for anonymous memory pages when
//! physical RAM is full. The kernel writes least-recently-used pages
//! to swap devices/files and reads them back on demand. swapon/swapoff
//! control swap areas. Swap priority determines which areas are used
//! first (higher priority = used first).

// ---------------------------------------------------------------------------
// swapon flags
// ---------------------------------------------------------------------------

/// Use the priority specified in the call.
pub const SWAP_FLAG_PREFER: u32 = 0x8000;
/// Priority mask (bits 0-14).
pub const SWAP_FLAG_PRIO_MASK: u32 = 0x7FFF;
/// Priority shift (not needed, mask already at bit 0).
pub const SWAP_FLAG_PRIO_SHIFT: u32 = 0;
/// Discard freed swap pages (SSD TRIM).
pub const SWAP_FLAG_DISCARD: u32 = 0x1_0000;
/// Discard on swap page allocation.
pub const SWAP_FLAG_DISCARD_ONCE: u32 = 0x2_0000;
/// Discard full swap area pages.
pub const SWAP_FLAG_DISCARD_PAGES: u32 = 0x4_0000;

// ---------------------------------------------------------------------------
// Swap priority range
// ---------------------------------------------------------------------------

/// Minimum swap priority.
pub const SWAP_PRIO_MIN: i32 = -1;
/// Maximum swap priority.
pub const SWAP_PRIO_MAX: i32 = 32767;

// ---------------------------------------------------------------------------
// Swap cluster sizes
// ---------------------------------------------------------------------------

/// Default swap cluster size (pages).
pub const SWAP_CLUSTER_MAX: u32 = 32;
/// Swap batch size for reclaim.
pub const SWAP_BATCH: u32 = 64;

// ---------------------------------------------------------------------------
// Swap header magic
// ---------------------------------------------------------------------------

/// Swap area magic string (at end of first page).
pub const SWAP_MAGIC: &str = "SWAPSPACE2";
/// Swap magic offset from page start (4096 - 10).
pub const SWAP_MAGIC_OFFSET: u32 = 4086;

// ---------------------------------------------------------------------------
// Swap limits
// ---------------------------------------------------------------------------

/// Maximum number of swap areas.
pub const MAX_SWAPFILES: u32 = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_swapon_flags_no_overlap() {
        // PREFER and DISCARD flags shouldn't overlap
        assert_eq!(SWAP_FLAG_PREFER & SWAP_FLAG_DISCARD, 0);
        assert_eq!(SWAP_FLAG_DISCARD & SWAP_FLAG_DISCARD_ONCE, 0);
        assert_eq!(SWAP_FLAG_DISCARD & SWAP_FLAG_DISCARD_PAGES, 0);
        assert_eq!(SWAP_FLAG_DISCARD_ONCE & SWAP_FLAG_DISCARD_PAGES, 0);
    }

    #[test]
    fn test_priority_range() {
        assert!(SWAP_PRIO_MIN < SWAP_PRIO_MAX);
    }

    #[test]
    fn test_prio_mask() {
        assert_eq!(SWAP_FLAG_PRIO_MASK, 0x7FFF);
        // Max priority fits in mask
        assert_eq!(
            (SWAP_PRIO_MAX as u32) & SWAP_FLAG_PRIO_MASK,
            SWAP_PRIO_MAX as u32
        );
    }

    #[test]
    fn test_magic_string() {
        assert_eq!(SWAP_MAGIC.len(), 10);
    }

    #[test]
    fn test_cluster_sizes_positive() {
        assert!(SWAP_CLUSTER_MAX > 0);
        assert!(SWAP_BATCH > 0);
    }

    #[test]
    fn test_max_swapfiles() {
        assert!(MAX_SWAPFILES > 0);
        assert!(MAX_SWAPFILES.is_power_of_two());
    }
}
